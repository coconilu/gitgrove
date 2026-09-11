// GitHub issue → 看板同步引擎（读 GitHub 写本地，不回写）。
// 网络层在 github.rs（fetch_*_for_sync），本文件只做纯策略：信号解析、
// 近期窗口过滤、upsert 与列迁移——全部可注入假数据单测，不打真网。

use std::collections::HashSet;

use rusqlite::params;
use serde::Serialize;

use super::order;
use super::store::{PmStore, new_id, now_ts, row_to_item, ITEM_COLS};
use crate::store::LinkedWorkItem;

/// 同步期间单个 issue 的最小快照（assignees 仅用于证明「仅指派不算 doing 信号」）
pub struct GithubIssueSnapshot {
    pub number: u64,
    pub title: String,
    /// open | closed
    pub state: String,
    pub labels: Vec<String>,
    pub assignees: Vec<String>,
    /// RFC3339 解析后的 closed 时间（unix 秒）；open 恒为 None
    pub closed_at: Option<i64>,
}

/// doing 信号：issue 被本项目任一 checkout 的 LinkedWorkItem 关联，
/// 或被仓库 open PR 以 closes/fixes/resolves #N 引用
pub struct SyncSignals {
    pub linked_issues: HashSet<u64>,
    pub pr_referenced: HashSet<u64>,
}

impl SyncSignals {
    fn has(&self, number: u64) -> bool {
        self.linked_issues.contains(&number) || self.pr_referenced.contains(&number)
    }
}

/// 从项目 checkouts 的 LinkedWorkItem 收集信号。
/// 只有 kind="issue" 直接入 doing 信号；kind="pr" 的工作树经该 PR 的
/// open 状态 + closes 扫描间接生效（fetch_open_prs_for_sync 已覆盖）
pub fn linked_issue_numbers(links: &[LinkedWorkItem]) -> HashSet<u64> {
    links
        .iter()
        .filter(|l| l.kind == "issue")
        .map(|l| l.number)
        .collect()
}

/// 扫文本里的 closes/fixes/resolves 系关键词 + #N（大小写不敏感、词边界，
/// 对齐 GitHub 收口规则；跨仓 o/r#N 形态不处理——同步按同仓 issue 粒度）
///
/// #74：按 char_indices 遍历而非按字节步进——中文 PR 标题/正文下按字节索引
/// 切片会在多字节字符内部 panic（byte index not a char boundary，async command
/// 里 panic 即整个同步永不返回）。词边界语义不变：非 ASCII 字符一律视作
/// 非词字符（与 GitHub \b 对 CJK 的行为一致，"同步closes #12" 照样命中）。
/// 内层的空白/冒号/#/数字扫描只消费 ASCII 字节，切片必落在字符边界上。
pub fn parse_close_refs(text: &str) -> Vec<u64> {
    const KEYWORDS: [&str; 9] = [
        "closes", "closed", "close", "fixes", "fixed", "fix", "resolves", "resolved", "resolve",
    ];
    let lower = text.to_lowercase();
    let bytes = lower.as_bytes();
    let mut out = Vec::new();
    let mut prev_ascii_alnum = false;
    for (i, ch) in lower.char_indices() {
        // 只在词首尝试：前一个字符不是字母数字（"unclosed #9" 不会命中 closed）
        if i == 0 || !prev_ascii_alnum {
            for kw in KEYWORDS {
                if lower[i..].starts_with(kw) {
                    let end = i + kw.len();
                    // 词尾边界：关键词后不能紧跟字母数字（"close" 不吃掉 "closes"）
                    let tail_ok = end >= bytes.len() || !bytes[end].is_ascii_alphanumeric();
                    if !tail_ok {
                        continue;
                    }
                    // 关键词后允许空白/冒号分隔，直到 #数字
                    let mut j = end;
                    while j < bytes.len() && (bytes[j].is_ascii_whitespace() || bytes[j] == b':') {
                        j += 1;
                    }
                    if j < bytes.len() && bytes[j] == b'#' {
                        let mut k = j + 1;
                        while k < bytes.len() && bytes[k].is_ascii_digit() {
                            k += 1;
                        }
                        if k > j + 1 {
                            if let Ok(n) = lower[j + 1..k].parse::<u64>() {
                                out.push(n);
                            }
                        }
                    }
                    break; // 该词首只消费一次
                }
            }
        }
        prev_ascii_alnum = ch.is_ascii_alphanumeric();
    }
    out
}

