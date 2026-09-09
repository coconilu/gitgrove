// PM 数据模型：Item / Milestone / 看板列定义，serde 全 camelCase 供前端直接绑定

use serde::{Deserialize, Serialize};

/// 默认看板列（首次启动写入 settings，之后以 settings 表为准，可增删改）
pub const DEFAULT_STATUSES: [(&str, &str); 4] = [
    ("backlog", "Backlog"),
    ("todo", "Todo"),
    ("doing", "Doing"),
    ("done", "Done"),
];

pub const PRIORITIES: [&str; 5] = ["none", "low", "medium", "high", "urgent"];

pub fn default_status() -> String {
    DEFAULT_STATUSES[0].0.to_string()
}

pub fn is_valid_priority(p: &str) -> bool {
    PRIORITIES.contains(&p)
}

/// 看板列定义，持久化在 settings 表（key = "statuses"，JSON 数组）
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StatusDef {
    pub id: String,
    pub name: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Item {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub body: String,
    #[serde(default = "default_status")]
    pub status: String,
    #[serde(default)]
    pub priority: String,
    #[serde(default)]
    pub milestone_id: Option<String>,
    #[serde(default)]
    pub labels: Vec<String>,
    #[serde(default)]
    pub repo_path: Option<String>,
    #[serde(default)]
    pub branch: Option<String>,
    /// YYYY-MM-DD
    #[serde(default)]
    pub due_date: Option<String>,
    /// 看板列内排序键（fractional indexing，字符串序）
    #[serde(rename = "order", default)]
    pub order: String,
    /// P2 GitHub 互通预留位（issue/PR 引用），本期不填
    #[serde(default)]
    pub github_ref: Option<String>,
    #[serde(default)]
    pub created_at: i64,
    #[serde(default)]
    pub updated_at: i64,
    /// 软删除 tombstone；导出时剔除
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deleted_at: Option<i64>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Milestone {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub due_date: Option<String>,
    /// open | closed
    #[serde(default)]
    pub status: String,
    /// P2 GitHub 互通预留位
    #[serde(default)]
    pub github_ref: Option<String>,
    #[serde(default)]
    pub created_at: i64,
    #[serde(default)]
    pub updated_at: i64,
}

/// pm_list_milestones 返回：附带 item 聚合（done = 最后一个看板列）
#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct MilestoneWithStats {
    #[serde(flatten)]
    pub milestone: Milestone,
    pub total: i64,
    pub done: i64,
}

/// pm_list_items 的过滤参数，全部可选；labels 为「同时包含」语义
#[derive(Deserialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct ItemFilter {
    pub milestone_id: Option<String>,
    pub repo_path: Option<String>,
    pub status: Option<String>,
    pub labels: Option<Vec<String>>,
    pub priority: Option<String>,
    pub search: Option<String>,
}

/// pm_create_item 入参（id / order / 时间戳由后端生成）
#[derive(Deserialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct NewItem {
    pub title: String,
    pub body: String,
    pub status: Option<String>,
    pub priority: Option<String>,
    pub milestone_id: Option<String>,
    pub labels: Vec<String>,
    pub repo_path: Option<String>,
    pub branch: Option<String>,
    pub due_date: Option<String>,
}

/// pm_export_json / pm_import_json 的整库快照（不含 tombstone）
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PmExport {
    pub version: u32,
    pub statuses: Vec<StatusDef>,
    pub items: Vec<Item>,
    pub milestones: Vec<Milestone>,
}

/// pm_import_json 返回的计数，供前端 toast
#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ImportResult {
    pub items: usize,
    pub milestones: usize,
}
