#![cfg(windows)]
use super::*;
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

struct Scratch(PathBuf);
impl Scratch {
    fn new() -> Self {
        static NEXT_SCRATCH: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let sequence = NEXT_SCRATCH.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "gitgrove-agents-{}-{stamp}-{sequence}",
            std::process::id()
        ));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn schema() -> Value {
    let mut schema = json!({"paths":{}, "components":{"schemas":{}}});
    for (path, field) in [
        ("/api/v1/workspaces", "root"),
        ("/api/v1/sessions", "workspace_id"),
    ] {
        schema["paths"][path] = json!({"post":{"requestBody":{"content":{"application/json":{"schema":{"type":"object", "properties":{field:{"type":"string"}}}}}}}});
    }
    schema["paths"]["/api/v1/sessions"]["get"] = json!({"parameters":[
        {"name":"workspace_id","in":"query","schema":{"type":"string"}}, {"name":"page_size","in":"query","schema":{"type":"integer"}},
        {"name":"busy","in":"query","schema":{"type":"boolean"}}, {"name":"include_archive","in":"query","schema":{"type":"boolean"}},
        {"name":"exclude_empty","in":"query","schema":{"type":"boolean"}}, {"name":"archived_only","in":"query","schema":{"type":"boolean"}}
    ]});
    schema
}

