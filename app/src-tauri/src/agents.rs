// GUI adapters. Credentials and authenticated URLs never cross the Tauri boundary.
#[cfg(not(windows))]
use reqwest::Client;
use reqwest::{Method, Url};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::Duration;

use crate::git;

#[cfg(windows)]
mod transport;
#[cfg(windows)]
use transport::Client;

const INCOMPATIBLE: &str = "Kimi Code 接口不兼容，请更新 Kimi Code 后重启 Web 服务再试。";
const AUTH_FAILED: &str = "Kimi Code 鉴权失败，请用 kimi web 重新打开服务，确认它与 GitGrove 使用同一个 KIMI_CODE_HOME 后重试。";
const UNREACHABLE: &str = "无法连接 Kimi Code 本地服务，请重新运行 kimi web 后重试。";
const UNVERIFIED_PEER: &str = "无法确认 Kimi Code 服务的进程或 Windows 用户身份，已停止发送凭证。请以当前 Windows 用户重新运行 kimi web 后重试。";
static OPEN_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Agent {
    Codex,
    Kimi,
}

#[tauri::command]
pub fn agent_open_support() -> bool {
    // Other platforms retain the existing editor/terminal adapters.
    cfg!(windows)
}

fn target_directory(path: &str) -> Result<PathBuf, String> {
    let target = Path::new(path);
    if !target.is_absolute() || !target.is_dir() {
        return Err("目标目录不存在或不是完整目录路径，请刷新项目并检查目录。".into());
    }
    let canonical = target
        .canonicalize()
        .map_err(|_| "无法访问目标目录，请检查文件夹权限。")?;
    // Windows extended paths are not accepted consistently by external apps.
    #[cfg(windows)]
    {
        let value = canonical.to_string_lossy();
        if let Some(unc) = value.strip_prefix(r"\\?\UNC\") {
            return Ok(PathBuf::from(format!(r"\\{unc}")));
        }
        if let Some(drive) = value.strip_prefix(r"\\?\") {
            return Ok(PathBuf::from(drive));
        }
    }
    Ok(canonical)
}

fn codex_url(path: &Path) -> Url {
    let mut url = Url::parse("codex://new").expect("constant URL");
    url.query_pairs_mut()
        .append_pair("path", &path.to_string_lossy());
    url
}

fn open_gui_url(url: &Url, agent: Agent) -> Result<(), String> {
    #[cfg(windows)]
    {
        use windows_sys::Win32::UI::{Shell::ShellExecuteW, WindowsAndMessaging::SW_SHOWNORMAL};
        let wide: Vec<u16> = url.as_str().encode_utf16().chain(Some(0)).collect();
        let verb: Vec<u16> = "open".encode_utf16().chain(Some(0)).collect();
        // ShellExecute receives one URL, never a cmd/PowerShell command string.
        let result = unsafe {
            ShellExecuteW(
                std::ptr::null_mut(),
                verb.as_ptr(),
                wide.as_ptr(),
                std::ptr::null(),
                std::ptr::null(),
                SW_SHOWNORMAL,
            )
        } as isize;
        if result > 32 {
            return Ok(());
        }
        return Err(match agent {
            Agent::Codex => "无法打开 Codex：请安装或修复 Codex 桌面应用，确认 Windows 已注册 codex 协议后重试。",
            Agent::Kimi => "工作区已登记，但浏览器未能打开。请检查 Windows 默认浏览器，然后重试。",
        }.into());
    }
    #[cfg(not(windows))]
    {
        let _ = (url, agent);
        Err("本版本的 Codex / Kimi Code 图形界面入口仅在 Windows 启用。".into())
    }
}

#[derive(Clone, Deserialize)]
struct Instance {
    host: String,
    port: u16,
    pid: u32,
    #[serde(default)]
    started_at: u64,
    #[serde(default)]
    heartbeat_at: u64,
}

impl Instance {
    fn base_url(&self) -> Option<Url> {
        // Never send a credential to a remote host, wildcard bind or DNS alias.
        let ip: std::net::IpAddr = self.host.parse().ok()?;
        if !ip.is_loopback() || self.port == 0 || self.pid == 0 {
            return None;
        }
        let addr = std::net::SocketAddr::new(ip, self.port);
        Url::parse(&format!("http://{addr}/")).ok()
    }
}

