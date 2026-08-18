//! 查询执行相关类型：流式事件、结果集、取消令牌。

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use serde::Serialize;

use crate::metadata::ColumnInfo;
use crate::value::Value;

/// 执行选项。
#[derive(Debug, Clone, Default)]
pub struct ExecOpts {
    pub timeout_ms: Option<u64>,
    /// 单结果集最多存储的行数（缓冲路径；超出的行被消费但标记 truncated）。
    pub max_rows: Option<usize>,
    /// 取消令牌：驱动每批之间检查，命中则中止（drop 结果流即取消服务端查询）。
    pub cancel: Option<CancellationToken>,
}

/// SQL 来源，用于历史记录归因。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum SqlOrigin {
    ManualEditor,
    DataEdit,
    SchemaEdit,
    Export,
    Ai,
    Plugin,
    Cli,
    #[default]
    Other,
}

impl SqlOrigin {
    pub fn as_str(&self) -> &'static str {
        match self {
            SqlOrigin::ManualEditor => "manual_editor",
            SqlOrigin::DataEdit => "data_edit",
            SqlOrigin::SchemaEdit => "schema_edit",
            SqlOrigin::Export => "export",
            SqlOrigin::Ai => "ai",
            SqlOrigin::Plugin => "plugin",
            SqlOrigin::Cli => "cli",
            SqlOrigin::Other => "other",
        }
    }

    pub fn parse(s: &str) -> SqlOrigin {
        match s {
            "manual_editor" => SqlOrigin::ManualEditor,
            "data_edit" => SqlOrigin::DataEdit,
            "schema_edit" => SqlOrigin::SchemaEdit,
            "export" => SqlOrigin::Export,
            "ai" => SqlOrigin::Ai,
            "plugin" => SqlOrigin::Plugin,
            "cli" => SqlOrigin::Cli,
            _ => SqlOrigin::Other,
        }
    }
}

/// 一个结果集。
#[derive(Debug, Clone, Serialize)]
pub struct ResultSet {
    pub columns: Vec<ColumnInfo>,
    pub rows: Vec<Vec<Value>>,
    pub truncated: bool,
}

/// 一次语句执行的输出（缓冲路径，可含多个结果集）。
#[derive(Debug, Clone, Serialize, Default)]
pub struct QueryOutput {
    pub result_sets: Vec<ResultSet>,
    pub affected_rows: u64,
    pub last_insert_id: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub info: Option<String>,
}

impl QueryOutput {
    pub fn first_result_set(&self) -> Option<&ResultSet> {
        self.result_sets.first()
    }
}

/// 流式执行事件（SELECT 吐 Columns + Rows 批次；DML 吐 Affected；多结果集边界/终止事件见 #28）。
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "event", content = "data", rename_all = "snake_case")]
pub enum StreamEvent {
    Columns(Vec<ColumnInfo>),
    Rows(Vec<Vec<Value>>),
    Affected {
        affected_rows: u64,
        last_insert_id: Option<u64>,
    },
    Info(Option<String>),
    ResultSetEnd, // 结果集边界
    Truncated,    // 超 max_rows 截断（协议预留，流式截断发射归 #35/#27）
    Done,         // 命令成功收尾（unit 变体：序列化为 {"event":"done"}，无 data）
    Error {
        kind: String,
        message: String,
    }, // 命令失败收尾（携带 kind，对齐 S5）
}

/// 结果接收器：驱动把流式事件推给实现者（如 Tauri Channel 或缓冲收集器）。
pub trait ResultSink: Send {
    fn on_event(&mut self, ev: StreamEvent);
}

/// 取消令牌：`AtomicBool` 快速无锁轮询 + `watch` 通道（`cancelled()` 无丢失唤醒）。
#[derive(Debug, Clone)]
pub struct CancellationToken {
    flag: Arc<AtomicBool>, // 快速无锁轮询（批内检查）
    tx: tokio::sync::watch::Sender<bool>,
    rx: tokio::sync::watch::Receiver<bool>,
}

impl CancellationToken {
    pub fn new() -> Self {
        let (tx, rx) = tokio::sync::watch::channel(false);
        Self {
            flag: Arc::new(AtomicBool::new(false)),
            tx,
            rx,
        }
    }
    pub fn cancel(&self) {
        self.flag.store(true, Ordering::SeqCst);
        let _ = self.tx.send(true); // watch 存最新值：后订阅者立即可见，无 Notify 丢失唤醒
    }
    pub fn is_cancelled(&self) -> bool {
        self.flag.load(Ordering::SeqCst)
    }
    /// 等待取消：先 cancel 后首 poll 也会立即返回（watch 存最新值，语义正确）。
    pub async fn cancelled(&self) {
        if self.is_cancelled() {
            return;
        }
        let mut rx = self.rx.clone();
        if *rx.borrow() {
            return;
        }
        let _ = rx.changed().await;
    }
}

impl Default for CancellationToken {
    fn default() -> Self {
        Self::new()
    }
}

/// 当前结果集构建器（缓冲路径内部状态）。
struct ResultSetBuilder {
    columns: Vec<ColumnInfo>,
    rows: Vec<Vec<Value>>,
    truncated: bool,
}

/// 缓冲收集 sink：把流式事件收集成一个 `QueryOutput`（带行数截断、多结果集分桶）。
pub struct CollectingSink {
    result_sets: Vec<ResultSet>,
    current: Option<ResultSetBuilder>,
    max_rows: usize,
    /// 顶层值 = 最后一个结果集的值（MySQL 多语句语义）。
    affected_rows: u64,
    last_insert_id: Option<u64>,
    info: Option<String>,
}

