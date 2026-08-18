//! `mysql_async::Value` → `dby_core::Value` 转换。
//!
//! 文本协议（`query_iter`）下所有非 NULL 值均以 `MyValue::Bytes` 到达
//! （行数据无类型信息），因此转换按列类型解析字节串；强类型变体分支为
//! 二进制协议预留（当前文本路径不产生）。

use dby_core::metadata::{ColumnType, ColumnTypeBase};
use dby_core::value::Value;
use mysql_async::consts::{ColumnFlags, ColumnType as MCT};
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

/// unsigned 标志 → base 取 U 族（`I8→U8 … I64→U64`；Bool/其它透传）。
/// 两条路径共用（R6：#33 元数据与查询路径的 unsigned 整数列 base 一致）。
pub(crate) fn to_unsigned(base: ColumnTypeBase) -> ColumnTypeBase {
    match base {
        ColumnTypeBase::I8 => ColumnTypeBase::U8,
        ColumnTypeBase::I16 => ColumnTypeBase::U16,
        ColumnTypeBase::I32 => ColumnTypeBase::U32,
        ColumnTypeBase::I64 => ColumnTypeBase::U64,
        other => other,
    }
}

/// 查询结果路径：由结果集列定义（`mysql_async::Column`）构造结构化列类型（#33）。
///
/// 按协议枚举 `column_type()` 映射 base（TINY + `column_length()==1` → `Bool`），
/// `character_set()==63`（二进制）把 Str 改判 Bytes，`UNSIGNED_FLAG` 把 base 取 U 族；
/// `column_length()/decimals()/character_set()` 尽力填充数值/长度/精度字段。
pub fn from_mysql_column(c: &mysql_async::Column) -> ColumnType {
    let is_tinyint1 = c.column_type() == MCT::MYSQL_TYPE_TINY && c.column_length() == 1;
    let mut base = match c.column_type() {
        MCT::MYSQL_TYPE_TINY => {
            if is_tinyint1 {
                ColumnTypeBase::Bool
            } else {
                ColumnTypeBase::I8
            }
        }
        MCT::MYSQL_TYPE_SHORT => ColumnTypeBase::I16,
        MCT::MYSQL_TYPE_INT24 | MCT::MYSQL_TYPE_LONG => ColumnTypeBase::I32,
        MCT::MYSQL_TYPE_LONGLONG => ColumnTypeBase::I64,
        MCT::MYSQL_TYPE_NEWDECIMAL => ColumnTypeBase::Decimal,
        MCT::MYSQL_TYPE_VAR_STRING
        | MCT::MYSQL_TYPE_VARCHAR
        | MCT::MYSQL_TYPE_STRING
        | MCT::MYSQL_TYPE_ENUM
        | MCT::MYSQL_TYPE_SET => ColumnTypeBase::Str,
        // TEXT/TINYTEXT/MEDIUMTEXT/LONGTEXT 以 MYSQL_TYPE_BLOB + 真实 charset（≠63）发送；
        // 只有二进制 BLOB 的 charset 才是 63（binary）——按 charset 区分 Str/Bytes（评审 Important）
        MCT::MYSQL_TYPE_TINY_BLOB
        | MCT::MYSQL_TYPE_MEDIUM_BLOB
        | MCT::MYSQL_TYPE_LONG_BLOB
        | MCT::MYSQL_TYPE_BLOB => {
            if c.character_set() == 63 {
                ColumnTypeBase::Bytes
            } else {
                ColumnTypeBase::Str
            }
        }
        MCT::MYSQL_TYPE_JSON => ColumnTypeBase::Json,
        MCT::MYSQL_TYPE_DATE | MCT::MYSQL_TYPE_NEWDATE => ColumnTypeBase::Date,
        MCT::MYSQL_TYPE_TIME => ColumnTypeBase::Time,
        MCT::MYSQL_TYPE_DATETIME | MCT::MYSQL_TYPE_TIMESTAMP => ColumnTypeBase::DateTime,
        MCT::MYSQL_TYPE_BIT => ColumnTypeBase::Bytes, // BIT(n) 位掩码
        MCT::MYSQL_TYPE_FLOAT => ColumnTypeBase::F32,
        MCT::MYSQL_TYPE_DOUBLE => ColumnTypeBase::F64,
        // spatial：与元数据路径 parse_column_type("geometry")→Bytes 一致（评审 Minor）
        MCT::MYSQL_TYPE_GEOMETRY => ColumnTypeBase::Bytes,
        _ => ColumnTypeBase::Unknown,
    };
    // BINARY/VARBINARY：文本协议下 column_type 为 STRING/VAR_STRING 且 charset=63（二进制）
    if base == ColumnTypeBase::Str && c.character_set() == 63 {
        base = ColumnTypeBase::Bytes;
    }
    let unsigned = c.flags().contains(ColumnFlags::UNSIGNED_FLAG);
    if unsigned {
        base = to_unsigned(base);
    }
    ColumnType {
        base,
        unsigned,
        numeric_precision: Some(c.column_length()),
        numeric_scale: Some(c.decimals() as u32),
        char_max_length: Some(c.column_length()), // 查询路径仅能取到字节长度
        temporal_precision: if c.decimals() > 0 {
            Some(c.decimals() as u32)
        } else {
            None
        },
        charset: Some(c.character_set().to_string()), // 仅数字 id（63=二进制）
        collation: None,                              // 查询路径无 collation 名
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

/// 以 `u32` 读取单元格（NULL/非数字返回 None；information_schema 数值列用）。
pub(crate) fn row_u32(row: &mysql_async::Row, index: usize) -> Option<u32> {
    row_string(row, index).and_then(|s| s.parse().ok())
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

    // —— from_mysql_column：查询结果路径列类型构造器（#33）——

    use mysql_async::consts::{ColumnFlags, ColumnType as MCT};
    use mysql_async::Column;

    fn col(ty: MCT) -> Column {
        Column::new(ty)
    }

    #[test]
    fn tinyint_len1_is_bool_else_i8() {
        assert_eq!(
            from_mysql_column(&col(MCT::MYSQL_TYPE_TINY).with_column_length(1)).base,
            ColumnTypeBase::Bool
        );
        assert_eq!(
            from_mysql_column(&col(MCT::MYSQL_TYPE_TINY).with_column_length(2)).base,
            ColumnTypeBase::I8
        );
    }

    #[test]
    fn integer_family_maps_to_signed_base() {
        assert_eq!(
            from_mysql_column(&col(MCT::MYSQL_TYPE_SHORT)).base,
            ColumnTypeBase::I16
        );
        assert_eq!(
            from_mysql_column(&col(MCT::MYSQL_TYPE_INT24)).base,
            ColumnTypeBase::I32
        );
        assert_eq!(
            from_mysql_column(&col(MCT::MYSQL_TYPE_LONG)).base,
            ColumnTypeBase::I32
        );
        assert_eq!(
            from_mysql_column(&col(MCT::MYSQL_TYPE_LONGLONG)).base,
            ColumnTypeBase::I64
        );
    }

    #[test]
    fn unsigned_flag_switches_base_to_u_family() {
        // R6：unsigned 整数列 base 取 U 族（与元数据路径 parse_column_type 一致）
        let ct =
            from_mysql_column(&col(MCT::MYSQL_TYPE_LONG).with_flags(ColumnFlags::UNSIGNED_FLAG));
        assert_eq!(ct.base, ColumnTypeBase::U32);
        assert!(ct.unsigned);
        assert_eq!(
            from_mysql_column(
                &col(MCT::MYSQL_TYPE_LONGLONG).with_flags(ColumnFlags::UNSIGNED_FLAG)
            )
            .base,
            ColumnTypeBase::U64
        );
        // 非整数（Decimal）透传
        assert_eq!(
            from_mysql_column(
                &col(MCT::MYSQL_TYPE_NEWDECIMAL).with_flags(ColumnFlags::UNSIGNED_FLAG)
            )
            .base,
            ColumnTypeBase::Decimal
        );
    }

    #[test]
    fn string_family_maps_to_str() {
        for ty in [
            MCT::MYSQL_TYPE_VAR_STRING,
            MCT::MYSQL_TYPE_VARCHAR,
            MCT::MYSQL_TYPE_STRING,
            MCT::MYSQL_TYPE_ENUM,
            MCT::MYSQL_TYPE_SET,
        ] {
            assert_eq!(
                from_mysql_column(&col(ty)).base,
                ColumnTypeBase::Str,
                "{ty:?}"
            );
        }
    }

    #[test]
    fn blob_family_with_binary_charset_is_bytes() {
        // 二进制 BLOB：MYSQL_TYPE_BLOB + charset=63（binary）→ Bytes
        for ty in [
            MCT::MYSQL_TYPE_TINY_BLOB,
            MCT::MYSQL_TYPE_MEDIUM_BLOB,
            MCT::MYSQL_TYPE_LONG_BLOB,
            MCT::MYSQL_TYPE_BLOB,
        ] {
            assert_eq!(
                from_mysql_column(&col(ty).with_character_set(63)).base,
                ColumnTypeBase::Bytes,
                "{ty:?}"
            );
        }
    }

    #[test]
    fn blob_family_with_text_charset_is_str() {
        // 评审 Important：#33 查询路径 TEXT 误判 Bytes——TEXT 以 BLOB + 真实 charset（≠63）发送，
        // 只有二进制 BLOB 的 charset 才是 63，故 charset≠63 的 BLOB 族应判 Str（与元数据路径一致）
        for ty in [
            MCT::MYSQL_TYPE_TINY_BLOB,
            MCT::MYSQL_TYPE_MEDIUM_BLOB,
            MCT::MYSQL_TYPE_LONG_BLOB,
            MCT::MYSQL_TYPE_BLOB,
        ] {
            assert_eq!(
                from_mysql_column(&col(ty).with_character_set(45)).base,
                ColumnTypeBase::Str,
                "{ty:?}"
            );
        }
    }

    #[test]
    fn temporal_and_special_types_map_to_bases() {
        assert_eq!(
            from_mysql_column(&col(MCT::MYSQL_TYPE_NEWDECIMAL)).base,
            ColumnTypeBase::Decimal
        );
        assert_eq!(
            from_mysql_column(&col(MCT::MYSQL_TYPE_JSON)).base,
            ColumnTypeBase::Json
        );
        assert_eq!(
            from_mysql_column(&col(MCT::MYSQL_TYPE_DATE)).base,
            ColumnTypeBase::Date
        );
        assert_eq!(
            from_mysql_column(&col(MCT::MYSQL_TYPE_NEWDATE)).base,
            ColumnTypeBase::Date
        );
        assert_eq!(
            from_mysql_column(&col(MCT::MYSQL_TYPE_TIME)).base,
            ColumnTypeBase::Time
        );
        assert_eq!(
            from_mysql_column(&col(MCT::MYSQL_TYPE_DATETIME)).base,
            ColumnTypeBase::DateTime
        );
        assert_eq!(
            from_mysql_column(&col(MCT::MYSQL_TYPE_TIMESTAMP)).base,
            ColumnTypeBase::DateTime
        );
        assert_eq!(
            from_mysql_column(&col(MCT::MYSQL_TYPE_BIT)).base,
            ColumnTypeBase::Bytes
        );
        assert_eq!(
            from_mysql_column(&col(MCT::MYSQL_TYPE_FLOAT)).base,
            ColumnTypeBase::F32
        );
        assert_eq!(
            from_mysql_column(&col(MCT::MYSQL_TYPE_DOUBLE)).base,
            ColumnTypeBase::F64
        );
    }

    #[test]
    fn binary_charset_63_maps_str_to_bytes() {
        assert_eq!(
            from_mysql_column(&col(MCT::MYSQL_TYPE_VAR_STRING).with_character_set(63)).base,
            ColumnTypeBase::Bytes
        );
        assert_eq!(
            from_mysql_column(&col(MCT::MYSQL_TYPE_VAR_STRING).with_character_set(255)).base,
            ColumnTypeBase::Str
        );
    }

    #[test]
    fn geometry_maps_to_bytes() {
        // 评审 Minor：spatial 与元数据路径 parse_column_type("geometry")→Bytes 对齐
        assert_eq!(
            from_mysql_column(&col(MCT::MYSQL_TYPE_GEOMETRY)).base,
            ColumnTypeBase::Bytes
        );
    }

    #[test]
    fn unsupported_enum_falls_back_to_unknown() {
        assert_eq!(
            from_mysql_column(&col(MCT::MYSQL_TYPE_NULL)).base,
            ColumnTypeBase::Unknown
        );
    }

    #[test]
    fn metadata_fields_filled_from_column() {
        let ct = from_mysql_column(
            &col(MCT::MYSQL_TYPE_NEWDECIMAL)
                .with_column_length(12)
                .with_decimals(2)
                .with_character_set(63),
        );
        assert_eq!(ct.numeric_precision, Some(12));
        assert_eq!(ct.numeric_scale, Some(2));
        assert_eq!(ct.char_max_length, Some(12));
        assert_eq!(ct.charset.as_deref(), Some("63"));
        assert_eq!(ct.collation, None);
        // 规格：temporal_precision = decimals>0（DECIMAL 的 scale 也满足，见 brief 1）
        assert_eq!(ct.temporal_precision, Some(2));

        let dt = from_mysql_column(&col(MCT::MYSQL_TYPE_DATETIME).with_decimals(6));
        assert_eq!(dt.temporal_precision, Some(6));
        let dt0 = from_mysql_column(&col(MCT::MYSQL_TYPE_DATETIME).with_decimals(0));
        assert_eq!(dt0.temporal_precision, None);
    }

    #[test]
    fn to_unsigned_maps_int_family_only() {
        assert_eq!(to_unsigned(ColumnTypeBase::I8), ColumnTypeBase::U8);
        assert_eq!(to_unsigned(ColumnTypeBase::I16), ColumnTypeBase::U16);
        assert_eq!(to_unsigned(ColumnTypeBase::I32), ColumnTypeBase::U32);
        assert_eq!(to_unsigned(ColumnTypeBase::I64), ColumnTypeBase::U64);
        assert_eq!(to_unsigned(ColumnTypeBase::Bool), ColumnTypeBase::Bool);
        assert_eq!(
            to_unsigned(ColumnTypeBase::Decimal),
            ColumnTypeBase::Decimal
        );
        assert_eq!(to_unsigned(ColumnTypeBase::Str), ColumnTypeBase::Str);
        assert_eq!(
            to_unsigned(ColumnTypeBase::Unknown),
            ColumnTypeBase::Unknown
        );
    }
}
