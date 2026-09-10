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

use tauri::Manager;
use tauri_plugin_window_state::StateFlags;

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

/// 系统托盘：菜单「显示主窗口」/「退出」；tooltip 应用名+版本。
/// macOS 上表现为状态栏图标，行为一致。
fn setup_tray(app: &mut tauri::App) -> tauri::Result<()> {
    use tauri::menu::{MenuBuilder, MenuItemBuilder};
    use tauri::tray::TrayIconBuilder;

    let show = MenuItemBuilder::with_id("show", "显示主窗口").build(app)?;
    let quit = MenuItemBuilder::with_id("quit", "退出").build(app)?;
    let menu = MenuBuilder::new(app).items(&[&show, &quit]).build()?;
    let tooltip = format!("GitGrove {}", app.package_info().version);

    TrayIconBuilder::new()
        .icon(
            app.default_window_icon()
                .expect("未配置默认窗口图标")
                .clone(),
        )
        .tooltip(tooltip)
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "show" => {
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.unminimize();
                    let _ = window.show();
                    let _ = window.set_focus();
                }
            }
            // app.exit 直接结束进程，不触发 CloseRequested 拦截
            "quit" => app.exit(0),
            _ => {}
        })
        .build(app)?;

    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let pm_store = pm::store::PmStore::open(&store::app_data_dir().join("pm.sqlite3"))
        .expect("初始化 PM 数据库失败");
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        // 窗口状态记忆：持久化主窗口尺寸/位置/最大化状态，启动时恢复。
        // 剔除 VISIBLE：关闭按钮是隐藏进托盘而非销毁，若持久化可见性，
        // 从托盘退出时会把 visible=false 写入状态，下次启动窗口直接不显示。
        // 插件恢复位置时会校验与当前显示器的交集，拔掉副屏后落在屏幕外
        // 会自动回退到可见位置（见 tauri-plugin-window-state restore_state）。
        .plugin(
            tauri_plugin_window_state::Builder::new()
                .with_state_flags(StateFlags::all() & !StateFlags::VISIBLE)
                .build(),
        )
        .manage(AppState {
            token: Mutex::new(None),
            http: github::Http::new(),
            has_project_scope: Mutex::new(None),
            projects_v2_cache: Mutex::new(None),
            pm: Mutex::new(pm_store),
        })
        .setup(|app| {
            #[cfg(windows)]
            {
                if let Some(window) = app.get_webview_window("main") {
                    window_theme::apply(&window);
                }
            }

            setup_tray(app)?;

            Ok(())
        })
        // 关闭拦截：点 X 不退出，隐藏进托盘；托盘「退出」走 app.exit，
        // 不触发 CloseRequested，不会死锁
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .invoke_handler(tauri::generate_handler![
            // 认证
            github::auth_status,
            github::login_pat,
            github::logout,
            // My GitHub
            github::list_my_repos,
            // PM（自研项目管理）
            pm::pm_list_statuses,
            pm::pm_update_statuses,
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
            projects::fetch_project,
            projects::pull_checkout,
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