/// 汇总两类 doing 信号
pub fn build_signals(links: &[LinkedWorkItem], open_pr_texts: &[String]) -> SyncSignals {
    SyncSignals {
        linked_issues: linked_issue_numbers(links),
        pr_referenced: open_pr_texts.iter().flat_map(|t| parse_close_refs(t)).collect(),
    }
}

/// closed 收录窗口：近 30 天，且至少保留最近 50 条（避免全量历史倒灌 done 列）
pub const CLOSED_WINDOW_SECS: i64 = 30 * 24 * 3600;
pub const CLOSED_KEEP_MIN: usize = 50;

/// open 全保留；closed 只保留近 30 天或最近 50 条（二者并集）
pub fn recent_issues(issues: Vec<GithubIssueSnapshot>, now: i64) -> Vec<GithubIssueSnapshot> {
    let (open, mut closed): (Vec<_>, Vec<_>) =
        issues.into_iter().partition(|i| i.state != "closed");
    closed.sort_by_key(|i| std::cmp::Reverse(i.closed_at));
    let kept: Vec<GithubIssueSnapshot> = closed
        .into_iter()
        .enumerate()
        .filter(|(idx, i)| {
            *idx < CLOSED_KEEP_MIN
                || i.closed_at.is_some_and(|t| now - t <= CLOSED_WINDOW_SECS)
        })
        .map(|(_, i)| i)
        .collect();
    open.into_iter().chain(kept).collect()
}

#[derive(Serialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct SyncResult {
    pub created: usize,
    pub updated: usize,
    pub moved: usize,
}

impl PmStore {
    /// 同步主入口：按 `owner/repo#number` upsert 卡片并按规则迁移列。
    /// issues 应先经 recent_issues 过滤；单条失败即返回错误（可重试续跑）。
    pub fn sync_github(
        &self,
        owner: &str,
        repo: &str,
        issues: &[GithubIssueSnapshot],
        signals: &SyncSignals,
    ) -> Result<SyncResult, String> {
        let statuses = self.statuses()?;
        if statuses.is_empty() {
            return Err("看板列为空".into());
        }
        // 新卡落 todo；自定义列集可能没有 id "todo"，兜底第二列（todo 的默认位），再兜底第一列
        let todo_id = statuses
            .iter()
            .find(|s| s.id == "todo")
            .or_else(|| statuses.get(1))
            .unwrap_or(&statuses[0])
            .id
            .clone();
        // doing 信号列：id 固定 "doing"，用户删过该列则跳过 doing 迁移规则
        let doing = statuses.iter().find(|s| s.id == "doing").map(|s| s.id.clone());
        let done_id = statuses.last().unwrap().id.clone();
        let pos = |status: &str| statuses.iter().position(|s| s.id == status);

        let mut result = SyncResult::default();
        for issue in issues {
            let gref = format!("{owner}/{repo}#{}", issue.number);
            let item = match self.find_by_github_ref(&gref)? {
                Some(item) => {
                    // 标题/标签随 GitHub 刷新；status/order/milestoneId/manualLock 不动
                    if item.title != issue.title || item.labels != issue.labels {
                        self.update_sync_fields(&item, &issue.title, &issue.labels)?;
                        result.updated += 1;
                    }
                    item
                }
                None => {
                    let item = self.create_sync_card(&gref, issue, &todo_id)?;
                    result.created += 1;
                    item
                }
            };
            if item.manual_lock {
                continue; // 人工锁定：字段可刷新，列位置不自动迁移
            }
            if issue.state == "closed" {
                if item.status != done_id {
                    self.sync_move(&item, &done_id)?;
                    result.moved += 1;
                }
            } else if item.status == done_id {
                // issue 在 done 列上重新打开 → 回 todo（唯一允许的回退）
                self.sync_move(&item, &todo_id)?;
                result.moved += 1;
            } else if signals.has(issue.number) {
                if let Some(doing_id) = &doing {
                    // 只前进：卡片已在 doing 或其后（自定义列序）则不动
                    let before_doing = pos(doing_id)
                        .is_some_and(|d| pos(&item.status).is_some_and(|p| p < d));
                    if item.status != *doing_id && before_doing {
                        self.sync_move(&item, doing_id)?;
                        result.moved += 1;
                    }
                }
            }
            // open 无信号：维持现状（信号消失不回退，仅 reopen 走上面的分支）
        }
        Ok(result)
    }

