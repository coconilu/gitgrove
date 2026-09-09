// 项目 / clone / worktree 命令

use serde::Serialize;
use std::io::BufRead;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use tauri::{AppHandle, Emitter, State};

use crate::git;
use crate::github;
use crate::store::{self, LinkedWorkItem, ProviderIdentity, StoredCheckout, StoredProject};
use crate::AppState;

/// 项目无 GitHub 身份时的统一错误（前端据此降级为空状态）
pub const ERR_NO_GITHUB: &str = "NO_GITHUB_REPO: 该项目未关联 GitHub 仓库（仅支持 github.com 远程）";

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct CheckoutInfo {
    pub id: String,
    pub path: String,
    pub branch: String,
    pub is_primary: bool,
    pub is_locked: bool,
    pub linked_work_item: Option<LinkedWorkItem>,
    pub ahead: u32,
    pub behind: u32,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Project {
    pub id: String,
    /// 展示名：GitHub 仓库名或目录名
    pub name: String,
    /// GitHub 身份；本地 / 非 GitHub 远程的仓库为 None
    pub provider_identity: Option<ProviderIdentity>,
    pub is_private: bool,
    pub fork_of: Option<String>,
    pub color: String,
    pub local_path: String,
    pub default_branch: String,
    pub added_at: i64,
    pub checkouts: Vec<CheckoutInfo>,
}

fn find_project<'a>(store: &'a store::Store, id: &str) -> Result<&'a StoredProject, String> {
    store
        .projects
        .iter()
        .find(|p| p.id == id)
        .ok_or_else(|| format!("项目不存在: {id}"))
}

/// 由存储的项目 + git worktree list 组装完整 Project
fn build_project(sp: &StoredProject) -> Project {
    let main = PathBuf::from(&sp.local_path);
    let mut checkouts = Vec::new();
    if main.exists() {
        if let Ok(wts) = git::worktree_list(&main) {
            for wt in wts {
                let wpath = wt.path.to_string_lossy().to_string();
                let is_primary = wt.path == main;
                if !is_primary && sp.hidden_worktrees.iter().any(|h| *h == wpath) {
                    continue;
                }
                let branch = wt.branch.unwrap_or_else(|| "(detached)".into());
                let (ahead, behind) = git::ahead_behind(&wt.path);
                let meta = sp.checkouts.iter().find(|c| c.path == wpath);
                checkouts.push(CheckoutInfo {
                    id: format!("{}::{}", sp.id, wpath),
                    path: wpath,
                    branch,
                    is_primary,
                    is_locked: wt.locked,
                    linked_work_item: meta.and_then(|m| m.linked_work_item.clone()),
                    ahead,
                    behind,
                });
            }
        } else {
            // 目录在但不是 git 仓库 / git 出错：至少给出主 clone
            let (ahead, behind) = git::ahead_behind(&main);
            checkouts.push(CheckoutInfo {
                id: format!("{}::{}", sp.id, sp.local_path),
                path: sp.local_path.clone(),
                branch: git::current_branch(&main),
                is_primary: true,
                is_locked: false,
                linked_work_item: None,
                ahead,
                behind,
            });
        }
    }
    checkouts.sort_by_key(|c| !c.is_primary);
    Project {
        id: sp.id.clone(),
        name: sp.display_name(),
        provider_identity: sp.provider_identity.clone(),
        is_private: sp.is_private,
        fork_of: sp.fork_of.clone(),
        color: sp.color.clone(),
        local_path: sp.local_path.clone(),
        default_branch: git::default_branch(&main),
        added_at: sp.added_at,
        checkouts,
    }
}

