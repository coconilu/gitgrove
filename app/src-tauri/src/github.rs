// GitHub API：token 只在 Rust 侧；仓库列表用 GraphQL 聚合，其余 REST

use std::io::Read as _;
use std::process::{Command, Output, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use serde::Serialize;
use serde_json::{json, Value};
use tauri::State;

use crate::AppState;

const KEYRING_SERVICE: &str = "gh-projects";
const KEYRING_USER: &str = "github-token";

/// 项目无 GitHub 身份时，按 owner/repo 调 API 的 command 统一返回该错误（前端据此降级）
fn require_repo(owner: &str, repo: &str) -> Result<(), String> {
    if owner.is_empty() || repo.is_empty() {
        Err(crate::projects::ERR_NO_GITHUB.to_string())
    } else {
        Ok(())
    }
}

fn keyring_entry() -> Result<keyring::Entry, String> {
    keyring::Entry::new(KEYRING_SERVICE, KEYRING_USER).map_err(|e| format!("keyring 不可用: {e}"))
}

/// 取 token：内存 → keyring（不含 gh CLI 兜底；auth_status 需要区分来源）
fn stored_token(state: &AppState) -> Option<String> {
    if let Some(t) = state.token.lock().unwrap().clone() {
        return Some(t);
    }
    if let Ok(entry) = keyring_entry() {
        if let Ok(t) = entry.get_password() {
            if !t.is_empty() {
                *state.token.lock().unwrap() = Some(t.clone());
                return Some(t);
            }
        }
    }
    None
}

/// 取 token：内存 → keyring → gh CLI（成功后写回 keyring）。
/// gh CLI 兜底必须在这里就有：pm_sync_github 等 command 不保证 auth_status 先行
/// 运行过（#66 排查记录：keyring 为空时所有命令直接「未登录」失败，即便本机
/// gh 已登录）；错误文案给出两种补救路径，前端错误横幅直接展示。
pub fn ensure_token(state: &AppState) -> Result<String, String> {
    if let Some(t) = stored_token(state) {
        return Ok(t);
    }
    if let Some(t) = try_gh_cli(state) {
        return Ok(t);
    }
    Err("未登录：应用内没有存储的 GitHub token，gh CLI 也未提供（gh auth login 或在应用内登录）".into())
}

/// gh CLI 兜底取 token 的总等待上限。#74：try_gh_cli 曾用 Command::output()
/// 无界阻塞，是同步链路（pm_sync_github → ensure_token）唯一的无界等待点——
/// 用户机上 gh 卡住（凭据助手挂起、杀软扫描等）时，看板同步就永远等不到结果。
/// `gh auth token` 正常耗时可忽略（实测 ~0.1s），5s 足以覆盖慢盘等合理抖动。
const GH_CLI_TIMEOUT: Duration = Duration::from_secs(5);

/// spawn 命令并至多等待 `timeout`：正常退出返回 Some(Output)，超时或状态未知
/// 时杀掉进程树返回 None。#74：这是 try_gh_cli 从 Command::output() 无界阻塞
/// 改出来的有界版本；保持同步签名，因为 ensure_token 的调用方分布在
/// projects.rs / pm 等非 async 上下文里，async 化超出本仓改动范围。
/// stdout/stderr 各由独立线程排空（进程终止后管道 EOF，线程随即结束），
/// 避免大输出塞满管道把子进程卡死在 write 上；stdin 置空让交互式提示立即
/// 失败而不是无限等待。
fn run_bounded(mut cmd: Command, timeout: Duration) -> Option<Output> {
    let started = Instant::now();
    let mut child = cmd
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .ok()?;
    let (tx_out, rx_out) = mpsc::channel();
    let mut stdout = child.stdout.take()?;
    std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = stdout.read_to_end(&mut buf);
        let _ = tx_out.send(buf);
    });
    let (tx_err, rx_err) = mpsc::channel();
    let mut stderr = child.stderr.take()?;
    std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = stderr.read_to_end(&mut buf);
        let _ = tx_err.send(buf);
    });
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) if Instant::now() >= started + timeout => break None,
            Ok(None) => std::thread::sleep(Duration::from_millis(20)),
            Err(_) => break None, // 状态未知：按失败处理（不敢按 PID 清树）
        }
    };
    let Some(status) = status else {
        kill_tree_bounded(&mut child);
        return None;
    };
    // 进程已退出，管道随之关闭，1s 只是防极端残留的兜底
    Some(Output {
        status,
        stdout: rx_out.recv_timeout(Duration::from_secs(1)).unwrap_or_default(),
        stderr: rx_err.recv_timeout(Duration::from_secs(1)).unwrap_or_default(),
    })
}