    fn find_by_github_ref(&self, gref: &str) -> Result<Option<super::model::Item>, String> {
        self.conn
            .query_row(
                &format!("SELECT {ITEM_COLS} FROM items WHERE github_ref = ?1 AND deleted_at IS NULL"),
                params![gref],
                row_to_item,
            )
            .map(Some)
            .or_else(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => Ok(None),
                e => Err(e.to_string()),
            })
    }

    /// 同步新建卡片：落指定列尾部，仅标题/标签/githubRef，其余取默认
    fn create_sync_card(
        &self,
        gref: &str,
        issue: &GithubIssueSnapshot,
        status: &str,
    ) -> Result<super::model::Item, String> {
        let last = self.last_key_in_column(status, None)?;
        let ord = order::key_between(last.as_deref(), None)?;
        let now = now_ts();
        let item = super::model::Item {
            id: new_id(),
            title: issue.title.clone(),
            body: String::new(),
            status: status.to_string(),
            priority: "none".into(),
            milestone_id: None,
            labels: issue.labels.clone(),
            repo_path: None,
            branch: None,
            due_date: None,
            order: ord,
            github_ref: Some(gref.to_string()),
            manual_lock: false,
            created_at: now,
            updated_at: now,
            deleted_at: None,
        };
        self.insert_item(&item)?;
        Ok(item)
    }

    /// 只刷标题/标签（不动 status/order/milestoneId/manualLock）
    fn update_sync_fields(
        &self,
        item: &super::model::Item,
        title: &str,
        labels: &[String],
    ) -> Result<(), String> {
        let labels = serde_json::to_string(labels).map_err(|e| e.to_string())?;
        self.conn
            .execute(
                "UPDATE items SET title=?2, labels=?3, updated_at=?4 WHERE id=?1 AND deleted_at IS NULL",
                params![item.id, title, labels, now_ts()],
            )
            .map_err(|e| format!("同步刷新 item 失败: {e}"))?;
        Ok(())
    }

    /// 同步迁移：挪到目标列尾部
    fn sync_move(&self, item: &super::model::Item, to_status: &str) -> Result<(), String> {
        let last = self.last_key_in_column(to_status, Some(&item.id))?;
        let key = order::key_between(last.as_deref(), None)?;
        self.conn
            .execute(
                "UPDATE items SET status=?2, ord=?3, updated_at=?4 WHERE id=?1 AND deleted_at IS NULL",
                params![item.id, to_status, key, now_ts()],
            )
            .map_err(|e| format!("同步迁移 item 失败: {e}"))?;
        Ok(())
    }
}

impl From<crate::github::SyncIssue> for GithubIssueSnapshot {
    fn from(i: crate::github::SyncIssue) -> Self {
        GithubIssueSnapshot {
            number: i.number,
            title: i.title,
            state: i.state,
            labels: i.labels.into_iter().map(|l| l.name).collect(),
            assignees: i.assignees,
            closed_at: i.closed_at.as_deref().and_then(parse_rfc3339_to_epoch),
        }
    }
}

