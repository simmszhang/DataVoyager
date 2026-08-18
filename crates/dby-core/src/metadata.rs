//! 数据库元数据模型（跨驱动通用结构）。

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize)]
pub struct TableInfo {
    pub name: String,
    /// "table" / "view" / "system" 等
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub comment: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ColumnTypeBase {
    Bool,
    I8,
    I16,
    I32,
    I64,
    U8,
    U16,
    U32,
    U64,
    F32,
    F64,
    Decimal,
    Str,
    Bytes,
    Date,
    Time,
    DateTime,
    Json,
    Uuid,
    Array,
    Map,
    #[default]
    Unknown,
}

/// 结构化列类型（#32）。`Copy` 的 `base` 使 `match ct.base` 不会 move 借用内容。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ColumnType {
    pub base: ColumnTypeBase,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub numeric_precision: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub numeric_scale: Option<u32>,
    #[serde(default)]
    pub unsigned: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub char_max_length: Option<u32>,
    /// datetime(6)/timestamp(6) 的小数秒位
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temporal_precision: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub charset: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub collation: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ColumnInfo {
    pub name: String,
    pub type_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub column_type: Option<ColumnType>,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn column_type_serializes_base_and_metadata() {
        let ct = ColumnType {
            base: ColumnTypeBase::Decimal,
            numeric_precision: Some(10),
            numeric_scale: Some(2),
            unsigned: false,
            char_max_length: None,
            temporal_precision: None,
            charset: None,
            collation: None,
        };
        let json = serde_json::to_value(&ct).unwrap();
        assert_eq!(json["base"], "decimal");
        assert_eq!(json["numeric_precision"], 10);
        assert_eq!(json["numeric_scale"], 2);
    }
}
