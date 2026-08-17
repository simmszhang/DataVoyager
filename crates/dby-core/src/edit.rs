//! 数据编辑 SQL 生成（方言感知）：行内编辑生成 UPDATE/INSERT/DELETE。

use crate::dialect::Dialect;
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
        .map(|(c, v)| format!("{} = {}", dialect.quote_identifier(c), quote_value(dialect, v)))
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
        .map(|(c, v)| format!("{} = {}", dialect.quote_identifier(c), quote_value(dialect, v)))
        .collect::<Vec<_>>()
        .join(" AND ")
}

#[cfg(test)]
mod tests {
    use super::*;

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
        assert_eq!(sql, "UPDATE `users` SET `name` = 'O\\'Brien' WHERE `id` = 1;");
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
        assert_eq!(insert, "INSERT INTO `users` (`name`, `age`) VALUES ('x', 3);");

        let delete = build_delete(&d, "users", &[("id".into(), Value::I64(9))]);
        assert_eq!(delete, "DELETE FROM `users` WHERE `id` = 9;");
    }
}
