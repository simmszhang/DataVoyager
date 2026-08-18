//! `mysql_async::Value` → `dby_core::Value` 转换。
//!
//! 文本协议（`query_iter`）下所有非 NULL 值均以 `MyValue::Bytes` 到达
//! （行数据无类型信息），因此转换按列类型解析字节串；强类型变体分支为
//! 二进制协议预留（当前文本路径不产生）。

use dby_core::metadata::{ColumnType, ColumnTypeBase};
use dby_core::value::Value;
use mysql_async::Value as MyValue;

/// 把 MySQL 单元格值按列类型转换为统一 Value。
///
/// 文本协议下非 NULL 值均为 `MyValue::Bytes`，故按 `ct.base` 解析字节串
/// （整数/浮点 parse、Decimal 保留原串、Json parse、Date/DATETIME 字符串直通、
/// Time 经 `normalize_time_str` 规范化、Bool 仅认 "0"/"1"）；
/// `Unknown` 等无类型信息列回退 `mysql_value_to_dby_legacy`。
pub fn mysql_value_to_dby(v: &MyValue, ct: &ColumnType) -> Value {
    match v {
        MyValue::NULL => Value::Null,
        MyValue::Bytes(b) => {
            let s = String::from_utf8_lossy(b);
            match &ct.base {
                ColumnTypeBase::Bool => match s.trim() {
                    "0" => Value::Bool(false),
                    "1" => Value::Bool(true),
                    _ if ct.unsigned => s
                        .trim()
                        .parse()
                        .map(Value::U64)
                        .unwrap_or_else(|_| Value::Str(s.into_owned())),
                    // TINYINT(1) 非 0/1 回退整数，不坍缩为 Bool
                    _ => s
                        .trim()
                        .parse()
                        .map(Value::I64)
                        .unwrap_or_else(|_| Value::Str(s.into_owned())),
                },
                ColumnTypeBase::I8
                | ColumnTypeBase::I16
                | ColumnTypeBase::I32
                | ColumnTypeBase::I64 => {
                    if ct.unsigned {
                        Value::U64(s.trim().parse().unwrap_or(0))
                    } else {
                        Value::I64(s.trim().parse().unwrap_or(0))
                    }
                }
                ColumnTypeBase::U8
                | ColumnTypeBase::U16
                | ColumnTypeBase::U32
                | ColumnTypeBase::U64 => Value::U64(s.trim().parse().unwrap_or(0)),
                ColumnTypeBase::F32 | ColumnTypeBase::F64 => {
                    Value::F64(s.trim().parse().unwrap_or(0.0))
                }
                // DECIMAL 保留原始字符串（无损，不转 f64）
                ColumnTypeBase::Decimal => Value::Decimal(s.into_owned()),
                ColumnTypeBase::Str => Value::Str(s.into_owned()),
                ColumnTypeBase::Bytes => Value::Bytes(b.clone()),
                ColumnTypeBase::Json => serde_json::from_str(&s)
                    .map(Value::Json)
                    .unwrap_or_else(|_| Value::Str(s.into_owned())),
                // 列类型已定，Date/DATETIME 字符串直通（不再按时分秒推断）
                ColumnTypeBase::Date => Value::Date(s.into_owned()),
                ColumnTypeBase::DateTime => Value::DateTime(s.into_owned()),
                ColumnTypeBase::Time => Value::Time(normalize_time_str(&s)),
                // Unknown/Uuid/Array/Map → 现有启发式
                _ => mysql_value_to_dby_legacy(v),
            }
        }
        // —— 二进制协议预留（当前文本协议不产这些变体；未来切 BinaryProtocol 才命中）——
        MyValue::Int(i) => Value::I64(*i),
        MyValue::UInt(u) => Value::U64(*u),
        MyValue::Float(f) => Value::F64(*f as f64),
        MyValue::Double(d) => Value::F64(*d),
        MyValue::Date(y, mo, d, h, mi, s, us) => {
            if *h == 0 && *mi == 0 && *s == 0 && *us == 0 {
                Value::Date(format!("{y:04}-{mo:02}-{d:02}"))
            } else if *us == 0 {
                Value::DateTime(format!("{y:04}-{mo:02}-{d:02} {h:02}:{mi:02}:{s:02}"))
            } else {
                Value::DateTime(format!(
                    "{y:04}-{mo:02}-{d:02} {h:02}:{mi:02}:{s:02}.{us:06}"
                ))
            }
        }
        MyValue::Time(neg, days, h, mi, s, us) => {
            let sign = if *neg { "-" } else { "" };
            Value::Time(format!("{sign}{days} {h:02}:{mi:02}:{s:02}.{us:06}"))
        }
    }
}