impl CollectingSink {
    pub fn new(max_rows: Option<usize>) -> Self {
        Self {
            result_sets: Vec::new(),
            current: None,
            max_rows: max_rows.unwrap_or(usize::MAX),
            affected_rows: 0,
            last_insert_id: None,
            info: None,
        }
    }

    /// 结算当前构建器为一个 `ResultSet` 并移入 `result_sets`（无当前构建器时为空操作）。
    fn settle_current(&mut self) {
        if let Some(cur) = self.current.take() {
            self.result_sets.push(ResultSet {
                columns: cur.columns,
                rows: cur.rows,
                truncated: cur.truncated,
            });
        }
    }

    pub fn into_output(mut self) -> QueryOutput {
        self.settle_current();
        QueryOutput {
            result_sets: self.result_sets,
            affected_rows: self.affected_rows,
            last_insert_id: self.last_insert_id,
            info: self.info,
        }
    }
}

impl ResultSink for CollectingSink {
    fn on_event(&mut self, ev: StreamEvent) {
        match ev {
            StreamEvent::Columns(cols) => {
                self.settle_current();
                self.current = Some(ResultSetBuilder {
                    columns: cols,
                    rows: Vec::new(),
                    truncated: false,
                });
            }
            StreamEvent::Rows(rows) => {
                let current = match self.current.as_mut() {
                    Some(cur) => cur,
                    None => return, // 无列头的行事件忽略
                };
                for r in rows {
                    if current.rows.len() < self.max_rows {
                        current.rows.push(r);
                    } else {
                        current.truncated = true;
                    }
                }
            }
            StreamEvent::Affected {
                affected_rows,
                last_insert_id,
            } => {
                self.affected_rows = affected_rows;
                self.last_insert_id = last_insert_id;
            }
            StreamEvent::Info(info) => self.info = info,
            StreamEvent::ResultSetEnd => self.settle_current(),
            StreamEvent::Truncated => {
                if let Some(cur) = self.current.as_mut() {
                    cur.truncated = true;
                }
            }
            StreamEvent::Done | StreamEvent::Error { .. } => {} // 终态：缓冲路径无需处理
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    #[test]
    fn cancel_token_works() {
        let t = CancellationToken::new();
        assert!(!t.is_cancelled());
        t.cancel();
        assert!(t.is_cancelled());
    }

    #[tokio::test]
    async fn cancelled_resolves_when_cancel_before_or_after_poll() {
        // 场景 A：先 cancel 再 poll cancelled() → 立即返回
        let t = CancellationToken::new();
        t.cancel();
        tokio::time::timeout(Duration::from_secs(1), t.cancelled())
            .await
            .unwrap();
        // 场景 B：poll 中 cancel → notified
        let t2 = CancellationToken::new();
        let fut = t2.cancelled();
        tokio::pin!(fut);
        tokio::select! {
            _ = &mut fut => panic!("不应提前返回"),
            _ = tokio::time::sleep(Duration::from_millis(10)) => {}
        }
        t2.cancel();
        tokio::time::timeout(Duration::from_secs(1), fut)
            .await
            .unwrap();
        assert!(t2.is_cancelled());
    }

    #[test]
    fn collecting_sink_truncates() {
        let mut sink = CollectingSink::new(Some(2));
        sink.on_event(StreamEvent::Columns(vec![ColumnInfo {
            name: "id".into(),
            type_name: "int".into(),
            column_type: None,
            nullable: None,
            primary_key: None,
            default: None,
            comment: None,
        }]));
        sink.on_event(StreamEvent::Rows(vec![
            vec![Value::I64(1)],
            vec![Value::I64(2)],
            vec![Value::I64(3)],
        ]));
        let out = sink.into_output();
        let rs = out.first_result_set().unwrap();
        assert_eq!(rs.rows.len(), 2);
        assert!(rs.truncated);
    }

    /// 构造一个最小 `ColumnInfo`（测试辅助）。
    fn col(name: &str) -> ColumnInfo {
        ColumnInfo {
            name: name.into(),
            type_name: "int".into(),
            column_type: None,
            nullable: None,
            primary_key: None,
            default: None,
            comment: None,
        }
    }

    #[test]
    fn collecting_sink_buckets_multiple_result_sets() {
        let mut sink = CollectingSink::new(Some(100));
        sink.on_event(StreamEvent::Columns(vec![col("a")]));
        sink.on_event(StreamEvent::Rows(vec![vec![Value::I64(1)]]));
        sink.on_event(StreamEvent::ResultSetEnd);
        sink.on_event(StreamEvent::Columns(vec![col("b")]));
        sink.on_event(StreamEvent::Rows(vec![vec![Value::I64(2)]]));
        let out = sink.into_output();
        assert_eq!(out.result_sets.len(), 2);
        assert_eq!(out.result_sets[0].columns[0].name, "a");
        assert_eq!(out.result_sets[1].columns[0].name, "b");
    }

    #[test]
    fn stream_event_serializes_tagged() {
        let ev = StreamEvent::Rows(vec![vec![Value::I64(1)]]);
        let json = serde_json::to_value(&ev).unwrap();
        assert_eq!(json["event"], serde_json::json!("rows"));
    }

    #[test]
    fn result_set_end_and_done_serialize() {
        assert_eq!(
            serde_json::to_value(StreamEvent::ResultSetEnd).unwrap()["event"],
            "result_set_end"
        );
        assert_eq!(
            serde_json::to_value(StreamEvent::Done).unwrap()["event"],
            "done" // unit 变体无 data
        );
        let err = StreamEvent::Error {
            kind: "cancelled".into(),
            message: "x".into(),
        };
        let json = serde_json::to_value(err).unwrap();
        assert_eq!(json["data"]["message"], "x");
        assert_eq!(json["data"]["kind"], "cancelled");
    }
}
