// git 命令封装：一律调用户的 git 二进制；Windows 子进程全部 CREATE_NO_WINDOW

use serde::Serialize;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

#[cfg(windows)]
use std::os::windows::process::CommandExt;
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

pub fn new_cmd(program: &str) -> Command {
    let mut c = Command::new(program);
    #[cfg(windows)]
    c.creation_flags(CREATE_NO_WINDOW);
    if program == "git" {
        // 禁止任何形式的交互式凭据提示（否则无控制台环境下会永久挂起）
        c.env("GIT_TERMINAL_PROMPT", "0");
    }
    c
}

/// 把 token 注入 https URL（x-access-token 形式；extraheader 在部分代理下会挂起 git）
pub fn auth_url(url: &str, token: &str) -> String {
    if token.is_empty() {
        return url.to_string();
    }
    if let Some(rest) = url.strip_prefix("https://") {
        format!("https://x-access-token:{token}@{rest}")
    } else {
        url.to_string()
    }
}

pub fn run(args: &[&str], cwd: Option<&Path>) -> Result<String, String> {
    let mut c = new_cmd("git");
    c.args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(d) = cwd {
        c.current_dir(d);
    }
    let out = c.output().map_err(|e| format!("无法启动 git: {e}"))?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).to_string())
    } else {
        let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
        Err(if err.is_empty() {
            format!("git {} 退出码 {:?}", args.join(" "), out.status.code())
        } else {
            err
        })
    }
}

pub fn run_in(dir: &Path, args: &[&str]) -> Result<String, String> {
    run(args, Some(dir))
}

pub fn home_dir() -> PathBuf {
    std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("."))
}

/// 系统代理检测（git 不读 Windows IE 代理设置，需要显式注入 http.proxy）
static SYSTEM_PROXY: std::sync::OnceLock<Option<String>> = std::sync::OnceLock::new();

pub fn system_proxy() -> Option<String> {
    SYSTEM_PROXY
        .get_or_init(|| {
            #[cfg(windows)]
            {
                const KEY: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Internet Settings";
                let enabled = new_cmd("reg")
                    .args(["query", KEY, "/v", "ProxyEnable"])
                    .output()
                    .ok()
                    .map(|o| String::from_utf8_lossy(&o.stdout).contains("0x1"))
                    .unwrap_or(false);
                if !enabled {
                    return None;
                }
                let out = new_cmd("reg")
                    .args(["query", KEY, "/v", "ProxyServer"])
                    .output()
                    .ok()?;
                let text = String::from_utf8_lossy(&out.stdout);
                for line in text.lines() {
                    let line = line.trim();
                    if line.starts_with("ProxyServer") {
                        if let Some(server) = line.split_whitespace().last() {
                            if server.contains(':') {
                                return Some(format!("http://{server}"));
                            }
                        }
                    }
                }
                None
            }
            #[cfg(not(windows))]
            {
                std::env::var("HTTPS_PROXY")
                    .or_else(|_| std::env::var("https_proxy"))
                    .ok()
            }
        })
        .clone()
}

/// 需要联网的 git 命令（clone/fetch/push）附带的 -c 参数
pub fn network_args() -> Vec<String> {
    match system_proxy() {
        Some(p) => vec![
            "-c".into(),
            format!("http.proxy={p}"),
            // schannel 与本机代理的 TLS 拦截不兼容，走代理时换 openssl
            "-c".into(),
            "http.sslBackend=openssl".into(),
        ],
        None => vec![],
    }
}

pub fn clone_root() -> PathBuf {
    home_dir().join("gh-projects")
}

/// worktree 目录：<repo>.worktrees/<slug>/，与主 clone 同级
pub fn worktree_dir(main_path: &Path, branch: &str) -> PathBuf {
    let parent = main_path.parent().unwrap_or_else(|| Path::new(".")).to_path_buf();
    let base = main_path
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "repo".into());
    parent
        .join(format!("{base}.worktrees"))
        .join(slugify(branch))
}

