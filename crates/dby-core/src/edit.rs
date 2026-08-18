//! 数据编辑 SQL 生成（方言感知）：行内编辑生成 UPDATE/INSERT/DELETE。

use crate::dialect::Dialect;
use crate::error::{DbError, Result};
use crate::metadata::{ColumnType, ColumnTypeBase};
use crate::value::Value;

/// 把 `Value` 格式化为 SQL 字面量（NULL / 数字 / 字符串转义 / bytes hex）。
pub fn quote_value(dialect: &dyn Dialect, v: &Value) -> String {
    match v {
        Value::Null => "NULL".to_string(),
        Value::Bool(b) => {
            if *b {
                "1".to_string()
            } else {
                "0".to_string()
            }
        }
        Value::I64(i) => i.to_string(),
        Value::U64(u) => u.to_string(),
        Value::F64(f) => f.to_string(),
        Value::Decimal(s)
        | Value::Str(s)
        | Value::Date(s)
        | Value::Time(s)
        | Value::DateTime(s)
        | Value::Uuid(s) => dialect.quote_string(s),
        Value::Bytes(b) => format!(
            "X'{}'",
            b.iter().map(|x| format!("{x:02x}")).collect::<String>()
        ),
        Value::Json(j) => dialect.quote_string(&j.to_string()),
        Value::Array(_) | Value::Map(_) => dialect.quote_string(&v.to_display_string()),
    }
}

/// 生成 `UPDATE table SET ... WHERE pk ...`。
pub fn build_update(
    dialect: &dyn Dialect,
    table: &str,
    pk: &[(String, Value)],
    set: &[(String, Value)],
) -> String {
    let set_clause = set
        .iter()
        .map(|(c, v)| {
            format!(
                "{} = {}",
                dialect.quote_identifier(c),
                quote_value(dialect, v)
            )
        })
        .collect::<Vec<_>>()
        .join(", ");
    let where_clause = build_where(dialect, pk);
    format!(
        "UPDATE {} SET {} WHERE {};",
        dialect.quote_identifier(table),
        set_clause,
        where_clause
    )
}

/// 生成 `INSERT INTO table (cols) VALUES (...)`。
pub fn build_insert(
    dialect: &dyn Dialect,
    table: &str,
    columns: &[String],
    values: &[Value],
) -> String {
    let cols = columns
        .iter()
        .map(|c| dialect.quote_identifier(c))
        .collect::<Vec<_>>()
        .join(", ");
    let vals = values
        .iter()
        .map(|v| quote_value(dialect, v))
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "INSERT INTO {} ({}) VALUES ({});",
        dialect.quote_identifier(table),
        cols,
        vals
    )
}

/// 生成 `DELETE FROM table WHERE pk ...`。
pub fn build_delete(dialect: &dyn Dialect, table: &str, pk: &[(String, Value)]) -> String {
    format!(
        "DELETE FROM {} WHERE {};",
        dialect.quote_identifier(table),
        build_where(dialect, pk)
    )
}

fn build_where(dialect: &dyn Dialect, pk: &[(String, Value)]) -> String {
    pk.iter()
        .map(|(c, v)| {
            format!(
                "{} = {}",
                dialect.quote_identifier(c),
                quote_value(dialect, v)
            )
        })
        .collect::<Vec<_>>()
        .join(" AND ")
}

