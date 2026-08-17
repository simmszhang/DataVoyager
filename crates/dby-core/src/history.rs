//! SQL 历史缓存：捕获、存储（SQLite + FTS5）、检索。
//!
//! 两张表：
//! * `executions` — 执行流水（append-only，审计）。
//! * `statements` — 语句库（按规范化哈希去重），配 `statements_fts` 全文索引。
//!
//! 说明：M0 采用同步写入（单条 INSERT 微秒级，不阻塞查询）；M1 再引入
//! 有界通道 + 后台 writer + WAL 优化写入路径。

use std::path::Path;
use std::sync::Mutex;

use chrono::{DateTime, Utc};
use rusqlite::{params, Connection};
use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::error::{DbError, Result};
use crate::query::SqlOrigin;

#[derive(Debug, Clone, Serialize)]
pub struct ExecutionRecord {
    pub id: String,
    pub project_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub connection_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub connection_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub database: Option<String>,
    pub sql: String,
    pub origin: SqlOrigin,
    /// "ok" 或错误信息
    pub status: String,
    pub rows_affected: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub row_count: Option<u64>,
    pub duration_ms: u64,
    pub started_at: DateTime<Utc>,
}

impl ExecutionRecord {
    pub fn new(project_id: impl Into<String>, sql: impl Into<String>, origin: SqlOrigin) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            project_id: project_id.into(),
            connection_id: None,
            connection_name: None,
            database: None,
            sql: sql.into(),
            origin,
            status: "ok".to_string(),
            rows_affected: 0,
            row_count: None,
            duration_ms: 0,
            started_at: Utc::now(),
        }
    }
}

/// 规范化：折叠空白 + 去尾分号（用于去重与检索）。
pub fn normalize_sql(sql: &str) -> String {
    sql.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .trim_end_matches(';')
        .trim()
        .to_string()
}

/// 规范化 SQL 的稳定内容哈希（大小写不敏感）。
pub fn sql_hash(normalized: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(normalized.to_lowercase().as_bytes());
    format!("{:x}", hasher.finalize())
}

#[derive(Debug, Clone, Default)]
pub struct HistoryFilter {
    pub project_id: Option<String>,
    pub connection_id: Option<String>,
    pub origin: Option<SqlOrigin>,
    pub only_errors: Option<bool>,
    pub limit: usize,
}

