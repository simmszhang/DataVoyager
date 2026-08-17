//! 统一单元格值类型。
//!
//! 跨驱动、跨进程（前后端）无损表示一个单元格值：用带类型标签的 JSON
//! envelope 序列化（`{"t": "...", "v": ...}`），前端据此着色/格式化/选编辑控件。

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "t", content = "v", rename_all = "snake_case")]
pub enum Value {
    Null,
    Bool(bool),
    I64(i64),
    U64(u64),
    F64(f64),
    /// 无损 DECIMAL（字符串承载，避免浮点精度丢失）
    Decimal(String),
    Str(String),
    /// BLOB / 二进制
    Bytes(Vec<u8>),
    /// ISO 日期 "YYYY-MM-DD"
    Date(String),
    /// 时间 "HH:MM:SS[.ffffff]"
    Time(String),
    /// 日期时间 "YYYY-MM-DD HH:MM:SS[.ffffff]"
    DateTime(String),
    /// 原生 JSON 列
    Json(serde_json::Value),
    Uuid(String),
    Array(Vec<Value>),
    Map(Vec<(String, Value)>),
}

impl Value {
    /// 稳定类型名（与 serde tag 一致），供前端/导出/类型映射使用。
    pub fn kind(&self) -> &'static str {
        match self {
            Value::Null => "null",
            Value::Bool(_) => "bool",
            Value::I64(_) => "i64",
            Value::U64(_) => "u64",
            Value::F64(_) => "f64",
            Value::Decimal(_) => "decimal",
            Value::Str(_) => "str",
            Value::Bytes(_) => "bytes",
            Value::Date(_) => "date",
            Value::Time(_) => "time",
            Value::DateTime(_) => "datetime",
            Value::Json(_) => "json",
            Value::Uuid(_) => "uuid",
            Value::Array(_) => "array",
            Value::Map(_) => "map",
        }
    }

    /// 人可读字符串（复制 / 导出 / 展示）。
    pub fn to_display_string(&self) -> String {
        match self {
            Value::Null => "NULL".to_string(),
            Value::Bool(b) => b.to_string(),
            Value::I64(i) => i.to_string(),
            Value::U64(u) => u.to_string(),
            Value::F64(f) => f.to_string(),
            Value::Decimal(s)
            | Value::Str(s)
            | Value::Date(s)
            | Value::Time(s)
            | Value::DateTime(s)
            | Value::Uuid(s) => s.clone(),
            Value::Bytes(b) => {
                format!("0x{}", b.iter().map(|x| format!("{x:02x}")).collect::<String>())
            }
            Value::Json(j) => j.to_string(),
            Value::Array(arr) => format!(
                "[{}]",
                arr.iter()
                    .map(|v| v.to_display_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            Value::Map(m) => format!(
                "{{{}}}",
                m.iter()
                    .map(|(k, v)| format!("{k}: {}", v.to_display_string()))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kind_names_match_serde_tags() {
        assert_eq!(Value::Null.kind(), "null");
        assert_eq!(Value::I64(1).kind(), "i64");
        assert_eq!(Value::U64(1).kind(), "u64");
        assert_eq!(Value::F64(1.5).kind(), "f64");
        assert_eq!(Value::Decimal("1.00".into()).kind(), "decimal");
        assert_eq!(Value::Str("x".into()).kind(), "str");
        assert_eq!(Value::Bytes(vec![1]).kind(), "bytes");
        assert_eq!(Value::Json(serde_json::json!({"a":1})).kind(), "json");
        assert_eq!(Value::Array(vec![]).kind(), "array");
        assert_eq!(Value::Map(vec![]).kind(), "map");
    }

    #[test]
    fn serializes_as_tagged_envelope() {
        let v = Value::I64(42);
        let json = serde_json::to_value(&v).unwrap();
        assert_eq!(json, serde_json::json!({"t": "i64", "v": 42}));

        let v = Value::Null;
        let json = serde_json::to_value(&v).unwrap();
        assert_eq!(json, serde_json::json!({"t": "null"}));
    }

    #[test]
    fn roundtrips_through_json() {
        let values = vec![
            Value::Null,
            Value::Bool(true),
            Value::I64(-7),
            Value::U64(7),
            Value::F64(3.5),
            Value::Decimal("123.450".into()),
            Value::Str("héllo".into()),
            Value::Bytes(vec![0xde, 0xad]),
            Value::Date("2024-01-02".into()),
            Value::DateTime("2024-01-02 03:04:05".into()),
            Value::Json(serde_json::json!([1, 2, 3])),
            Value::Uuid("x".into()),
            Value::Array(vec![Value::I64(1), Value::Str("a".into())]),
            Value::Map(vec![("k".to_string(), Value::Bool(false))]),
        ];
        for v in values {
            let json = serde_json::to_string(&v).unwrap();
            let back: Value = serde_json::from_str(&json).unwrap();
            assert_eq!(v, back, "roundtrip failed for {json}");
        }
    }

    #[test]
    fn display_string_for_common_cases() {
        assert_eq!(Value::Null.to_display_string(), "NULL");
        assert_eq!(Value::I64(10).to_display_string(), "10");
        assert_eq!(Value::Bytes(vec![0xde, 0xad]).to_display_string(), "0xdead");
        assert_eq!(Value::Str("hi".into()).to_display_string(), "hi");
    }
}
