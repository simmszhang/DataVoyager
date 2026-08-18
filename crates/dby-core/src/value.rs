//! 统一单元格值类型。
//!
//! 跨驱动、跨进程（前后端）无损表示一个单元格值：用带类型标签的 JSON
//! envelope 序列化（`{"t": "...", "v": ...}`），前端据此着色/格式化/选编辑控件。

use serde::ser::SerializeMap;

#[derive(Debug, Clone, PartialEq)]
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

// 手写 serde：`I64/U64` 的 `v` 用十进制字符串承载（JSON number 超过 2^53 会丢精度），
// 其余变体保持 `{"t": ..., "v": ...}` envelope 语义不变。反序列化时 `i64/u64` 同时接受
// 字符串与旧 number（迁移过渡，见 design §4.1）。
impl serde::Serialize for Value {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let mut map = serializer.serialize_map(Some(2))?;
        match self {
            Value::Null => {
                map.serialize_entry("t", "null")?;
            }
            Value::Bool(b) => {
                map.serialize_entry("t", "bool")?;
                map.serialize_entry("v", b)?;
            }
            Value::I64(i) => {
                map.serialize_entry("t", "i64")?;
                map.serialize_entry("v", &i.to_string())?;
            }
            Value::U64(u) => {
                map.serialize_entry("t", "u64")?;
                map.serialize_entry("v", &u.to_string())?;
            }
            Value::F64(f) => {
                map.serialize_entry("t", "f64")?;
                map.serialize_entry("v", f)?;
            }
            Value::Decimal(s)
            | Value::Str(s)
            | Value::Date(s)
            | Value::Time(s)
            | Value::DateTime(s)
            | Value::Uuid(s) => {
                map.serialize_entry("t", self.kind())?;
                map.serialize_entry("v", s)?;
            }
            Value::Bytes(b) => {
                map.serialize_entry("t", "bytes")?;
                map.serialize_entry("v", b)?;
            }
            Value::Json(j) => {
                map.serialize_entry("t", "json")?;
                map.serialize_entry("v", j)?;
            }
            Value::Array(a) => {
                map.serialize_entry("t", "array")?;
                map.serialize_entry("v", a)?;
            }
            Value::Map(m) => {
                map.serialize_entry("t", "map")?;
                map.serialize_entry("v", m)?;
            }
        }
        map.end()
    }
}

impl<'de> serde::Deserialize<'de> for Value {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        struct ValueVisitor;

        impl<'de> serde::de::Visitor<'de> for ValueVisitor {
            type Value = Value;

            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                write!(f, "a tagged value envelope")
            }

            fn visit_map<A>(self, mut map: A) -> Result<Value, A::Error>
            where
                A: serde::de::MapAccess<'de>,
            {
                let mut tag: Option<String> = None;
                let mut content: Option<serde_json::Value> = None;
                while let Some(key) = map.next_key::<String>()? {
                    match key.as_str() {
                        "t" => tag = Some(map.next_value()?),
                        "v" => content = Some(map.next_value()?),
                        _ => {
                            let _: serde::de::IgnoredAny = map.next_value()?;
                        }
                    }
                }
                let tag = tag.ok_or_else(|| serde::de::Error::missing_field("t"))?;
                let content = content.unwrap_or(serde_json::Value::Null);
                Ok(match tag.as_str() {
                    "null" => Value::Null,
                    "bool" => Value::Bool(
                        serde_json::from_value(content).map_err(serde::de::Error::custom)?,
                    ),
                    "i64" => Value::I64(int_content(content, "i64")?),
                    "u64" => Value::U64(uint_content(content, "u64")?),
                    "f64" => Value::F64(
                        serde_json::from_value(content).map_err(serde::de::Error::custom)?,
                    ),
                    "decimal" => Value::Decimal(string_content(content, "decimal")?),
                    "str" => Value::Str(string_content(content, "str")?),
                    "bytes" => Value::Bytes(
                        serde_json::from_value(content).map_err(serde::de::Error::custom)?,
                    ),
                    "date" => Value::Date(string_content(content, "date")?),
                    "time" => Value::Time(string_content(content, "time")?),
                    "datetime" => Value::DateTime(string_content(content, "datetime")?),
                    "json" => Value::Json(content),
                    "uuid" => Value::Uuid(string_content(content, "uuid")?),
                    "array" => Value::Array(
                        serde_json::from_value(content).map_err(serde::de::Error::custom)?,
                    ),
                    "map" => Value::Map(
                        serde_json::from_value(content).map_err(serde::de::Error::custom)?,
                    ),
                    _ => return Err(serde::de::Error::custom("unknown value tag")),
                })
            }
        }

        deserializer.deserialize_map(ValueVisitor)
    }
}