/// 按列类型把编辑输入串解析为 `Value`（design §4.6，#11/#69）。
///
/// - `"NULL"`（大小写不敏感，trim 后）→ `Value::Null`（SQL NULL，旧前端启发式行为）；
/// - Bool：`"true"/"1"`→true、`"false"/"0"`→false；非 0/1 回退整数（与读路径 conv.rs 一致，不坍缩）；
/// - 整型/浮点按对应宽度解析；Decimal 保留原串；Bytes 取 UTF-8 字节；
/// - Date/Time/DateTime 校验格式后产出类型化值（不再产出未校验的 Str）；
/// - Json 走 `serde_json`；Str/Uuid/Array/Map/Unknown 原样收为 `Value::Str`。
///
/// 解析失败统一 `DbError::Other("无法将 '<input>' 解析为 <type>")`。
pub fn parse_value(input: &str, ct: &ColumnType) -> Result<Value> {
    let t = input.trim();
    // 输入 "NULL"（大小写不敏感）→ SQL NULL，与旧前端启发式一致（§4.6 未列，非禁止）。
    // `quote_value` 已把 `Value::Null` 渲染为 "NULL"，与 build_update 组合正确。
    if t.eq_ignore_ascii_case("NULL") {
        return Ok(Value::Null);
    }
    match &ct.base {
        ColumnTypeBase::Bool => match t {
            "true" | "1" => Ok(Value::Bool(true)),
            "false" | "0" => Ok(Value::Bool(false)),
            // 与读路径 conv.rs 一致：TINYINT(1) 非 0/1 回退整数，不坍缩为 Bool
            _ if ct.unsigned => t
                .parse::<u64>()
                .map(Value::U64)
                .map_err(|_| parse_err(t, ct.base)),
            _ => t
                .parse::<i64>()
                .map(Value::I64)
                .map_err(|_| parse_err(t, ct.base)),
        },
        ColumnTypeBase::I8 | ColumnTypeBase::I16 | ColumnTypeBase::I32 | ColumnTypeBase::I64 => t
            .parse::<i64>()
            .map(Value::I64)
            .map_err(|_| parse_err(t, ct.base)),
        ColumnTypeBase::U8 | ColumnTypeBase::U16 | ColumnTypeBase::U32 | ColumnTypeBase::U64 => t
            .parse::<u64>()
            .map(Value::U64)
            .map_err(|_| parse_err(t, ct.base)),
        ColumnTypeBase::F32 | ColumnTypeBase::F64 => t
            .parse::<f64>()
            .map(Value::F64)
            .map_err(|_| parse_err(t, ct.base)),
        ColumnTypeBase::Decimal => Ok(Value::Decimal(t.to_string())),
        ColumnTypeBase::Date => validate_date(t).map(Value::Date),
        ColumnTypeBase::Time => validate_time(t).map(Value::Time),
        ColumnTypeBase::DateTime => validate_datetime(t).map(Value::DateTime),
        ColumnTypeBase::Json => serde_json::from_str(t)
            .map(Value::Json)
            .map_err(|_| parse_err(t, ct.base)),
        ColumnTypeBase::Bytes => Ok(Value::Bytes(t.as_bytes().to_vec())),
        ColumnTypeBase::Str
        | ColumnTypeBase::Uuid
        | ColumnTypeBase::Array
        | ColumnTypeBase::Map
        | ColumnTypeBase::Unknown => Ok(Value::Str(t.to_string())),
    }
}

fn parse_err(input: &str, base: ColumnTypeBase) -> DbError {
    DbError::Other(format!("无法将 '{input}' 解析为 {}", base_name(base)))
}

fn base_name(base: ColumnTypeBase) -> &'static str {
    match base {
        ColumnTypeBase::Bool => "bool",
        ColumnTypeBase::I8 => "i8",
        ColumnTypeBase::I16 => "i16",
        ColumnTypeBase::I32 => "i32",
        ColumnTypeBase::I64 => "i64",
        ColumnTypeBase::U8 => "u8",
        ColumnTypeBase::U16 => "u16",
        ColumnTypeBase::U32 => "u32",
        ColumnTypeBase::U64 => "u64",
        ColumnTypeBase::F32 => "f32",
        ColumnTypeBase::F64 => "f64",
        ColumnTypeBase::Decimal => "decimal",
        ColumnTypeBase::Str => "str",
        ColumnTypeBase::Bytes => "bytes",
        ColumnTypeBase::Date => "date",
        ColumnTypeBase::Time => "time",
        ColumnTypeBase::DateTime => "datetime",
        ColumnTypeBase::Json => "json",
        ColumnTypeBase::Uuid => "uuid",
        ColumnTypeBase::Array => "array",
        ColumnTypeBase::Map => "map",
        ColumnTypeBase::Unknown => "unknown",
    }
}

/// "YYYY-MM-DD"。
fn validate_date(s: &str) -> Result<String> {
    if is_date(s) {
        Ok(s.to_string())
    } else {
        Err(parse_err(s, ColumnTypeBase::Date))
    }
}

fn is_date(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 10
        && b[4] == b'-'
        && b[7] == b'-'
        && b[..4].iter().all(|c| c.is_ascii_digit())
        && b[5..7].iter().all(|c| c.is_ascii_digit())
        && b[8..10].iter().all(|c| c.is_ascii_digit())
}

/// "HH:MM:SS[.ffffff]"。
fn validate_time(s: &str) -> Result<String> {
    if is_time(s) {
        Ok(s.to_string())
    } else {
        Err(parse_err(s, ColumnTypeBase::Time))
    }
}