#[tauri::command]
pub fn list_projects() -> Result<Vec<Project>, String> {
    let st = store::load();
    let mut out: Vec<Project> = st.projects.iter().map(build_project).collect();
    out.sort_by_key(|p| p.added_at);
    Ok(out)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CloneTargetInfo {
    pub target: String,
    pub exists: bool,
}

#[tauri::command]
pub fn check_clone_target(repo: String) -> Result<CloneTargetInfo, String> {
    let target = git::clone_root().join(&repo);
    Ok(CloneTargetInfo {
        exists: target.exists(),
        target: target.to_string_lossy().to_string(),
    })
}

fn require_identity(sp: &StoredProject) -> Result<&ProviderIdentity, String> {
    sp.provider_identity.as_ref().ok_or_else(|| ERR_NO_GITHUB.to_string())
}

async fn register_project(state: &AppState, identity: Option<ProviderIdentity>, path: &Path) -> Result<Project, String> {
    let token = github::ensure_token(state).unwrap_or_default();
    // 只有带 GitHub 身份的项目才去拉 is_private / fork 信息
    let (is_private, fork_of) = match (&identity, token.is_empty()) {
        (Some(id), false) => github::repo_meta(&state.http, &token, &id.owner, &id.repo).await,
        _ => (false, None),
    };
    let mut st = store::load();
    // 已存在同路径项目则直接返回
    let ps = path.to_string_lossy().to_string();
    if let Some(existing) = st.projects.iter().find(|p| p.local_path == ps) {
        return Ok(build_project(existing));
    }
    let name = path
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "project".into());
    let sp = StoredProject {
        id: store::new_id(),
        provider_identity: identity,
        owner: None,
        repo: None,
        is_private,
        fork_of,
        color: store::color_for(&name),
        local_path: ps,
        added_at: store::now_ts(),
        checkouts: vec![],
        hidden_worktrees: vec![],
    };
    st.projects.push(sp.clone());
    store::save(&st)?;
    Ok(build_project(&sp))
}

#[tauri::command]
pub async fn clone_repo(app: AppHandle, state: State<'_, AppState>, url: String, repo: String) -> Result<Project, String> {
    let target = git::clone_root().join(&repo);
    if target.exists() {
        return Err(format!("目标目录已存在: {}", target.to_string_lossy()));
    }
    let token = github::ensure_token(&state).unwrap_or_default();
    let target_s = target.to_string_lossy().to_string();
    let url_s = url.clone();
    let app2 = app.clone();

    tokio::task::spawn_blocking(move || -> Result<(), String> {
        std::fs::create_dir_all(git::clone_root()).map_err(|e| format!("创建目录失败: {e}"))?;
        let mut args: Vec<String> = git::network_args();
        let clone_url = git::auth_url(&url_s, &token);
        args.extend(["clone".into(), "--progress".into(), clone_url, target_s.clone()]);
        let mut child = git::new_cmd("git")
            .args(&args)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("无法启动 git: {e}"))?;
        if let Some(err) = child.stderr.take() {
            let reader = std::io::BufReader::new(err);
            let mut lines = reader.lines();
            let mut last = String::new();
            while let Some(Ok(chunk)) = lines.next() {
                // git 进度用 \r 分隔，取最后一段
                for seg in chunk.split('\r') {
                    let s = seg.trim();
                    if !s.is_empty() {
                        last = s.to_string();
                    }
                }
                if !last.is_empty() {
                    // 进度行里不应含 token，但兜底脱敏
                    let _ = app2.emit("clone-progress", last.replace(&token, "***"));
                }
            }
        }
        let status = child.wait().map_err(|e| e.to_string())?;
        if status.success() {
            // 把 origin 恢复为干净 URL（token 不留在 .git/config）
            let _ = git::run_in(
                Path::new(&target_s),
                &["remote", "set-url", "origin", &url_s],
            );
            Ok(())
        } else {
            let _ = std::fs::remove_dir_all(&target_s);
            Err(format!("git clone 失败（退出码 {:?}）", status.code()))
        }
    })
    .await
    .map_err(|e| e.to_string())??;

    let (owner, repo_name) = git::parse_github_url(&url).ok_or("无法解析仓库 URL")?;
    register_project(
        &state,
        Some(ProviderIdentity {
            provider: "github".into(),
            owner,
            repo: repo_name,
        }),
        &target,
    )
    .await
}