/// `v` 必须是 JSON 字符串（`i64/u64` 新格式）。
fn string_content<E: serde::de::Error>(
    v: serde_json::Value,
    tag: &'static str,
) -> Result<String, E> {
    match v {
        serde_json::Value::String(s) => Ok(s),
        _ => Err(E::custom(format!("{tag} v must be a string"))),
    }
}

/// `i64` 的 `v` 接受字符串（新格式）或旧 number（迁移过渡），并校验范围。
fn int_content<E: serde::de::Error>(v: serde_json::Value, tag: &'static str) -> Result<i64, E> {
    match v {
        serde_json::Value::String(s) => s.parse().map_err(serde::de::Error::custom),
        serde_json::Value::Number(n) => n
            .as_i64()
            .ok_or_else(|| E::custom(format!("{tag} out of range"))),
        _ => Err(E::custom(format!("{tag} v must be a string or number"))),
    }
}

fn uint_content<E: serde::de::Error>(v: serde_json::Value, tag: &'static str) -> Result<u64, E> {
    match v {
        serde_json::Value::String(s) => s.parse().map_err(serde::de::Error::custom),
        serde_json::Value::Number(n) => n
            .as_u64()
            .ok_or_else(|| E::custom(format!("{tag} out of range"))),
        _ => Err(E::custom(format!("{tag} v must be a string or number"))),
    }
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
                format!(
                    "0x{}",
                    b.iter().map(|x| format!("{x:02x}")).collect::<String>()
                )
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

    /// 转为普通 JSON 值（用于导出 JSON；与带 tag 的 IPC envelope 不同）。
    pub fn to_json_value(&self) -> serde_json::Value {
        match self {
            Value::Null => serde_json::Value::Null,
            Value::Bool(b) => serde_json::Value::Bool(*b),
            Value::I64(i) => serde_json::Value::from(*i),
            Value::U64(u) => serde_json::Value::from(*u),
            Value::F64(f) => serde_json::Number::from_f64(*f)
                .map(serde_json::Value::Number)
                .unwrap_or(serde_json::Value::Null),
            Value::Decimal(s)
            | Value::Str(s)
            | Value::Date(s)
            | Value::Time(s)
            | Value::DateTime(s)
            | Value::Uuid(s) => serde_json::Value::String(s.clone()),
            Value::Bytes(b) => serde_json::Value::String(format!(
                "0x{}",
                b.iter().map(|x| format!("{x:02x}")).collect::<String>()
            )),
            Value::Json(j) => j.clone(),
            Value::Array(a) => {
                serde_json::Value::Array(a.iter().map(|v| v.to_json_value()).collect())
            }
            Value::Map(m) => serde_json::Value::Object(
                m.iter()
                    .map(|(k, v)| (k.clone(), v.to_json_value()))
                    .collect(),
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
        assert_eq!(json, serde_json::json!({"t": "i64", "v": "42"}));

        let v = Value::Null;
        let json = serde_json::to_value(&v).unwrap();
        assert_eq!(json, serde_json::json!({"t": "null"}));
    }

    #[test]
    fn i64_serializes_as_string() {
        assert_eq!(
            serde_json::to_value(Value::I64(9223372036854775807)).unwrap(),
            serde_json::json!({"t": "i64", "v": "9223372036854775807"})
        );
        let back: Value =
            serde_json::from_str("{\"t\":\"i64\",\"v\":\"9223372036854775807\"}").unwrap();
        assert_eq!(back, Value::I64(9223372036854775807));
        let u = Value::U64(18446744073709551615);
        assert_eq!(
            serde_json::to_value(&u).unwrap(),
            serde_json::json!({"t": "u64", "v": "18446744073709551615"})
        );
    }

    #[test]
    fn i64_deserializes_legacy_number() {
        // 迁移过渡：旧 IPC 负载中 i64/u64 的 v 是 JSON number，必须仍能反序列化。
        let back: Value = serde_json::from_str("{\"t\":\"i64\",\"v\":42}").unwrap();
        assert_eq!(back, Value::I64(42));
        let back: Value = serde_json::from_str("{\"t\":\"u64\",\"v\":7}").unwrap();
        assert_eq!(back, Value::U64(7));
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
