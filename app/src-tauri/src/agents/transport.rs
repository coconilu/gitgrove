//! Kimi-only HTTP/1 transport. Authenticate the established socket before the
//! first HTTP byte; never let a higher-level client silently reconnect or proxy.
use super::{Instance, INCOMPATIBLE, UNREACHABLE, UNVERIFIED_PEER};
use bytes::Bytes;
use http_body_util::{BodyExt, Full, Limited};
use hyper::{header, Request};
use hyper_util::rt::TokioIo;
use reqwest::{Method, StatusCode, Url};
use serde_json::Value;
use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr};
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::path::Path;
use std::time::Duration;
use tokio::net::TcpStream;
use windows_sys::Win32::{
    Foundation::{
        GetLastError, ERROR_INSUFFICIENT_BUFFER, ERROR_INVALID_PARAMETER, FILETIME, HANDLE,
        WAIT_TIMEOUT,
    },
    NetworkManagement::IpHelper::{
        GetExtendedTcpTable, MIB_TCP6ROW_OWNER_PID, MIB_TCP6TABLE_OWNER_PID, MIB_TCPROW_OWNER_PID,
        MIB_TCPTABLE_OWNER_PID, TCP_TABLE_OWNER_PID_ALL,
    },
    Networking::WinSock::{AF_INET, AF_INET6},
    Security::{GetLengthSid, GetTokenInformation, IsValidSid, TokenUser, TOKEN_QUERY, TOKEN_USER},
    System::Threading::{
        GetCurrentProcess, GetProcessTimes, OpenProcess, OpenProcessToken,
        QueryFullProcessImageNameW, WaitForSingleObject, PROCESS_QUERY_LIMITED_INFORMATION,
        PROCESS_SYNCHRONIZE,
    },
};

#[derive(Clone)]
pub(super) struct Client {
    image_name: String,
    user_sid: Vec<u8>,
}

impl Client {
    pub(super) fn new() -> Result<Self, String> {
        Ok(Self {
            image_name: "kimi.exe".into(),
            user_sid: process_user(unsafe { GetCurrentProcess() })?,
        })
    }

    #[cfg(test)]
    pub(super) fn for_test_current_process() -> Self {
        Self {
            image_name: std::env::current_exe()
                .unwrap()
                .file_name()
                .unwrap()
                .to_string_lossy()
                .into_owned(),
            ..Self::new().unwrap()
        }
    }

    #[cfg(test)]
    pub(super) fn for_test_expected_user(user_sid: Vec<u8>) -> Self {
        Self {
            user_sid,
            ..Self::for_test_current_process()
        }
    }

    pub(super) async fn json(
        &self,
        instance: &Instance,
        url: Url,
        method: Method,
        token: &str,
        body: Option<Value>,
    ) -> Result<(StatusCode, Value), String> {
        tokio::time::timeout(
            Duration::from_secs(8),
            self.json_inner(instance, url, method, token, body),
        )
        .await
        .map_err(|_| UNREACHABLE)?
    }

    pub(super) async fn verify_peer(
        &self,
        instance: &Instance,
    ) -> Result<VerifiedConnection, String> {
        tokio::time::timeout(Duration::from_secs(8), self.connect_verified(instance))
            .await
            .map_err(|_| UNREACHABLE)?
    }

    async fn connect_verified(&self, instance: &Instance) -> Result<VerifiedConnection, String> {
        instance.base_url().ok_or(UNVERIFIED_PEER)?;
        let peer = PinnedPeer::open(instance, self)?;
        let address = SocketAddr::new(
            instance.host.parse().map_err(|_| UNVERIFIED_PEER)?,
            instance.port,
        );
        let stream = tokio::time::timeout(Duration::from_millis(700), TcpStream::connect(address))
            .await
            .map_err(|_| UNREACHABLE)?
            .map_err(|_| UNREACHABLE)?;
        peer.verify_connection(&stream).await?;
        Ok(VerifiedConnection { peer, stream })
    }