/// 无列类型信息（Unknown/Uuid/Array/Map）时的回退转换：原有启发式
/// （`Bytes` 可 UTF-8 → Str，否则 Bytes；强类型变体直通）。
pub fn mysql_value_to_dby_legacy(v: &MyValue) -> Value {
    match v {
        MyValue::NULL => Value::Null,
        MyValue::Bytes(b) => match std::str::from_utf8(b) {
            Ok(s) => Value::Str(s.to_string()),
            Err(_) => Value::Bytes(b.clone()),
        },
        MyValue::Int(i) => Value::I64(*i),
        MyValue::UInt(u) => Value::U64(*u),
        MyValue::Float(f) => Value::F64(*f as f64),
        MyValue::Double(d) => Value::F64(*d),
        MyValue::Date(y, mo, d, h, mi, s, us) => {
            if *h == 0 && *mi == 0 && *s == 0 && *us == 0 {
                Value::Date(format!("{y:04}-{mo:02}-{d:02}"))
            } else if *us == 0 {
                Value::DateTime(format!("{y:04}-{mo:02}-{d:02} {h:02}:{mi:02}:{s:02}"))
            } else {
                Value::DateTime(format!(
                    "{y:04}-{mo:02}-{d:02} {h:02}:{mi:02}:{s:02}.{us:06}"
                ))
            }
        }
        MyValue::Time(neg, days, h, mi, s, us) => {
            let sign = if *neg { "-" } else { "" };
            Value::Time(format!("{sign}{days} {h:02}:{mi:02}:{s:02}.{us:06}"))
        }
    }
}

/// 文本协议 `TIME` 规范化（#60）：服务器文本形如 `HH:MM:SS[.ffffff]`
/// （仅 `fsp>0` 才带小数）。微秒为零（`.000000` 后缀）时剥离；
/// 负 TIME 的前导 `-` 与超 24h 的 `HHH:MM:SS` 原样保留。
fn normalize_time_str(s: &str) -> String {
    match s.strip_suffix(".000000") {
        Some(trimmed) => trimmed.to_string(),
        None => s.to_string(),
    }
}