impl HistoryFilter {
    pub fn new() -> Self {
        Self {
            limit: 100,
            ..Default::default()
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct StatementHit {
    pub hash: String,
    pub sql: String,
    pub project_id: String,
    pub run_count: u64,
    pub last_run_at: DateTime<Utc>,
    pub pinned: bool,
    pub tags: Vec<String>,
}

pub struct HistoryStore {
    conn: Mutex<Connection>,
}

impl HistoryStore {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let conn = Connection::open(path)?;
        Self::init_schema(&conn)?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    /// 内存态（测试用）。
    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        Self::init_schema(&conn)?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    fn init_schema(conn: &Connection) -> Result<()> {
        conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS executions (
                id              TEXT PRIMARY KEY,
                project_id      TEXT NOT NULL,
                connection_id   TEXT,
                connection_name TEXT,
                database        TEXT,
                sql             TEXT NOT NULL,
                sql_hash        TEXT NOT NULL,
                origin          TEXT NOT NULL,
                status          TEXT NOT NULL,
                rows_affected   INTEGER NOT NULL DEFAULT 0,
                row_count       INTEGER,
                duration_ms     INTEGER NOT NULL DEFAULT 0,
                started_at      TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_exec_project_time ON executions(project_id, started_at DESC);
            CREATE INDEX IF NOT EXISTS idx_exec_hash ON executions(sql_hash);

            CREATE TABLE IF NOT EXISTS statements (
                sql_hash       TEXT PRIMARY KEY,
                project_id     TEXT NOT NULL,
                normalized_sql TEXT NOT NULL,
                raw_sql        TEXT NOT NULL,
                run_count      INTEGER NOT NULL DEFAULT 0,
                first_seen_at  TEXT NOT NULL,
                last_seen_at   TEXT NOT NULL,
                pinned         INTEGER NOT NULL DEFAULT 0,
                tags           TEXT NOT NULL DEFAULT '[]'
            );

            CREATE VIRTUAL TABLE IF NOT EXISTS statements_fts USING fts5(
                sql_hash UNINDEXED,
                normalized_sql
            );
            "#,
        )?;
        Ok(())
    }

    fn lock(&self) -> Result<std::sync::MutexGuard<'_, Connection>> {
        self.conn
            .lock()
            .map_err(|_| DbError::Storage("history store lock poisoned".to_string()))
    }

    /// 记录一次执行：写执行流水 + 去重更新语句库（新语句才入 FTS）。
    pub fn record(&self, rec: &ExecutionRecord) -> Result<()> {
        let conn = self.lock()?;
        let normalized = normalize_sql(&rec.sql);
        let hash = sql_hash(&normalized);
        let now = rec.started_at.to_rfc3339();

        conn.execute(
            "INSERT INTO executions
             (id, project_id, connection_id, connection_name, database, sql, sql_hash, origin, status, rows_affected, row_count, duration_ms, started_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
            params![
                rec.id,
                rec.project_id,
                rec.connection_id,
                rec.connection_name,
                rec.database,
                rec.sql,
                hash,
                rec.origin.as_str(),
                rec.status,
                rec.rows_affected as i64,
                rec.row_count.map(|v| v as i64),
                rec.duration_ms as i64,
                now,
            ],
        )?;

        let exists: bool = conn
            .query_row(
                "SELECT 1 FROM statements WHERE sql_hash = ?1",
                [&hash],
                |_| Ok(true),
            )
            .unwrap_or(false);
        if exists {
            conn.execute(
                "UPDATE statements SET run_count = run_count + 1, last_seen_at = ?1, raw_sql = ?2, project_id = ?3 WHERE sql_hash = ?4",
                params![now, rec.sql, rec.project_id, hash],
            )?;
        } else {
            conn.execute(
                "INSERT INTO statements (sql_hash, project_id, normalized_sql, raw_sql, run_count, first_seen_at, last_seen_at, pinned, tags)
                 VALUES (?1, ?2, ?3, ?4, 1, ?5, ?5, 0, '[]')",
                params![hash, rec.project_id, normalized, rec.sql, now],
            )?;
            conn.execute(
                "INSERT INTO statements_fts (sql_hash, normalized_sql) VALUES (?1, ?2)",
                params![hash, normalized],
            )?;
        }
        Ok(())
    }

    /// 全文检索语句库。
    pub fn search(&self, query: &str, filter: &HistoryFilter) -> Result<Vec<StatementHit>> {
        let conn = self.lock()?;
        let q = query.trim();
        if q.is_empty() {
            return self.statements_inner(&conn, filter);
        }
        let fts = to_fts_query(q);
        let mut sql = String::from(
            "SELECT s.sql_hash, s.normalized_sql, s.project_id, s.run_count, s.last_seen_at, s.pinned, s.tags
             FROM statements_fts f JOIN statements s ON s.sql_hash = f.sql_hash
             WHERE statements_fts MATCH ?1",
        );
        let mut args: Vec<Box<dyn rusqlite::ToSql>> = vec![Box::new(fts)];
        push_filter(&mut sql, &mut args, "s", filter);
        sql.push_str(" ORDER BY s.last_seen_at DESC LIMIT ?");
        args.push(Box::new(filter.limit as i64));

        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(rusqlite::params_from_iter(args.iter().map(|b| b.as_ref())), |r| {
            Ok(StatementHit {
                hash: r.get(0)?,
                sql: r.get(1)?,
                project_id: r.get(2)?,
                run_count: r.get::<_, i64>(3)? as u64,
                last_run_at: parse_rfc3339(&r.get::<_, String>(4)?),
                pinned: r.get::<_, i64>(5)? != 0,
                tags: parse_tags(&r.get::<_, String>(6)?),
            })
        })?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(DbError::from)
    }

    /// 语句库（去重视图）。
    pub fn statements(&self, filter: &HistoryFilter) -> Result<Vec<StatementHit>> {
        let conn = self.lock()?;
        self.statements_inner(&conn, filter)
    }

    fn statements_inner(
        &self,
        conn: &Connection,
        filter: &HistoryFilter,
    ) -> Result<Vec<StatementHit>> {
        let mut sql = String::from(
            "SELECT sql_hash, normalized_sql, project_id, run_count, last_seen_at, pinned, tags
             FROM statements WHERE 1=1",
        );
        let mut args: Vec<Box<dyn rusqlite::ToSql>> = vec![];
        push_filter(&mut sql, &mut args, "", filter);
        sql.push_str(" ORDER BY last_seen_at DESC LIMIT ?");
        args.push(Box::new(filter.limit as i64));

        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(rusqlite::params_from_iter(args.iter().map(|b| b.as_ref())), |r| {
            Ok(StatementHit {
                hash: r.get(0)?,
                sql: r.get(1)?,
                project_id: r.get(2)?,
                run_count: r.get::<_, i64>(3)? as u64,
                last_run_at: parse_rfc3339(&r.get::<_, String>(4)?),
                pinned: r.get::<_, i64>(5)? != 0,
                tags: parse_tags(&r.get::<_, String>(6)?),
            })
        })?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(DbError::from)
    }

    /// 执行流水（审计视图）。
    pub fn executions(&self, filter: &HistoryFilter) -> Result<Vec<ExecutionRecord>> {
        let conn = self.lock()?;
        let mut sql = String::from(
            "SELECT id, project_id, connection_id, connection_name, database, sql, origin, status, rows_affected, row_count, duration_ms, started_at
             FROM executions WHERE 1=1",
        );
        let mut args: Vec<Box<dyn rusqlite::ToSql>> = vec![];
        push_filter(&mut sql, &mut args, "", filter);
        sql.push_str(" ORDER BY started_at DESC LIMIT ?");
        args.push(Box::new(filter.limit as i64));

        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(rusqlite::params_from_iter(args.iter().map(|b| b.as_ref())), |r| {
            Ok(ExecutionRecord {
                id: r.get(0)?,
                project_id: r.get(1)?,
                connection_id: r.get(2)?,
                connection_name: r.get(3)?,
                database: r.get(4)?,
                sql: r.get(5)?,
                origin: SqlOrigin::from_str(&r.get::<_, String>(6)?),
                status: r.get(7)?,
                rows_affected: r.get::<_, i64>(8)? as u64,
                row_count: r.get::<_, Option<i64>>(9)?.map(|v| v as u64),
                duration_ms: r.get::<_, i64>(10)? as u64,
                started_at: parse_rfc3339(&r.get::<_, String>(11)?),
            })
        })?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(DbError::from)
    }

    /// 固定/取消固定某语句。
    pub fn pin_statement(&self, hash: &str, pinned: bool) -> Result<()> {
        let conn = self.lock()?;
        conn.execute(
            "UPDATE statements SET pinned = ?1 WHERE sql_hash = ?2",
            params![pinned as i64, hash],
        )?;
        Ok(())
    }

    /// 删除单条执行流水（审计记录）。
    pub fn delete_execution(&self, id: &str) -> Result<()> {
        let conn = self.lock()?;
        conn.execute("DELETE FROM executions WHERE id = ?1", [id])?;
        Ok(())
    }

    /// 清空某项目（或全部）的历史。
    pub fn clear(&self, project_id: Option<&str>) -> Result<()> {
        let conn = self.lock()?;
        match project_id {
            Some(p) => {
                conn.execute("DELETE FROM executions WHERE project_id = ?1", [p])?;
                // 语句库删除该项目下、且无其他项目引用的语句（简化：仅删 execution，语句库保留跨项目）
                conn.execute("DELETE FROM statements WHERE project_id = ?1", [p])?;
            }
            None => {
                conn.execute("DELETE FROM executions", [])?;
                conn.execute("DELETE FROM statements", [])?;
            }
        }
        Ok(())
    }
}

/// 组装过滤条件；`alias` 为空表示不拼别名前缀（executions/statements 直查），
/// 否则使用 `alias.`（FTS join 场景）。
fn push_filter(
    sql: &mut String,
    args: &mut Vec<Box<dyn rusqlite::ToSql>>,
    alias: &str,
    filter: &HistoryFilter,
) {
    let col = |name: &str| {
        if alias.is_empty() {
            name.to_string()
        } else {
            format!("{alias}.{name}")
        }
    };
    if let Some(p) = &filter.project_id {
        sql.push_str(&format!(" AND {} = ?", col("project_id")));
        args.push(Box::new(p.clone()));
    }
    if let Some(c) = &filter.connection_id {
        sql.push_str(&format!(" AND {} = ?", col("connection_id")));
        args.push(Box::new(c.clone()));
    }
    if let Some(o) = filter.origin {
        sql.push_str(&format!(" AND {} = ?", col("origin")));
        args.push(Box::new(o.as_str().to_string()));
    }
    if let Some(err) = filter.only_errors {
        if err {
            sql.push_str(&format!(" AND {} != 'ok'", col("status")));
        }
    }
}

/// 用户查询 → FTS5 MATCH 表达式（每个词做短语，AND 组合）。
fn to_fts_query(query: &str) -> String {
    query
        .split_whitespace()
        .map(|t| format!("\"{}\"", t.replace('"', "\"\"")))
        .collect::<Vec<_>>()
        .join(" AND ")
}

fn parse_rfc3339(s: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(s)
        .map(|d| d.with_timezone(&Utc))
        .unwrap_or_else(|_| Utc::now())
}

fn parse_tags(s: &str) -> Vec<String> {
    serde_json::from_str(s).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rec(project: &str, sql: &str) -> ExecutionRecord {
        ExecutionRecord::new(project, sql, SqlOrigin::ManualEditor)
    }

    #[test]
    fn normalize_collapses_whitespace_and_strips_semicolon() {
        assert_eq!(normalize_sql("  SELECT  1  ;  "), "SELECT 1");
        assert_eq!(normalize_sql("SELECT *\nFROM t"), "SELECT * FROM t");
    }

    #[test]
    fn sql_hash_is_case_insensitive() {
        let a = sql_hash(&normalize_sql("SELECT 1"));
        let b = sql_hash(&normalize_sql("select 1"));
        assert_eq!(a, b);
        let c = sql_hash(&normalize_sql("SELECT 2"));
        assert_ne!(a, c);
    }

    #[test]
    fn record_dedupes_statements_and_keeps_executions() {
        let store = HistoryStore::open_in_memory().unwrap();
        let r1 = rec("p1", "SELECT 1");
        let r2 = rec("p1", "SELECT 1"); // 同一语句再执行
        let r3 = rec("p1", "SELECT 2");
        store.record(&r1).unwrap();
        store.record(&r2).unwrap();
        store.record(&r3).unwrap();

        let stmts = store.statements(&HistoryFilter::new()).unwrap();
        assert_eq!(stmts.len(), 2, "语句库应去重");
        let one = stmts.iter().find(|s| s.sql == "SELECT 1").unwrap();
        assert_eq!(one.run_count, 2);

        let execs = store.executions(&HistoryFilter::new()).unwrap();
        assert_eq!(execs.len(), 3, "执行流水应保留每条");
    }

    #[test]
    fn search_finds_by_keyword() {
        let store = HistoryStore::open_in_memory().unwrap();
        store.record(&rec("p1", "SELECT * FROM users")).unwrap();
        store.record(&rec("p1", "SELECT * FROM orders")).unwrap();

        let hits = store.search("users", &HistoryFilter::new()).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].sql, "SELECT * FROM users");
    }

    #[test]
    fn filter_by_project() {
        let store = HistoryStore::open_in_memory().unwrap();
        store.record(&rec("p1", "SELECT 1")).unwrap();
        store.record(&rec("p2", "SELECT 2")).unwrap();

        let mut f = HistoryFilter::new();
        f.project_id = Some("p1".to_string());
        assert_eq!(store.executions(&f).unwrap().len(), 1);
        assert_eq!(store.statements(&f).unwrap().len(), 1);
    }

    #[test]
    fn clear_project() {
        let store = HistoryStore::open_in_memory().unwrap();
        store.record(&rec("p1", "SELECT 1")).unwrap();
        store.record(&rec("p2", "SELECT 2")).unwrap();
        store.clear(Some("p1")).unwrap();
        let f = HistoryFilter::new();
        assert_eq!(store.executions(&f).unwrap().len(), 1);
    }

    #[test]
    fn pin_and_delete_execution() {
        let store = HistoryStore::open_in_memory().unwrap();
        let r = rec("p1", "SELECT 1");
        let hash = sql_hash(&normalize_sql("SELECT 1"));
        store.record(&r).unwrap();

        store.pin_statement(&hash, true).unwrap();
        let hits = store.statements(&HistoryFilter::new()).unwrap();
        assert!(hits[0].pinned);

        store.delete_execution(&r.id).unwrap();
        assert!(store.executions(&HistoryFilter::new()).unwrap().is_empty());
    }
}