    async fn json_inner(
        &self,
        instance: &Instance,
        url: Url,
        method: Method,
        token: &str,
        body: Option<Value>,
    ) -> Result<(StatusCode, Value), String> {
        let base = instance.base_url().ok_or(UNVERIFIED_PEER)?;
        if url.origin() != base.origin() || !url.username().is_empty() || url.password().is_some() {
            return Err(UNVERIFIED_PEER.into());
        }
        // Hold the process object, not only a reusable PID, throughout this request.
        let VerifiedConnection { peer, stream } = self.connect_verified(instance).await?;
        let address = SocketAddr::new(
            instance.host.parse().map_err(|_| UNVERIFIED_PEER)?,
            instance.port,
        );

        // This handshake only wraps the already connected / verified stream.
        // No connector, redirect handler, pool or automatic reconnect exists.
        let (mut sender, connection) = hyper::client::conn::http1::handshake(TokioIo::new(stream))
            .await
            .map_err(|_| UNREACHABLE)?;
        let _driver = Driver(tokio::spawn(async move {
            let _ = connection.await;
        }));
        let mut auth = header::HeaderValue::from_str(&format!("Bearer {token}"))
            .map_err(|_| super::AUTH_FAILED)?;
        auth.set_sensitive(true);
        let route = match url.query() {
            Some(query) => format!("{}?{query}", url.path()),
            None => url.path().to_string(),
        };
        let request = Request::builder()
            .method(method)
            .uri(route)
            .header(header::HOST, address.to_string())
            .header(header::AUTHORIZATION, auth)
            .header(header::CONTENT_TYPE, "application/json")
            .header(header::CONNECTION, "close")
            .body(Full::new(Bytes::from(
                body.map(|v| v.to_string()).unwrap_or_default(),
            )))
            .map_err(|_| INCOMPATIBLE)?;
        // Recheck the held process object immediately before the credential sink.
        if !peer.alive() {
            return Err(UNVERIFIED_PEER.into());
        }
        let response = sender
            .send_request(request)
            .await
            .map_err(|_| UNREACHABLE)?;
        let status = response.status();
        // Return status without parsing/echoing potentially hostile error bodies.
        if !status.is_success() {
            return Ok((status, Value::Null));
        }
        let body = Limited::new(response.into_body(), 16 * 1024 * 1024)
            .collect()
            .await
            .map_err(|_| INCOMPATIBLE)?
            .to_bytes();
        let result = serde_json::from_slice(&body).map_err(|_| INCOMPATIBLE)?;
        Ok((status, result))
    }
}

// Held through the browser handoff; this socket sends no credential/HTTP data.
// Browser connections after handoff still follow Kimi's own Web protocol.
pub(super) struct VerifiedConnection {
    peer: PinnedPeer,
    stream: TcpStream,
}

struct Driver(tokio::task::JoinHandle<()>);
impl Drop for Driver {
    fn drop(&mut self) {
        self.0.abort();
    }
}

struct PinnedPeer {
    process: OwnedHandle,
    pid: u32,
}
impl PinnedPeer {
    fn open(instance: &Instance, client: &Client) -> Result<Self, String> {
        let raw = unsafe {
            OpenProcess(
                PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE,
                0,
                instance.pid,
            )
        };
        if raw.is_null() {
            return Err(if unsafe { GetLastError() } == ERROR_INVALID_PARAMETER {
                UNREACHABLE
            } else {
                UNVERIFIED_PEER
            }
            .into());
        }
        let process = unsafe { OwnedHandle::from_raw_handle(raw) };
        let peer = Self {
            process,
            pid: instance.pid,
        };
        if !peer.alive() {
            return Err(UNREACHABLE.into());
        }
        let image = process_image(raw)?;
        if !Path::new(&image).file_name().is_some_and(|name| {
            name.to_string_lossy()
                .eq_ignore_ascii_case(&client.image_name)
        }) || process_user(raw)? != client.user_sid
            || instance.started_at == 0
            || creation_time_ms(raw)? > instance.started_at
        {
            return Err(UNVERIFIED_PEER.into());
        }
        Ok(peer)
    }

    fn alive(&self) -> bool {
        unsafe { WaitForSingleObject(self.process.as_raw_handle(), 0) == WAIT_TIMEOUT }
    }

    async fn verify_connection(&self, stream: &TcpStream) -> Result<(), String> {
        let local = stream.local_addr().map_err(|_| UNVERIFIED_PEER)?;
        let remote = stream.peer_addr().map_err(|_| UNVERIFIED_PEER)?;
        if !local.ip().is_loopback() || !remote.ip().is_loopback() {
            return Err(UNVERIFIED_PEER.into());
        }
        // The server-side ESTABLISHED row may appear just after connect completes.
        // Fail closed if the exact reverse tuple cannot be attributed in time.
        for _ in 0..10 {
            let owner = connection_owner(local, remote)?;
            match owner {
                Some(pid) if pid == self.pid && self.alive() => return Ok(()),
                Some(_) => return Err(UNVERIFIED_PEER.into()),
                None => tokio::time::sleep(Duration::from_millis(20)).await,
            }
        }
        Err(UNVERIFIED_PEER.into())
    }
}

fn process_image(process: HANDLE) -> Result<String, String> {
    let mut buffer = vec![0u16; 32768];
    let mut length = buffer.len() as u32;
    if unsafe { QueryFullProcessImageNameW(process, 0, buffer.as_mut_ptr(), &mut length) } == 0 {
        return Err(UNVERIFIED_PEER.into());
    }
    String::from_utf16(&buffer[..length as usize]).map_err(|_| UNVERIFIED_PEER.into())
}