/// 超时清理：仅当子进程确认仍存活时才按 PID 清理进程树（对齐 agents.rs
/// StartedServer::tree_cleanup_allowed——已回收或状态未知时 PID 可能已被 OS
/// 复用，taskkill 会误杀无关进程树），再 kill + wait 收尸；返回是否执行了强杀。
fn kill_tree_bounded(child: &mut std::process::Child) -> bool {
    if !matches!(child.try_wait(), Ok(None)) {
        return false;
    }
    #[cfg(windows)]
    if let Some(root) = std::env::var_os("SystemRoot") {
        let taskkill = std::path::PathBuf::from(root).join("System32/taskkill.exe");
        let _ = crate::git::new_cmd(&taskkill.to_string_lossy())
            .args(["/T", "/F", "/PID", &child.id().to_string()])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
    let _ = child.kill(); // 句柄级兜底（不依赖 PID，无复用风险）
    let _ = child.wait();
    true
}

/// gh CLI 兜底取 token。#74：走 run_bounded 有界等待（GH_CLI_TIMEOUT），
/// 超时按失败处理返回 None，ensure_token 随即报「未登录」，前端横幅可展示，
/// 同步不再被挂死的 gh 进程无限拖住。
fn try_gh_cli(state: &AppState) -> Option<String> {
    let mut cmd = crate::git::new_cmd("gh");
    cmd.args(["auth", "token"]);
    let out = run_bounded(cmd, GH_CLI_TIMEOUT)?;
    if !out.status.success() {
        return None;
    }
    let t = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if t.is_empty() {
        return None;
    }
    // 写回 keyring
    if let Ok(entry) = keyring_entry() {
        let _ = entry.set_password(&t);
    }
    *state.token.lock().unwrap() = Some(t.clone());
    Some(t)
}

/// 直连优先、系统代理兜底的 HTTP 客户端（本机代理可能时开时关，单侧失败自动换另一侧）
pub struct Http {
    direct: reqwest::Client,
    proxied: Option<reqwest::Client>,
}

impl Http {
    pub fn new() -> Http {
        let mk = |b: reqwest::ClientBuilder| {
            b.user_agent("gh-projects/0.1")
                // #61：HTTP 调用必须带上界。此前只有请求级 timeout（20s）而无
                // connect_timeout——TCP 黑洞（直连 GitHub 被丢包、代理假活）要白等满
                // 20s 才算失败再换代理侧重试，pm_sync_github 一次同步最坏可拖几十秒；
                // 配合旧版 PmPanel「渲染前先 await 同步」，看板就永远停在「正在加载」。
                // 现在前端已改为本地数据先渲染、同步后台化（不阻断），这里再把单请求
                // 耗时兜死：connect 5s 让黑洞快速失败、尽早切到另一侧 client（send()
                // 对 connect/timeout 失败会重试一次）；总时长 10s——reqwest 的 timeout
                // 是请求级 deadline，覆盖 DNS/TLS/响应头/读 body，不是空闲超时。
                // pm_sync_github 与 list_issues/list_prs 等全部 GitHub 调用共用本客户端。
                .connect_timeout(std::time::Duration::from_secs(5))
                .timeout(std::time::Duration::from_secs(10))
                .build()
                .expect("failed to build http client")
        };
        // no_proxy()：禁用系统代理自动检测，保证这是纯直连 client
        let direct = mk(reqwest::Client::builder().no_proxy());
        let proxied = crate::git::system_proxy().and_then(|p| {
            reqwest::Proxy::all(&p).ok().map(|proxy| mk(reqwest::Client::builder().proxy(proxy)))
        });
        Http { direct, proxied }
    }

    /// 连接/超时失败时换另一侧 client 重试一次
    pub(crate) async fn send(
        &self,
        mk_req: impl Fn(&reqwest::Client) -> reqwest::RequestBuilder,
    ) -> Result<reqwest::Response, String> {
        match mk_req(&self.direct).send().await {
            Ok(r) => Ok(r),
            Err(e) if (e.is_connect() || e.is_timeout()) && self.proxied.is_some() => {
                mk_req(self.proxied.as_ref().unwrap())
                    .send()
                    .await
                    .map_err(|e2| format!("网络错误: {e2}"))
            }
            Err(e) => Err(format!("网络错误: {e}")),
        }
    }
}

pub async fn gh_get(http: &Http, token: &str, path: &str, query: &[(&str, &str)]) -> Result<Value, String> {
    let url = format!("https://api.github.com{path}");
    let resp = http
        .send(|c| {
            c.get(&url)
                .bearer_auth(token)
                .query(query)
                .header("Accept", "application/vnd.github+json")
                .header("X-GitHub-Api-Version", "2022-11-28")
        })
        .await?;
    let status = resp.status();
    let text = resp.text().await.map_err(|e| format!("读取响应失败: {e}"))?;
    if !status.is_success() {
        return Err(format!("GitHub API {status}: {}", &text[..text.len().min(300)]));
    }
    serde_json::from_str(&text).map_err(|e| format!("解析响应失败: {e}"))
}

pub async fn gh_post(http: &Http, token: &str, path: &str, body: Value) -> Result<Value, String> {
    let url = format!("https://api.github.com{path}");
    let resp = http
        .send(|c| {
            c.post(&url)
                .bearer_auth(token)
                .header("Accept", "application/vnd.github+json")
                .header("X-GitHub-Api-Version", "2022-11-28")
                .json(&body)
        })
        .await?;
    let status = resp.status();
    let text = resp.text().await.map_err(|e| format!("读取响应失败: {e}"))?;
    if !status.is_success() {
        return Err(format!("GitHub API {status}: {}", &text[..text.len().min(300)]));
    }
    if text.trim().is_empty() {
        Ok(Value::Null)
    } else {
        serde_json::from_str(&text).map_err(|e| format!("解析响应失败: {e}"))
    }
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct AuthState2 {
    pub logged_in: bool,
    pub login: String,
    pub name: String,
    pub avatar_url: String,
    pub source: String,
    /// 当前 token 是否具备 project scope；None = 无法判断（响应无 X-OAuth-Scopes 头，如 fine-grained PAT）
    pub has_project_scope: Option<bool>,
}

fn logged_out() -> AuthState2 {
    AuthState2 {
        logged_in: false,
        login: String::new(),
        name: String::new(),
        avatar_url: String::new(),
        source: String::new(),
        has_project_scope: None,
    }
}

/// 从 /user 响应头 X-OAuth-Scopes 解析是否具备 project scope
fn has_project_scope(headers: &reqwest::header::HeaderMap) -> Option<bool> {
    let v = headers.get("x-oauth-scopes")?.to_str().ok()?;
    Some(v.split(',').map(|s| s.trim()).any(|s| s == "project" || s == "read:project"))
}

async fn fetch_viewer(http: &Http, token: &str, source: &str) -> Result<AuthState2, String> {
    // 不用 gh_get：需要同时读响应头里的 X-OAuth-Scopes
    let resp = http
        .send(|c| {
            c.get("https://api.github.com/user")
                .bearer_auth(token)
                .header("Accept", "application/vnd.github+json")
                .header("X-GitHub-Api-Version", "2022-11-28")
        })
        .await?;
    let status = resp.status();
    let project_scope = has_project_scope(resp.headers());
    let text = resp.text().await.map_err(|e| format!("读取响应失败: {e}"))?;
    if !status.is_success() {
        return Err(format!("GitHub API {status}: {}", &text[..text.len().min(300)]));
    }
    let v: Value = serde_json::from_str(&text).map_err(|e| format!("解析响应失败: {e}"))?;
    Ok(AuthState2 {
        logged_in: true,
        login: v["login"].as_str().unwrap_or("").to_string(),
        name: v["name"].as_str().unwrap_or("").to_string(),
        avatar_url: v["avatar_url"].as_str().unwrap_or("").to_string(),
        source: source.to_string(),
        has_project_scope: project_scope,
    })
}

#[tauri::command]
pub async fn auth_status(state: State<'_, AppState>) -> Result<AuthState2, String> {
    let record = |me: &AuthState2| *state.has_project_scope.lock().unwrap() = me.has_project_scope;
    // 失败 / 未登录路径统一清空，避免残留旧 token 的 scope 状态
    let out = |state: &AppState| {
        *state.has_project_scope.lock().unwrap() = None;
        logged_out()
    };
    // 内存 / keyring 的存储 token。失效（过期/被撤销）不能直接判未登录：
    // keyring 可能残留旧 token，而 gh CLI 往往持有新 token，可救回登录态（#66）
    if let Some(t) = stored_token(&state) {
        if let Ok(me) = fetch_viewer(&state.http, &t, "keyring").await {
            record(&me);
            return Ok(me);
        }
        // 清内存槽，避免遮蔽下面 try_gh_cli 的写回
        *state.token.lock().unwrap() = None;
    }
    // gh CLI 兜底（成功后写回 keyring，之后不依赖 gh 也在）
    if let Some(t) = try_gh_cli(&state) {
        if let Ok(me) = fetch_viewer(&state.http, &t, "gh CLI").await {
            record(&me);
            return Ok(me);
        }
    }
    Ok(out(&state))
}

#[tauri::command]
pub async fn login_pat(state: State<'_, AppState>, token: String) -> Result<AuthState2, String> {
    let me = fetch_viewer(&state.http, &token, "PAT").await?;
    let entry = keyring_entry()?;
    entry.set_password(&token).map_err(|e| format!("写入凭据管理器失败: {e}"))?;
    *state.token.lock().unwrap() = Some(token);
    *state.has_project_scope.lock().unwrap() = me.has_project_scope;
    *state.projects_v2_cache.lock().unwrap() = None;
    Ok(me)
}

#[tauri::command]
pub async fn logout(state: State<'_, AppState>) -> Result<(), String> {
    *state.token.lock().unwrap() = None;
    *state.has_project_scope.lock().unwrap() = None;
    *state.projects_v2_cache.lock().unwrap() = None;
    if let Ok(entry) = keyring_entry() {
        let _ = entry.delete_credential();
    }
    Ok(())
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct RepoInfo {
    pub name: String,
    pub name_with_owner: String,
    pub is_private: bool,
    pub is_fork: bool,
    pub parent: Option<String>,
    pub language: Option<String>,
    pub description: Option<String>,
    pub pushed_at: Option<String>,
    pub url: String,
}

/// GraphQL 一次拉全我的仓库（含私有），按 pushed_at 排序，自动翻页
#[tauri::command]
pub async fn list_my_repos(state: State<'_, AppState>) -> Result<Vec<RepoInfo>, String> {
    let token = ensure_token(&state)?;
    let query = r#"query($cursor:String){viewer{repositories(first:100,after:$cursor,orderBy:{field:PUSHED_AT,direction:DESC},affiliations:[OWNER,COLLABORATOR,ORGANIZATION_MEMBER]){nodes{name nameWithOwner isPrivate isFork url description pushedAt primaryLanguage{name} parent{nameWithOwner}} pageInfo{hasNextPage endCursor}}}}"#;
    let mut out: Vec<RepoInfo> = Vec::new();
    let mut cursor: Option<String> = None;
    for _ in 0..10 {
        let body = json!({"query": query, "variables": {"cursor": cursor}});
        let v = gh_post(&state.http, &token, "/graphql", body).await?;
        if let Some(errs) = v.get("errors") {
            return Err(format!("GraphQL 错误: {errs}"));
        }
        let repos = &v["data"]["viewer"]["repositories"];
        for n in repos["nodes"].as_array().cloned().unwrap_or_default() {
            out.push(RepoInfo {
                name: n["name"].as_str().unwrap_or("").into(),
                name_with_owner: n["nameWithOwner"].as_str().unwrap_or("").into(),
                is_private: n["isPrivate"].as_bool().unwrap_or(false),
                is_fork: n["isFork"].as_bool().unwrap_or(false),
                parent: n["parent"]["nameWithOwner"].as_str().map(|s| s.to_string()),
                language: n["primaryLanguage"]["name"].as_str().map(|s| s.to_string()),
                description: n["description"].as_str().map(|s| s.to_string()),
                pushed_at: n["pushedAt"].as_str().map(|s| s.to_string()),
                url: n["url"].as_str().unwrap_or("").into(),
            });
        }
        let pi = &repos["pageInfo"];
        if pi["hasNextPage"].as_bool() == Some(true) {
            cursor = pi["endCursor"].as_str().map(|s| s.to_string());
        } else {
            break;
        }
    }
    Ok(out)
}

/// clone / 添加已有项目时补充 GitHub 元信息（私有、fork 来源）
pub async fn repo_meta(http: &Http, token: &str, owner: &str, repo: &str) -> (bool, Option<String>) {
    match gh_get(http, token, &format!("/repos/{owner}/{repo}"), &[]).await {
        Ok(v) => (
            v["private"].as_bool().unwrap_or(false),
            v["parent"]["full_name"].as_str().map(|s| s.to_string()),
        ),
        Err(_) => (false, None),
    }
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct LabelInfo {
    pub name: String,
    pub color: String,
}

fn parse_labels(i: &Value) -> Vec<LabelInfo> {
    i["labels"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .iter()
        .map(|l| LabelInfo {
            name: l["name"].as_str().unwrap_or("").into(),
            color: l["color"].as_str().unwrap_or("6e7681").into(),
        })
        .collect()
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct IssueInfo {
    pub number: u64,
    pub title: String,
    pub state: String,
    pub labels: Vec<LabelInfo>,
    pub assignee: Option<String>,
    pub url: String,
    pub created_at: String,
}

#[tauri::command]
pub async fn list_issues(state: State<'_, AppState>, owner: String, repo: String) -> Result<Vec<IssueInfo>, String> {
    require_repo(&owner, &repo)?;
    let token = ensure_token(&state)?;
    let v = gh_get(
        &state.http,
        &token,
        &format!("/repos/{owner}/{repo}/issues"),
        &[("state", "all"), ("per_page", "100")],
    )
    .await?;
    let mut out = Vec::new();
    for i in v.as_array().cloned().unwrap_or_default() {
        if i.get("pull_request").is_some() {
            continue; // issues 端点混入 PR，过滤掉
        }
        out.push(IssueInfo {
            number: i["number"].as_u64().unwrap_or(0),
            title: i["title"].as_str().unwrap_or("").into(),
            state: i["state"].as_str().unwrap_or("open").into(),
            labels: parse_labels(&i),
            assignee: i["assignees"][0]["login"].as_str().map(|s| s.to_string()),
            url: i["html_url"].as_str().unwrap_or("").into(),
            created_at: i["created_at"].as_str().unwrap_or("").into(),
        });
    }
    Ok(out)
}

/// pm_sync_github 用：issue 首页全量（open + closed，按 created 倒序，100 条），
/// 比 IssueInfo 多带 closed_at / body / assignees 供同步策略用
#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SyncIssue {
    pub number: u64,
    pub title: String,
    pub state: String,
    pub labels: Vec<LabelInfo>,
    pub assignees: Vec<String>,
    pub url: String,
    pub created_at: String,
    pub closed_at: Option<String>,
    pub body: Option<String>,
}

pub async fn fetch_issues_for_sync(http: &Http, token: &str, owner: &str, repo: &str) -> Result<Vec<SyncIssue>, String> {
    let v = gh_get(
        http,
        token,
        &format!("/repos/{owner}/{repo}/issues"),
        &[("state", "all"), ("per_page", "100"), ("sort", "created"), ("direction", "desc")],
    )
    .await?;
    Ok(v.as_array()
        .cloned()
        .unwrap_or_default()
        .iter()
        .filter(|i| i.get("pull_request").is_none()) // issues 端点混入 PR，过滤掉
        .map(|i| SyncIssue {
            number: i["number"].as_u64().unwrap_or(0),
            title: i["title"].as_str().unwrap_or("").into(),
            state: i["state"].as_str().unwrap_or("open").into(),
            labels: parse_labels(i),
            assignees: i["assignees"]
                .as_array()
                .cloned()
                .unwrap_or_default()
                .iter()
                .filter_map(|a| a["login"].as_str().map(|s| s.to_string()))
                .collect(),
            url: i["html_url"].as_str().unwrap_or("").into(),
            created_at: i["created_at"].as_str().unwrap_or("").into(),
            closed_at: i["closed_at"].as_str().map(|s| s.to_string()),
            body: i["body"].as_str().map(|s| s.to_string()),
        })
        .collect())
}

/// pm_sync_github 用：open PR 的 title+body，供 closes/fixes #N 引用扫描
#[derive(Serialize, Clone)]
pub struct SyncPr {
    pub number: u64,
    pub title: String,
    pub body: Option<String>,
}

pub async fn fetch_open_prs_for_sync(http: &Http, token: &str, owner: &str, repo: &str) -> Result<Vec<SyncPr>, String> {
    let v = gh_get(
        http,
        token,
        &format!("/repos/{owner}/{repo}/pulls"),
        &[("state", "open"), ("per_page", "100")],
    )
    .await?;
    Ok(v.as_array()
        .cloned()
        .unwrap_or_default()
        .iter()
        .map(|i| SyncPr {
            number: i["number"].as_u64().unwrap_or(0),
            title: i["title"].as_str().unwrap_or("").into(),
            body: i["body"].as_str().map(|s| s.to_string()),
        })
        .collect())
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct PrInfo {
    pub number: u64,
    pub title: String,
    pub state: String,
    pub merged: bool,
    pub draft: bool,
    pub user: String,
    pub head_ref: String,
    pub url: String,
    pub created_at: String,
}

#[tauri::command]
pub async fn list_prs(state: State<'_, AppState>, owner: String, repo: String) -> Result<Vec<PrInfo>, String> {
    require_repo(&owner, &repo)?;
    let token = ensure_token(&state)?;
    let v = gh_get(
        &state.http,
        &token,
        &format!("/repos/{owner}/{repo}/pulls"),
        &[("state", "all"), ("per_page", "100")],
    )
    .await?;
    Ok(v.as_array()
        .cloned()
        .unwrap_or_default()
        .iter()
        .map(|i| PrInfo {
            number: i["number"].as_u64().unwrap_or(0),
            title: i["title"].as_str().unwrap_or("").into(),
            state: i["state"].as_str().unwrap_or("open").into(),
            merged: !i["merged_at"].is_null(),
            draft: i["draft"].as_bool().unwrap_or(false),
            user: i["user"]["login"].as_str().unwrap_or("").into(),
            head_ref: i["head"]["ref"].as_str().unwrap_or("").into(),
            url: i["html_url"].as_str().unwrap_or("").into(),
            created_at: i["created_at"].as_str().unwrap_or("").into(),
        })
        .collect())
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowInfo {
    pub id: u64,
    pub name: String,
    pub path: String,
    pub state: String,
}

#[tauri::command]
pub async fn list_workflows(state: State<'_, AppState>, owner: String, repo: String) -> Result<Vec<WorkflowInfo>, String> {
    require_repo(&owner, &repo)?;
    let token = ensure_token(&state)?;
    let v = gh_get(&state.http, &token, &format!("/repos/{owner}/{repo}/actions/workflows"), &[("per_page", "100")]).await?;
    Ok(v["workflows"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .iter()
        .map(|w| WorkflowInfo {
            id: w["id"].as_u64().unwrap_or(0),
            name: w["name"].as_str().unwrap_or("").into(),
            path: w["path"].as_str().unwrap_or("").into(),
            state: w["state"].as_str().unwrap_or("").into(),
        })
        .collect())
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct RunInfo {
    pub id: u64,
    pub run_number: u64,
    pub name: String,
    pub branch: String,
    pub status: String,
    pub conclusion: Option<String>,
    pub actor: String,
    pub created_at: String,
    pub url: String,
}

fn parse_run(r: &Value) -> RunInfo {
    RunInfo {
        id: r["id"].as_u64().unwrap_or(0),
        run_number: r["run_number"].as_u64().unwrap_or(0),
        name: r["name"].as_str().unwrap_or("").into(),
        branch: r["head_branch"].as_str().unwrap_or("").into(),
        status: r["status"].as_str().unwrap_or("").into(),
        conclusion: r["conclusion"].as_str().map(|s| s.to_string()),
        actor: r["actor"]["login"].as_str().unwrap_or("").into(),
        created_at: r["created_at"].as_str().unwrap_or("").into(),
        url: r["html_url"].as_str().unwrap_or("").into(),
    }
}

#[tauri::command]
pub async fn list_runs(state: State<'_, AppState>, owner: String, repo: String, workflow_id: u64) -> Result<Vec<RunInfo>, String> {
    require_repo(&owner, &repo)?;
    let token = ensure_token(&state)?;
    let v = gh_get(
        &state.http,
        &token,
        &format!("/repos/{owner}/{repo}/actions/workflows/{workflow_id}/runs"),
        &[("per_page", "20")],
    )
    .await?;
    Ok(v["workflow_runs"].as_array().cloned().unwrap_or_default().iter().map(parse_run).collect())
}

#[tauri::command]
pub async fn latest_run_for_branch(state: State<'_, AppState>, owner: String, repo: String, branch: String) -> Result<Option<RunInfo>, String> {
    require_repo(&owner, &repo)?;
    let token = ensure_token(&state)?;
    let v = gh_get(
        &state.http,
        &token,
        &format!("/repos/{owner}/{repo}/actions/runs"),
        &[("branch", branch.as_str()), ("per_page", "1")],
    )
    .await?;
    Ok(v["workflow_runs"].as_array().and_then(|a| a.first()).map(parse_run))
}

#[tauri::command]
pub async fn rerun_run(state: State<'_, AppState>, owner: String, repo: String, run_id: u64) -> Result<(), String> {
    require_repo(&owner, &repo)?;
    let token = ensure_token(&state)?;
    gh_post(&state.http, &token, &format!("/repos/{owner}/{repo}/actions/runs/{run_id}/rerun"), json!({})).await?;
    Ok(())
}

#[tauri::command]
pub async fn cancel_run(state: State<'_, AppState>, owner: String, repo: String, run_id: u64) -> Result<(), String> {
    require_repo(&owner, &repo)?;
    let token = ensure_token(&state)?;
    gh_post(&state.http, &token, &format!("/repos/{owner}/{repo}/actions/runs/{run_id}/cancel"), json!({})).await?;
    Ok(())
}

#[tauri::command]
pub async fn dispatch_workflow(state: State<'_, AppState>, owner: String, repo: String, workflow_id: u64, r#ref: String, inputs: Option<std::collections::BTreeMap<String, String>>) -> Result<(), String> {
    require_repo(&owner, &repo)?;
    let token = ensure_token(&state)?;
    gh_post(
        &state.http,
        &token,
        &format!("/repos/{owner}/{repo}/actions/workflows/{workflow_id}/dispatches"),
        json!({"ref": r#ref, "inputs": inputs.unwrap_or_default()}),
    )
    .await?;
    Ok(())
}

/// 创建 PR，返回 html_url
#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowInput {
    name: String,
    description: String,
    kind: String,
    required: bool,
    default_value: String,
    options: Vec<String>,
}
#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowDetails {
    default_branch: String,
    dispatch: bool,
    inputs: Vec<WorkflowInput>,
}

fn parse_workflow_details(source: &str, default_branch: String) -> Result<WorkflowDetails, String> {
    use serde_yaml_ng::Value as Yaml;
    let document: Yaml = serde_yaml_ng::from_str(source).map_err(|e| format!("流程配置解析失败: {e}"))?;
    // 兼容 YAML 1.1 解析器把未引用的 on 识别为 true。
    let on = document.get("on").or_else(|| document.as_mapping().and_then(|m| m.get(Yaml::Bool(true))));
    let dispatch = on.is_some_and(|on| {
        on.as_str() == Some("workflow_dispatch")
            || on.as_sequence().is_some_and(|items| items.iter().any(|i| i.as_str() == Some("workflow_dispatch")))
            || on.as_mapping().is_some_and(|m| m.contains_key(Yaml::String("workflow_dispatch".into())))
    });
    let scalar = |v: &Yaml| -> String {
        match v { Yaml::String(s) => s.clone(), Yaml::Bool(b) => b.to_string(), Yaml::Number(n) => n.to_string(), _ => String::new() }
    };
    let inputs = on.and_then(|v| v.get("workflow_dispatch")).and_then(|v| v.get("inputs"))
        .and_then(Yaml::as_mapping).map(|fields| fields.iter().filter_map(|(key, v)| {
            Some(WorkflowInput {
                name: key.as_str()?.to_string(),
                description: v.get("description").and_then(Yaml::as_str).unwrap_or("").into(),
                kind: v.get("type").and_then(Yaml::as_str).unwrap_or("string").into(),
                required: v.get("required").and_then(Yaml::as_bool).unwrap_or(false),
                default_value: v.get("default").map(scalar).unwrap_or_default(),
                options: v.get("options").and_then(Yaml::as_sequence).map(|v| v.iter().map(scalar).collect()).unwrap_or_default(),
            })
        }).collect()).unwrap_or_default();
    Ok(WorkflowDetails { default_branch, dispatch, inputs })
}

#[tauri::command]
pub async fn workflow_details(state: State<'_, AppState>, owner: String, repo: String, workflow_id: u64) -> Result<WorkflowDetails, String> {
    require_repo(&owner, &repo)?;
    let token = ensure_token(&state)?;
    let metadata = gh_get(&state.http, &token, &format!("/repos/{owner}/{repo}"), &[]).await?;
    let default_branch = metadata["default_branch"].as_str().ok_or("无法确定默认分支")?;
    let workflow = gh_get(&state.http, &token, &format!("/repos/{owner}/{repo}/actions/workflows/{workflow_id}"), &[]).await?;
    let path = workflow["path"].as_str().ok_or("流程路径不存在")?;
    if !path.starts_with(".github/workflows/") || path.contains("..") {
        return Err("不支持的流程配置路径".into());
    }
    let url = format!("https://api.github.com/repos/{owner}/{repo}/contents/{path}");
    let response = state.http.send(|c| c.get(&url).bearer_auth(&token)
        .query(&[("ref", default_branch)]).header("Accept", "application/vnd.github.raw+json")).await?;
    if !response.status().is_success() { return Err(format!("无法读取流程配置: HTTP {}", response.status())); }
    let source = response.text().await.map_err(|e| format!("读取流程配置失败: {e}"))?;
    parse_workflow_details(&source, default_branch.into())
}

#[cfg(test)]
mod workflow_tests {
    use super::parse_workflow_details;
    use std::process::Command;
    #[test]
    fn supports_all_dispatch_forms_and_preserves_defaults() {
        for source in ["on: workflow_dispatch", "on: [push, workflow_dispatch]", "on:\n  workflow_dispatch:", "'on':\n  workflow_dispatch:"] {
            assert!(parse_workflow_details(source, "master".into()).unwrap().dispatch);
        }
        assert!(!parse_workflow_details("on: [push, pull_request]", "master".into()).unwrap().dispatch);
        let details = parse_workflow_details("on:\n  workflow_dispatch:\n    inputs:\n      bump:\n        type: choice\n        required: true\n        default: patch\n        options: [patch, minor, major, none]\n      dry:\n        type: boolean\n        default: false", "master".into()).unwrap();
        assert_eq!(details.default_branch, "master");
        assert_eq!(details.inputs[0].default_value, "patch");
        assert_eq!(details.inputs[0].options.len(), 4);
        assert!(details.inputs[0].required);
        assert_eq!(details.inputs[1].default_value, "false");
    }

    /// 诊断/回归（#66，#[ignore] 本地手动跑）：验证 keyring 里应用存的 token 能被
    /// 当前构建读出且仍有效（/user 200）。keyring 3 缺 windows-native feature 时
    /// 静默退化为 mock store——get_password 恒 NoEntry，本测试立即失败。
    /// 跑法：cargo test diagnostic_keyring_token -- --ignored --nocapture
    #[test]
    #[ignore = "诊断用：读真实凭据管理器，只打印有效性不打印 token"]
    fn diagnostic_keyring_token() {
        use super::{Http, KEYRING_SERVICE, KEYRING_USER};
        let entry = keyring::Entry::new(KEYRING_SERVICE, KEYRING_USER).expect("entry new");
        match entry.get_password() {
            Ok(t) => {
                println!("keyring 读取成功，token 长度 {}", t.len());
                let rt = tokio::runtime::Runtime::new().unwrap();
                let http = Http::new();
                let res = rt.block_on(async {
                    http.send(|c| c.get("https://api.github.com/user").bearer_auth(&t))
                        .await
                        .map(|r| r.status().to_string())
                });
                println!("token /user 状态: {res:?}");
                assert!(res.as_ref().map(String::as_str) == Ok("200 OK"), "keyring token 已失效: {res:?}");
            }
            Err(e) => panic!("keyring 读取失败: {e}"),
        }
    }

    /// #66 回归：ensure_token 内存命中直接返回（不依赖 keyring / gh CLI）。
    /// mock store 时代 keyring 恒空曾让所有 command 首调「未登录」失败。
    #[test]
    fn ensure_token_uses_memory_slot_first() {
        use super::{ensure_token, AppState, Http};
        use std::sync::Mutex;
        let state = AppState {
            token: Mutex::new(Some("mem-token".into())),
            http: Http::new(),
            has_project_scope: Mutex::new(None),
            projects_v2_cache: Mutex::new(None),
            pm: Mutex::new(crate::pm::store::PmStore::open_memory()),
        };
        assert_eq!(ensure_token(&state).unwrap(), "mem-token");
        // 内存槽保持不变（ensure_token 不得清空已持有的 token）
        assert_eq!(state.token.lock().unwrap().as_deref(), Some("mem-token"));
    }

    /// 跨平台测试命令：正常退出并回显一行文本
    #[cfg(windows)]
    fn echo_command() -> Command {
        let mut c = crate::git::new_cmd("cmd");
        c.args(["/c", "echo", "run_bounded_ok"]);
        c
    }
    #[cfg(not(windows))]
    fn echo_command() -> Command {
        let mut c = crate::git::new_cmd("echo");
        c.arg("run_bounded_ok");
        c
    }

    /// 跨平台测试命令：挂死 ~30s（远超测试时限），靠 run_bounded 杀掉收敛
    #[cfg(windows)]
    fn hang_command() -> Command {
        // ping 回环 30 次（~29s）：无外部网络依赖，Windows 自带
        let mut c = crate::git::new_cmd("ping");
        c.args(["-n", "30", "127.0.0.1"]);
        c
    }
    #[cfg(not(windows))]
    fn hang_command() -> Command {
        let mut c = crate::git::new_cmd("sleep");
        c.arg("30");
        c
    }

    /// #74 回归：挂死的子进程必须在时限内收敛（杀进程树 + 返回 None），
    /// 而不是像旧的 Command::output() 那样无界等待。
    #[test]
    fn run_bounded_kills_hanging_process_in_time() {
        use super::run_bounded;
        use std::time::{Duration, Instant};
        let started = Instant::now();
        let out = run_bounded(hang_command(), Duration::from_millis(500));
        let elapsed = started.elapsed();
        assert!(out.is_none(), "挂死进程应超时返回 None，实际 {out:?}");
        assert!(elapsed >= Duration::from_millis(500), "应至少等满时限: {elapsed:?}");
        assert!(elapsed < Duration::from_secs(10), "超时后必须杀进程收敛，不得等它自然退出: {elapsed:?}");
    }

    /// 快路径不受有界等待影响：正常退出的命令完整返回 stdout/退出码
    #[test]
    fn run_bounded_returns_output_of_quick_process() {
        use super::run_bounded;
        use std::time::Duration;
        let out = run_bounded(echo_command(), Duration::from_secs(10)).expect("正常命令应返回 Some");
        assert!(out.status.success());
        assert!(String::from_utf8_lossy(&out.stdout).contains("run_bounded_ok"));
    }

    /// 对已回收的子进程不得按 PID 强杀——PID 可能已被 OS 复用，taskkill 会
    /// 误杀无关进程树（对齐 agents.rs StartedServer::tree_cleanup_allowed 的守卫）
    #[test]
    fn kill_tree_bounded_skips_reaped_child() {
        use super::kill_tree_bounded;
        let mut child = echo_command().spawn().unwrap();
        let _ = child.wait(); // 已退出并回收
        assert!(!kill_tree_bounded(&mut child), "已退出的进程不得触发按 PID 强杀");
    }

    /// #74 诊断（#[ignore]，本地手动跑）：逐阶段计时复刻 pm_sync_github 的
    /// 后端链路，定位「GitHub 同步超时」横幅的耗时来源。本机凭据管理器没有
    /// gh-projects 条目时，走的正是用户机同款「keyring 空 → gh CLI 兜底」路径。
    /// 跑法：cargo test diagnostic_sync_phase_timing -- --ignored --nocapture
    #[test]
    #[ignore = "诊断用：依赖本机凭据 / gh CLI 与 GitHub 连通性"]
    fn diagnostic_sync_phase_timing() {
        use super::{
            GH_CLI_TIMEOUT, AppState, Http, ensure_token, fetch_issues_for_sync,
            fetch_open_prs_for_sync, keyring_entry, stored_token, try_gh_cli,
        };
        use crate::pm::store::{PmStore, now_ts};
        use crate::pm::sync::{GithubIssueSnapshot, build_signals, recent_issues};
        use std::sync::Mutex;
        use std::time::Instant;
        let mk_state = || AppState {
            token: Mutex::new(None),
            http: Http::new(),
            has_project_scope: Mutex::new(None),
            projects_v2_cache: Mutex::new(None),
            pm: Mutex::new(PmStore::open_memory()),
        };

        let state = mk_state();
        let t = Instant::now();
        let stored = stored_token(&state);
        println!("[1] stored_token（内存→keyring）: {:?}，命中={}", t.elapsed(), stored.is_some());
        // try_gh_cli 成功会把 token 写回 keyring；开始时若为空则事后还原，
        // 保证重复跑诊断始终走 gh CLI 路径
        let keyring_was_empty = stored.is_none();
        let restore_keyring = || {
            if keyring_was_empty {
                if let Ok(entry) = keyring_entry() {
                    let _ = entry.delete_credential();
                }
            }
        };

        let state = mk_state();
        let t = Instant::now();
        let gh = try_gh_cli(&state);
        println!("[2] try_gh_cli（gh auth token，上限 {GH_CLI_TIMEOUT:?}）: {:?}，成功={}", t.elapsed(), gh.is_some());
        restore_keyring();

        let state = mk_state();
        let t = Instant::now();
        let token = ensure_token(&state);
        println!("[3] ensure_token 总计: {:?}，成功={}", t.elapsed(), token.is_ok());
        let token = token.expect("本机应能取到 token（keyring 或 gh CLI），否则无法继续计时");

        // pm_sync_github 同款：issues 首页与 open PR 并行拉取
        let (issues, prs) = tokio::runtime::Runtime::new().unwrap().block_on(async {
            let t = Instant::now();
            let issues = fetch_issues_for_sync(&state.http, &token, "coconilu", "gitgrove").await;
            println!("[4] fetch_issues_for_sync: {:?}，成功={}，条数={}", t.elapsed(), issues.is_ok(), issues.as_ref().map(Vec::len).unwrap_or(0));
            let t = Instant::now();
            let prs = fetch_open_prs_for_sync(&state.http, &token, "coconilu", "gitgrove").await;
            println!("[5] fetch_open_prs_for_sync: {:?}，成功={}，条数={}", t.elapsed(), prs.is_ok(), prs.as_ref().map(Vec::len).unwrap_or(0));
            (issues, prs)
        });
        let issues = issues.expect("fetch issues 失败");
        let prs = prs.expect("fetch PRs 失败");

        let pr_texts: Vec<String> = prs
            .iter()
            .map(|p| format!("{}\n{}", p.title, p.body.as_deref().unwrap_or_default()))
            .collect();
        let signals = build_signals(&[], &pr_texts);
        let snapshots = recent_issues(issues.into_iter().map(GithubIssueSnapshot::from).collect(), now_ts());
        let t = Instant::now();
        let r = state.pm.lock().unwrap().sync_github("coconilu", "gitgrove", &snapshots, &signals);
        println!("[6] recent_issues + sync_github（upsert {} 条）: {:?}，结果={:?}", snapshots.len(), t.elapsed(), r.as_ref().map(|x| (x.created, x.updated, x.moved)));
        r.expect("sync_github 失败");
        restore_keyring();
    }
}

pub async fn create_pr_api(http: &Http, token: &str, owner: &str, repo: &str, title: &str, head: &str, base: &str, body: &str) -> Result<String, String> {
    let v = gh_post(
        http,
        token,
        &format!("/repos/{owner}/{repo}/pulls"),
        json!({"title": title, "head": head, "base": base, "body": body}),
    )
    .await?;
    Ok(v["html_url"].as_str().unwrap_or("").to_string())
}

/// PR 详情（head ref / title / url），供开 review worktree 用
pub async fn pr_detail(http: &Http, token: &str, owner: &str, repo: &str, number: u64) -> Result<(String, String, String), String> {
    let v = gh_get(http, token, &format!("/repos/{owner}/{repo}/pulls/{number}"), &[]).await?;
    Ok((
        v["head"]["ref"].as_str().unwrap_or("").to_string(),
        v["title"].as_str().unwrap_or("").to_string(),
        v["html_url"].as_str().unwrap_or("").to_string(),
    ))
}