pub fn slugify(s: &str) -> String {
    let mut out = String::new();
    for ch in s.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
        } else if !out.ends_with('-') && !out.is_empty() {
            out.push('-');
        }
    }
    let out = out.trim_matches('-').to_string();
    if out.is_empty() { "wt".into() } else { out.chars().take(40).collect() }
}

pub struct RawWorktree {
    pub path: PathBuf,
    pub branch: Option<String>,
    pub locked: bool,
}

/// git worktree list --porcelain
pub fn worktree_list(repo: &Path) -> Result<Vec<RawWorktree>, String> {
    let out = run_in(repo, &["worktree", "list", "--porcelain"])?;
    let mut list = Vec::new();
    let mut path: Option<PathBuf> = None;
    let mut branch: Option<String> = None;
    let mut locked = false;
    let mut push = |path: &mut Option<PathBuf>, branch: &mut Option<String>, locked: &mut bool| {
        if let Some(p) = path.take() {
            list.push(RawWorktree { path: p, branch: branch.take(), locked: *locked });
        }
        *locked = false;
    };
    for line in out.lines() {
        if let Some(p) = line.strip_prefix("worktree ") {
            push(&mut path, &mut branch, &mut locked);
            path = Some(PathBuf::from(p));
        } else if let Some(b) = line.strip_prefix("branch refs/heads/") {
            branch = Some(b.to_string());
        } else if line.starts_with("locked") {
            locked = true;
        }
    }
    push(&mut path, &mut branch, &mut locked);
    Ok(list)
}

pub fn worktree_add(repo: &Path, path: &Path, branch: &str, create: bool, base: Option<&str>) -> Result<(), String> {
    let p = path.to_string_lossy().to_string();
    let mut args: Vec<String> = vec!["worktree".into(), "add".into()];
    if create {
        args.push("-b".into());
        args.push(branch.into());
        args.push(p);
        args.push(base.unwrap_or("HEAD").into());
    } else {
        args.push(p);
        args.push(branch.into());
    }
    let refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
    run_in(repo, &refs).map(|_| ())
}

pub fn worktree_lock(path: &Path, lock: bool) -> Result<(), String> {
    let p = path.to_string_lossy().to_string();
    run(&["worktree", if lock { "lock" } else { "unlock" }, &p], None).map(|_| ())
}

pub fn worktree_prune(repo: &Path) {
    let _ = run_in(repo, &["worktree", "prune"]);
}

pub fn current_branch(path: &Path) -> String {
    run_in(path, &["branch", "--show-current"])
        .map(|s| s.trim().to_string())
        .unwrap_or_default()
}

pub fn default_branch(repo: &Path) -> String {
    if let Ok(s) = run_in(repo, &["symbolic-ref", "--short", "refs/remotes/origin/HEAD"]) {
        if let Some(b) = s.trim().strip_prefix("origin/") {
            return b.to_string();
        }
    }
    // 退路：main / master 二选一
    if run_in(repo, &["rev-parse", "--verify", "main"]).is_ok() {
        "main".into()
    } else {
        "master".into()
    }
}