/// 以 `String` 读取单元格（NULL 或非文本返回 None，非 panicking）。
pub fn row_string(row: &mysql_async::Row, index: usize) -> Option<String> {
    row.get_opt::<String, usize>(index).and_then(|r| r.ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ct(base: ColumnTypeBase) -> ColumnType {
        ColumnType {
            base,
            ..Default::default()
        }
    }

    fn ct_unsigned(base: ColumnTypeBase) -> ColumnType {
        ColumnType {
            base,
            unsigned: true,
            ..Default::default()
        }
    }

    // —— 文本协议：Bytes 字符串按列类型解析 ——

    #[test]
    fn int_column_parses_bytes_to_i64() {
        assert_eq!(
            mysql_value_to_dby(&MyValue::Bytes(b"42".to_vec()), &ct(ColumnTypeBase::I32)),
            Value::I64(42)
        );
    }

    #[test]
    fn unsigned_int_column_parses_to_u64() {
        assert_eq!(
            mysql_value_to_dby(
                &MyValue::Bytes(b"18446744073709551615".to_vec()),
                &ct(ColumnTypeBase::U64)
            ),
            Value::U64(18446744073709551615)
        );
    }

    #[test]
    fn signed_int_column_with_unsigned_flag_yields_u64() {
        assert_eq!(
            mysql_value_to_dby(
                &MyValue::Bytes(b"42".to_vec()),
                &ct_unsigned(ColumnTypeBase::I32)
            ),
            Value::U64(42)
        );
    }

    #[test]
    fn decimal_is_decimal_not_str() {
        assert_eq!(
            mysql_value_to_dby(
                &MyValue::Bytes(b"123.450".to_vec()),
                &ct(ColumnTypeBase::Decimal)
            ),
            Value::Decimal("123.450".into())
        );
    }

    #[test]
    fn blob_is_bytes_not_str() {
        assert_eq!(
            mysql_value_to_dby(
                &MyValue::Bytes(vec![0xff, 0xfe]),
                &ct(ColumnTypeBase::Bytes)
            ),
            Value::Bytes(vec![0xff, 0xfe])
        );
    }

    #[test]
    fn tinyint1_zero_one_bool_else_i64() {
        assert_eq!(
            mysql_value_to_dby(&MyValue::Bytes(b"1".to_vec()), &ct(ColumnTypeBase::Bool)),
            Value::Bool(true)
        );
        // 非 0/1 不坍缩为 Bool，回退整数
        assert_eq!(
            mysql_value_to_dby(&MyValue::Bytes(b"2".to_vec()), &ct(ColumnTypeBase::Bool)),
            Value::I64(2)
        );
    }

    #[test]
    fn midnight_datetime_stays_datetime() {
        assert_eq!(
            mysql_value_to_dby(
                &MyValue::Bytes(b"2024-01-02 00:00:00".to_vec()),
                &ct(ColumnTypeBase::DateTime)
            ),
            Value::DateTime("2024-01-02 00:00:00".into())
        );
    }

    #[test]
    fn date_column_keeps_date_string() {
        assert_eq!(
            mysql_value_to_dby(
                &MyValue::Bytes(b"2024-01-02".to_vec()),
                &ct(ColumnTypeBase::Date)
            ),
            Value::Date("2024-01-02".into())
        );
    }

    #[test]
    fn time_string_kept_without_fraction() {
        assert_eq!(
            mysql_value_to_dby(
                &MyValue::Bytes(b"03:04:05".to_vec()),
                &ct(ColumnTypeBase::Time)
            ),
            Value::Time("03:04:05".into())
        );
    }

    #[test]
    fn time_string_strips_zero_microsecond_fraction() {
        // #60：us==0 时剥离 .000000
        assert_eq!(
            mysql_value_to_dby(
                &MyValue::Bytes(b"03:04:05.000000".to_vec()),
                &ct(ColumnTypeBase::Time)
            ),
            Value::Time("03:04:05".into())
        );
        // 负 TIME 保留前导符号
        assert_eq!(
            mysql_value_to_dby(
                &MyValue::Bytes(b"-03:04:05.000000".to_vec()),
                &ct(ColumnTypeBase::Time)
            ),
            Value::Time("-03:04:05".into())
        );
        // 非零小数秒保留
        assert_eq!(
            mysql_value_to_dby(
                &MyValue::Bytes(b"03:04:05.123456".to_vec()),
                &ct(ColumnTypeBase::Time)
            ),
            Value::Time("03:04:05.123456".into())
        );
        // 超 24h 的 HHH:MM:SS 原样保留
        assert_eq!(
            mysql_value_to_dby(
                &MyValue::Bytes(b"838:59:59".to_vec()),
                &ct(ColumnTypeBase::Time)
            ),
            Value::Time("838:59:59".into())
        );
    }

    #[test]
    fn float_column_parses_to_f64() {
        assert_eq!(
            mysql_value_to_dby(&MyValue::Bytes(b"3.25".to_vec()), &ct(ColumnTypeBase::F64)),
            Value::F64(3.25)
        );
    }

    #[test]
    fn json_column_parses_to_json_else_str() {
        assert_eq!(
            mysql_value_to_dby(
                &MyValue::Bytes(b"{\"a\":1}".to_vec()),
                &ct(ColumnTypeBase::Json)
            ),
            Value::Json(serde_json::json!({"a": 1}))
        );
        // 非法 JSON 回退 Str（不丢值）
        assert_eq!(
            mysql_value_to_dby(
                &MyValue::Bytes(b"not-json".to_vec()),
                &ct(ColumnTypeBase::Json)
            ),
            Value::Str("not-json".into())
        );
    }

    #[test]
    fn unknown_column_falls_back_to_legacy_heuristic() {
        // Unknown → legacy：可 UTF-8 字节串 → Str；不可 → Bytes
        assert_eq!(
            mysql_value_to_dby(
                &MyValue::Bytes(b"hi".to_vec()),
                &ct(ColumnTypeBase::Unknown)
            ),
            Value::Str("hi".into())
        );
        assert_eq!(
            mysql_value_to_dby(
                &MyValue::Bytes(vec![0xff, 0xfe]),
                &ct(ColumnTypeBase::Unknown)
            ),
            Value::Bytes(vec![0xff, 0xfe])
        );
    }

    // —— 二进制协议预留（强类型变体直通）——

    #[test]
    fn maps_scalars() {
        assert_eq!(
            mysql_value_to_dby(&MyValue::NULL, &ct(ColumnTypeBase::Unknown)),
            Value::Null
        );
        assert_eq!(
            mysql_value_to_dby(&MyValue::Int(-5), &ct(ColumnTypeBase::I64)),
            Value::I64(-5)
        );
        assert_eq!(
            mysql_value_to_dby(&MyValue::UInt(5), &ct(ColumnTypeBase::U64)),
            Value::U64(5)
        );
    }

    #[test]
    fn maps_datetime_and_date() {
        assert_eq!(
            mysql_value_to_dby(
                &MyValue::Date(2024, 1, 2, 0, 0, 0, 0),
                &ct(ColumnTypeBase::Date)
            ),
            Value::Date("2024-01-02".into())
        );
        assert_eq!(
            mysql_value_to_dby(
                &MyValue::Date(2024, 1, 2, 3, 4, 5, 0),
                &ct(ColumnTypeBase::DateTime)
            ),
            Value::DateTime("2024-01-02 03:04:05".into())
        );
    }

    #[test]
    fn legacy_maps_all_variants() {
        assert_eq!(mysql_value_to_dby_legacy(&MyValue::NULL), Value::Null);
        assert_eq!(mysql_value_to_dby_legacy(&MyValue::Int(-5)), Value::I64(-5));
        assert_eq!(mysql_value_to_dby_legacy(&MyValue::UInt(5)), Value::U64(5));
        assert_eq!(
            mysql_value_to_dby_legacy(&MyValue::Bytes(b"hi".to_vec())),
            Value::Str("hi".into())
        );
        assert_eq!(
            mysql_value_to_dby_legacy(&MyValue::Bytes(vec![0xff, 0xfe])),
            Value::Bytes(vec![0xff, 0xfe])
        );
    }
}
