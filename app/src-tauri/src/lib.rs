mod agents;
mod fsx;
mod git;
mod github;
mod launch;
mod projects;
mod store;
#[cfg(windows)]
mod window_theme;

use std::sync::Mutex;

pub struct AppState {
    pub token: Mutex<Option<String>>,
    pub http: github::Http,
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .manage(AppState {
            token: Mutex::new(None),
            http: github::Http::new(),
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
            // 项目 / clone
            projects::check_clone_target,
            projects::clone_repo,
            projects::add_existing_project,
            projects::list_projects,
            projects::remove_project,
            // worktree / 分支
            projects::list_branches,
            projects::create_worktree,
            projects::remove_worktree,
            projects::lock_worktree,
            // 文件系统
            fsx::read_dir,
            fsx::read_file_preview,
            fsx::checkout_status,
            fsx::trash_path,
            launch::open_in_editor,
            launch::open_in_terminal,
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
