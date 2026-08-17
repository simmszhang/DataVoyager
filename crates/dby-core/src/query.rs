//! 查询执行相关类型。

use serde::Serialize;

use crate::metadata::ColumnInfo;
use crate::value::Value;

/// 执行选项。
#[derive(Debug, Clone, Default)]
pub struct ExecOpts {
    pub timeout_ms: Option<u64>,
    /// 单结果集最多存储的行数（超出的行仍被消费，但标记 truncated）
    pub max_rows: Option<usize>,
}

/// SQL 来源，用于历史记录归因。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SqlOrigin {
    ManualEditor,
    DataEdit,
    SchemaEdit,
    Export,
    Ai,
    Plugin,
    Cli,
    Other,
}

impl Default for SqlOrigin {
    fn default() -> Self {
        SqlOrigin::Other
    }
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

    pub fn from_str(s: &str) -> SqlOrigin {
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
    /// 是否因行数上限被截断
    pub truncated: bool,
}

/// 一次语句执行的输出（可含多个结果集）。
#[derive(Debug, Clone, Serialize, Default)]
pub struct QueryOutput {
    pub result_sets: Vec<ResultSet>,
    pub affected_rows: u64,
    pub last_insert_id: Option<u64>,
    /// 服务器返回的 info 字符串（如 "Records: 5"）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub info: Option<String>,
}

impl QueryOutput {
    /// 便捷：首个结果集（若有）。
    pub fn first_result_set(&self) -> Option<&ResultSet> {
        self.result_sets.first()
    }
}
