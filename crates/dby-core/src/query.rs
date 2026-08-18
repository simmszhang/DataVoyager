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

/// 流式执行事件（SELECT 吐 Columns + Rows 批次；DML 吐 Affected）。
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "event", content = "data", rename_all = "snake_case")]
pub enum StreamEvent {
    Columns(Vec<ColumnInfo>),
    Rows(Vec<Vec<Value>>),
    Affected { affected_rows: u64, last_insert_id: Option<u64> },
    Info(Option<String>),
}

/// 结果接收器：驱动把流式事件推给实现者（如 Tauri Channel 或缓冲收集器）。
pub trait ResultSink: Send {
    fn on_event(&mut self, ev: StreamEvent);
}

/// 极简取消令牌（避免 tokio-util 依赖）。
#[derive(Debug, Clone, Default)]
pub struct CancellationToken(Arc<AtomicBool>);

impl CancellationToken {
    pub fn new() -> Self {
        Self(Arc::new(AtomicBool::new(false)))
    }
    pub fn cancel(&self) {
        self.0.store(true, Ordering::SeqCst);
    }
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
}

/// 缓冲收集 sink：把流式事件收集成一个 `QueryOutput`（带行数截断）。
pub struct CollectingSink {
    columns: Option<Vec<ColumnInfo>>,
    rows: Vec<Vec<Value>>,
    max_rows: usize,
    truncated: bool,
    affected_rows: u64,
    last_insert_id: Option<u64>,
    info: Option<String>,
}

impl CollectingSink {
    pub fn new(max_rows: Option<usize>) -> Self {
        Self {
            columns: None,
            rows: Vec::new(),
            max_rows: max_rows.unwrap_or(usize::MAX),
            truncated: false,
            affected_rows: 0,
            last_insert_id: None,
            info: None,
        }
    }

    pub fn into_output(self) -> QueryOutput {
        let result_sets = match self.columns {
            Some(cols) => vec![ResultSet {
                columns: cols,
                rows: self.rows,
                truncated: self.truncated,
            }],
            None => vec![],
        };
        QueryOutput {
            result_sets,
            affected_rows: self.affected_rows,
            last_insert_id: self.last_insert_id,
            info: self.info,
        }
    }
}

impl ResultSink for CollectingSink {
    fn on_event(&mut self, ev: StreamEvent) {
        match ev {
            StreamEvent::Columns(cols) => self.columns = Some(cols),
            StreamEvent::Rows(rows) => {
                for r in rows {
                    if self.rows.len() < self.max_rows {
                        self.rows.push(r);
                    } else {
                        self.truncated = true;
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
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cancel_token_works() {
        let t = CancellationToken::new();
        assert!(!t.is_cancelled());
        t.cancel();
        assert!(t.is_cancelled());
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

    #[test]
    fn stream_event_serializes_tagged() {
        let ev = StreamEvent::Rows(vec![vec![Value::I64(1)]]);
        let json = serde_json::to_value(&ev).unwrap();
        assert_eq!(json["event"], serde_json::json!("rows"));
    }
}
