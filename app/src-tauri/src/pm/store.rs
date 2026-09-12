// PM SQLite store：rusqlite + migration 表，数据库文件放 app 数据目录（pm.sqlite3）

use std::path::Path;
use std::time::Duration;

use rusqlite::{params, Connection};

use super::model::*;
use super::order;

const EXPORT_VERSION: u32 = 1;
/// 列是否已存在。SQLite 没有 ADD COLUMN IF NOT EXISTS，用 PRAGMA table_info
/// 做幂等守卫：让含 ALTER 的迁移在「schema 已在、版本未记账」的中间态
/// （进程被杀 / 异常中断残留）下可自愈，不再 duplicate column 永久 wedge。
fn column_exists(conn: &Connection, table: &str, column: &str) -> Result<bool, String> {
    let mut stmt = conn
        .prepare(&format!("PRAGMA table_info({table})"))
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], |r| r.get::<_, String>(1))
        .map_err(|e| e.to_string())?;
    for name in rows {
        if name.map_err(|e| e.to_string())? == column {
            return Ok(true);
        }
    }
    Ok(false)
}

fn items_has_manual_lock(conn: &Connection) -> Result<bool, String> {
    column_exists(conn, "items", "manual_lock")
}

fn items_has_closed_at(conn: &Connection) -> Result<bool, String> {
    column_exists(conn, "items", "closed_at")
}

/// 版本化迁移：按序应用；每个版本「DDL + 版本记账」包在同一事务里，
/// 中途被杀整体回滚、下次 open 原子重试。v1 初始 schema（全 IF NOT EXISTS）；
/// v2 items 加 manual_lock（GitHub 同步卡片的人工锁定标记）；v3 github_ref
/// 索引（同步引擎按 owner/repo#number 匹配 upsert）；v4 closed_at（进入
/// 最后一列的时间，#80 done 列折叠按「最近完成」倒序，老数据为 NULL 前端回退 updatedAt）。
type Guard = fn(&Connection) -> Result<bool, String>;
const MIGRATIONS: &[(u32, &str, Option<Guard>)] = &[
    (
        1,
        "CREATE TABLE IF NOT EXISTS items (
            id TEXT PRIMARY KEY,
            title TEXT NOT NULL,
            body TEXT NOT NULL DEFAULT '',
            status TEXT NOT NULL,
            priority TEXT NOT NULL DEFAULT 'none',
            milestone_id TEXT,
            labels TEXT NOT NULL DEFAULT '[]',
            repo_path TEXT,
            branch TEXT,
            due_date TEXT,
            ord TEXT NOT NULL,
            github_ref TEXT,
            created_at INTEGER NOT NULL,
            updated_at INTEGER NOT NULL,
            deleted_at INTEGER
        );
        CREATE INDEX IF NOT EXISTS idx_items_status_ord ON items(status, ord);
        CREATE INDEX IF NOT EXISTS idx_items_milestone ON items(milestone_id);
        CREATE TABLE IF NOT EXISTS milestones (
            id TEXT PRIMARY KEY,
            title TEXT NOT NULL,
            description TEXT NOT NULL DEFAULT '',
            due_date TEXT,
            status TEXT NOT NULL DEFAULT 'open',
            github_ref TEXT,
            created_at INTEGER NOT NULL,
            updated_at INTEGER NOT NULL
        );
        CREATE TABLE IF NOT EXISTS settings (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL
        );",
        None,
    ),
    (
        2,
        "ALTER TABLE items ADD COLUMN manual_lock INTEGER NOT NULL DEFAULT 0;",
        Some(items_has_manual_lock),
    ),
    (
        3,
        "CREATE INDEX IF NOT EXISTS idx_items_github_ref ON items(github_ref);",
        None,
    ),
    (
        4,
        "ALTER TABLE items ADD COLUMN closed_at INTEGER;",
        Some(items_has_closed_at),
    ),
];
const STATUSES_KEY: &str = "statuses";
/// 单实例锁在插件 setup 时才创建，晚于 PmStore::open；瞬态里二次实例可能
/// 撞上首实例的写事务。rusqlite 默认 busy_timeout=0 会立即返回 SQLITE_BUSY，
/// 让 run() 的 expect panic、双击激活静默失效；留 3s 等待窗口兜底。
const BUSY_TIMEOUT_MS: u64 = 3000;

pub struct PmStore {
    pub(crate) conn: Connection,
}

pub(crate) fn now_ts() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

pub fn new_id() -> String {
    ulid::Ulid::generate().to_string()
}

pub(crate) fn row_to_item(row: &rusqlite::Row<'_>) -> rusqlite::Result<Item> {
    let labels_json: String = row.get("labels")?;
    Ok(Item {
        id: row.get("id")?,
        title: row.get("title")?,
        body: row.get("body")?,
        status: row.get("status")?,
        priority: row.get("priority")?,
        milestone_id: row.get("milestone_id")?,
        labels: serde_json::from_str(&labels_json).unwrap_or_default(),
        repo_path: row.get("repo_path")?,
        branch: row.get("branch")?,
        due_date: row.get("due_date")?,
        order: row.get("ord")?,
        github_ref: row.get("github_ref")?,
        manual_lock: row.get("manual_lock")?,
        closed_at: row.get("closed_at")?,
        created_at: row.get("created_at")?,
        updated_at: row.get("updated_at")?,
        deleted_at: row.get("deleted_at")?,
    })
}

fn row_to_milestone(row: &rusqlite::Row<'_>) -> rusqlite::Result<Milestone> {
    Ok(Milestone {
        id: row.get("id")?,
        title: row.get("title")?,
        description: row.get("description")?,
        due_date: row.get("due_date")?,
        status: row.get("status")?,
        github_ref: row.get("github_ref")?,
        created_at: row.get("created_at")?,
        updated_at: row.get("updated_at")?,
    })
}

pub(crate) const ITEM_COLS: &str =
    "id, title, body, status, priority, milestone_id, labels, repo_path, branch, due_date, ord, github_ref, manual_lock, closed_at, created_at, updated_at, deleted_at";