/// GitHub RFC3339 UTC 时间（`2026-09-10T12:34:56Z`，可带小数秒）→ unix 秒
fn parse_rfc3339_to_epoch(s: &str) -> Option<i64> {
    let (date, time) = s.split_once('T')?;
    let mut dp = date.split('-');
    let y: i64 = dp.next()?.parse().ok()?;
    let m: u32 = dp.next()?.parse().ok()?;
    let d: u32 = dp.next()?.parse().ok()?;
    let mut tp = time.trim_end_matches('Z').split(':');
    let hh: i64 = tp.next()?.parse().ok()?;
    let mm: i64 = tp.next()?.parse().ok()?;
    let ss: i64 = tp.next()?.split('.').next()?.parse().ok()?;
    Some(days_from_civil(y, m, d) * 86400 + hh * 3600 + mm * 60 + ss)
}

/// Hinnant days_from_civil：公历日期 → 自 1970-01-01 起的天数
fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = y - (m <= 2) as i64;
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = if m > 2 { m as i64 - 3 } else { m as i64 + 9 };
    let doy = (153 * mp + 2) / 5 + d as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pm::model::{Item, ItemFilter};
    use crate::pm::store::PmStore;

    fn issue(number: u64, state: &str) -> GithubIssueSnapshot {
        GithubIssueSnapshot {
            number,
            title: format!("issue {number}"),
            state: state.into(),
            labels: vec![],
            assignees: vec![],
            closed_at: None,
        }
    }

    fn issue_closed_at(number: u64, closed_at: i64) -> GithubIssueSnapshot {
        GithubIssueSnapshot { closed_at: Some(closed_at), ..issue(number, "closed") }
    }

    fn sync1(
        store: &PmStore,
        issues: &[GithubIssueSnapshot],
        linked: &[u64],
        pr_refs: &[u64],
    ) -> SyncResult {
        store
            .sync_github(
                "o",
                "r",
                issues,
                &SyncSignals {
                    linked_issues: linked.iter().copied().collect(),
                    pr_referenced: pr_refs.iter().copied().collect(),
                },
            )
            .unwrap()
    }

    fn card(store: &PmStore, number: u64) -> Item {
        store
            .list_items(&ItemFilter::default())
            .unwrap()
            .into_iter()
            .find(|i| i.github_ref.as_deref() == Some(&format!("o/r#{number}")))
            .unwrap()
    }

    #[test]
    fn upsert_is_idempotent() {
        let store = PmStore::open_memory();
        let r = sync1(&store, &[issue(1, "open")], &[], &[]);
        assert_eq!((r.created, r.updated, r.moved), (1, 0, 0));
        let r2 = sync1(&store, &[issue(1, "open")], &[], &[]);
        assert_eq!((r2.created, r2.updated, r2.moved), (0, 0, 0));
        let all = store.list_items(&ItemFilter::default()).unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].github_ref.as_deref(), Some("o/r#1"));
        assert_eq!(all[0].status, "todo"); // 新 issue 落 todo
    }

    #[test]
    fn refreshes_title_labels_without_touching_position() {
        let store = PmStore::open_memory();
        sync1(&store, &[issue(1, "open")], &[], &[]);
        let before = card(&store, 1);
        store.move_item(&before.id, "doing", None).unwrap();
        let moved = card(&store, 1);

        let mut renamed = issue(1, "open");
        renamed.title = "改名了".into();
        renamed.labels = vec!["bug".into()];
        let r = sync1(&store, &[renamed], &[], &[]);
        assert_eq!((r.created, r.updated, r.moved), (0, 1, 0));
        let after = card(&store, 1);
        assert_eq!(after.title, "改名了");
        assert_eq!(after.labels, vec!["bug".to_string()]);
        assert_eq!(after.status, "doing"); // 字段刷新不动列
        assert_eq!(after.order, moved.order);
        // 无变化时不再写
        let mut renamed2 = issue(1, "open");
        renamed2.title = "改名了".into();
        renamed2.labels = vec!["bug".into()];
        let r2 = sync1(&store, &[renamed2], &[], &[]);
        assert_eq!((r2.created, r2.updated, r2.moved), (0, 0, 0));
    }

    #[test]
    fn closed_issue_moves_to_last_column() {
        let store = PmStore::open_memory();
        sync1(&store, &[issue(1, "open")], &[], &[]);
        assert_eq!(card(&store, 1).status, "todo");
        // 近期 closed：todo → done（最后一列）
        let r = sync1(&store, &[issue(1, "closed")], &[], &[]);
        assert_eq!(r.moved, 1);
        assert_eq!(card(&store, 1).status, "done");
        // 已在 done：重复 closed 同步不再动
        let r2 = sync1(&store, &[issue(1, "closed")], &[], &[]);
        assert_eq!((r2.created, r2.updated, r2.moved), (0, 0, 0));
        assert_eq!(card(&store, 1).status, "done");
    }

    #[test]
    fn new_closed_issue_lands_done_directly() {
        let store = PmStore::open_memory();
        sync1(&store, &[issue(1, "closed")], &[], &[]);
        assert_eq!(card(&store, 1).status, "done");
    }

    #[test]
    fn linked_worktree_signal_moves_to_doing() {
        let store = PmStore::open_memory();
        let r = sync1(&store, &[issue(1, "open")], &[1], &[]);
        assert_eq!((r.created, r.moved), (1, 1));
        assert_eq!(card(&store, 1).status, "doing");
        // 已在 doing：重复同步不动
        let r2 = sync1(&store, &[issue(1, "open")], &[1], &[]);
        assert_eq!(r2.moved, 0);
    }

    #[test]
    fn pr_reference_signal_moves_to_doing() {
        let store = PmStore::open_memory();
        sync1(&store, &[issue(1, "open")], &[], &[1]);
        assert_eq!(card(&store, 1).status, "doing");
    }

    #[test]
    fn assignee_alone_is_not_a_doing_signal() {
        let store = PmStore::open_memory();
        let mut assigned = issue(1, "open");
        assigned.assignees = vec!["someone".into()];
        sync1(&store, &[assigned], &[], &[]);
        assert_eq!(card(&store, 1).status, "todo");
    }

    #[test]
    fn signal_loss_does_not_regress() {
        let store = PmStore::open_memory();
        sync1(&store, &[issue(1, "open")], &[1], &[]);
        assert_eq!(card(&store, 1).status, "doing");
        // 信号消失（工作树删了、PR 关了）：不回退 todo
        let r = sync1(&store, &[issue(1, "open")], &[], &[]);
        assert_eq!(r.moved, 0);
        assert_eq!(card(&store, 1).status, "doing");
    }

    #[test]
    fn reopen_moves_done_back_to_todo() {
        let store = PmStore::open_memory();
        sync1(&store, &[issue(1, "closed")], &[], &[]);
        assert_eq!(card(&store, 1).status, "done");
        sync1(&store, &[issue(1, "open")], &[], &[]);
        assert_eq!(card(&store, 1).status, "todo");
    }

    #[test]
    fn manual_lock_skips_migration_but_refreshes_fields() {
        let store = PmStore::open_memory();
        sync1(&store, &[issue(1, "open")], &[], &[]);
        let locked = card(&store, 1);
        store
            .update_item(&Item { manual_lock: true, ..locked.clone() })
            .unwrap();

        let mut closed = issue(1, "closed");
        closed.title = "issue 1 (closed)".into();
        let r = sync1(&store, &[closed], &[], &[]);
        // 字段刷新了，位置没动
        assert_eq!(r.updated, 1);
        assert_eq!(r.moved, 0);
        let after = card(&store, 1);
        assert_eq!(after.status, "todo");
        assert_eq!(after.title, "issue 1 (closed)");
        assert!(after.manual_lock);
    }

    #[test]
    fn custom_columns_todo_fallback_and_doing_skip() {
        let store = PmStore::open_memory();
        store
            .update_statuses(&[
                crate::pm::model::StatusDef { id: "icebox".into(), name: "Icebox".into() },
                crate::pm::model::StatusDef { id: "wip".into(), name: "Wip".into() },
                crate::pm::model::StatusDef { id: "shipped".into(), name: "Shipped".into() },
            ])
            .unwrap();
        // 没有 todo 列：新卡落第二列 wip
        sync1(&store, &[issue(1, "open")], &[1], &[]);
        assert_eq!(card(&store, 1).status, "wip"); // 无 doing 列：doing 规则跳过
        sync1(&store, &[issue(2, "closed")], &[], &[]);
        assert_eq!(card(&store, 2).status, "shipped"); // closed → 最后一列
    }

    #[test]
    fn doing_rule_only_moves_forward() {
        let store = PmStore::open_memory();
        store
            .update_statuses(&[
                crate::pm::model::StatusDef { id: "todo".into(), name: "Todo".into() },
                crate::pm::model::StatusDef { id: "doing".into(), name: "Doing".into() },
                crate::pm::model::StatusDef { id: "review".into(), name: "Review".into() },
                crate::pm::model::StatusDef { id: "done".into(), name: "Done".into() },
            ])
            .unwrap();
        sync1(&store, &[issue(1, "open")], &[], &[]);
        let item = card(&store, 1);
        assert_eq!(item.status, "todo");
        // 人工挪到 review（doing 之后）：信号出现也不回退
        store.move_item(&item.id, "review", None).unwrap();
        let r = sync1(&store, &[issue(1, "open")], &[1], &[]);
        assert_eq!(r.moved, 0);
        assert_eq!(card(&store, 1).status, "review");
    }

    #[test]
    fn recent_issues_keeps_open_all_and_windowed_closed() {
        let now = 1_800_000_000;
        let day = 24 * 3600;
        let mut issues = vec![
            issue(1, "open"),
            issue_closed_at(2, now - 5 * day),  // 窗口内 → 保留
            issue_closed_at(3, now - 45 * day), // 窗口外且非最近 50 → 丢弃
        ];
        // 60 条 40 天前 closed：只保留最近 50 条
        for n in 10..70 {
            issues.push(issue_closed_at(n, now - 40 * day));
        }
        let kept = recent_issues(issues, now);
        assert!(kept.iter().any(|i| i.number == 1));
        assert!(kept.iter().any(|i| i.number == 2));
        assert!(!kept.iter().any(|i| i.number == 3));
        // 「最近 50 条」是全部 closed 共享的配额：issue2（5d 前最新）占掉一个名额，
        // 40d 组只剩 49 个位置
        let old_kept = kept.iter().filter(|i| (10..70).contains(&i.number)).count();
        assert_eq!(old_kept, 49);
    }

    #[test]
    fn parse_close_refs_scanner() {
        assert_eq!(parse_close_refs("Closes #12"), vec![12]);
        assert_eq!(parse_close_refs("fixes #3, resolves #4"), vec![3, 4]);
        assert_eq!(parse_close_refs("Fixes: #7"), vec![7]);
        assert_eq!(parse_close_refs("close\n #42"), vec![42]);
        assert_eq!(parse_close_refs("FIX #1 and CLOSES #2"), vec![1, 2]);
        // 非信号 / 词边界否定用例
        assert!(parse_close_refs("no refs, see issue #5").is_empty());
        assert!(parse_close_refs("unclosed #9").is_empty());
        assert!(parse_close_refs("closing #8").is_empty());
        assert!(parse_close_refs("fixable #6").is_empty());
        assert!(parse_close_refs("fixed the bug without ref").is_empty());
    }

    /// #74 回归：中文标题/正文按字符遍历不再 panic（曾按字节扫词，多字节字符
    /// 内部切片直接 panic，同步链路整个崩掉）。词边界语义：CJK 是非词字符，
    /// 紧邻的关键词照样命中（对齐 GitHub \b 行为）。
    #[test]
    fn parse_close_refs_handles_multibyte_text() {
        // PR #52 真实形状：标题 + 中文开头的 body（曾让 pm_sync_github 必 panic）
        let pr_text = "chore: release v1.2.3\n同步版本与 CHANGELOG 到 v1.2.3。\n\n本机手动发布自动运行版本提交的 CI，若通过后按分支使用护栏规则合并。";
        assert!(parse_close_refs(pr_text).is_empty());
        // 中文紧邻关键词 / 全角标点后跟关键词：词边界成立，正常解析
        assert_eq!(parse_close_refs("同步修复 Closes #12"), vec![12]);
        assert_eq!(parse_close_refs("中文 fixes: #7 与 Fixes #9"), vec![7, 9]);
        assert_eq!(parse_close_refs("修.fixes #3"), vec![3]);
        assert_eq!(parse_close_refs("修复了问题。resolves #8"), vec![8]);
        // 关键词后紧跟中文（无空白/冒号分隔）：不解析出编号，与 ASCII 行为一致
        assert!(parse_close_refs("closes中文 #12").is_empty());
    }

    /// #74 回归：build_signals 吃真实中文 pr_texts 不 panic 且信号正常
    #[test]
    fn build_signals_survives_multibyte_pr_texts() {
        let s = build_signals(&[], &["chore: release v1.2.3\n同步版本与 CHANGELOG 到 v1.2.3。".into()]);
        assert!(s.linked_issues.is_empty() && s.pr_referenced.is_empty());
        let s = build_signals(&[], &["同步版本，closes #5".into()]);
        assert!(s.pr_referenced.contains(&5));
    }

    #[test]
    fn build_signals_merges_linked_and_pr_refs() {
        let links = vec![
            LinkedWorkItem { kind: "issue".into(), number: 1, title: "a".into(), url: String::new() },
            LinkedWorkItem { kind: "pr".into(), number: 20, title: "b".into(), url: String::new() },
        ];
        let s = build_signals(&links, &["This closes #2 and fixes #3".into()]);
        assert!(s.linked_issues.contains(&1));
        assert!(!s.linked_issues.contains(&20)); // pr 关联的工作树走 open-PR 扫描，不直接入信号
        assert!(s.pr_referenced.contains(&2) && s.pr_referenced.contains(&3));
        assert!(s.has(1) && s.has(2) && s.has(3) && !s.has(9));
    }

    #[test]
    fn rfc3339_parsing() {
        assert_eq!(parse_rfc3339_to_epoch("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(parse_rfc3339_to_epoch("2024-02-29T00:00:00Z"), Some(1709164800));
        assert_eq!(parse_rfc3339_to_epoch("2000-03-01T12:00:00Z"), Some(951868800 + 43200));
        assert_eq!(parse_rfc3339_to_epoch("2024-02-29T00:00:00.123Z"), Some(1709164800));
        assert_eq!(parse_rfc3339_to_epoch("not-a-date"), None);
    }

    #[test]
    fn snapshot_conversion_from_api_shape() {
        use crate::github::{LabelInfo, SyncIssue};
        let api = SyncIssue {
            number: 7,
            title: "t".into(),
            state: "closed".into(),
            labels: vec![
                LabelInfo { name: "bug".into(), color: "ff0000".into() },
                LabelInfo { name: "p1".into(), color: "00ff00".into() },
            ],
            assignees: vec!["alice".into()],
            url: "https://github.com/o/r/issues/7".into(),
            created_at: "2026-08-01T00:00:00Z".into(),
            closed_at: Some("2026-08-11T00:00:00Z".into()),
            body: None,
        };
        let snap: GithubIssueSnapshot = api.into();
        assert_eq!(snap.labels, vec!["bug".to_string(), "p1".to_string()]);
        assert_eq!(snap.assignees, vec!["alice".to_string()]);
        assert_eq!(snap.closed_at, parse_rfc3339_to_epoch("2026-08-11T00:00:00Z"));
    }

    /// 真实网络冒烟（#[ignore]，本地手动跑；CI 不依赖外网/登录）：
    /// 复刻 pm_sync_github 的完整后端链路——ensure_token（keyring → gh CLI 兜底，
    /// 正是 #66 断掉的环节）→ 并行拉 issues+open PR → recent_issues 过滤 →
    /// 写入临时库。用公开仓 coconilu/gitgrove 验证验收标准：sync 后 open issue
    /// 全部落 todo（含 #53/#40/#29）。
    /// 跑法：cargo test real_sync_chain -- --ignored --nocapture
    #[test]
    #[ignore = "真实网络：依赖本机凭据与 GitHub 连通性"]
    fn real_sync_chain_lands_open_issues_in_todo() {
        use std::sync::Mutex;
        let state = crate::AppState {
            token: Mutex::new(None),
            http: crate::github::Http::new(),
            has_project_scope: Mutex::new(None),
            projects_v2_cache: Mutex::new(None),
            pm: Mutex::new(PmStore::open_memory()),
        };
        let token = crate::github::ensure_token(&state)
            .expect("ensure_token 失败：keyring 与 gh CLI 均未提供 token");
        let http = &state.http;

        let (issues, prs) = tokio::runtime::Runtime::new().unwrap().block_on(async {
            tokio::join!(
                crate::github::fetch_issues_for_sync(http, &token, "coconilu", "gitgrove"),
                crate::github::fetch_open_prs_for_sync(http, &token, "coconilu", "gitgrove"),
            )
        });
        let issues = issues.expect("fetch_issues_for_sync 失败");
        let prs = prs.expect("fetch_open_prs_for_sync 失败");
        assert!(!issues.is_empty(), "issues 拉取为空：解析层把全部条目丢了");
        assert!(
            issues.iter().all(|i| i.number > 0 && !i.title.is_empty()),
            "issue 解析出坏数据"
        );
        assert!(prs.iter().all(|p| p.number > 0), "open PR 解析出坏数据");

        let signals = build_signals(&[], &[]);
        let snapshots = recent_issues(
            issues.into_iter().map(GithubIssueSnapshot::from).collect(),
            crate::pm::store::now_ts(),
        );

        let scratch = std::env::temp_dir().join(format!("gitgrove-sync-smoke-{}", std::process::id()));
        let store = PmStore::open(&scratch.join("pm.sqlite3")).unwrap();
        let r = store
            .sync_github("coconilu", "gitgrove", &snapshots, &signals)
            .expect("sync_github 失败");
        println!("sync result: created={} updated={} moved={}", r.created, r.updated, r.moved);
        assert_eq!(r.created, snapshots.len(), "首次 sync 应逐条建卡");

        let all = store
            .list_items(&crate::pm::model::ItemFilter::default())
            .unwrap();
        assert_eq!(all.len(), snapshots.len());
        // 验收（#66）：三个 open issue 必须落 todo 列
        for n in [53, 40, 29] {
            let card = all
                .iter()
                .find(|i| i.github_ref.as_deref() == Some(&format!("coconilu/gitgrove#{n}")))
                .unwrap_or_else(|| panic!("issue #{n} 未入库"));
            assert_eq!(card.status, "todo", "open issue #{n} 应落 todo，实际 {}", card.status);
            assert!(!card.title.is_empty());
        }
        // 其余 open issue 同样落 todo；closed 落最后一列
        let done_id = "done";
        for i in &all {
            let number: u64 = i.github_ref.as_deref().unwrap().rsplit('#').next().unwrap().parse().unwrap();
            let snap = snapshots.iter().find(|s| s.number == number).unwrap();
            if snap.state == "open" {
                assert_eq!(i.status, "todo", "open issue #{number} 应落 todo");
            } else {
                assert_eq!(i.status, done_id, "closed issue #{number} 应落 done");
            }
        }
        let _ = std::fs::remove_dir_all(&scratch);
    }
}
