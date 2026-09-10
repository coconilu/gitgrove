// 自研项目管理（P0）：SQLite store + Tauri commands，serde 全 camelCase

pub mod model;
mod order;
pub mod store;
pub mod sync;

pub use model::{
    ImportResult, Item, ItemFilter, Milestone, MilestoneWithStats, NewItem, PmExport, StatusDef,
};
pub use sync::SyncResult;

use tauri::State;

use crate::AppState;

#[tauri::command]
pub fn pm_list_statuses(state: State<'_, AppState>) -> Result<Vec<StatusDef>, String> {
    state.pm.lock().unwrap().statuses()
}

/// 整体写回看板列（PUT 语义）：至少一列、id 唯一；被删列上的 item 归到第一列（任务不丢）
#[tauri::command]
pub fn pm_update_statuses(
    state: State<'_, AppState>,
    statuses: Vec<StatusDef>,
) -> Result<Vec<StatusDef>, String> {
    state.pm.lock().unwrap().update_statuses(&statuses)
}

#[tauri::command]
pub fn pm_list_items(
    state: State<'_, AppState>,
    filter: Option<ItemFilter>,
) -> Result<Vec<Item>, String> {
    state.pm.lock().unwrap().list_items(&filter.unwrap_or_default())
}

#[tauri::command]
pub fn pm_create_item(state: State<'_, AppState>, input: NewItem) -> Result<Item, String> {
    state.pm.lock().unwrap().create_item(&input)
}

/// 全字段更新（PUT 语义）：前端传回完整 item，order/createdAt/githubRef 由服务端保留
#[tauri::command]
pub fn pm_update_item(state: State<'_, AppState>, item: Item) -> Result<Item, String> {
    state.pm.lock().unwrap().update_item(&item)
}

/// 跨列移动 + 列内定位：beforeItemId 为 null 时排到目标列尾部
#[tauri::command]
pub fn pm_move_item(
    state: State<'_, AppState>,
    item_id: String,
    to_status: String,
    before_item_id: Option<String>,
) -> Result<Item, String> {
    state
        .pm
        .lock()
        .unwrap()
        .move_item(&item_id, &to_status, before_item_id.as_deref())
}

/// 软删除：item 保留 tombstone，不再出现在列表
#[tauri::command]
pub fn pm_delete_item(state: State<'_, AppState>, item_id: String) -> Result<(), String> {
    state.pm.lock().unwrap().delete_item(&item_id)
}

#[tauri::command]
pub fn pm_list_milestones(state: State<'_, AppState>) -> Result<Vec<MilestoneWithStats>, String> {
    state.pm.lock().unwrap().list_milestones()
}

#[tauri::command]
pub fn pm_create_milestone(
    state: State<'_, AppState>,
    title: String,
    description: Option<String>,
    due_date: Option<String>,
) -> Result<Milestone, String> {
    state
        .pm
        .lock()
        .unwrap()
        .create_milestone(&title, description.as_deref(), due_date.as_deref())
}

#[tauri::command]
pub fn pm_update_milestone(
    state: State<'_, AppState>,
    milestone: Milestone,
) -> Result<Milestone, String> {
    state.pm.lock().unwrap().update_milestone(&milestone)
}

/// 删除 milestone，关联 items 的 milestoneId 置空
#[tauri::command]
pub fn pm_delete_milestone(state: State<'_, AppState>, milestone_id: String) -> Result<(), String> {
    state.pm.lock().unwrap().delete_milestone(&milestone_id)
}

/// 整库导出（不含 tombstone）
#[tauri::command]
pub fn pm_export_json(state: State<'_, AppState>) -> Result<PmExport, String> {
    state.pm.lock().unwrap().export()
}

/// 整库导入（replace-all，单事务）
#[tauri::command]
pub fn pm_import_json(state: State<'_, AppState>, data: PmExport) -> Result<ImportResult, String> {
    state.pm.lock().unwrap().import(&data)
}

/// GitHub issue → 看板同步（手动触发；只读 GitHub，不回写）：
/// 按 project_id 找 provider 仓库，拉 open + 近期 closed issues 与 open PR，
/// upsert 卡片（新卡落 todo）并按 LinkedWorkItem / PR 引用信号自动迁移列。
#[tauri::command]
pub async fn pm_sync_github(
    state: State<'_, AppState>,
    project_id: String,
) -> Result<SyncResult, String> {
    let st = crate::store::load(); // projects.json（注意与 pm::store 区分）
    let sp = st
        .projects
        .iter()
        .find(|p| p.id == project_id)
        .ok_or_else(|| format!("项目不存在: {project_id}"))?;
    let identity = sp
        .provider_identity
        .as_ref()
        .ok_or_else(|| crate::projects::ERR_NO_GITHUB.to_string())?;
    if identity.provider != "github" {
        return Err(format!("暂不支持的服务商: {}", identity.provider));
    }
    let links: Vec<crate::store::LinkedWorkItem> = sp
        .checkouts
        .iter()
        .filter_map(|c| c.linked_work_item.clone())
        .collect();
    let token = crate::github::ensure_token(&state)?;
    // 网络阶段不持 pm 锁；issues 首页与 open PR 并行拉取
    let (issues, prs) = tokio::join!(
        crate::github::fetch_issues_for_sync(&state.http, &token, &identity.owner, &identity.repo),
        crate::github::fetch_open_prs_for_sync(&state.http, &token, &identity.owner, &identity.repo),
    );
    let issues = issues?;
    let prs = prs?;
    let pr_texts: Vec<String> = prs
        .iter()
        .map(|p| format!("{}\n{}", p.title, p.body.as_deref().unwrap_or_default()))
        .collect();
    let signals = sync::build_signals(&links, &pr_texts);
    let snapshots = sync::recent_issues(
        issues.into_iter().map(sync::GithubIssueSnapshot::from).collect(),
        store::now_ts(),
    );
    state
        .pm
        .lock()
        .unwrap()
        .sync_github(&identity.owner, &identity.repo, &snapshots, &signals)
}
