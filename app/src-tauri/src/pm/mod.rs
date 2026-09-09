// 自研项目管理（P0）：SQLite store + Tauri commands，serde 全 camelCase

pub mod model;
mod order;
pub mod store;

pub use model::{ImportResult, Item, ItemFilter, Milestone, MilestoneWithStats, NewItem, PmExport};

use tauri::State;

use crate::AppState;

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