#[test]
fn paths_round_trip_without_shell_interpretation() {
    let scratch = Scratch::new();
    for name in [
        "ordinary",
        "with spaces",
        "中文目录",
        "with & ampersand",
        "with # hash",
    ] {
        let path = scratch.0.join(name);
        std::fs::create_dir(&path).unwrap();
        let target = target_directory(path.to_str().unwrap()).unwrap();
        let url = codex_url(&target);
        assert_eq!(
            url.query_pairs().collect::<Vec<_>>(),
            vec![("path".into(), target.to_string_lossy())]
        );
        assert!(url.fragment().is_none());
        assert!(!target.to_string_lossy().starts_with(r"\\?\"));
    }
    assert!(target_directory("relative").is_err());
    assert!(target_directory(scratch.0.join("absent").to_str().unwrap()).is_err());
    let file = scratch.0.join("file");
    std::fs::write(&file, "a").unwrap();
    assert!(target_directory(file.to_str().unwrap()).is_err());
}

#[test]
fn discovery_only_accepts_loopback_and_real_instance_records() {
    let scratch = Scratch::new();
    let dir = scratch.0.join("server/instances");
    std::fs::create_dir_all(&dir).unwrap();
    for (idx, host) in [
        "127.0.0.1",
        "::1",
        "0.0.0.0",
        "192.168.0.4",
        "localhost",
        "127.0.0.1@example.org",
        "https://evil.test",
    ]
    .into_iter()
    .enumerate()
    {
        std::fs::write(
            dir.join(format!("{idx}.json")),
            json!({"host":host,"port":50000+idx,"pid":123,"heartbeat_at":idx}).to_string(),
        )
        .unwrap();
    }
    std::fs::write(dir.join("invalid.json"), "broken").unwrap();
    std::fs::write(
        dir.join("pending.json.tmp"),
        json!({"host":"127.0.0.1","port":12,"pid":123}).to_string(),
    )
    .unwrap();
    let found = instances(&scratch.0).unwrap();
    assert_eq!(found.len(), 2);
    assert_eq!(found[0].base_url().unwrap().as_str(), "http://[::1]:50001/");
}

#[test]
fn incompatible_request_schema_and_unsafe_ids_are_rejected() {
    let original = schema();
    check_schema(&original).unwrap();
    let mut incompatible = original.clone();
    incompatible["paths"]["/api/v1/workspaces"]["post"]["requestBody"]["content"]
        ["application/json"]["schema"]["required"] = json!(["root", "trust"]);
    assert!(check_schema(&incompatible).is_err());
    incompatible = original.clone();
    incompatible["paths"]["/api/v1/sessions"]["get"]["parameters"]
        .as_array_mut()
        .unwrap()
        .push(json!({"name":"secret_new_field","in":"query","required":true}));
    assert!(check_schema(&incompatible).is_err());
    let mut refs = original.clone();
    refs["components"]["schemas"]["Workspace"] = refs["paths"]["/api/v1/workspaces"]["post"]
        ["requestBody"]["content"]["application/json"]["schema"]
        .clone();
    refs["paths"]["/api/v1/workspaces"]["post"]["requestBody"]["content"]["application/json"]
        ["schema"] = json!({"$ref":"#/components/schemas/Workspace"});
    check_schema(&refs).unwrap();
    for id in [
        "",
        ".",
        "..",
        "../evil",
        "//evil.test",
        "a#token=oops",
        "a?prompt=hi",
    ] {
        assert!(safe_id(&json!({"id":id}), "id").is_err());
    }
}

#[derive(Clone)]
struct Request {
    method: String,
    target: String,
    body: Value,
    auth: Option<String>,
}
struct Mock {
    base: Url,
    requests: Arc<Mutex<Vec<Request>>>,
    task: tokio::task::JoinHandle<()>,
}
impl Drop for Mock {
    fn drop(&mut self) {
        self.task.abort();
    }
}
impl Mock {
    async fn new(
        handler: impl Fn(&Request) -> (u16, String, Value) + Send + Sync + 'static,
    ) -> Self {
        Self::new_at("127.0.0.1", handler).await
    }
    async fn new_at(
        host: &str,
        handler: impl Fn(&Request) -> (u16, String, Value) + Send + Sync + 'static,
    ) -> Self {
        let listener = tokio::net::TcpListener::bind((host, 0)).await.unwrap();
        let base = Url::parse(&format!("http://{}/", listener.local_addr().unwrap())).unwrap();
        let requests = Arc::new(Mutex::new(Vec::new()));
        let captured = requests.clone();
        let task = tokio::spawn(async move {
            loop {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut bytes = Vec::new();
                let mut chunk = [0; 4096];
                loop {
                    let count = socket.read(&mut chunk).await.unwrap();
                    if count == 0 {
                        break;
                    }
                    bytes.extend_from_slice(&chunk[..count]);
                    if let Some(end) = bytes.windows(4).position(|b| b == b"\r\n\r\n") {
                        let headers = String::from_utf8_lossy(&bytes[..end]);
                        let length = headers
                            .lines()
                            .find_map(|l| {
                                l.to_lowercase()
                                    .strip_prefix("content-length:")
                                    .map(|v| v.trim().parse::<usize>().unwrap())
                            })
                            .unwrap_or(0);
                        if bytes.len() >= end + 4 + length {
                            break;
                        }
                    }
                }
                let Some(end) = bytes.windows(4).position(|b| b == b"\r\n\r\n") else {
                    continue;
                };
                let headers = String::from_utf8_lossy(&bytes[..end]);
                let mut first = headers.lines().next().unwrap().split_whitespace();
                let request = Request {
                    method: first.next().unwrap().into(),
                    target: first.next().unwrap().into(),
                    body: serde_json::from_slice(&bytes[end + 4..]).unwrap_or(Value::Null),
                    auth: headers.lines().find_map(|l| {
                        l.strip_prefix("authorization:")
                            .map(|s| s.trim().to_string())
                    }),
                };
                let (status, extra_headers, body) = handler(&request);
                captured.lock().unwrap().push(request);
                let body = body.to_string();
                let response = format!("HTTP/1.1 {status} Test\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n{extra_headers}\r\n{body}", body.len());
                socket.write_all(response.as_bytes()).await.unwrap();
            }
        });
        Self {
            base,
            requests,
            task,
        }
    }
    fn kimi(&self) -> Kimi {
        Kimi {
            http: Client::for_test_current_process(),
            base: self.base.clone(),
            token: "test-only-secret".into(),
            instance: self.instance(),
        }
    }
    fn instance(&self) -> Instance {
        Instance {
            host: self
                .base
                .host_str()
                .unwrap()
                .trim_matches(['[', ']'])
                .to_string(),
            port: self.base.port().unwrap(),
            pid: std::process::id(),
            started_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis() as u64,
            heartbeat_at: 0,
        }
    }
    fn register(&self, home: &Path, index: u64) {
        let dir = home.join("server/instances");
        std::fs::create_dir_all(&dir).unwrap();
        let instance = self.instance();
        std::fs::write(dir.join(format!("{index}.json")), json!({"host":instance.host,"port":instance.port,"pid":instance.pid,"started_at":instance.started_at,"heartbeat_at":index}).to_string()).unwrap();
    }
}

fn success(data: Value) -> (u16, String, Value) {
    (200, String::new(), json!({"code":0,"data":data}))
}

#[tokio::test]
async fn reopens_workspace_and_session_without_duplicate_or_prompt() {
    let scratch = Scratch::new();
    let target = target_directory(scratch.0.to_str().unwrap()).unwrap();
    let root = target.clone();
    let created = Arc::new(Mutex::new(false));
    let flag = created.clone();
    let mock = Mock::new(move |request| {
        if request.target == "/api/v1/workspaces" {
            assert_eq!(request.body, json!({"root":root}));
            return success(json!({"id":"wd_project_0123456789ab","root":root}));
        }
        if request.target.starts_with("/api/v1/sessions?") {
            assert!(request
                .target
                .contains("workspace_id=wd_project_0123456789ab"));
            let items = if *flag.lock().unwrap() {
                json!([{"id":"session-1","workspace_id":"wd_project_0123456789ab"}])
            } else {
                json!([])
            };
            return success(json!({"items":items}));
        }
        assert_eq!(request.target, "/api/v1/sessions");
        assert_eq!(request.method, "POST");
        assert_eq!(
            request.body,
            json!({"workspace_id":"wd_project_0123456789ab"})
        );
        *flag.lock().unwrap() = true;
        success(json!({"id":"session-1","workspace_id":"wd_project_0123456789ab"}))
    })
    .await;
    for _ in 0..2 {
        let url = mock.kimi().session_url(&target).await.unwrap();
        assert_eq!(url.path(), "/sessions/session-1");
        assert_eq!(url.fragment(), Some("token=test-only-secret"));
        assert!(url.query().is_none());
    }
    let requests = mock.requests.lock().unwrap();
    assert_eq!(
        requests
            .iter()
            .filter(|r| r.method == "POST" && r.target == "/api/v1/sessions")
            .count(),
        1
    );
    assert!(requests
        .iter()
        .all(|r| r.auth.as_deref() == Some("Bearer test-only-secret")));
}

#[tokio::test]
async fn auth_failure_redirect_and_business_errors_do_not_leak_credentials() {
    for (status, extra, body) in [
        (401, String::new(), json!({"msg":"test-only-secret"})),
        (
            302,
            "Location: http://127.0.0.1:1/stolen\r\n".into(),
            json!({}),
        ),
        (
            200,
            "".into(),
            json!({"code":40101,"msg":"test-only-secret"}),
        ),
        (
            200,
            "".into(),
            json!({"code":50001,"data":{"id":"wrong"},"msg":"test-only-secret"}),
        ),
    ] {
        let mock = Mock::new(move |_| (status, extra.clone(), body.clone())).await;
        let error = mock
            .kimi()
            .request(Method::POST, "api/v1/workspaces", None)
            .await
            .unwrap_err();
        assert!(!error.contains("test-only-secret"));
        assert!(!error.contains("http"));
        assert_eq!(mock.requests.lock().unwrap().len(), 1);
    }
}

#[tokio::test]
async fn multiple_instances_skip_stale_or_incompatible_before_starting() {
    let scratch = Scratch::new();
    std::fs::write(scratch.0.join("server.token"), "test-only-secret").unwrap();
    let good = Mock::new(|r| match r.target.as_str() {
        "/api/v1/healthz" => success(json!({"ok":true})),
        "/api/v1/meta" => {
            success(json!({"server_version":"future-compatible","dangerous_bypass_auth":false}))
        }
        "/openapi.json" => (200, String::new(), schema()),
        _ => panic!("unexpected route"),
    })
    .await;
    let bad = Mock::new(|r| {
        if r.target == "/api/v1/healthz" {
            success(json!({"ok":true}))
        } else {
            (401, String::new(), json!({}))
        }
    })
    .await;
    good.register(&scratch.0, 1);
    bad.register(&scratch.0, 2);
    let selected = discover(&scratch.0, &Client::for_test_current_process())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(selected.base, good.base);
    assert_eq!(bad.requests.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn unsafe_auth_configuration_is_not_reused() {
    let mock =
        Mock::new(|_| success(json!({"server_version":"1","dangerous_bypass_auth":true}))).await;
    assert!(mock.kimi().verify().await.unwrap_err().contains("默认鉴权"));
    assert_eq!(mock.requests.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn impersonated_health_server_never_receives_fake_token() {
    let scratch = Scratch::new();
    std::fs::write(scratch.0.join("server.token"), "test-only-secret").unwrap();
    // An impostor would return successful health/meta bodies if queried.
    let mock = Mock::new(|_| {
        success(json!({"ok":true,"server_version":"0.40.1","dangerous_bypass_auth":false}))
    })
    .await;
    mock.register(&scratch.0, 1);
    let record = scratch.0.join("server/instances/1.json");
    let original: Value = serde_json::from_slice(&std::fs::read(&record).unwrap()).unwrap();
    for (field, invalid) in [
        ("started_at", 1u64),
        ("started_at", 0),
        ("pid", u32::MAX as u64),
    ] {
        let mut changed = original.clone();
        changed[field] = json!(invalid);
        std::fs::write(&record, changed.to_string()).unwrap();
        assert!(!matches!(
            discover(&scratch.0, &Client::for_test_current_process()).await,
            Ok(Some(_))
        ));
    }
    std::fs::write(&record, original.to_string()).unwrap();
    // Production policy rejects even a same-user listener with a non-Kimi image.
    assert!(discover(&scratch.0, &Client::new().unwrap()).await.is_err());
    assert!(mock.requests.lock().unwrap().is_empty());
}

#[tokio::test]
async fn different_user_sid_and_unconfirmable_process_send_no_authorization() {
    let mock = Mock::new(|_| success(json!({}))).await;
    // A valid NULL SID (S-1-0-0) models a different expected OS identity. No test
    // account is created; the actual listener's TokenUser still comes from Win32.
    let other_user = Client::for_test_expected_user(vec![1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    let error = other_user
        .json(
            &mock.instance(),
            mock.base.join("api/v1/meta").unwrap(),
            Method::GET,
            "test-only-secret",
            None,
        )
        .await
        .unwrap_err();
    assert_eq!(error, UNVERIFIED_PEER);
    let mut inaccessible = mock.instance();
    inaccessible.pid = 4; // Windows System: access/identity cannot match this test peer.
    assert!(Client::for_test_current_process()
        .json(
            &inaccessible,
            mock.base.join("api/v1/meta").unwrap(),
            Method::GET,
            "test-only-secret",
            None
        )
        .await
        .is_err());
    assert!(mock.requests.lock().unwrap().is_empty());
}

#[tokio::test]
async fn authenticated_ipv6_request_uses_the_verified_connection() {
    let mock = Mock::new_at("::1", |_| success(json!({"version":"test"}))).await;
    mock.kimi()
        .request(Method::GET, "api/v1/meta", None)
        .await
        .unwrap();
    assert_eq!(
        mock.requests.lock().unwrap()[0].auth.as_deref(),
        Some("Bearer test-only-secret")
    );
}

#[test]
#[ignore = "Subprocess fixture only: requires a test-owned port and scratch directory"]
fn peer_listener_child() {
    use std::io::{Read, Write};
    let directory = PathBuf::from(std::env::var("GITGROVE_PEER_TEST_DIR").unwrap());
    let port: u16 = std::env::var("GITGROVE_PEER_TEST_PORT")
        .unwrap()
        .parse()
        .unwrap();
    let socket = socket2::Socket::new(socket2::Domain::IPV4, socket2::Type::STREAM, None).unwrap();
    socket.set_reuse_address(true).unwrap();
    let address: std::net::SocketAddr = format!("127.0.0.1:{port}").parse().unwrap();
    socket.bind(&address.into()).unwrap();
    socket.listen(8).unwrap();
    let listener: std::net::TcpListener = socket.into();
    std::fs::write(directory.join("ready"), std::process::id().to_string()).unwrap();
    let (mut stream, _) = listener.accept().unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    let mut bytes = [0u8; 8192];
    let length = stream.read(&mut bytes).unwrap();
    std::fs::write(directory.join("captured"), &bytes[..length]).unwrap();
    if length > 0 {
        let body = r#"{"code":0,"data":{"ok":true}}"#;
        let _ = write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
    }
}

async fn wait_for_file(path: &Path) {
    for _ in 0..100 {
        if path.is_file() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("test helper did not become ready");
}

#[tokio::test]
async fn reconnect_to_same_port_with_a_new_owner_never_sends_authorization() {
    let scratch = Scratch::new();
    let mock = Mock::new(|_| success(json!({"ok":true}))).await;
    let kimi = mock.kimi();
    // First connection really authenticates with our own verified listener.
    kimi.request(Method::GET, "api/v1/meta", None)
        .await
        .unwrap();
    assert_eq!(mock.requests.lock().unwrap().len(), 1);
    let port = mock.base.port().unwrap();
    drop(mock);
    tokio::time::sleep(Duration::from_millis(50)).await;
    // A different process with the SAME image and OS user takes the old port.
    // Checking only the old PID's liveness would still pass (it is this test).
    let child = git::new_cmd(std::env::current_exe().unwrap().to_str().unwrap())
        .args([
            "--ignored",
            "--exact",
            "agents::tests::peer_listener_child",
            "--nocapture",
        ])
        .env("GITGROVE_PEER_TEST_DIR", &scratch.0)
        .env("GITGROVE_PEER_TEST_PORT", port.to_string())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let _owned = StartedServer(Some(child));
    wait_for_file(&scratch.0.join("ready")).await;
    let error = kimi
        .request(Method::GET, "api/v1/meta", None)
        .await
        .unwrap_err();
    assert_eq!(error, UNVERIFIED_PEER);
    wait_for_file(&scratch.0.join("captured")).await;
    assert!(std::fs::read(scratch.0.join("captured"))
        .unwrap()
        .is_empty());
    // A final browser-handoff validation also refuses the stale instance.
    assert!(kimi.http.verify_peer(&kimi.instance).await.is_err());
}

#[tokio::test]
async fn final_browser_peer_check_is_credential_free() {
    let mock = Mock::new(|_| panic!("handoff check must not send an HTTP request")).await;
    let kimi = mock.kimi();
    let checked = kimi.http.verify_peer(&kimi.instance).await.unwrap();
    drop(checked);
    tokio::task::yield_now().await;
    assert!(mock.requests.lock().unwrap().is_empty());
}

#[tokio::test]
async fn backend_busy_guard_rejects_direct_reentry() {
    let _guard = OPEN_LOCK.lock().await;
    let result = open_in_agent("ignored".into(), Agent::Kimi).await;
    assert!(result
        .err()
        .unwrap()
        .contains(if cfg!(windows) { "等待" } else { "Windows" }));
}

#[test]
fn server_start_uses_individual_arguments_and_neutral_cwd() {
    let home = Path::new(r"C:\Users\test\Kimi 空格 & #");
    let exe = Path::new(r"C:\Programs\Kimi Code\kimi.exe");
    let command = server_command(exe, home, 49231);
    assert_eq!(command.get_program(), exe);
    assert_eq!(command.get_current_dir(), Some(home));
    assert_eq!(
        command.get_args().collect::<Vec<_>>(),
        ["web", "--host", "127.0.0.1", "--port", "49231", "--no-open"]
    );
    assert!(command
        .get_envs()
        .any(|(key, value)| key == "KIMI_CODE_HOME" && value == Some(home.as_os_str())));
}

#[tokio::test]
#[ignore = "Windows GUI acceptance: opens an external app, requires explicit target and tool"]
async fn desktop_open() {
    let path = std::env::var("GITGROVE_AGENT_TARGET")
        .expect("set GITGROVE_AGENT_TARGET to an existing absolute directory");
    let agent = match std::env::var("GITGROVE_AGENT_TOOL").as_deref() {
        Ok("codex") => Agent::Codex,
        Ok("kimi") => Agent::Kimi,
        _ => panic!("set GITGROVE_AGENT_TOOL=codex or kimi"),
    };
    let receipt = open_in_agent(path, agent).await.unwrap();
    println!("{}", receipt.message);
}

#[tokio::test]
#[ignore = "Starts a real Kimi service in an explicit isolated home; kills only the process started here"]
async fn isolated_cold_start() {
    let home = PathBuf::from(
        std::env::var("GITGROVE_TEST_KIMI_HOME").expect("set an isolated absolute Kimi home"),
    );
    assert!(home.is_absolute());
    assert!(
        instances(&home).unwrap().is_empty(),
        "test needs an isolated home with no instances"
    );
    let target = target_directory(&std::env::var("GITGROVE_AGENT_TARGET").unwrap()).unwrap();
    let (kimi, owned) = connect_kimi(&home).await.unwrap();
    assert!(owned.is_some());
    let first = kimi.session_url(&target).await.unwrap();
    let second = kimi.session_url(&target).await.unwrap();
    assert_eq!(first.path(), second.path());
    let handoff = kimi.http.verify_peer(&kimi.instance).await.unwrap();
    drop(handoff);
    println!(
        "verified port={} path={} (credential omitted)",
        kimi.base.port().unwrap(),
        first.path()
    );
    drop(owned);
}

#[tokio::test]
#[ignore = "Read-only compatibility probe: sends only a fixed fake token, never reads server.token"]
async fn existing_kimi_instances_accept_verified_socket_and_reject_fake_token() {
    let client = Client::new().unwrap();
    let records = instances(&kimi_home().unwrap()).unwrap();
    assert!(
        !records.is_empty(),
        "requires an already running Kimi Web instance"
    );
    for instance in records {
        let url = instance.base_url().unwrap().join("api/v1/meta").unwrap();
        let (status, _) = client
            .json(
                &instance,
                url,
                Method::GET,
                "gitgrove-peer-probe-not-a-real-token",
                None,
            )
            .await
            .unwrap();
        assert_eq!(status, 401);
        println!(
            "verified pid={} port={} fake-token status=401",
            instance.pid, instance.port
        );
    }
}
