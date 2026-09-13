// GUI adapters. Credentials and authenticated URLs never cross the Tauri boundary.
#[cfg(not(windows))]
use reqwest::Client;
use reqwest::{Method, Url};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use crate::{git, store};

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
    Dsh,
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
            Agent::Codex => "无法打开 Codex：请安装或修复 Codex 桌面应用，确认 Windows 已注册 codex 协议后重试。".to_string(),
            Agent::Kimi => "工作区已登记，但浏览器未能打开。请检查 Windows 默认浏览器，然后重试。".to_string(),
            // 认证 URL 的 query 里带 token，绝不进入返回给前端的错误文本：
            // 只展示源地址，供手动访问。
            Agent::Dsh => format!(
                "DSH 已在本地 {} 启动，但浏览器未能打开。请检查 Windows 默认浏览器，然后重试。",
                url.origin().ascii_serialization()
            ),
        });
    }
    #[cfg(not(windows))]
    {
        let _ = (url, agent);
        Err("本版本的 Codex / Kimi Code / DSH 图形界面入口仅在 Windows 启用。".into())
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

// 子进程经 .cmd shim / npx 启动时（dsh 的情况）：仅 kill 顶层会把 npm/node
// 孙进程留成孤儿继续占用端口。Windows 先用 taskkill /T /F 清掉整棵进程树——
// 树必须在 shim 仍存活时枚举，故调用方保证它先于 kill() 执行。Job Object
// 更彻底，但对常量参数的短生命周期进程，taskkill 已足够（与本仓 NSIS hooks
// 的 KILL_ON_JOB_CLOSE 思路同源）。
fn kill_process_tree(pid: u32) {
    #[cfg(windows)]
    if let Some(root) = std::env::var_os("SystemRoot") {
        let taskkill = PathBuf::from(root).join("System32/taskkill.exe");
        let _ = git::new_cmd(&taskkill.to_string_lossy())
            .args(["/T", "/F", "/PID", &pid.to_string()])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
    #[cfg(not(windows))]
    let _ = pid;
}

struct StartedServer(Option<Child>);
impl StartedServer {
    /// 仅当子进程确认仍存活时才允许按 PID 清理进程树：启动即退（已回收）或
    /// 状态未知时 PID 可能已被 OS 复用，taskkill 会误杀无关进程树。
    fn tree_cleanup_allowed(child: &mut Child) -> bool {
        matches!(child.try_wait(), Ok(None))
    }
}
impl Drop for StartedServer {
    fn drop(&mut self) {
        let Some(child) = &mut self.0 else { return };
        if !Self::tree_cleanup_allowed(child) {
            // 已退出：无树可清，回收退出状态即可。
            let _ = child.wait();
            return;
        }
        kill_process_tree(child.id());
        let _ = child.kill();
        let _ = child.wait();
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

// DSH 没有实例注册表，运行检测只能靠 TCP 端口探测：dsh 进程是 node.exe，
// 按进程名无法与其它 Node 服务区分（进程名探测不可靠）。
fn dsh_candidates(
    windows_layout: bool,
    appdata: Option<PathBuf>,
    path_dirs: Vec<PathBuf>,
) -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    if windows_layout {
        // Windows：全局安装（npm install -g）落在 %APPDATA%\npm\dsh.cmd；
        // 未全局安装时用 npx.cmd 兜底现场拉起 @deepseek-ai/dsh。
        if let Some(npm) = appdata.map(|dir| dir.join("npm")) {
            candidates.push(npm.join("dsh.cmd"));
            candidates.push(npm.join("npx.cmd"));
        }
        for dir in path_dirs {
            candidates.push(dir.join("dsh.cmd"));
            candidates.push(dir.join("npx.cmd"));
        }
    } else {
        // macOS：仅按 PATH 探测 dsh / npx（未实测，等待真实环境验证）。
        for dir in path_dirs {
            candidates.push(dir.join("dsh"));
            candidates.push(dir.join("npx"));
        }
    }
    candidates
}

fn dsh_executable() -> Result<PathBuf, String> {
    let appdata = if cfg!(windows) {
        std::env::var_os("APPDATA").map(PathBuf::from)
    } else {
        None
    };
    let path_dirs = std::env::var_os("PATH")
        .map(|p| {
            std::env::split_paths(&p)
                .filter(|p| p.is_absolute())
                .collect()
        })
        .unwrap_or_default();
    dsh_candidates(cfg!(windows), appdata, path_dirs)
        .into_iter()
        .find(|p| p.is_file())
        .ok_or_else(|| {
            "未找到 DSH 命令行工具。请先运行 npm install -g @deepseek-ai/dsh，或将 npx 加入 PATH 后重启 GitGrove。".into()
        })
}

fn dsh_command(executable: &Path, port: u16) -> Command {
    let mut command = git::new_cmd(&executable.to_string_lossy());
    // npx 兜底：--yes 跳过非交互环境下的安装确认；包名之后原样透传 dsh 参数。
    if executable.file_name().is_some_and(|name| {
        name.eq_ignore_ascii_case("npx.cmd") || name.eq_ignore_ascii_case("npx")
    }) {
        command.args(["--yes", "@deepseek-ai/dsh"]);
    }
    // 目录不作为位置参数传递：dsh 无法携带项目目录，首次由用户在其界面选择。
    // --host 显式固定回环：0.1.x 预览版的默认绑定值可能漂移，就绪探测与
    // 打开的 URL 都依赖回环，不能依赖默认行为。
    command.args([
        "web",
        "--host",
        "127.0.0.1",
        "--port",
        &port.to_string(),
        "--no-open",
    ]);
    // stdout/stderr 由 start_dsh 接管：认证 URL 只由 dsh 在启动输出里公布。
    command.current_dir(git::home_dir());
    command
}

// DSH 默认端口（用户手动 `dsh web` 时的监听口）；本应用 spawn 时避开它选空闲端口。
const DSH_DEFAULT_PORT: u16 = 3080;

// 记录最多保留的实例条数。
const DSH_RECORD_LIMIT: usize = 8;

// 认证 URL 捕获的有界等待（生产值）：URL 行在服务绑定后才打印，正常秒级
// 出现；就绪后再给足慢机余量，超时则回退裸地址（测试传更短值加速）。
const DSH_URL_CAPTURE_TIMEOUT: Duration = Duration::from_secs(30);

// DSH 没有实例注册表（kimi 有 server/instances）：复用与本应用自己起的实例
// 清理只能依据 spawn 时记录的端口与端口占用者 PID。记录里的端口都来自本应用
// 发起的 dsh，并在启动时就核对过占用者（#84 据此只清理自己留下的实例）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct DshRecordEntry {
    port: u16,
    /// 启动时核对到的监听进程 PID；0 表示旧版本记录（只有端口）或当时核对不出来。
    pid: u32,
    /// 认证链接已交给浏览器：它的 cookie 在浏览器里有效，无凭据探测到的 401
    /// 不代表不可用；跨重启也必须放过，否则重启后会误杀用户正在用的实例。
    opened: bool,
}

#[derive(Deserialize, Serialize)]
#[serde(untagged)]
enum DshRecordItem {
    /// v1.2.6 及更早的格式：数组里直接是端口。
    Port(u16),
    Owned {
        port: u16,
        pid: u32,
        #[serde(default)]
        opened: bool,
    },
}

fn dsh_port_record() -> PathBuf {
    store::app_data_dir().join("dsh-web-ports.json")
}

fn dsh_read_entries(record: &Path) -> Vec<DshRecordEntry> {
    std::fs::read_to_string(record)
        .ok()
        .and_then(|text| serde_json::from_str::<Vec<DshRecordItem>>(&text).ok())
        .unwrap_or_default()
        .into_iter()
        .map(|item| match item {
            DshRecordItem::Port(port) => DshRecordEntry {
                port,
                pid: 0,
                opened: false,
            },
            DshRecordItem::Owned { port, pid, opened } => DshRecordEntry { port, pid, opened },
        })
        .filter(|entry| entry.port != 0)
        .take(DSH_RECORD_LIMIT)
        .collect()
}

fn dsh_write_entries(record: &Path, entries: &[DshRecordEntry]) {
    let items: Vec<DshRecordItem> = entries
        .iter()
        .map(|entry| DshRecordItem::Owned {
            port: entry.port,
            pid: entry.pid,
            opened: entry.opened,
        })
        .collect();
    if let Ok(text) = serde_json::to_string(&items) {
        let _ = std::fs::write(record, text);
    }
}

// 认证链接交付浏览器后落盘标记：该实例的 cookie 在浏览器里（30 天），探测到的
// 401 只说明本应用没有凭据，不代表用户打不开——清理必须放过它。记录里一定有
// 这个端口（spawn 成功即写入）；写不出记录时不新建条目，那一次打开就少了保护。
fn dsh_mark_opened(record: &Path, port: u16) {
    let mut entries = dsh_read_entries(record);
    match entries.iter_mut().find(|entry| entry.port == port) {
        Some(entry) if !entry.opened => entry.opened = true,
        _ => return,
    }
    dsh_write_entries(record, &entries);
}

fn dsh_record_port(record: &Path, port: u16, pid: u32) {
    let mut entries = dsh_read_entries(record);
    entries.retain(|existing| existing.port != port);
    // 新实例没有浏览器 cookie，opened 从 false 开始（同一端口的上一条记录作废）。
    entries.insert(
        0,
        DshRecordEntry {
            port,
            pid,
            opened: false,
        },
    );
    entries.truncate(DSH_RECORD_LIMIT);
    dsh_write_entries(record, &entries);
}

// 复用候选顺序：默认端口（用户手动 dsh web 的监听口）优先于本应用历史
// 记录。历史记录可能指向遗留实例——其认证 token 已随旧进程消亡、浏览器
// 也没有它的认证 cookie（#71：:3080 在手却打开了 54964 遗留实例的认证页）；
// 而用户手动实例的浏览器会话通常已持有 cookie，裸地址即可进入。清理过一遍
// 后仍留在记录里的，只有健康应答（可复用）的实例。
fn dsh_reuse_candidates(entries: &[DshRecordEntry]) -> Vec<u16> {
    let mut candidates = vec![DSH_DEFAULT_PORT];
    for entry in entries {
        if !candidates.contains(&entry.port) {
            candidates.push(entry.port);
        }
    }
    candidates
}

// 端口占用者核对只在 Windows 有实现（DSH 图形界面入口同样只在 Windows 启用）；
// 其它平台返回 None，清理退化为仅删记录、绝不动进程。
#[cfg(windows)]
fn dsh_listener_pid(port: u16) -> Option<u32> {
    let pid = transport::loopback_listener(port).ok().flatten()?;
    // 端口可能被别的程序或别的用户的进程占着：镜像须为 node.exe（dsh 的运行体，
    // .cmd shim 与 npx 最终都落到它），且进程属于当前用户，才认作本应用的实例。
    let image = transport::same_user_image_name(pid).ok().flatten()?;
    let name = Path::new(&image).file_name()?.to_string_lossy().into_owned();
    name.eq_ignore_ascii_case("node.exe").then_some(pid)
}

#[cfg(not(windows))]
fn dsh_listener_pid(_port: u16) -> Option<u32> {
    None
}

async fn dsh_first_live_port(candidates: Vec<u16>) -> Option<u16> {
    for port in candidates {
        let probe = tokio::time::timeout(
            Duration::from_millis(400),
            tokio::net::TcpStream::connect(("127.0.0.1", port)),
        )
        .await;
        if matches!(probe, Ok(Ok(_))) {
            return Some(port);
        }
    }
    None
}

// 无凭据的健康探测：dsh 的认证栅栏（#71 已核对源码 packages/client/connection/
// src/browser-auth.ts）在没有认证 cookie 时对根路径返回 401。本应用不读 secret、
// 不伪造 cookie，只发一个不带任何凭据的 GET，按状态行判断实例是否可用。
const DSH_HEALTH_TIMEOUT: Duration = Duration::from_millis(700);
const DSH_STATUS_HEAD_LIMIT: usize = 512;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DshHealth {
    /// 2xx/3xx：无需认证即可使用，保留复用。
    Serving,
    /// 401/403：dsh 存活但要求认证 cookie，本应用拿不到——#84 的僵尸实例。
    AuthGate,
    /// 监听存在但不应答 HTTP，或返回其它状态码：异常实例。
    Broken,
    /// 端口没有监听。
    Dead,
}

fn dsh_health_of_status_line(line: &str) -> DshHealth {
    let code = line
        .strip_prefix("HTTP/")
        .and_then(|rest| rest.split_whitespace().nth(1))
        .and_then(|code| code.parse::<u16>().ok());
    match code {
        Some(200..=399) => DshHealth::Serving,
        Some(401 | 403) => DshHealth::AuthGate,
        _ => DshHealth::Broken,
    }
}

async fn dsh_health(port: u16) -> DshHealth {
    let Ok(Ok(mut stream)) = tokio::time::timeout(
        DSH_HEALTH_TIMEOUT,
        tokio::net::TcpStream::connect(("127.0.0.1", port)),
    )
    .await
    else {
        return DshHealth::Dead;
    };
    let request = format!(
        "GET / HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nAccept: */*\r\nConnection: close\r\n\r\n"
    );
    if tokio::time::timeout(DSH_HEALTH_TIMEOUT, stream.write_all(request.as_bytes()))
        .await
        .is_err()
    {
        return DshHealth::Broken;
    }
    // 只读状态行（封顶 512B）：正文可能很大或流式，读它与判定无关。
    let mut head = Vec::new();
    let mut chunk = [0u8; 64];
    while head.len() < DSH_STATUS_HEAD_LIMIT && !head.contains(&b'\n') {
        match tokio::time::timeout(DSH_HEALTH_TIMEOUT, stream.read(&mut chunk)).await {
            Ok(Ok(read)) if read > 0 => head.extend_from_slice(&chunk[..read]),
            _ => break,
        }
    }
    let head = String::from_utf8_lossy(&head);
    let line = head.split(['\r', '\n']).next().unwrap_or_default();
    if line.is_empty() {
        DshHealth::Broken
    } else {
        dsh_health_of_status_line(line)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DshCleanup {
    /// 健康可复用：记录与进程都保留。
    Keep,
    /// 只删掉这条记录：端口已无监听、换了主人或占用者身份核对不出来。
    Forget,
    /// 记录过、核对为本实例、健康检查也失败：按 PID 清进程树并删记录。
    Kill(u32),
}

// 纯决策，副作用由调用方执行。三条同时成立才清理：本应用记录过这个端口 +
// 端口占用者是本用户的 node 进程（dsh 的运行体）+ 健康检查失败。任何一条
// 不成立都只删记录、绝不动进程。
fn dsh_cleanup_decision(
    entry: DshRecordEntry,
    health: DshHealth,
    owner: Option<u32>,
) -> DshCleanup {
    if health == DshHealth::Dead {
        return DshCleanup::Forget;
    }
    let Some(owner) = owner else {
        return DshCleanup::Forget;
    };
    if entry.pid != 0 && entry.pid != owner {
        // 记录里的实例已退出、端口换了主人：不是本应用起的那个进程。
        return DshCleanup::Forget;
    }
    // pid=0 只有 v1.2.6 及更早的记录会给出（那时记录不带 PID）：判据退化为
    // 「占用者是本用户的 node 进程 + 健康检查失败」——正是 #84 要清的遗留实例，
    // 因此保留这条路径；写回时补上核对到的 PID（可进一步收紧）留待后续。
    match health {
        DshHealth::Serving => DshCleanup::Keep,
        _ => DshCleanup::Kill(owner),
    }
}

const DSH_CLEANUP_TIMEOUT: Duration = Duration::from_secs(8);

// 清理本应用记录过的 DSH 实例：健康检查失败的（认证栅栏 / 异常应答）按 PID
// 清掉，记录里只留仍健康可复用的、默认端口那条、以及认证链接已交付浏览器的
// 实例。返回清理后的记录。
async fn dsh_cleanup(record: &Path, entries: &[DshRecordEntry]) -> Vec<DshRecordEntry> {
    let mut kept = Vec::new();
    for entry in entries {
        // 默认端口是用户手动 dsh web 的监听口，opened 的实例认证链接已交付浏览器
        // （cookie 可用）：两者都不探测、不清理——探测它们的 401 只反映本应用没有
        // 凭据，据此清理会误杀用户正在用的实例。
        if entry.port == DSH_DEFAULT_PORT || entry.opened {
            kept.push(*entry);
            continue;
        }
        let health = dsh_health(entry.port).await;
        let owner = if health == DshHealth::Dead {
            None
        } else {
            dsh_listener_pid(entry.port)
        };
        match dsh_cleanup_decision(*entry, health, owner) {
            DshCleanup::Keep => kept.push(*entry),
            DshCleanup::Forget => {}
            DshCleanup::Kill(pid) => kill_process_tree(pid),
        }
    }
    if kept.len() != entries.len() {
        dsh_write_entries(record, &kept);
    }
    kept
}

// 清理是打开路径上的前置步骤，整体设上限：超时按原记录继续（复用与新建两条
// 路都还能走通），不写回半清理状态。
async fn dsh_cleanup_bounded(record: &Path) -> Vec<DshRecordEntry> {
    let entries = dsh_read_entries(record);
    if entries.is_empty() {
        return entries;
    }
    match tokio::time::timeout(DSH_CLEANUP_TIMEOUT, dsh_cleanup(record, &entries)).await {
        Ok(kept) => kept,
        Err(_) => entries,
    }
}

const DSH_READY_TIMEOUT: Duration = Duration::from_secs(120);

// 行内第一个 http(s) URL 的宽松提取：dsh 0.1.x 是预览版，输出格式可能漂移，
// 不做整行正则，只找 http 起头、以空白/引号为界的片段并容忍结尾标点。
// `to_ascii_lowercase` 保持字节长度不变，索引在两个字符串间通用。
fn first_http_url(line: &str) -> Option<String> {
    let lower = line.to_ascii_lowercase();
    for (at, _) in lower.match_indices("http") {
        let candidate = line[at..]
            .split(|c: char| c.is_whitespace() || c == '"' || c == '\'')
            .next()
            .unwrap_or_default()
            .trim_end_matches(['.', ',', ';', ':', ')', '）', '。']);
        if let Ok(parsed) = Url::parse(candidate) {
            if matches!(parsed.scheme(), "http" | "https") {
                return Some(candidate.to_string());
            }
        }
    }
    None
}

// 从捕获的输出行里挑出本实例的认证 URL。候选必须指向本实例：回环 + 本
// 端口优先，其次本端口任意 host（dsh 在 0.0.0.0 绑定时会附打印 LAN 地址，
// 与本实例同机）；输出里可能出现的无关链接（文档地址等）一律不采纳，由
// 调用方回退本端口的裸地址，而不是误开无关页面。
fn pick_dsh_url(lines: &[String], port: u16) -> Option<String> {
    let mut on_port = None;
    for line in lines {
        let Some(url) = first_http_url(line) else {
            continue;
        };
        let Ok(parsed) = Url::parse(&url) else {
            continue;
        };
        if parsed.port() != Some(port) {
            continue;
        }
        let loopback = matches!(
            parsed.host_str(),
            Some("127.0.0.1") | Some("localhost") | Some("::1")
        );
        if loopback {
            return Some(url);
        }
        if on_port.is_none() {
            on_port = Some(url);
        }
    }
    on_port
}

// 持续排空子进程一路输出直到 EOF：认证 URL 行随时可能出现，而不排空会让
// dsh 在 OS 管道缓冲写满后卡死。行数封顶，超出仅丢弃；内容含 token，绝不
// 进日志或任何返回给前端的文本（文件头契约），捕获窗口结束后即被丢弃。
fn dsh_drain_output<R: std::io::Read + Send + 'static>(stream: R, sink: Arc<Mutex<Vec<String>>>) {
    let mut reader = BufReader::new(stream);
    let mut raw = Vec::new();
    loop {
        raw.clear();
        match reader.read_until(b'\n', &mut raw) {
            Ok(0) | Err(_) => return,
            Ok(_) => {
                let line = String::from_utf8_lossy(&raw)
                    .trim_end_matches(['\r', '\n'])
                    .to_string();
                let mut lines = sink.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
                if lines.len() < 512 {
                    lines.push(line);
                }
            }
        }
    }
}

// 子进程输出的有界驻留，仅供启动窗口内的认证 URL 提取。
struct DshCapture {
    lines: Arc<Mutex<Vec<String>>>,
}

impl DshCapture {
    fn attach(child: &mut Child) -> Self {
        let lines = Arc::new(Mutex::new(Vec::new()));
        // stdout/stderr 类型不同，先擦除为统一的 Read 再交给排空线程。
        for stream in [
            child
                .stdout
                .take()
                .map(|s| Box::new(s) as Box<dyn std::io::Read + Send>),
            child
                .stderr
                .take()
                .map(|s| Box::new(s) as Box<dyn std::io::Read + Send>),
        ]
        .into_iter()
        .flatten()
        {
            let sink = lines.clone();
            std::thread::spawn(move || dsh_drain_output(stream, sink));
        }
        Self { lines }
    }

    fn authenticated_url(&self, port: u16) -> Option<String> {
        pick_dsh_url(
            &self
                .lines
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner()),
            port,
        )
    }
}

async fn start_dsh(
    executable: &Path,
    ready_timeout: Duration,
    url_wait: Duration,
) -> Result<(u16, StartedServer, Option<String>), String> {
    // 向系统要一个空闲端口（参照 connect_kimi）：默认 3080 可能被占用。
    // 端口释放与 dsh 实际绑定之间存在竞窗，桌面环境下足够小。
    let socket = std::net::TcpListener::bind("127.0.0.1:0")
        .map_err(|_| "无法分配本地端口，请检查系统网络设置。")?;
    let port = socket
        .local_addr()
        .map_err(|_| "无法分配本地端口，请检查系统网络设置。")?
        .port();
    drop(socket);
    let mut command = dsh_command(executable, port);
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command
        .spawn()
        .map_err(|_| "DSH 启动失败，请检查 Node.js 与 npm 安装后重试。")?;
    let capture = DshCapture::attach(&mut child);
    let mut owned = StartedServer(Some(child));
    // URL 行在服务绑定之后打印：就绪时刻才开始计捕获窗口（url_wait），慢启动
    // 也拿满余量；窗口内没等到 URL 就回退裸地址。
    let mut capture_deadline: Option<tokio::time::Instant> = None;
    let ready_deadline = tokio::time::Instant::now() + ready_timeout;
    loop {
        if let Some(url) = capture.authenticated_url(port) {
            return Ok((port, owned, Some(url)));
        }
        // 就绪检测同样是 TCP 端口探测，进程名不可靠。
        let ready = tokio::net::TcpStream::connect(("127.0.0.1", port))
            .await
            .is_ok();
        if ready {
            let deadline = capture_deadline.get_or_insert(tokio::time::Instant::now() + url_wait);
            if tokio::time::Instant::now() >= *deadline {
                return Ok((port, owned, None));
            }
        }
        if owned
            .0
            .as_mut()
            .unwrap()
            .try_wait()
            .map_err(|_| "DSH 启动失败，请检查 Node.js 与 npm 安装后重试。")?
            .is_some()
        {
            return Err("DSH 启动后即退出，请检查 npm 全局安装或网络后重试。".into());
        }
        if !ready && tokio::time::Instant::now() >= ready_deadline {
            return Err("DSH 启动超时，本次启动的进程已停止。请检查网络或 npx 安装后重试。".into());
        }
        tokio::time::sleep(Duration::from_millis(150)).await;
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
        return Err("本版本的 Codex / Kimi Code / DSH 图形界面入口仅在 Windows 启用。".into());
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
        Agent::Dsh => {
            // dsh 无法携带项目目录：打开 web 首页，首次由用户在 DSH 界面选择
            // 项目目录（dsh 会记住）。绝不走 launch.rs 的编辑器 spec 模式——
            // 那条路会把目录当位置参数传入并被 dsh 静默忽略。
            //
            // 认证模型（dsh 源码 packages/client/connection/src/browser-auth.ts）：
            // 根路径 ?token= 的 launch token 由进程内随机生成、仅存内存、随
            // 进程消亡，绝不落盘（~/.dsh/.credentials.yaml 只存 cookie 签名
            // secret，无法据此重建 URL）；token 换发 30 天 HttpOnly 认证
            // cookie。因此：自己 spawn 的实例从启动输出捕获带 token 的 URL
            // 打开（100% 可用）；复用实例拿不到 token，只能开裸地址，依赖
            // 浏览器里已有的认证 cookie（用户此前打开过 dsh 打印的链接，或
            // 本会话曾以认证链接打开过），没有 cookie 时 dsh 返回 401 认证
            // 页，由返回文案给出处理办法。不做读取 secret 伪造 cookie 等
            // 扩大化方案。
            //
            // 打开前先做一次保守清理（#84）：本应用记录过的端口若健康检查失败
            // （无 cookie 的 401 认证栅栏 / 不应答），那是自己留下的、token 已
            // 不可得的遗留实例，按占用者 PID 清掉，让本次打开走带 token 的新实例；
            // 用户手动起的实例（默认端口或不在记录里）、以及认证链接已交付浏览器
            // 的实例（记录里 opened）一律不动。
            let record = dsh_port_record();
            let reusable = dsh_cleanup_bounded(&record).await;
            if let Some(port) = dsh_first_live_port(dsh_reuse_candidates(&reusable)).await {
                let url = Url::parse(&format!("http://127.0.0.1:{port}/")).expect("constant URL");
                open_gui_url(&url, agent)?;
                return Ok(OpenReceipt {
                    message: "DSH 已在本地运行，已请求浏览器打开本地服务。若浏览器显示认证页，请关闭该 DSH 实例后重试，GitGrove 会重新启动并自动携带认证链接。",
                });
            }
            let executable = dsh_executable()?;
            let (port, owned, authenticated) =
                start_dsh(&executable, DSH_READY_TIMEOUT, DSH_URL_CAPTURE_TIMEOUT).await?;
            dsh_record_port(&record, port, dsh_listener_pid(port).unwrap_or(0));
            // 常驻进程先转交后台再打开浏览器：浏览器失败时服务仍在，
            // 错误文案里的本地地址可手动访问。
            owned.retain();
            match authenticated {
                // URL 来自 dsh 自己的 stdout：token 不入日志、不入错误文案。
                Some(url) => {
                    let url =
                        Url::parse(&url).map_err(|_| "DSH 打印的认证链接无法解析，请重试。")?;
                    open_gui_url(&url, agent)?;
                    // 认证链接已交给浏览器：这个实例跨重启都受保护，不再进入清理。
                    dsh_mark_opened(&record, port);
                    Ok(OpenReceipt {
                        message:
                            "DSH 已启动，已通过认证链接打开本地服务。首次使用需在 DSH 界面选择项目目录。",
                    })
                }
                None => {
                    let url =
                        Url::parse(&format!("http://127.0.0.1:{port}/")).expect("constant URL");
                    open_gui_url(&url, agent)?;
                    Ok(OpenReceipt {
                        message:
                            "DSH 已启动，但未捕获到认证链接，已打开服务首页。若浏览器显示认证页，请关闭 DSH 后重试。",
                    })
                }
            }
        }
    }
}

#[cfg(test)]
mod tests;