fn kimi_home() -> Result<PathBuf, String> {
    let home = std::env::var_os("KIMI_CODE_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| git::home_dir().join(".kimi-code"));
    if !home.is_absolute() {
        return Err("KIMI_CODE_HOME 必须是绝对路径，请修正环境变量后重启 GitGrove。".into());
    }
    Ok(home)
}

fn instances(home: &Path) -> Result<Vec<Instance>, String> {
    let entries = match std::fs::read_dir(home.join("server/instances")) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(_) => {
            return Err("无法读取 Kimi Code 实例目录，请检查 KIMI_CODE_HOME 及文件权限。".into())
        }
    };
    let mut result = Vec::new();
    for entry in entries.flatten() {
        if entry.path().extension().is_some_and(|e| e == "json") {
            if let Ok(data) = std::fs::read(entry.path()) {
                if let Ok(instance) = serde_json::from_slice::<Instance>(&data) {
                    if instance.base_url().is_some() {
                        result.push(instance);
                    }
                }
            }
        }
    }
    // Deterministic choice. Probe every candidate before considering a new server.
    result.sort_by_key(|i| std::cmp::Reverse(i.heartbeat_at));
    Ok(result)
}

fn token(home: &Path) -> Result<String, String> {
    let token = std::fs::read_to_string(home.join("server.token")).map_err(|_| AUTH_FAILED)?;
    let token = token.trim();
    if token.is_empty() || token.len() > 4096 || token.chars().any(char::is_control) {
        return Err(AUTH_FAILED.into());
    }
    Ok(token.to_string())
}

fn local_client() -> Result<Client, String> {
    #[cfg(windows)]
    {
        Client::new()
    }
    #[cfg(not(windows))]
    {
        Err(UNVERIFIED_PEER.into())
    }
}

// Intentionally no Debug / Serialize: this object contains a server credential.
struct Kimi {
    http: Client,
    base: Url,
    token: String,
    instance: Instance,
}

impl Kimi {
    #[cfg(windows)]
    async fn request(
        &self,
        method: Method,
        route: &str,
        body: Option<Value>,
    ) -> Result<Value, String> {
        let url = self.base.join(route).map_err(|_| INCOMPATIBLE)?;
        let (status, value) = self
            .http
            .json(&self.instance, url, method, &self.token, body)
            .await?;
        if status == 401 || status == 403 {
            return Err(AUTH_FAILED.into());
        }
        if !status.is_success() {
            return Err("Kimi Code 拒绝了请求，请检查 Web 服务状态或更新版本后重试。".into());
        }
        if route == "openapi.json" {
            return Ok(value);
        }
        match value["code"].as_i64() {
            Some(0) if !value["data"].is_null() => Ok(value["data"].clone()),
            Some(40101) => Err(AUTH_FAILED.into()),
            Some(40409) => Err("Kimi Code 无法访问目标目录，请检查文件夹及权限后重试。".into()),
            _ => Err(INCOMPATIBLE.into()),
        }
    }

    #[cfg(not(windows))]
    async fn request(
        &self,
        _method: Method,
        _route: &str,
        _body: Option<Value>,
    ) -> Result<Value, String> {
        Err(UNVERIFIED_PEER.into())
    }

    async fn verify(&self) -> Result<(), String> {
        let meta = self.request(Method::GET, "api/v1/meta", None).await?;
        if meta["dangerous_bypass_auth"] != false || !meta["server_version"].is_string() {
            return Err("此 Kimi Code 实例未启用默认鉴权或无法确认鉴权状态，请用不带危险选项的 kimi web 重启服务。".into());
        }
        let schema = self.request(Method::GET, "openapi.json", None).await?;
        check_schema(&schema)
    }