fn process_user(process: HANDLE) -> Result<Vec<u8>, String> {
    let mut raw = std::ptr::null_mut();
    if unsafe { OpenProcessToken(process, TOKEN_QUERY, &mut raw) } == 0 {
        return Err(UNVERIFIED_PEER.into());
    }
    let _token = unsafe { OwnedHandle::from_raw_handle(raw) };
    let mut length = 0;
    unsafe {
        GetTokenInformation(raw, TokenUser, std::ptr::null_mut(), 0, &mut length);
    }
    if length == 0 || length > 65536 {
        return Err(UNVERIFIED_PEER.into());
    }
    let mut buffer = vec![0usize; (length as usize).div_ceil(std::mem::size_of::<usize>())];
    if unsafe {
        GetTokenInformation(
            raw,
            TokenUser,
            buffer.as_mut_ptr().cast(),
            length,
            &mut length,
        )
    } == 0
    {
        return Err(UNVERIFIED_PEER.into());
    }
    let user = unsafe { std::ptr::read_unaligned(buffer.as_ptr().cast::<TOKEN_USER>()) };
    if unsafe { IsValidSid(user.User.Sid) } == 0 {
        return Err(UNVERIFIED_PEER.into());
    }
    let sid = unsafe {
        std::slice::from_raw_parts(
            user.User.Sid.cast::<u8>(),
            GetLengthSid(user.User.Sid) as usize,
        )
    };
    Ok(sid.to_vec())
}

fn creation_time_ms(process: HANDLE) -> Result<u64, String> {
    let mut created = FILETIME::default();
    let mut exited = FILETIME::default();
    let mut kernel = FILETIME::default();
    let mut user = FILETIME::default();
    if unsafe { GetProcessTimes(process, &mut created, &mut exited, &mut kernel, &mut user) } == 0 {
        return Err(UNVERIFIED_PEER.into());
    }
    let ticks = (u64::from(created.dwHighDateTime) << 32) | u64::from(created.dwLowDateTime);
    ticks
        .checked_sub(116_444_736_000_000_000)
        .map(|ticks| ticks / 10_000)
        .ok_or_else(|| UNVERIFIED_PEER.into())
}

fn connection_owner(client: SocketAddr, server: SocketAddr) -> Result<Option<u32>, String> {
    const ESTABLISHED: u32 = 5;
    match (client, server) {
        (SocketAddr::V4(client), SocketAddr::V4(server)) => {
            let rows = tcp_rows::<MIB_TCPROW_OWNER_PID>(
                AF_INET as u32,
                std::mem::offset_of!(MIB_TCPTABLE_OWNER_PID, table),
            )?;
            Ok(rows
                .into_iter()
                .find(|row| {
                    row.dwState == ESTABLISHED
                        && Ipv4Addr::from(row.dwLocalAddr.to_ne_bytes()) == *server.ip()
                        && u16::from_be(row.dwLocalPort as u16) == server.port()
                        && Ipv4Addr::from(row.dwRemoteAddr.to_ne_bytes()) == *client.ip()
                        && u16::from_be(row.dwRemotePort as u16) == client.port()
                })
                .map(|row| row.dwOwningPid))
        }
        (SocketAddr::V6(client), SocketAddr::V6(server)) => {
            let rows = tcp_rows::<MIB_TCP6ROW_OWNER_PID>(
                AF_INET6 as u32,
                std::mem::offset_of!(MIB_TCP6TABLE_OWNER_PID, table),
            )?;
            Ok(rows
                .into_iter()
                .find(|row| {
                    row.dwState == ESTABLISHED
                        && Ipv6Addr::from(row.ucLocalAddr) == *server.ip()
                        && row.dwLocalScopeId == server.scope_id()
                        && u16::from_be(row.dwLocalPort as u16) == server.port()
                        && Ipv6Addr::from(row.ucRemoteAddr) == *client.ip()
                        && row.dwRemoteScopeId == client.scope_id()
                        && u16::from_be(row.dwRemotePort as u16) == client.port()
                })
                .map(|row| row.dwOwningPid))
        }
        _ => Err(UNVERIFIED_PEER.into()),
    }
}

fn tcp_rows<T: Copy>(family: u32, offset: usize) -> Result<Vec<T>, String> {
    let mut size = 0;
    unsafe {
        GetExtendedTcpTable(
            std::ptr::null_mut(),
            &mut size,
            0,
            family,
            TCP_TABLE_OWNER_PID_ALL,
            0,
        );
    }
    for _ in 0..4 {
        if size < 4 || size > 8 * 1024 * 1024 {
            return Err(UNVERIFIED_PEER.into());
        }
        let mut storage = vec![0u64; (size as usize).div_ceil(8)];
        let result = unsafe {
            GetExtendedTcpTable(
                storage.as_mut_ptr().cast(),
                &mut size,
                0,
                family,
                TCP_TABLE_OWNER_PID_ALL,
                0,
            )
        };
        if result == ERROR_INSUFFICIENT_BUFFER {
            continue;
        }
        if result != 0 {
            return Err(UNVERIFIED_PEER.into());
        }
        let base = storage.as_ptr().cast::<u8>();
        let count = unsafe { std::ptr::read_unaligned(base.cast::<u32>()) } as usize;
        if offset > size as usize || count > (size as usize - offset) / std::mem::size_of::<T>() {
            return Err(UNVERIFIED_PEER.into());
        }
        return Ok((0..count)
            .map(|index| unsafe {
                std::ptr::read_unaligned(
                    base.add(offset + index * std::mem::size_of::<T>())
                        .cast::<T>(),
                )
            })
            .collect());
    }
    Err(UNVERIFIED_PEER.into())
}