#[tauri::command]
pub async fn add_existing_project(state: State<'_, AppState>, path: String) -> Result<Project, String> {
    let p = PathBuf::from(&path);
    if !p.exists() {
        return Err(format!("目录不存在: {path}"));
    }
    // 只要是 git 仓库即可注册；GitHub 身份尽力解析，没有就是纯本地项目
    let is_git = git::run_in(&p, &["rev-parse", "--is-inside-work-tree"])
        .map(|s| s.trim() == "true")
        .unwrap_or(false);
    if !is_git {
        return Err("该目录不是 git 仓库，无法注册".into());
    }
    let identity = git::remote_owner_repo(&p).map(|(owner, repo)| ProviderIdentity {
        provider: "github".into(),
        owner,
        repo,
    });
    register_project(&state, identity, &p).await
}

#[tauri::command]
pub fn remove_project(project_id: String, delete_files: bool) -> Result<(), String> {
    let mut st = store::load();
    let sp = find_project(&st, &project_id)?.clone();
    if delete_files {
        let main = PathBuf::from(&sp.local_path);
        if let Ok(wts) = git::worktree_list(&main) {
            for wt in wts {
                if wt.path != main && wt.path.exists() {
                    trash::delete(&wt.path).map_err(|e| format!("回收 worktree 失败: {e}"))?;
                }
            }
        }
        if main.exists() {
            trash::delete(&main).map_err(|e| format!("回收目录失败: {e}"))?;
        }
        // 清理 .worktrees 兄弟目录（若已空），命名与 git::worktree_dir 一致（目录名）
        let base = main
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "repo".into());
        let sib = main.parent().map(|p| p.join(format!("{base}.worktrees")));
        if let Some(s) = sib {
            if s.exists() && s.read_dir().map(|mut d| d.next().is_none()).unwrap_or(false) {
                let _ = std::fs::remove_dir(&s);
            }
        }
    }
    st.projects.retain(|p| p.id != project_id);
    store::save(&st)
}

#[tauri::command]
pub fn list_branches(project_id: String) -> Result<Vec<git::BranchInfo>, String> {
    let st = store::load();
    let sp = find_project(&st, &project_id)?;
    git::branches(Path::new(&sp.local_path))
}

/// 拉取后的 ahead/behind，供前端即时刷新同步状态
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AheadBehind {
    pub ahead: u32,
    pub behind: u32,
}

/// git fetch --all --prune：让 ahead/behind 与分支列表反映远端真实状态
#[tauri::command]
pub fn fetch_project(project_id: String) -> Result<(), String> {
    let st = store::load();
    let sp = find_project(&st, &project_id)?;
    git::fetch(Path::new(&sp.local_path))
}

/// ff-only 拉取某个工作树；非快进时原样返回 git 报错（前端 toast 展示）
#[tauri::command]
pub fn pull_checkout(path: String) -> Result<AheadBehind, String> {
    let p = Path::new(&path);
    git::pull_ff_only(p)?;
    let (ahead, behind) = git::ahead_behind(p);
    Ok(AheadBehind { ahead, behind })
}

/// 删除已合并分支的预览：返回将删除的清单（供前端确认弹窗）与被保护跳过的分支
#[tauri::command]
pub fn merged_branches_plan(project_id: String) -> Result<git::BranchDeletePlan, String> {
    let st = store::load();
    let sp = find_project(&st, &project_id)?;
    git::merged_branch_plan(Path::new(&sp.local_path))
}

/// 批量删除本地分支（确认后执行）；返回 deletedCount 供前端反馈「成功删除 N 个」
#[tauri::command]
pub fn delete_merged_branches(project_id: String, branches: Vec<String>) -> Result<git::BranchDeleteResult, String> {
    let st = store::load();
    let sp = find_project(&st, &project_id)?;
    git::delete_local_branches(Path::new(&sp.local_path), &branches)
}

