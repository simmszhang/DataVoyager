//! 数据库元数据模型（跨驱动通用结构）。

use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct TableInfo {
    pub name: String,
    /// "table" / "view" / "system" 等
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub comment: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ColumnInfo {
    pub name: String,
    pub type_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nullable: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub primary_key: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub comment: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct IndexInfo {
    pub name: String,
    pub unique: bool,
    pub columns: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ForeignKeyInfo {
    pub name: String,
    pub columns: Vec<String>,
    pub referenced_table: String,
    pub referenced_columns: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TriggerInfo {
    pub name: String,
    pub timing: String,
    pub event: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProcedureInfo {
    pub name: String,
    /// "PROCEDURE" / "FUNCTION"
    pub kind: String,
}
