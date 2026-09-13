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

/// 解析 `owner/repo#number`（sync 引擎写入 github_ref 的格式，与前端
/// model.ts parseGithubRef 同一套规则）；格式不符返回 None（老数据/手改字段容错）
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GithubIssueRef {
    pub owner: String,
    pub repo: String,
    pub number: u64,
}

pub fn parse_github_ref(gref: &str) -> Option<GithubIssueRef> {
    let (slug, number) = gref.split_once('#')?;
    let (owner, repo) = slug.split_once('/')?;
    let plain = |s: &str| {
        !s.is_empty() && !s.contains(|c: char| c == '/' || c == '#' || c.is_whitespace())
    };
    if !plain(owner) || !plain(repo) || !number.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    Some(GithubIssueRef {
        owner: owner.to_string(),
        repo: repo.to_string(),
        number: number.parse().ok()?,
    })
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
    /// GitHub 互通引用，格式 `owner/repo#number`；同步引擎按此匹配 upsert
    #[serde(default)]
    pub github_ref: Option<String>,
    /// 人工拖动过的 GitHub 同步卡片：自动迁移跳过（字段/标题/标签仍刷新）
    #[serde(default)]
    pub manual_lock: bool,
    /// 进入最后一列（done 约定）的时间戳（unix 秒）；离开最后一列时清空。
    /// done 列折叠（#80）按此倒序取「最近完成」，不受标题/标签刷新扰动
    #[serde(default)]
    pub closed_at: Option<i64>,
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

#[cfg(test)]
mod tests {
    use super::*;

    fn parsed(gref: &str) -> Option<GithubIssueRef> {
        parse_github_ref(gref)
    }

    #[test]
    fn parse_github_ref_accepts_owner_repo_number() {
        assert_eq!(
            parsed("coconilu/gitgrove#83"),
            Some(GithubIssueRef {
                owner: "coconilu".into(),
                repo: "gitgrove".into(),
                number: 83,
            })
        );
        // 仓库名可带点/下划线/短横线（GitHub 允许）
        assert_eq!(parsed("a/b.c-d_e#1").unwrap().repo, "b.c-d_e");
    }

    #[test]
    fn parse_github_ref_rejects_malformed() {
        for bad in [
            "",
            "coconilu/gitgrove",
            "coconilu/gitgrove#",
            "coconilu/gitgrove#abc",
            "coconilu/gitgrove#1#2",
            "coconilu#83",
            "/gitgrove#83",
            "coconilu/#83",
            "co conilu/gitgrove#83",
            "coconilu/gi tgrove#83",
            "a/b/c#83",
        ] {
            assert_eq!(parsed(bad), None, "应拒绝: {bad:?}");
        }
    }
}