/// ahead/behind 相对 upstream；无 upstream 时返回 (0,0)
pub fn ahead_behind(path: &Path) -> (u32, u32) {
    let out = match run_in(path, &["rev-list", "--left-right", "--count", "HEAD...@{upstream}"]) {
        Ok(s) => s,
        Err(_) => return (0, 0),
    };
    let mut it = out.split_whitespace();
    let a = it.next().and_then(|x| x.parse().ok()).unwrap_or(0);
    let b = it.next().and_then(|x| x.parse().ok()).unwrap_or(0);
    (a, b)
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct BranchInfo {
    pub name: String,
    pub remote: bool,
    pub ahead: u32,
    pub behind: u32,
    pub last_commit_ts: i64,
    pub sha: String,
    pub subject: String,
}

/// 本地 + 远程分支列表（含 upstream track）
pub fn branches(repo: &Path) -> Result<Vec<BranchInfo>, String> {
    let fmt = "%(refname)%09%(upstream:track)%09%(committerdate:unix)%09%(objectname:short)%09%(contents:subject)";
    let out = run_in(repo, &["for-each-ref", &format!("--format={fmt}"), "refs/heads", "refs/remotes"])?;
    let mut list = Vec::new();
    for line in out.lines() {
        let cols: Vec<&str> = line.split('\t').collect();
        if cols.len() < 5 {
            continue;
        }
        let refname = cols[0];
        let (name, remote) = if let Some(n) = refname.strip_prefix("refs/heads/") {
            (n.to_string(), false)
        } else if let Some(n) = refname.strip_prefix("refs/remotes/") {
            if n.ends_with("/HEAD") {
                continue;
            }
            (n.to_string(), true)
        } else {
            continue;
        };
        let track = cols[1];
        let mut ahead = 0u32;
        let mut behind = 0u32;
        if let Some(n) = extract_num(track, "ahead ") {
            ahead = n;
        }
        if let Some(n) = extract_num(track, "behind ") {
            behind = n;
        }
        list.push(BranchInfo {
            name,
            remote,
            ahead,
            behind,
            last_commit_ts: cols[2].parse().unwrap_or(0),
            sha: cols[3].to_string(),
            subject: cols[4].chars().take(72).collect(),
        });
    }
    list.sort_by_key(|b| (b.remote, -b.last_commit_ts));
    Ok(list)
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SkippedBranch {
    pub name: String,
    /// 跳过原因：当前分支 / 主干分支 / 已在 worktree 检出 / 未合并到主干
    pub reason: String,
}

/// 删除前预览：deletable 将出现在确认弹窗里
#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct BranchDeletePlan {
    pub base: String,
    pub deletable: Vec<String>,
    pub skipped: Vec<SkippedBranch>,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct BranchDeleteResult {
    pub deleted: Vec<String>,
    pub deleted_count: u32,
    pub skipped: Vec<SkippedBranch>,
    pub failed: Vec<SkippedBranch>,
}

/// 本地分支名集合（refs/heads）
fn local_branch_names(repo: &Path) -> Result<Vec<String>, String> {
    let out = run_in(repo, &["for-each-ref", "--format=%(refname:strip=2)", "refs/heads"])?;
    Ok(out.lines().map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect())
}

/// 已合并进 base 的本地分支（git branch --merged <base> 语义）
pub fn merged_local_branches(repo: &Path, base: &str) -> Result<Vec<String>, String> {
    // base 本地不存在时退到 origin/<base>
    let base_ref = if run_in(repo, &["rev-parse", "--verify", "--quiet", &format!("refs/heads/{base}")]).is_ok() {
        base.to_string()
    } else {
        format!("refs/remotes/origin/{base}")
    };
    let out = run_in(repo, &["for-each-ref", &format!("--merged={base_ref}"), "--format=%(refname:strip=2)", "refs/heads"])?;
    Ok(out.lines().map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect())
}

/// 被任意 worktree 检出的分支集合（这些分支 git 拒绝删除）
fn checked_out_branches(repo: &Path) -> Vec<String> {
    worktree_list(repo)
        .map(|wts| wts.into_iter().filter_map(|w| w.branch).collect())
        .unwrap_or_default()
}

fn classify_branch(name: &str, current: &str, base: &str, checked_out: &[String], merged: &[String]) -> Option<String> {
    if name == current {
        return Some("当前分支".into());
    }
    if name == base || name == "master" || name == "main" {
        return Some("主干分支".into());
    }
    if checked_out.iter().any(|b| b == name) {
        return Some("已在 worktree 检出".into());
    }
    if !merged.iter().any(|b| b == name) {
        return Some("未合并到主干".into());
    }
    None
}

/// 删除已合并分支的预览清单：相对主干 base，保护当前/主干/未合并/已检出分支
pub fn merged_branch_plan(repo: &Path) -> Result<BranchDeletePlan, String> {
    let base = default_branch(repo);
    let current = current_branch(repo);
    let checked_out = checked_out_branches(repo);
    let merged = merged_local_branches(repo, &base)?;
    let mut deletable = Vec::new();
    let mut skipped = Vec::new();
    for name in local_branch_names(repo)? {
        match classify_branch(&name, &current, &base, &checked_out, &merged) {
            Some(reason) => skipped.push(SkippedBranch { name, reason }),
            None => deletable.push(name),
        }
    }
    Ok(BranchDeletePlan { base, deletable, skipped })
}

/// 批量删除本地分支；每个名字都重新过一遍保护规则，未通过的一律跳过不删
pub fn delete_local_branches(repo: &Path, names: &[String]) -> Result<BranchDeleteResult, String> {
    let base = default_branch(repo);
    let current = current_branch(repo);
    let checked_out = checked_out_branches(repo);
    let merged = merged_local_branches(repo, &base)?;
    let locals = local_branch_names(repo)?;
    let mut deleted = Vec::new();
    let mut skipped = Vec::new();
    let mut failed = Vec::new();
    for name in names {
        if let Some(reason) = classify_branch(name, &current, &base, &checked_out, &merged) {
            skipped.push(SkippedBranch { name: name.clone(), reason });
            continue;
        }
        if !locals.iter().any(|b| b == name) {
            failed.push(SkippedBranch { name: name.clone(), reason: "本地分支不存在".into() });
            continue;
        }
        match run_in(repo, &["branch", "-d", name]) {
            Ok(_) => deleted.push(name.clone()),
            Err(e) => failed.push(SkippedBranch { name: name.clone(), reason: e }),
        }
    }
    let deleted_count = deleted.len() as u32;
    Ok(BranchDeleteResult { deleted, deleted_count, skipped, failed })
}

fn extract_num(track: &str, key: &str) -> Option<u32> {
    let i = track.find(key)?;
    let rest = &track[i + key.len()..];
    let num: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
    num.parse().ok()
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct StatusEntry {
    pub path: String,
    pub code: String, // M / A / D
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct StatusMap {
    pub changes: Vec<StatusEntry>,
    pub ignored: Vec<String>,
}

/// git status --porcelain=v1 -z --ignored（含 ignored 目录，用于灰显）
pub fn status_porcelain(path: &Path) -> Result<StatusMap, String> {
    let out = run_in(path, &["status", "--porcelain=v1", "-z", "--ignored"])?;
    let mut changes = Vec::new();
    let mut ignored = Vec::new();
    let mut parts = out.split('\0');
    while let Some(rec) = parts.next() {
        if rec.len() < 4 {
            continue;
        }
        let xy = &rec[..2];
        let mut p = rec[3..].to_string();
        if xy.starts_with('R') || xy.starts_with('C') {
            // 重命名记录后跟一个原始路径记录，跳过
            let _ = parts.next();
        }
        if p.ends_with('/') {
            p.pop();
        }
        if p.is_empty() {
            continue;
        }
        if xy == "!!" {
            ignored.push(p);
            continue;
        }
        let code = if xy == "??" || xy.contains('A') {
            "A"
        } else if xy.contains('D') {
            "D"
        } else {
            "M"
        };
        changes.push(StatusEntry { path: p, code: code.into() });
    }
    Ok(StatusMap { changes, ignored })
}

/// 从 remote.origin.url 解析 owner/repo
pub fn remote_owner_repo(path: &Path) -> Option<(String, String)> {
    let url = run_in(path, &["config", "--get", "remote.origin.url"]).ok()?;
    parse_github_url(url.trim())
}

pub fn parse_github_url(url: &str) -> Option<(String, String)> {
    let rest = url
        .strip_prefix("https://github.com/")
        .or_else(|| url.strip_prefix("http://github.com/"))
        .or_else(|| url.strip_prefix("git@github.com:"))
        .or_else(|| url.strip_prefix("ssh://git@github.com/"))?;
    let rest = rest.trim_end_matches('/').trim_end_matches(".git");
    let mut it = rest.splitn(2, '/');
    let owner = it.next()?.to_string();
    let repo = it.next()?.to_string();
    if owner.is_empty() || repo.is_empty() {
        None
    } else {
        Some((owner, repo))
    }
}
