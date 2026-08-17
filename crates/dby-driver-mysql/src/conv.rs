//! `mysql_async::Value` → `dby_core::Value` 转换。

use dby_core::value::Value;
use mysql_async::Value as MyValue;

/// 把 MySQL 单元格值转换为统一 Value。
///
/// 注意：DECIMAL/NEWDECIMAL 在文本协议下通常以字节串返回，此处先标记为 `Str`
/// （值本身无损）；基于列类型的精确标记（Decimal 变体）属 M1 的类型映射。
pub fn mysql_value_to_dby(v: &MyValue) -> Value {
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

/// 以 `String` 读取单元格（NULL 或非文本返回 None，非 panicking）。
pub fn row_string(row: &mysql_async::Row, index: usize) -> Option<String> {
    row.get_opt::<String, usize>(index).and_then(|r| r.ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_scalars() {
        assert_eq!(mysql_value_to_dby(&MyValue::NULL), Value::Null);
        assert_eq!(mysql_value_to_dby(&MyValue::Int(-5)), Value::I64(-5));
        assert_eq!(mysql_value_to_dby(&MyValue::UInt(5)), Value::U64(5));
        assert_eq!(
            mysql_value_to_dby(&MyValue::Bytes(b"hi".to_vec())),
            Value::Str("hi".into())
        );
        assert_eq!(
            mysql_value_to_dby(&MyValue::Bytes(vec![0xff, 0xfe])),
            Value::Bytes(vec![0xff, 0xfe])
        );
    }

    #[test]
    fn maps_datetime_and_date() {
        assert_eq!(
            mysql_value_to_dby(&MyValue::Date(2024, 1, 2, 0, 0, 0, 0)),
            Value::Date("2024-01-02".into())
        );
        assert_eq!(
            mysql_value_to_dby(&MyValue::Date(2024, 1, 2, 3, 4, 5, 0)),
            Value::DateTime("2024-01-02 03:04:05".into())
        );
    }
}