    async fn session_url(&self, target: &Path) -> Result<Url, String> {
        let workspace = self
            .request(
                Method::POST,
                "api/v1/workspaces",
                Some(json!({"root":target})),
            )
            .await?;
        let wid = safe_id(&workspace, "id")?;
        let root = workspace["root"].as_str().ok_or(INCOMPATIBLE)?;
        if target_directory(root)? != target {
            return Err(
                "Kimi Code 返回了其他目录的工作区，已停止打开。请更新 Kimi Code 后重试。".into(),
            );
        }
        let mut route = Url::parse("http://localhost/api/v1/sessions").expect("constant URL");
        route.query_pairs_mut().extend_pairs([
            ("workspace_id", wid.as_str()),
            ("page_size", "1"),
            ("busy", "false"),
            ("include_archive", "false"),
            ("exclude_empty", "false"),
            ("archived_only", "false"),
        ]);
        let sessions = self
            .request(
                Method::GET,
                &format!("api/v1/sessions?{}", route.query().unwrap()),
                None,
            )
            .await?;
        let items = sessions["items"].as_array().ok_or(INCOMPATIBLE)?;
        let session = if let Some(session) = items.first() {
            session.clone()
        } else {
            // An empty session only: no prompt, agent config, trust or execution endpoint.
            self.request(
                Method::POST,
                "api/v1/sessions",
                Some(json!({"workspace_id":wid})),
            )
            .await?
        };
        if session["workspace_id"].as_str() != Some(&wid) {
            return Err(INCOMPATIBLE.into());
        }
        let sid = safe_id(&session, "id")?;
        let mut url = self
            .base
            .join(&format!("sessions/{sid}"))
            .map_err(|_| INCOMPATIBLE)?;
        let fragment =
            reqwest::Url::parse_with_params("http://localhost", [("token", &self.token)])
                .map_err(|_| AUTH_FAILED)?;
        url.set_fragment(fragment.query());
        Ok(url)
    }
}

fn safe_id(value: &Value, key: &str) -> Result<String, String> {
    value[key]
        .as_str()
        .filter(|id| {
            !id.is_empty()
                && id.len() < 256
                && *id != "."
                && *id != ".."
                && id
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'-' || c == b'.')
        })
        .map(str::to_string)
        .ok_or_else(|| INCOMPATIBLE.into())
}

fn schema_node<'a>(schema: &'a Value, node: &'a Value) -> &'a Value {
    // Runtime OpenAPI may inline schemas or use local component references.
    let mut current = node;
    for _ in 0..8 {
        match current["$ref"]
            .as_str()
            .and_then(|r| r.strip_prefix('#'))
            .and_then(|r| schema.pointer(r))
        {
            Some(next) => current = next,
            None => break,
        }
    }
    current
}

fn check_schema(schema: &Value) -> Result<(), String> {
    for (path, field) in [
        ("/api/v1/workspaces", "root"),
        ("/api/v1/sessions", "workspace_id"),
    ] {
        let operation = &schema["paths"][path]["post"];
        let body = schema_node(schema, &operation["requestBody"]);
        let object = schema_node(schema, &body["content"]["application/json"]["schema"]);
        if schema_node(schema, &object["properties"][field])["type"] != "string"
            || object["required"]
                .as_array()
                .is_some_and(|required| required.iter().any(|r| r.as_str() != Some(field)))
        {
            return Err(INCOMPATIBLE.into());
        }
    }
    let params = schema["paths"]["/api/v1/sessions"]["get"]["parameters"]
        .as_array()
        .ok_or(INCOMPATIBLE)?;
    let supported = [
        "workspace_id",
        "page_size",
        "busy",
        "include_archive",
        "exclude_empty",
        "archived_only",
    ];
    for name in supported {
        let expected_type = match name {
            "workspace_id" => "string",
            "page_size" => "integer",
            _ => "boolean",
        };
        if !params.iter().map(|p| schema_node(schema, p)).any(|p| {
            p["name"] == name
                && p["in"] == "query"
                && schema_node(schema, &p["schema"])["type"] == expected_type
        }) {
            return Err(INCOMPATIBLE.into());
        }
    }
    if params
        .iter()
        .any(|p| p["required"] == true && !supported.contains(&p["name"].as_str().unwrap_or("")))
    {
        return Err(INCOMPATIBLE.into());
    }
    Ok(())
}

async fn discover(home: &Path, http: &Client) -> Result<Option<Kimi>, String> {
    let mut failure = None;
    for instance in instances(home)? {
        let base = instance.base_url().expect("validated instance");
        let kimi = Kimi {
            http: http.clone(),
            base,
            token: token(home)?,
            instance,
        };
        match kimi.verify().await {
            Ok(()) => return Ok(Some(kimi)),
            Err(error) if error == UNREACHABLE => continue,
            Err(error) => failure = Some(error),
        }
    }
    if let Some(error) = failure {
        Err(error)
    } else {
        Ok(None)
    }
}

fn kimi_executable() -> Result<PathBuf, String> {
    let mut paths: Vec<PathBuf> = std::env::var_os("PATH")
        .map(|p| {
            std::env::split_paths(&p)
                .filter(|p| p.is_absolute())
                .map(|p| p.join("kimi.exe"))
                .collect()
        })
        .unwrap_or_default();
    paths.push(git::home_dir().join(".kimi-code/bin/kimi.exe"));
    paths.into_iter().find(|p| p.is_file()).ok_or_else(|| "未安装 Kimi Code。请安装 Windows 版 Kimi Code，并将 kimi.exe 加入 PATH 后重启 GitGrove。".into())
}

