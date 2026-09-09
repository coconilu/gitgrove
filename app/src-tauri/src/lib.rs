mod agents;
mod fsx;
mod git;
mod github;
mod launch;
mod pm;
mod projects;
mod store;
#[cfg(windows)]
mod window_theme;

use std::sync::Mutex;

pub struct AppState {
    pub token: Mutex<Option<String>>,
    pub http: github::Http,
    /// 当前 token 是否具备 project scope（None = 未探测/无法判断），登录与 auth_status 时记录
    pub has_project_scope: Mutex<Option<bool>>,
    /// Projects V2 后端已退役（自研 PM 模块替代）。github.rs 登录/登出时仍会清空此槽位，
    /// 保留占位避免跨 scope 改动；P2 GitHub 互通若需 items 缓存可复用
    pub projects_v2_cache: Mutex<Option<()>>,
    /// 自研 PM 数据层（SQLite，app 数据目录 pm.sqlite3）
    pub pm: Mutex<pm::store::PmStore>,
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let pm_store = pm::store::PmStore::open(&store::app_data_dir().join("pm.sqlite3"))
        .expect("初始化 PM 数据库失败");
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .manage(AppState {
            token: Mutex::new(None),
            http: github::Http::new(),
            has_project_scope: Mutex::new(None),
            projects_v2_cache: Mutex::new(None),
            pm: Mutex::new(pm_store),
        })
        .setup(|_app| {
            #[cfg(windows)]
            {
                use tauri::Manager;
                if let Some(window) = _app.get_webview_window("main") {
                    window_theme::apply(&window);
                }
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            // 认证
            github::auth_status,
            github::login_pat,
            github::logout,
            // My GitHub
            github::list_my_repos,
            // Projects V2（已退役，P2 互通时由 pm 模块承接）
            // PM（自研项目管理）
            pm::pm_list_items,
            pm::pm_create_item,
            pm::pm_update_item,
            pm::pm_move_item,
            pm::pm_delete_item,
            pm::pm_list_milestones,
            pm::pm_create_milestone,
            pm::pm_update_milestone,
            pm::pm_delete_milestone,
            pm::pm_export_json,
            pm::pm_import_json,
            // 项目 / clone
            projects::check_clone_target,
            projects::clone_repo,
            projects::add_existing_project,
            projects::list_projects,
            projects::remove_project,
            // worktree / 分支
            projects::list_branches,
            projects::merged_branches_plan,
            projects::delete_merged_branches,
            projects::create_worktree,
            projects::remove_worktree,
            projects::lock_worktree,
            // 文件系统
            fsx::read_dir,
            fsx::read_file_preview,
            fsx::checkout_status,
            fsx::allow_asset_scope,
            fsx::trash_path,
            launch::open_in_editor,
            launch::open_in_terminal,
            launch::open_in_zcode,
            launch::get_launch_preferences,
            launch::save_launch_preferences,
            launch::discover_launch_apps,
            agents::agent_open_support,
            agents::open_in_agent,
            fsx::reveal_in_explorer,
            // issues / PRs
            github::list_issues,
            github::list_prs,
            projects::spawn_issue_worktree,
            projects::spawn_pr_worktree,
            projects::create_pr,
            // Actions
            github::list_workflows,
            github::workflow_details,
            github::list_runs,
            github::rerun_run,
            github::cancel_run,
            github::dispatch_workflow,
            github::latest_run_for_branch,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