fn is_time(s: &str) -> bool {
    let b = s.as_bytes();
    let (hms, frac) = match b.iter().position(|&c| c == b'.') {
        Some(i) => (&b[..i], Some(&b[i + 1..])),
        None => (b, None),
    };
    hms.len() == 8
        && hms[2] == b':'
        && hms[5] == b':'
        && hms[..2].iter().all(|c| c.is_ascii_digit())
        && hms[3..5].iter().all(|c| c.is_ascii_digit())
        && hms[6..8].iter().all(|c| c.is_ascii_digit())
        && frac.is_none_or(|f| (1..=6).contains(&f.len()) && f.iter().all(|c| c.is_ascii_digit()))
}

/// "YYYY-MM-DD HH:MM:SS[.ffffff]"。
fn validate_datetime(s: &str) -> Result<String> {
    if let Some((date, time)) = s.split_once(' ') {
        if is_date(date) && is_time(time) {
            return Ok(s.to_string());
        }
    }
    Err(parse_err(s, ColumnTypeBase::DateTime))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metadata::{ColumnType, ColumnTypeBase};

    struct TestDialect;
    impl Dialect for TestDialect {
        fn quote_identifier(&self, ident: &str) -> String {
            format!("`{}`", ident.replace('`', "``"))
        }
        fn quote_string(&self, s: &str) -> String {
            format!("'{}'", s.replace('\\', "\\\\").replace('\'', "\\'"))
        }
        fn limit_clause(&self, limit: Option<u64>, offset: Option<u64>) -> String {
            match (limit, offset) {
                (Some(l), Some(o)) => format!("LIMIT {o}, {l}"),
                (Some(l), None) => format!("LIMIT {l}"),
                _ => String::new(),
            }
        }
        fn parse_column_type(&self, _raw: &str) -> Option<ColumnType> {
            None
        }
        fn display_type_name(&self, ct: &ColumnType) -> String {
            format!("{:?}", ct.base)
        }
    }

    #[test]
    fn quote_value_covers_types() {
        let d = TestDialect;
        assert_eq!(quote_value(&d, &Value::Null), "NULL");
        assert_eq!(quote_value(&d, &Value::I64(5)), "5");
        assert_eq!(quote_value(&d, &Value::Bool(true)), "1");
        assert_eq!(quote_value(&d, &Value::Str("a'b".into())), "'a\\'b'");
        assert_eq!(quote_value(&d, &Value::Bytes(vec![0xde, 0xad])), "X'dead'");
        assert_eq!(quote_value(&d, &Value::Decimal("1.50".into())), "'1.50'");
    }

    #[test]
    fn build_update_quotes_identifiers_and_values() {
        let d = TestDialect;
        let sql = build_update(
            &d,
            "users",
            &[("id".into(), Value::I64(1))],
            &[("name".into(), Value::Str("O'Brien".into()))],
        );
        assert_eq!(
            sql,
            "UPDATE `users` SET `name` = 'O\\'Brien' WHERE `id` = 1;"
        );
    }

    #[test]
    fn build_insert_and_delete() {
        let d = TestDialect;
        let insert = build_insert(
            &d,
            "users",
            &["name".into(), "age".into()],
            &[Value::Str("x".into()), Value::I64(3)],
        );
        assert_eq!(
            insert,
            "INSERT INTO `users` (`name`, `age`) VALUES ('x', 3);"
        );

        let delete = build_delete(&d, "users", &[("id".into(), Value::I64(9))]);
        assert_eq!(delete, "DELETE FROM `users` WHERE `id` = 9;");
    }

    #[test]
    fn parse_value_by_column_type() {
        let i64ct = ColumnType {
            base: ColumnTypeBase::I64,
            ..Default::default()
        };
        assert_eq!(parse_value("42", &i64ct).unwrap(), Value::I64(42));
        assert!(parse_value("abc", &i64ct).is_err());
        // 评审（Important）：输入 "NULL" → SQL NULL（旧前端启发式行为），不报解析错误
        assert_eq!(parse_value("NULL", &i64ct).unwrap(), Value::Null);
        let decct = ColumnType {
            base: ColumnTypeBase::Decimal,
            ..Default::default()
        };
        assert_eq!(
            parse_value("1.50", &decct).unwrap(),
            Value::Decimal("1.50".into())
        );
        let date_ct = ColumnType {
            base: ColumnTypeBase::Date,
            ..Default::default()
        };
        assert_eq!(
            parse_value("2024-01-02", &date_ct).unwrap(),
            Value::Date("2024-01-02".into())
        );
        assert!(parse_value("garbage", &date_ct).is_err()); // 时间类校验格式，不产出未校验 Str
        let jsonct = ColumnType {
            base: ColumnTypeBase::Json,
            ..Default::default()
        };
        assert_eq!(
            parse_value("{\"a\":1}", &jsonct).unwrap(),
            Value::Json(serde_json::json!({"a": 1}))
        );
    }

    /// 其余 base 的解析 + 失败路径（design §4.6 / §6）：Bool/unsigned/F32/F64/
    /// Time/DateTime/Bytes/Uuid/Unknown，以及错误消息统一为「无法将 '<input>' 解析为 <type>」。
    #[test]
    fn parse_value_covers_remaining_bases() {
        let boolct = ColumnType {
            base: ColumnTypeBase::Bool,
            ..Default::default()
        };
        assert_eq!(parse_value("true", &boolct).unwrap(), Value::Bool(true));
        assert_eq!(parse_value("1", &boolct).unwrap(), Value::Bool(true));
        assert_eq!(parse_value("false", &boolct).unwrap(), Value::Bool(false));
        assert_eq!(parse_value("0", &boolct).unwrap(), Value::Bool(false));
        // TINYINT(1) 非 0/1：与读路径 conv.rs 一致回退整数，不坍缩为 Bool（评审 Minor 顺手修复）
        assert_eq!(parse_value("2", &boolct).unwrap(), Value::I64(2));
        let ubool = ColumnType {
            base: ColumnTypeBase::Bool,
            unsigned: true,
            ..Default::default()
        };
        assert_eq!(parse_value("2", &ubool).unwrap(), Value::U64(2));
        assert!(parse_value("abc", &boolct).is_err());

        // NULL 输入大小写不敏感、trim 后生效，Str 列同样产出 SQL NULL（评审 Important）
        let i64ct = ColumnType {
            base: ColumnTypeBase::I64,
            ..Default::default()
        };
        let str_ct = ColumnType {
            base: ColumnTypeBase::Str,
            ..Default::default()
        };
        assert_eq!(parse_value("null", &i64ct).unwrap(), Value::Null);
        assert_eq!(parse_value(" Null ", &str_ct).unwrap(), Value::Null);

        let u64ct = ColumnType {
            base: ColumnTypeBase::U64,
            ..Default::default()
        };
        assert_eq!(parse_value("42", &u64ct).unwrap(), Value::U64(42));
        assert!(parse_value("-1", &u64ct).is_err());

        let f64ct = ColumnType {
            base: ColumnTypeBase::F64,
            ..Default::default()
        };
        assert_eq!(parse_value("1.5", &f64ct).unwrap(), Value::F64(1.5));
        assert!(parse_value("x", &f64ct).is_err());

        let time_ct = ColumnType {
            base: ColumnTypeBase::Time,
            ..Default::default()
        };
        assert_eq!(
            parse_value("10:30:00", &time_ct).unwrap(),
            Value::Time("10:30:00".into())
        );
        assert_eq!(
            parse_value("10:30:00.500000", &time_ct).unwrap(),
            Value::Time("10:30:00.500000".into())
        );
        assert!(parse_value("25:99", &time_ct).is_err());

        let dt_ct = ColumnType {
            base: ColumnTypeBase::DateTime,
            ..Default::default()
        };
        assert_eq!(
            parse_value("2024-01-02 03:04:05", &dt_ct).unwrap(),
            Value::DateTime("2024-01-02 03:04:05".into())
        );
        assert_eq!(
            parse_value("2024-01-02 03:04:05.123456", &dt_ct).unwrap(),
            Value::DateTime("2024-01-02 03:04:05.123456".into())
        );
        assert!(parse_value("2024-01-02", &dt_ct).is_err());
        assert!(parse_value("garbage", &dt_ct).is_err());

        let bytes_ct = ColumnType {
            base: ColumnTypeBase::Bytes,
            ..Default::default()
        };
        assert_eq!(
            parse_value("hi", &bytes_ct).unwrap(),
            Value::Bytes(b"hi".to_vec())
        );

        assert_eq!(parse_value("x", &str_ct).unwrap(), Value::Str("x".into()));

        let uuid_ct = ColumnType {
            base: ColumnTypeBase::Uuid,
            ..Default::default()
        };
        assert_eq!(
            parse_value("u1", &uuid_ct).unwrap(),
            Value::Str("u1".into())
        );

        let unknown_ct = ColumnType::unknown();
        assert_eq!(
            parse_value("abc", &unknown_ct).unwrap(),
            Value::Str("abc".into())
        );

        // 错误消息统一为「无法将 '<input>' 解析为 <type>」
        let err = parse_value("abc", &i64ct).unwrap_err();
        assert_eq!(err.to_string(), "无法将 'abc' 解析为 i64");
    }
}