fn server_command(executable: &Path, home: &Path, port: u16) -> Command {
    let mut command = git::new_cmd(&executable.to_string_lossy());
    command
        .args([
            "web",
            "--host",
            "127.0.0.1",
            "--port",
            &port.to_string(),
            "--no-open",
        ])
        .env("KIMI_CODE_HOME", home)
        .current_dir(home)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    command
}

struct StartedServer(Option<Child>);
impl Drop for StartedServer {
    fn drop(&mut self) {
        if let Some(child) = &mut self.0 {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}
impl StartedServer {
    fn retain(mut self) {
        if let Some(mut child) = self.0.take() {
            std::thread::spawn(move || {
                let _ = child.wait();
            });
        }
    }
}

async fn connect_kimi(home: &Path) -> Result<(Kimi, Option<StartedServer>), String> {
    tokio::time::timeout(Duration::from_secs(45), connect_kimi_inner(home))
        .await
        .map_err(|_| "Kimi Code 连接超时，请检查运行中的 Web 服务后重试。")?
}

async fn connect_kimi_inner(home: &Path) -> Result<(Kimi, Option<StartedServer>), String> {
    let http = local_client()?;
    if let Some(kimi) = discover(home, &http).await? {
        return Ok((kimi, None));
    }
    let executable = kimi_executable()?;
    std::fs::create_dir_all(home).map_err(|_| "无法创建 Kimi Code 数据目录，请检查目录权限。")?;
    // Ask the OS for a free port. Kimi handles the race by trying subsequent ports;
    // readiness always comes from its instance record, never the requested port.
    let socket = std::net::TcpListener::bind("127.0.0.1:0")
        .map_err(|_| "无法分配本地端口，请检查系统网络设置。")?;
    let port = socket.local_addr().map_err(|_| UNREACHABLE)?.port();
    drop(socket);
    let child = server_command(&executable, home, port)
        .spawn()
        .map_err(|_| "Kimi Code Web 启动失败，请运行 kimi web 检查安装后重试。")?;
    let mut owned = StartedServer(Some(child));
    let deadline = tokio::time::Instant::now() + Duration::from_secs(25);
    loop {
        if let Some(kimi) = discover(home, &http).await? {
            return Ok((kimi, Some(owned)));
        }
        if owned
            .0
            .as_mut()
            .unwrap()
            .try_wait()
            .map_err(|_| UNREACHABLE)?
            .is_some()
        {
            return Err(
                "Kimi Code Web 启动后退出，请运行 kimi web 检查安装或端口配置后重试。".into(),
            );
        }
        if tokio::time::Instant::now() >= deadline {
            return Err(
                "Kimi Code Web 启动超时，本次启动的服务已停止。请手动运行 kimi web 后重试。".into(),
            );
        }
        tokio::time::sleep(Duration::from_millis(300)).await;
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenReceipt {
    message: &'static str,
}

#[tauri::command]
pub async fn open_in_agent(path: String, agent: Agent) -> Result<OpenReceipt, String> {
    if !agent_open_support() {
        return Err("本版本的 Codex / Kimi Code 图形界面入口仅在 Windows 启用。".into());
    }
    // A backend guard also protects multiple windows and callers bypassing the UI.
    let _guard = OPEN_LOCK
        .try_lock()
        .map_err(|_| "正在打开工具，请等待当前操作完成。")?;
    let target = target_directory(&path)?;
    match agent {
        Agent::Codex => {
            open_gui_url(&codex_url(&target), agent)?;
            Ok(OpenReceipt {
                message: "已向 Codex 发送打开请求，请在 Codex 中确认项目。",
            })
        }
        Agent::Kimi => {
            let (kimi, owned) = connect_kimi(&kimi_home()?).await?;
            let url = kimi.session_url(&target).await?;
            // Stop if the original process or socket owner changed after the API
            // requests; hold this credential-free connection through handoff.
            #[cfg(windows)]
            let _handoff_peer = kimi.http.verify_peer(&kimi.instance).await?;
            open_gui_url(&url, agent)?;
            if let Some(owned) = owned {
                owned.retain();
            }
            Ok(OpenReceipt {
                message: "Kimi Code 工作区已登记，已请求浏览器打开对应会话。",
            })
        }
    }
}

#[cfg(test)]
mod tests;