fn upsert_checkout_meta(sp: &mut StoredProject, path: &str, linked: Option<LinkedWorkItem>) {
    sp.hidden_worktrees.retain(|h| h != path);
    if let Some(c) = sp.checkouts.iter_mut().find(|c| c.path == path) {
        c.linked_work_item = linked;
    } else {
        sp.checkouts.push(StoredCheckout { path: path.to_string(), linked_work_item: linked });
    }
}

fn make_checkout_info(project_id: &str, path: &Path, linked: Option<LinkedWorkItem>) -> CheckoutInfo {
    let (ahead, behind) = git::ahead_behind(path);
    CheckoutInfo {
        id: format!("{project_id}::{}", path.to_string_lossy()),
        path: path.to_string_lossy().to_string(),
        branch: git::current_branch(path),
        is_primary: false,
        is_locked: false,
        linked_work_item: linked,
        ahead,
        behind,
    }
}

#[tauri::command]
pub fn create_worktree(
    project_id: String,
    branch: String,
    base: Option<String>,
    create_branch: bool,
    linked: Option<LinkedWorkItem>,
) -> Result<CheckoutInfo, String> {
    let mut st = store::load();
    let sp = st
        .projects
        .iter_mut()
        .find(|p| p.id == project_id)
        .ok_or("项目不存在")?;
    let main = PathBuf::from(&sp.local_path);
    let path = git::worktree_dir(&main, &branch);
    if path.exists() {
        return Err(format!("目录已存在: {}", path.to_string_lossy()));
    }
    git::worktree_add(&main, &path, &branch, create_branch, base.as_deref())?;
    let ps = path.to_string_lossy().to_string();
    upsert_checkout_meta(sp, &ps, linked.clone());
    store::save(&st)?;
    Ok(make_checkout_info(&project_id, &path, linked))
}

#[tauri::command]
pub fn remove_worktree(project_id: String, path: String, to_trash: bool) -> Result<(), String> {
    let mut st = store::load();
    let sp = st
        .projects
        .iter_mut()
        .find(|p| p.id == project_id)
        .ok_or("项目不存在")?;
    let main = PathBuf::from(&sp.local_path);
    let p = PathBuf::from(&path);
    if p == main {
        return Err("主 clone 不能删除，只能移除项目".into());
    }
    if to_trash {
        if p.exists() {
            trash::delete(&p).map_err(|e| format!("移入回收站失败: {e}"))?;
        }
        git::worktree_prune(&main);
    } else {
        sp.hidden_worktrees.push(path.clone());
    }
    sp.checkouts.retain(|c| c.path != path);
    store::save(&st)
}

#[tauri::command]
pub fn lock_worktree(path: String, lock: bool) -> Result<(), String> {
    git::worktree_lock(Path::new(&path), lock)
}

/// 从 issue 一键开 worktree：fix/<n>-<slug>，写入 linkedWorkItem
#[tauri::command]
pub async fn spawn_issue_worktree(state: State<'_, AppState>, project_id: String, number: u64, title: String) -> Result<CheckoutInfo, String> {
    let token = github::ensure_token(&state).unwrap_or_default();
    let mut st = store::load();
    let (main_path, owner, repo) = {
        let sp = find_project(&st, &project_id)?;
        let id = require_identity(sp)?;
        (sp.local_path.clone(), id.owner.clone(), id.repo.clone())
    };
    let main = PathBuf::from(&main_path);
    let base = git::default_branch(&main);
    let fetch_url = git::auth_url(&format!("https://github.com/{owner}/{repo}.git"), &token);
    let fetch_args: Vec<String> = git::network_args()
        .into_iter()
        .chain(["fetch".into(), fetch_url, base.clone()])
        .collect();
    let fetch_refs: Vec<&str> = fetch_args.iter().map(|s| s.as_str()).collect();
    let _ = git::run_in(&main, &fetch_refs);
    let slug: String = git::slugify(&title).chars().take(24).collect();
    let branch = format!("fix/{number}-{}", slug.trim_matches('-'));
    let path = git::worktree_dir(&main, &branch);
    if path.exists() {
        return Err(format!("worktree 目录已存在: {}", path.to_string_lossy()));
    }
    git::worktree_add(&main, &path, &branch, true, Some(&format!("origin/{base}")))?;
    let linked = LinkedWorkItem {
        kind: "issue".into(),
        number,
        title: title.clone(),
        url: format!("https://github.com/{owner}/{repo}/issues/{number}"),
    };
    {
        let sp = st.projects.iter_mut().find(|p| p.id == project_id).unwrap();
        upsert_checkout_meta(sp, &path.to_string_lossy(), Some(linked.clone()));
        store::save(&st)?;
    }
    Ok(make_checkout_info(&project_id, &path, Some(linked)))
}