impl PmStore {
    pub fn open(path: &Path) -> Result<Self, String> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| format!("创建数据目录失败: {e}"))?;
        }
        let conn = Connection::open(path).map_err(|e| format!("打开 PM 数据库失败: {e}"))?;
        conn.busy_timeout(Duration::from_millis(BUSY_TIMEOUT_MS))
            .map_err(|e| format!("设置 busy_timeout 失败: {e}"))?;
        let store = Self { conn };
        store.migrate()?;
        Ok(store)
    }

    #[cfg(test)]
    pub(crate) fn open_memory() -> Self {
        let conn = Connection::open_in_memory().expect("打开内存数据库失败");
        let store = Self { conn };
        store.migrate().expect("migration 失败");
        store
    }

    fn migrate(&self) -> Result<(), String> {
        self.conn
            .execute_batch(
                "CREATE TABLE IF NOT EXISTS _pm_migrations (
                    version INTEGER PRIMARY KEY,
                    applied_at INTEGER NOT NULL
                );",
            )
            .map_err(|e| e.to_string())?;
        let applied: std::collections::HashSet<u32> = {
            let mut stmt = self
                .conn
                .prepare("SELECT version FROM _pm_migrations")
                .map_err(|e| e.to_string())?;
            let rows = stmt
                .query_map([], |r| r.get(0))
                .map_err(|e| e.to_string())?;
            rows.collect::<Result<Vec<u32>, _>>()
                .map_err(|e| e.to_string())?
                .into_iter()
                .collect()
        };
        for (version, sql, guard) in MIGRATIONS {
            if applied.contains(version) {
                continue;
            }
            // guard 命中 = DDL 已在但版本未记账的中间态：补记账自愈，跳过 DDL
            if let Some(guard) = guard {
                if guard(&self.conn)? {
                    self.record_version(*version)?;
                    continue;
                }
            }
            // 单事务原子应用：DDL 失败连记账一起回滚，不会留下半迁移状态
            let tx = self.conn.unchecked_transaction().map_err(|e| e.to_string())?;
            tx.execute_batch(sql)
                .map_err(|e| format!("PM schema v{version} 迁移失败: {e}"))?;
            tx.execute(
                "INSERT OR IGNORE INTO _pm_migrations(version, applied_at) VALUES (?1, ?2)",
                params![version, now_ts()],
            )
            .map_err(|e| e.to_string())?;
            tx.commit().map_err(|e| e.to_string())?;
        }
        Ok(())
    }

    fn record_version(&self, version: u32) -> Result<(), String> {
        self.conn
            .execute(
                "INSERT OR IGNORE INTO _pm_migrations(version, applied_at) VALUES (?1, ?2)",
                params![version, now_ts()],
            )
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    /// 看板列定义；首次访问写入默认值
    pub fn statuses(&self) -> Result<Vec<StatusDef>, String> {
        let raw: Option<String> = self
            .conn
            .query_row(
                "SELECT value FROM settings WHERE key = ?1",
                params![STATUSES_KEY],
                |r| r.get(0),
            )
            .ok();
        if let Some(raw) = raw {
            return serde_json::from_str(&raw).map_err(|e| format!("看板列定义损坏: {e}"));
        }
        let defaults: Vec<StatusDef> = DEFAULT_STATUSES
            .iter()
            .map(|(id, name)| StatusDef { id: id.to_string(), name: name.to_string() })
            .collect();
        let json = serde_json::to_string(&defaults).map_err(|e| e.to_string())?;
        self.conn
            .execute(
                "INSERT OR IGNORE INTO settings(key, value) VALUES (?1, ?2)",
                params![STATUSES_KEY, json],
            )
            .map_err(|e| e.to_string())?;
        Ok(defaults)
    }

    /// 整体写回看板列（PUT 语义）：校验至少一列、id 非空且唯一；
    /// 被删列上的 item 归到第一列（列管理删除列时任务不丢），排序键原样保留
    pub fn update_statuses(&self, defs: &[StatusDef]) -> Result<Vec<StatusDef>, String> {
        if defs.is_empty() {
            return Err("至少需要一列看板列".into());
        }
        let mut seen = std::collections::HashSet::new();
        for d in defs {
            if d.id.trim().is_empty() {
                return Err("看板列 id 不能为空".into());
            }
            if !seen.insert(&d.id) {
                return Err(format!("看板列 id 重复: {}", d.id));
            }
        }
        let json = serde_json::to_string(defs).map_err(|e| e.to_string())?;
        let tx = self.conn.unchecked_transaction().map_err(|e| e.to_string())?;
        tx.execute(
            "INSERT INTO settings(key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![STATUSES_KEY, json],
        )
        .map_err(|e| e.to_string())?;
        // 孤儿 item 归位：status 不在新列集合中的活动 item 统一改到第一列
        let valid: std::collections::HashSet<&str> = defs.iter().map(|d| d.id.as_str()).collect();
        let orphans: Vec<String> = {
            let mut stmt = tx
                .prepare("SELECT DISTINCT status FROM items WHERE deleted_at IS NULL")
                .map_err(|e| e.to_string())?;
            let rows = stmt
                .query_map([], |r| r.get(0))
                .map_err(|e| e.to_string())?;
            rows.collect::<Result<Vec<String>, _>>()
                .map_err(|e| e.to_string())?
                .into_iter()
                .filter(|s| !valid.contains(s.as_str()))
                .collect()
        };
        let now = now_ts();
        for s in orphans {
            // 孤儿离开原列归到第一列：若原列是最后一列，closed_at 一并清空
            tx.execute(
                "UPDATE items SET status=?2, updated_at=?3, closed_at=NULL WHERE status=?1 AND deleted_at IS NULL",
                params![s, defs[0].id, now],
            )
            .map_err(|e| format!("归位孤儿 item 失败: {e}"))?;
        }
        tx.commit().map_err(|e| e.to_string())?;
        Ok(defs.to_vec())
    }

    fn validate_status(&self, status: &str) -> Result<(), String> {
        if self.statuses()?.iter().any(|s| s.id == status) {
            Ok(())
        } else {
            Err(format!("未知看板列: {status}"))
        }
    }

    fn validate_priority(priority: &str) -> Result<(), String> {
        if is_valid_priority(priority) {
            Ok(())
        } else {
            Err(format!("未知优先级: {priority}"))
        }
    }

    fn validate_title(title: &str) -> Result<(), String> {
        if title.trim().is_empty() {
            Err("标题不能为空".into())
        } else {
            Ok(())
        }
    }

    fn milestone_exists(&self, id: &str) -> Result<bool, String> {
        let n: i64 = self
            .conn
            .query_row(
                "SELECT COUNT(*) FROM milestones WHERE id = ?1",
                params![id],
                |r| r.get(0),
            )
            .map_err(|e| e.to_string())?;
        Ok(n > 0)
    }

    fn get_active_item(&self, id: &str) -> Result<Item, String> {
        self.conn
            .query_row(
                &format!("SELECT {ITEM_COLS} FROM items WHERE id = ?1 AND deleted_at IS NULL"),
                params![id],
                row_to_item,
            )
            .map_err(|_| format!("item {id} 不存在或已删除"))
    }

    /// 列内最大排序键（排除指定 item 与 tombstone）
    pub(crate) fn last_key_in_column(&self, status: &str, exclude_id: Option<&str>) -> Result<Option<String>, String> {
        self.conn
            .query_row(
                "SELECT ord FROM items WHERE status = ?1 AND deleted_at IS NULL AND id IS NOT ?2 ORDER BY ord DESC LIMIT 1",
                params![status, exclude_id],
                |r| r.get(0),
            )
            .map(Some)
            .or_else(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => Ok(None),
                e => Err(e.to_string()),
            })
    }

    pub fn list_items(&self, filter: &ItemFilter) -> Result<Vec<Item>, String> {
        let mut sql = format!("SELECT {ITEM_COLS} FROM items WHERE deleted_at IS NULL");
        let mut binds: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
        for (col, val) in [
            ("milestone_id", filter.milestone_id.as_ref()),
            ("repo_path", filter.repo_path.as_ref()),
            ("status", filter.status.as_ref()),
            ("priority", filter.priority.as_ref()),
        ] {
            if let Some(v) = val {
                sql.push_str(&format!(" AND {col} = ?{}", binds.len() + 1));
                binds.push(Box::new(v.clone()));
            }
        }
        let mut stmt = self.conn.prepare(&sql).map_err(|e| e.to_string())?;
        let params = rusqlite::params_from_iter(binds.iter().map(|b| b.as_ref()));
        let rows = stmt
            .query_map(params, row_to_item)
            .map_err(|e| e.to_string())?;
        let mut items = rows.collect::<Result<Vec<Item>, _>>().map_err(|e| e.to_string())?;

        // labels（同时包含）与 search（title/body 忽略大小写）在 Rust 侧过滤
        if let Some(labels) = &filter.labels {
            if !labels.is_empty() {
                items.retain(|i| labels.iter().all(|l| i.labels.contains(l)));
            }
        }
        if let Some(q) = &filter.search {
            let q = q.to_lowercase();
            if !q.is_empty() {
                items.retain(|i| {
                    i.title.to_lowercase().contains(&q) || i.body.to_lowercase().contains(&q)
                });
            }
        }

        // 按看板列顺序 + 列内排序键排序，前端直接分组渲染
        let statuses = self.statuses()?;
        let order_of = |status: &str| -> usize {
            statuses.iter().position(|s| s.id == status).unwrap_or(usize::MAX)
        };
        items.sort_by(|a, b| {
            order_of(&a.status)
                .cmp(&order_of(&b.status))
                .then_with(|| a.order.cmp(&b.order))
        });
        Ok(items)
    }

    /// 最后一列（done 约定，见前端 model.ts doneStatusId）
    pub(crate) fn done_status_id(&self) -> Result<String, String> {
        Ok(self
            .statuses()?
            .last()
            .map(|s| s.id.clone())
            .unwrap_or_else(|| "done".into()))
    }

    pub fn create_item(&self, input: &NewItem) -> Result<Item, String> {
        Self::validate_title(&input.title)?;
        let status = input.status.clone().unwrap_or_else(default_status);
        self.validate_status(&status)?;
        let priority = input.priority.clone().unwrap_or_else(|| "none".into());
        Self::validate_priority(&priority)?;
        if let Some(mid) = &input.milestone_id {
            if !self.milestone_exists(mid)? {
                return Err(format!("milestone {mid} 不存在"));
            }
        }
        let last = self.last_key_in_column(&status, None)?;
        let order = order::key_between(last.as_deref(), None)?;
        let now = now_ts();
        // 直接建在最后一列的任务同样打点（进入 done 的时间，#80 折叠排序键）
        let closed_at = if status == self.done_status_id()? {
            Some(now)
        } else {
            None
        };
        let item = Item {
            id: new_id(),
            title: input.title.trim().to_string(),
            body: input.body.clone(),
            status,
            priority,
            milestone_id: input.milestone_id.clone(),
            labels: input.labels.clone(),
            repo_path: input.repo_path.clone(),
            branch: input.branch.clone(),
            due_date: input.due_date.clone(),
            order,
            github_ref: None,
            manual_lock: false,
            closed_at,
            created_at: now,
            updated_at: now,
            deleted_at: None,
        };
        self.insert_item(&item)?;
        Ok(item)
    }

    pub(crate) fn insert_item(&self, item: &Item) -> Result<(), String> {
        let labels = serde_json::to_string(&item.labels).map_err(|e| e.to_string())?;
        self.conn
            .execute(
                "INSERT INTO items(id, title, body, status, priority, milestone_id, labels, repo_path, branch, due_date, ord, github_ref, manual_lock, closed_at, created_at, updated_at, deleted_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17)",
                params![
                    item.id, item.title, item.body, item.status, item.priority,
                    item.milestone_id, labels, item.repo_path, item.branch, item.due_date,
                    item.order, item.github_ref, item.manual_lock, item.closed_at,
                    item.created_at, item.updated_at, item.deleted_at,
                ],
            )
            .map_err(|e| format!("写入 item 失败: {e}"))?;
        Ok(())
    }

    /// 全字段更新（PUT 语义）：order / created_at / github_ref 由服务端保留；
    /// manualLock 随 PUT 透传（人工拖动 GitHub 卡片时由前端置位）
    pub fn update_item(&self, patch: &Item) -> Result<Item, String> {
        let existing = self.get_active_item(&patch.id)?;
        Self::validate_title(&patch.title)?;
        self.validate_status(&patch.status)?;
        Self::validate_priority(&patch.priority)?;
        if let Some(mid) = &patch.milestone_id {
            if !self.milestone_exists(mid)? {
                return Err(format!("milestone {mid} 不存在"));
            }
        }
        // 跨列直接改 status 时重排到目标列尾部，避免外列排序键混入
        let now = now_ts();
        let order = if patch.status != existing.status {
            let last = self.last_key_in_column(&patch.status, Some(&patch.id))?;
            order::key_between(last.as_deref(), None)?
        } else {
            existing.order.clone()
        };
        // 进最后一列打点 closed_at，离开清空（#80 done 折叠排序键）
        let closed_at = if patch.status != existing.status {
            if patch.status == self.done_status_id()? {
                Some(now)
            } else {
                None
            }
        } else {
            existing.closed_at
        };
        let item = Item {
            title: patch.title.trim().to_string(),
            body: patch.body.clone(),
            status: patch.status.clone(),
            priority: patch.priority.clone(),
            milestone_id: patch.milestone_id.clone(),
            labels: patch.labels.clone(),
            repo_path: patch.repo_path.clone(),
            branch: patch.branch.clone(),
            due_date: patch.due_date.clone(),
            manual_lock: patch.manual_lock,
            order,
            closed_at,
            updated_at: now,
            ..existing
        };
        let labels = serde_json::to_string(&item.labels).map_err(|e| e.to_string())?;
        self.conn
            .execute(
                "UPDATE items SET title=?2, body=?3, status=?4, priority=?5, milestone_id=?6, labels=?7, repo_path=?8, branch=?9, due_date=?10, ord=?11, manual_lock=?12, closed_at=?13, updated_at=?14 WHERE id=?1",
                params![
                    item.id, item.title, item.body, item.status, item.priority,
                    item.milestone_id, labels, item.repo_path, item.branch, item.due_date,
                    item.order, item.manual_lock, item.closed_at, item.updated_at,
                ],
            )
            .map_err(|e| format!("更新 item 失败: {e}"))?;
        Ok(item)
    }

    /// 跨列移动 + 定位：before_item_id 为 None 时排到目标列尾部
    pub fn move_item(
        &self,
        item_id: &str,
        to_status: &str,
        before_item_id: Option<&str>,
    ) -> Result<Item, String> {
        let item = self.get_active_item(item_id)?;
        self.validate_status(to_status)?;
        let (prev, next) = match before_item_id {
            Some(bid) => {
                if bid == item_id {
                    return Err("beforeItemId 不能是被移动的 item 本身".into());
                }
                let before = self.get_active_item(bid)?;
                if before.status != to_status {
                    return Err(format!("beforeItem {bid} 不在目标列 {to_status}"));
                }
                let prev: Option<String> = self
                    .conn
                    .query_row(
                        "SELECT ord FROM items WHERE status = ?1 AND deleted_at IS NULL AND id IS NOT ?2 AND ord < ?3 ORDER BY ord DESC LIMIT 1",
                        params![to_status, item_id, before.order],
                        |r| r.get(0),
                    )
                    .map(Some)
                    .or_else(|e| match e {
                        rusqlite::Error::QueryReturnedNoRows => Ok(None),
                        e => Err(e.to_string()),
                    })?;
                (prev, Some(before.order))
            }
            None => (self.last_key_in_column(to_status, Some(item_id))?, None),
        };
        let new_key = order::key_between(prev.as_deref(), next.as_deref())?;
        let now = now_ts();
        // 进最后一列打点 closed_at，离开清空（#80 done 折叠排序键）
        let closed_at = if item.status != to_status {
            if to_status == self.done_status_id()? {
                Some(now)
            } else {
                None
            }
        } else {
            item.closed_at
        };
        self.conn
            .execute(
                "UPDATE items SET status=?2, ord=?3, updated_at=?4, closed_at=?5 WHERE id=?1",
                params![item_id, to_status, new_key, now, closed_at],
            )
            .map_err(|e| format!("移动 item 失败: {e}"))?;
        Ok(Item {
            status: to_status.into(),
            order: new_key,
            updated_at: now,
            closed_at,
            ..item
        })
    }

    /// 软删除：写 tombstone，列表查询不再返回
    pub fn delete_item(&self, id: &str) -> Result<(), String> {
        let now = now_ts();
        let n = self
            .conn
            .execute(
                "UPDATE items SET deleted_at=?2, updated_at=?2 WHERE id=?1 AND deleted_at IS NULL",
                params![id, now],
            )
            .map_err(|e| e.to_string())?;
        if n == 0 {
            return Err(format!("item {id} 不存在或已删除"));
        }
        Ok(())
    }

    pub fn list_milestones(&self) -> Result<Vec<MilestoneWithStats>, String> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, title, description, due_date, status, github_ref, created_at, updated_at FROM milestones ORDER BY created_at ASC, id ASC")
            .map_err(|e| e.to_string())?;
        let rows = stmt.query_map([], row_to_milestone).map_err(|e| e.to_string())?;
        let milestones = rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())?;

        // done = 最后一个看板列（默认 done）
        let done_status = self
            .statuses()?
            .last()
            .map(|s| s.id.clone())
            .unwrap_or_else(|| "done".into());
        let mut stats = self
            .conn
            .prepare(
                "SELECT milestone_id, COUNT(*), COALESCE(SUM(status = ?1), 0) FROM items WHERE deleted_at IS NULL AND milestone_id IS NOT NULL GROUP BY milestone_id",
            )
            .map_err(|e| e.to_string())?;
        let rows = stats
            .query_map(params![done_status], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?, r.get::<_, i64>(2)?))
            })
            .map_err(|e| e.to_string())?;
        let mut map = std::collections::HashMap::new();
        for r in rows {
            let (mid, total, done) = r.map_err(|e| e.to_string())?;
            map.insert(mid, (total, done));
        }
        Ok(milestones
            .into_iter()
            .map(|m| {
                let (total, done) = map.get(&m.id).copied().unwrap_or((0, 0));
                MilestoneWithStats { milestone: m, total, done }
            })
            .collect())
    }

    pub fn create_milestone(
        &self,
        title: &str,
        description: Option<&str>,
        due_date: Option<&str>,
    ) -> Result<Milestone, String> {
        Self::validate_title(title)?;
        let now = now_ts();
        let m = Milestone {
            id: new_id(),
            title: title.trim().to_string(),
            description: description.unwrap_or_default().to_string(),
            due_date: due_date.map(|s| s.to_string()),
            status: "open".into(),
            github_ref: None,
            created_at: now,
            updated_at: now,
        };
        self.insert_milestone(&m)?;
        Ok(m)
    }

    fn insert_milestone(&self, m: &Milestone) -> Result<(), String> {
        self.conn
            .execute(
                "INSERT INTO milestones(id, title, description, due_date, status, github_ref, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![m.id, m.title, m.description, m.due_date, m.status, m.github_ref, m.created_at, m.updated_at],
            )
            .map_err(|e| format!("写入 milestone 失败: {e}"))?;
        Ok(())
    }

    /// 全字段更新（PUT 语义）：github_ref / created_at 由服务端保留
    pub fn update_milestone(&self, patch: &Milestone) -> Result<Milestone, String> {
        let existing: Milestone = self
            .conn
            .query_row(
                "SELECT id, title, description, due_date, status, github_ref, created_at, updated_at FROM milestones WHERE id = ?1",
                params![patch.id],
                row_to_milestone,
            )
            .map_err(|_| format!("milestone {} 不存在", patch.id))?;
        Self::validate_title(&patch.title)?;
        if patch.status != "open" && patch.status != "closed" {
            return Err(format!("未知 milestone 状态: {}", patch.status));
        }
        let m = Milestone {
            title: patch.title.trim().to_string(),
            description: patch.description.clone(),
            due_date: patch.due_date.clone(),
            status: patch.status.clone(),
            updated_at: now_ts(),
            ..existing
        };
        self.conn
            .execute(
                "UPDATE milestones SET title=?2, description=?3, due_date=?4, status=?5, updated_at=?6 WHERE id=?1",
                params![m.id, m.title, m.description, m.due_date, m.status, m.updated_at],
            )
            .map_err(|e| format!("更新 milestone 失败: {e}"))?;
        Ok(m)
    }

    /// 硬删除 milestone，关联 items 的 milestone_id 置空
    pub fn delete_milestone(&self, id: &str) -> Result<(), String> {
        let tx = self.conn.unchecked_transaction().map_err(|e| e.to_string())?;
        tx.execute(
            "UPDATE items SET milestone_id = NULL, updated_at = ?2 WHERE milestone_id = ?1",
            params![id, now_ts()],
        )
        .map_err(|e| e.to_string())?;
        let n = tx
            .execute("DELETE FROM milestones WHERE id = ?1", params![id])
            .map_err(|e| e.to_string())?;
        if n == 0 {
            return Err(format!("milestone {id} 不存在"));
        }
        tx.commit().map_err(|e| e.to_string())?;
        Ok(())
    }

    /// 整库导出（不含 tombstone），id 稳定序保证往返一致
    pub fn export(&self) -> Result<PmExport, String> {
        let mut stmt = self
            .conn
            .prepare(&format!("SELECT {ITEM_COLS} FROM items WHERE deleted_at IS NULL ORDER BY id ASC"))
            .map_err(|e| e.to_string())?;
        let rows = stmt.query_map([], row_to_item).map_err(|e| e.to_string())?;
        let items = rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())?;
        let mut stmt = self
            .conn
            .prepare("SELECT id, title, description, due_date, status, github_ref, created_at, updated_at FROM milestones ORDER BY id ASC")
            .map_err(|e| e.to_string())?;
        let rows = stmt.query_map([], row_to_milestone).map_err(|e| e.to_string())?;
        let milestones = rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())?;
        Ok(PmExport { version: EXPORT_VERSION, statuses: self.statuses()?, items, milestones })
    }

    /// 整库导入（replace-all，单事务）：tombstone 一并清空
    pub fn import(&self, data: &PmExport) -> Result<ImportResult, String> {
        for item in &data.items {
            if item.id.is_empty() {
                return Err("导入数据存在空 id 的 item".into());
            }
            if !order::is_valid_key(&item.order) {
                return Err(format!("item {} 的排序键非法: {:?}", item.id, item.order));
            }
            Self::validate_priority(&item.priority)?;
        }
        let statuses = if data.statuses.is_empty() {
            DEFAULT_STATUSES
                .iter()
                .map(|(id, name)| StatusDef { id: id.to_string(), name: name.to_string() })
                .collect()
        } else {
            data.statuses.clone()
        };
        let tx = self.conn.unchecked_transaction().map_err(|e| e.to_string())?;
        tx.execute("DELETE FROM items", []).map_err(|e| e.to_string())?;
        tx.execute("DELETE FROM milestones", []).map_err(|e| e.to_string())?;
        let json = serde_json::to_string(&statuses).map_err(|e| e.to_string())?;
        tx.execute(
            "INSERT INTO settings(key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![STATUSES_KEY, json],
        )
        .map_err(|e| e.to_string())?;
        for m in &data.milestones {
            tx.execute(
                "INSERT INTO milestones(id, title, description, due_date, status, github_ref, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![m.id, m.title, m.description, m.due_date, m.status, m.github_ref, m.created_at, m.updated_at],
            )
            .map_err(|e| format!("导入 milestone 失败: {e}"))?;
        }
        for item in &data.items {
            if item.deleted_at.is_some() {
                continue; // 防御：导出不含 tombstone，导入方也不接受
            }
            let labels = serde_json::to_string(&item.labels).map_err(|e| e.to_string())?;
            tx.execute(
                "INSERT INTO items(id, title, body, status, priority, milestone_id, labels, repo_path, branch, due_date, ord, github_ref, manual_lock, closed_at, created_at, updated_at, deleted_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, NULL)",
                params![
                    item.id, item.title, item.body, item.status, item.priority,
                    item.milestone_id, labels, item.repo_path, item.branch, item.due_date,
                    item.order, item.github_ref, item.manual_lock, item.closed_at,
                    item.created_at, item.updated_at,
                ],
            )
            .map_err(|e| format!("导入 item 失败: {e}"))?;
        }
        tx.commit().map_err(|e| e.to_string())?;
        Ok(ImportResult { items: data.items.len(), milestones: data.milestones.len() })
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    fn new_item(title: &str, status: Option<&str>) -> NewItem {
        NewItem {
            title: title.into(),
            status: status.map(|s| s.into()),
            ..Default::default()
        }
    }

    fn column(store: &PmStore, status: &str) -> Vec<Item> {
        store
            .list_items(&ItemFilter { status: Some(status.into()), ..Default::default() })
            .unwrap()
    }

    #[test]
    fn closed_at_tracks_last_column_transitions() {
        let store = PmStore::open_memory();
        let a = store.create_item(&new_item("a", Some("todo"))).unwrap();
        assert_eq!(a.closed_at, None);

        // 拖进最后一列 → 打点
        let a1 = store.move_item(&a.id, "done", None).unwrap();
        assert!(a1.closed_at.is_some());
        assert_eq!(store.get_active_item(&a.id).unwrap().closed_at, a1.closed_at);

        // 最后一列内移动：closed_at 不动
        let b = store.create_item(&new_item("b", Some("done"))).unwrap();
        assert!(b.closed_at.is_some()); // 直接建在 done 也打点
        let b1 = store.move_item(&b.id, "done", Some(&a.id)).unwrap();
        assert_eq!(b1.closed_at, b.closed_at);

        // 拖出最后一列 → 清空
        let a2 = store.move_item(&a.id, "doing", None).unwrap();
        assert_eq!(a2.closed_at, None);

        // PUT 改状态进出 done 同样打点/清空
        let c = store.create_item(&new_item("c", Some("todo"))).unwrap();
        let c1 = store
            .update_item(&Item { status: "done".into(), ..c.clone() })
            .unwrap();
        assert!(c1.closed_at.is_some());
        let c2 = store
            .update_item(&Item { status: "todo".into(), ..c1 })
            .unwrap();
        assert_eq!(c2.closed_at, None);
    }

    #[test]
    fn migration_is_idempotent() {
        let store = PmStore::open_memory();
        store.migrate().unwrap();
        store.migrate().unwrap();
        // 默认看板列只写入一次
        let s1 = store.statuses().unwrap();
        let s2 = store.statuses().unwrap();
        assert_eq!(s1, s2);
        assert_eq!(s1.len(), 4);
    }

    #[test]
    fn migrate_self_heals_wedged_version_state() {
        // 复刻非原子迁移的中间态：v1 schema 已建、manual_lock 列已 ALTER，
        // 但 _pm_migrations 未记 v2/v3（进程在 DDL 与记账之间被杀的残留）。
        // open 必须补记账自愈，而不是重试 ALTER 报 duplicate column 永久失败
        let scratch = Scratch::new();
        let db = scratch.0.join("pm.sqlite3");
        {
            let conn = Connection::open(&db).unwrap();
            // 真实 v1 schema + 记账（crash 前提：v1 已完整应用）
            conn.execute_batch(
                "CREATE TABLE IF NOT EXISTS _pm_migrations (
                    version INTEGER PRIMARY KEY,
                    applied_at INTEGER NOT NULL
                );",
            )
            .unwrap();
            conn.execute_batch(MIGRATIONS[0].1).unwrap();
            conn.execute(
                "INSERT INTO _pm_migrations(version, applied_at) VALUES (1, 0)",
                [],
            )
            .unwrap();
            conn.execute_batch("ALTER TABLE items ADD COLUMN manual_lock INTEGER NOT NULL DEFAULT 0;")
                .unwrap();
            // crash 点：ALTER 已落盘、v2/v3 未记账
        }

        let store = PmStore::open(&db).unwrap();
        // v2 靠 guard 跳过 DDL 只补记账；v3 索引补齐；v4 ALTER 干净应用
        let mut versions: Vec<u32> = {
            let mut stmt = store
                .conn
                .prepare("SELECT version FROM _pm_migrations")
                .unwrap();
            stmt.query_map([], |r| r.get(0))
                .unwrap()
                .map(|r| r.unwrap())
                .collect()
        };
        versions.sort();
        assert_eq!(versions, vec![1, 2, 3, 4]);
        let indexes: i64 = store
            .conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='index' AND name='idx_items_github_ref'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(indexes, 1);
        // 自愈后库可用，老数据行 manual_lock 默认 false
        let item = store.create_item(&new_item("自愈后可用", None)).unwrap();
        assert!(!item.manual_lock);
        assert!(store.get_active_item(&item.id).is_ok());
    }

    #[test]
    fn item_crud() {
        let store = PmStore::open_memory();
        let item = store
            .create_item(&NewItem {
                title: "  第一个任务 ".into(),
                body: "详情".into(),
                labels: vec!["bug".into()],
                ..Default::default()
            })
            .unwrap();
        assert_eq!(item.title, "第一个任务");
        assert_eq!(item.status, "backlog");
        assert_eq!(item.priority, "none");
        assert_eq!(item.id.len(), 26); // ULID

        // 非法 status / priority 被拒绝
        assert!(store.create_item(&new_item("x", Some("nope"))).is_err());
        assert!(store
            .create_item(&NewItem { title: "x".into(), priority: Some("p9".into()), ..Default::default() })
            .is_err());
        assert!(store.create_item(&new_item("   ", None)).is_err());

        // 全字段更新
        let updated = store
            .update_item(&Item {
                title: "改名".into(),
                priority: "high".into(),
                labels: vec!["bug".into(), "p1".into()],
                ..item.clone()
            })
            .unwrap();
        assert_eq!(updated.title, "改名");
        assert_eq!(updated.priority, "high");
        assert_eq!(updated.labels.len(), 2);
        assert_eq!(updated.order, item.order); // 同列更新不动排序键
        assert_eq!(updated.created_at, item.created_at);
        assert!(updated.updated_at >= item.updated_at);

        // 更新不存在的 item
        assert!(store.update_item(&Item { id: "01JXXXX".into(), ..Default::default() }).is_err());

        // filter：labels 同时包含 + search
        let hits = store
            .list_items(&ItemFilter { labels: Some(vec!["bug".into(), "p1".into()]), ..Default::default() })
            .unwrap();
        assert_eq!(hits.len(), 1);
        let hits = store
            .list_items(&ItemFilter { search: Some("改名".into()), ..Default::default() })
            .unwrap();
        assert_eq!(hits.len(), 1);
        let hits = store
            .list_items(&ItemFilter { labels: Some(vec!["bug".into(), "missing".into()]), ..Default::default() })
            .unwrap();
        assert!(hits.is_empty());
    }

    #[test]
    fn move_orders_and_cross_column() {
        let store = PmStore::open_memory();
        let a = store.create_item(&new_item("a", Some("todo"))).unwrap();
        let b = store.create_item(&new_item("b", Some("todo"))).unwrap();
        let c = store.create_item(&new_item("c", Some("todo"))).unwrap();
        assert!(a.order < b.order && b.order < c.order);

        // 列内：c 挪到 a 前面
        let c2 = store.move_item(&c.id, "todo", Some(&a.id)).unwrap();
        let ids: Vec<String> = column(&store, "todo").iter().map(|i| i.id.clone()).collect();
        assert_eq!(ids, vec![c.id.clone(), a.id.clone(), b.id.clone()]);
        assert!(c2.order < a.order);

        // 跨列：b 挪到 doing（尾部）
        let b2 = store.move_item(&b.id, "doing", None).unwrap();
        assert_eq!(b2.status, "doing");
        let todo_ids: Vec<String> = column(&store, "todo").iter().map(|i| i.id.clone()).collect();
        assert_eq!(todo_ids, vec![c.id.clone(), a.id.clone()]);
        let doing = column(&store, "doing");
        assert_eq!(doing.len(), 1);
        assert_eq!(doing[0].id, b.id);

        // 跨列 + 定位：a 挪到 doing 里 b 前面
        store.move_item(&a.id, "doing", Some(&b.id)).unwrap();
        let doing_ids: Vec<String> = column(&store, "doing").iter().map(|i| i.id.clone()).collect();
        assert_eq!(doing_ids, vec![a.id.clone(), b.id.clone()]);

        // beforeItem 不在目标列 → 报错
        assert!(store.move_item(&c.id, "todo", Some(&b.id)).is_err());
        // beforeItem 是自身 → 报错
        assert!(store.move_item(&c.id, "todo", Some(&c.id)).is_err());
        // 未知列 → 报错
        assert!(store.move_item(&c.id, "nope", None).is_err());
    }

    #[test]
    fn tombstone_excluded_from_list() {
        let store = PmStore::open_memory();
        let a = store.create_item(&new_item("a", None)).unwrap();
        store.create_item(&new_item("b", None)).unwrap();
        store.delete_item(&a.id).unwrap();
        let items = store.list_items(&ItemFilter::default()).unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].title, "b");
        // 重复删除 / 删除后移动都报错
        assert!(store.delete_item(&a.id).is_err());
        assert!(store.move_item(&a.id, "todo", None).is_err());
        // 导出不带 tombstone
        let export = store.export().unwrap();
        assert_eq!(export.items.len(), 1);
    }

    #[test]
    fn milestone_crud_and_stats() {
        let store = PmStore::open_memory();
        let m = store.create_milestone("v1.0", Some("首版"), Some("2026-10-01")).unwrap();
        let done_item = store
            .create_item(&NewItem { title: "完工".into(), status: Some("done".into()), milestone_id: Some(m.id.clone()), ..Default::default() })
            .unwrap();
        store
            .create_item(&NewItem { title: "未完".into(), milestone_id: Some(m.id.clone()), ..Default::default() })
            .unwrap();

        let list = store.list_milestones().unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].total, 2);
        assert_eq!(list[0].done, 1);

        let updated = store
            .update_milestone(&Milestone { title: "v1.1".into(), status: "closed".into(), ..m.clone() })
            .unwrap();
        assert_eq!(updated.title, "v1.1");
        assert_eq!(updated.status, "closed");
        assert_eq!(updated.created_at, m.created_at);
        assert!(store.update_milestone(&Milestone { id: "nope".into(), ..Default::default() }).is_err());

        // 删除后 items 的 milestone_id 置空
        store.delete_milestone(&m.id).unwrap();
        assert!(store.list_milestones().unwrap().is_empty());
        let item = store.get_active_item(&done_item.id).unwrap();
        assert_eq!(item.milestone_id, None);
        assert!(store.delete_milestone(&m.id).is_err());
    }

    #[test]
    fn statuses_read_defaults_and_round_trip() {
        let store = PmStore::open_memory();
        let defaults = store.statuses().unwrap();
        assert_eq!(
            defaults.iter().map(|s| s.id.as_str()).collect::<Vec<_>>(),
            vec!["backlog", "todo", "doing", "done"]
        );

        let custom = vec![
            StatusDef { id: "icebox".into(), name: "Icebox".into() },
            StatusDef { id: "wip".into(), name: "进行中".into() },
            StatusDef { id: "shipped".into(), name: "Shipped".into() },
        ];
        let written = store.update_statuses(&custom).unwrap();
        assert_eq!(written, custom);
        assert_eq!(store.statuses().unwrap(), custom);
    }

    #[test]
    fn statuses_reject_invalid_input() {
        let store = PmStore::open_memory();
        // 空数组
        assert!(store.update_statuses(&[]).is_err());
        // 重复 id
        let dup = vec![
            StatusDef { id: "a".into(), name: "A".into() },
            StatusDef { id: "a".into(), name: "A2".into() },
        ];
        assert!(store.update_statuses(&dup).is_err());
        // 空 id
        let blank = vec![StatusDef { id: " ".into(), name: "X".into() }];
        assert!(store.update_statuses(&blank).is_err());
        // 拒绝后原列定义不变
        assert_eq!(store.statuses().unwrap().len(), 4);
    }

    #[test]
    fn statuses_orphan_items_move_to_first_column() {
        let store = PmStore::open_memory();
        let kept = store.create_item(&new_item("留", Some("todo"))).unwrap();
        let orphan = store.create_item(&new_item("孤", Some("done"))).unwrap();

        // 删除 done 列并改名 backlog → 孤儿归到第一列（icebox）
        store
            .update_statuses(&[
                StatusDef { id: "icebox".into(), name: "Icebox".into() },
                StatusDef { id: "todo".into(), name: "Todo".into() },
                StatusDef { id: "doing".into(), name: "Doing".into() },
            ])
            .unwrap();

        let orphan_after = store.get_active_item(&orphan.id).unwrap();
        assert_eq!(orphan_after.status, "icebox");
        let kept_after = store.get_active_item(&kept.id).unwrap();
        assert_eq!(kept_after.status, "todo"); // 未删列上的 item 不动
        // 归位后的 item 可被新列校验接受
        assert!(store.move_item(&orphan.id, "todo", None).is_ok());
    }

    #[test]
    fn export_import_round_trip() {
        let store = PmStore::open_memory();
        let m = store.create_milestone("v1.0", None, None).unwrap();
        store
            .create_item(&NewItem { title: "a".into(), milestone_id: Some(m.id.clone()), labels: vec!["x".into()], ..Default::default() })
            .unwrap();
        store.create_item(&new_item("b", Some("doing"))).unwrap();
        let doomed = store.create_item(&new_item("c", None)).unwrap();
        store.delete_item(&doomed.id).unwrap();

        let export = store.export().unwrap();
        let json = serde_json::to_string(&export).unwrap(); // 确认可序列化为 JSON

        let fresh = PmStore::open_memory();
        let result = fresh.import(&serde_json::from_str(&json).unwrap()).unwrap();
        assert_eq!(result.items, 2);
        assert_eq!(result.milestones, 1);
        let re_export = fresh.export().unwrap();
        assert_eq!(export, re_export);

        // 导入非法排序键被拒，且原库不被污染
        let mut bad = export.clone();
        bad.items[0].order = "!!!".into();
        assert!(fresh.import(&bad).is_err());
        assert_eq!(fresh.export().unwrap(), export);
    }

    #[test]
    fn manual_lock_persists_and_defaults_false_for_legacy_rows() {
        let store = PmStore::open_memory();
        let item = store.create_item(&new_item("a", None)).unwrap();
        assert!(!item.manual_lock);

        // PUT 透传：manualLock 由前端传入并落库
        let updated = store
            .update_item(&Item { manual_lock: true, ..item.clone() })
            .unwrap();
        assert!(updated.manual_lock);
        assert!(store.get_active_item(&item.id).unwrap().manual_lock);

        // v1 老行（无 manual_lock 列值）→ 默认 false
        store
            .conn
            .execute(
                "INSERT INTO items(id, title, body, status, priority, labels, ord, created_at, updated_at)
                 VALUES ('legacy', '老行', '', 'todo', 'none', '[]', 'g', 0, 0)",
                [],
            )
            .unwrap();
        assert!(!store.get_active_item("legacy").unwrap().manual_lock);

        // 导出/导入往返保留 manualLock
        let export = store.export().unwrap();
        assert!(export.items.iter().find(|i| i.id == item.id).unwrap().manual_lock);
        let fresh = PmStore::open_memory();
        let n = fresh.import(&export).unwrap();
        assert_eq!(n.items, 2);
        assert!(fresh.get_active_item(&item.id).unwrap().manual_lock);
    }

    struct Scratch(std::path::PathBuf);
    impl Scratch {
        fn new() -> Self {
            let stamp = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let dir = std::env::temp_dir().join(format!("gitgrove-pm-store-{stamp}"));
            std::fs::create_dir_all(&dir).unwrap();
            Scratch(dir)
        }
    }
    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn open_sets_busy_timeout_and_waits_for_concurrent_writer() {
        let scratch = Scratch::new();
        let db = scratch.0.join("pm.sqlite3");
        let first = PmStore::open(&db).unwrap();
        // busy_timeout 必须生效（rusqlite 默认 0，撞锁立即报错）
        let timeout_ms: i64 = first
            .conn
            .query_row("PRAGMA busy_timeout", [], |r| r.get(0))
            .unwrap();
        assert_eq!(timeout_ms, BUSY_TIMEOUT_MS as i64);

        // 复刻二次启动场景：首实例连接持写事务时，第二个实例 open+migrate
        // 在 busy_timeout 窗口内等到锁释放并成功。busy_timeout 失效时这里
        // 会立即 SQLITE_BUSY 失败（防回归）；未撞上窗口时 open 直接成功，不会误报
        let holder = Connection::open(&db).unwrap();
        holder.execute_batch("BEGIN IMMEDIATE").unwrap();
        let second = {
            let db = db.clone();
            std::thread::spawn(move || PmStore::open(&db))
        };
        std::thread::sleep(Duration::from_millis(300));
        holder.execute_batch("COMMIT").unwrap();
        second.join().unwrap().unwrap();
    }
}