/// 从 PR 开 review worktree：fetch pull/<n>/head 到本地分支 pr-<n>
#[tauri::command]
pub async fn spawn_pr_worktree(state: State<'_, AppState>, project_id: String, number: u64) -> Result<CheckoutInfo, String> {
    let token = github::ensure_token(&state)?;
    let mut st = store::load();
    let (main_path, owner, repo) = {
        let sp = find_project(&st, &project_id)?;
        let id = require_identity(sp)?;
        (sp.local_path.clone(), id.owner.clone(), id.repo.clone())
    };
    let main = PathBuf::from(&main_path);
    let (_head_ref, title, url) = github::pr_detail(&state.http, &token, &owner, &repo, number).await?;
    let branch = format!("pr-{number}");
    // 幂等：分支可能已存在
    let fetch_url = git::auth_url(&format!("https://github.com/{owner}/{repo}.git"), &token);
    let fetch_args: Vec<String> = git::network_args()
        .into_iter()
        .chain(["fetch".into(), fetch_url, format!("pull/{number}/head:{branch}")])
        .collect();
    let fetch_refs: Vec<&str> = fetch_args.iter().map(|s| s.as_str()).collect();
    let _ = git::run_in(&main, &fetch_refs);
    let path = git::worktree_dir(&main, &branch);
    if path.exists() {
        return Err(format!("worktree 目录已存在: {}", path.to_string_lossy()));
    }
    git::worktree_add(&main, &path, &branch, false, None)?;
    let linked = LinkedWorkItem { kind: "pr".into(), number, title, url };
    {
        let sp = st.projects.iter_mut().find(|p| p.id == project_id).unwrap();
        upsert_checkout_meta(sp, &path.to_string_lossy(), Some(linked.clone()));
        store::save(&st)?;
    }
    Ok(make_checkout_info(&project_id, &path, Some(linked)))
}

/// push 当前分支并创建 PR，返回 PR URL
#[tauri::command]
pub async fn create_pr(state: State<'_, AppState>, project_id: String, checkout_path: String, title: String, body: String) -> Result<String, String> {
    let token = github::ensure_token(&state)?;
    let st = store::load();
    let sp = find_project(&st, &project_id)?;
    let identity = require_identity(sp)?;
    let path = PathBuf::from(&checkout_path);
    let branch = git::current_branch(&path);
    if branch.is_empty() {
        return Err("当前处于 detached HEAD，无法创建 PR".into());
    }
    // push 需要鉴权：临时把 origin 换成带 token 的 URL，推完恢复
    let clean_url = git::run_in(&path, &["config", "--get", "remote.origin.url"])
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|_| format!("https://github.com/{}/{}.git", identity.owner, identity.repo));
    let authed = git::auth_url(&clean_url, &token);
    let _ = git::run_in(&path, &["remote", "set-url", "origin", &authed]);
    let push_args: Vec<String> = git::network_args()
        .into_iter()
        .chain(["push".into(), "-u".into(), "origin".into(), branch.clone()])
        .collect();
    let push_refs: Vec<&str> = push_args.iter().map(|s| s.as_str()).collect();
    let push_res = git::run_in(&path, &push_refs);
    let _ = git::run_in(&path, &["remote", "set-url", "origin", &clean_url]);
    push_res?;
    let main = PathBuf::from(&sp.local_path);
    let base = git::default_branch(&main);
    github::create_pr_api(&state.http, &token, &identity.owner, &identity.repo, &title, &branch, &base, &body).await
}
