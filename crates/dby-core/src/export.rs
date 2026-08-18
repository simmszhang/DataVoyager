//! 结果集导出：CSV / JSON / Markdown / INSERT 语句。

use crate::dialect::Dialect;
use crate::edit::quote_value;
use crate::query::ResultSet;

fn csv_escape(s: &str) -> String {
    if s.contains(',') || s.contains('"') || s.contains('\n') || s.contains('\r') {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

/// CSV（表头 + 行，RFC4180 风格转义）。
pub fn to_csv(rs: &ResultSet) -> String {
    let mut out = String::new();
    out.push_str(
        &rs.columns
            .iter()
            .map(|c| csv_escape(&c.name))
            .collect::<Vec<_>>()
            .join(","),
    );
    out.push_str("\r\n");
    for row in &rs.rows {
        out.push_str(
            &row.iter()
                .map(|v| csv_escape(&v.to_display_string()))
                .collect::<Vec<_>>()
                .join(","),
        );
        out.push_str("\r\n");
    }
    out
}

/// JSON（对象数组，值为普通 JSON——无损类型映射见 `Value::to_json_value`）。
pub fn to_json(rs: &ResultSet) -> String {
    let arr: Vec<serde_json::Value> = rs
        .rows
        .iter()
        .map(|row| {
            let obj: serde_json::Map<String, serde_json::Value> = rs
                .columns
                .iter()
                .zip(row.iter())
                .map(|(c, v)| (c.name.clone(), v.to_json_value()))
                .collect();
            serde_json::Value::Object(obj)
        })
        .collect();
    serde_json::to_string_pretty(&serde_json::Value::Array(arr)).unwrap_or_else(|_| "[]".to_string())
}

/// Markdown 表格。
pub fn to_markdown(rs: &ResultSet) -> String {
    let mut out = String::new();
    out.push('|');
    for c in &rs.columns {
        out.push_str(&format!(" {} |", c.name));
    }
    out.push('\n');
    out.push('|');
    for _ in &rs.columns {
        out.push_str(" --- |");
    }
    out.push('\n');
    for row in &rs.rows {
        out.push('|');
        for v in row {
            let s = v.to_display_string().replace('|', "\\|").replace('\n', "<br>");
            out.push_str(&format!(" {s} |"));
        }
        out.push('\n');
    }
    out
}

/// INSERT 语句（每行一条，字面量方言感知）。
pub fn to_insert_sql(dialect: &dyn Dialect, table: &str, rs: &ResultSet) -> String {
    let cols = rs
        .columns
        .iter()
        .map(|c| dialect.quote_identifier(&c.name))
        .collect::<Vec<_>>()
        .join(", ");
    let mut out = String::new();
    for row in &rs.rows {
        let vals = row
            .iter()
            .map(|v| quote_value(dialect, v))
            .collect::<Vec<_>>()
            .join(", ");
        out.push_str(&format!(
            "INSERT INTO {} ({cols}) VALUES ({vals});\n",
            dialect.quote_identifier(table)
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metadata::{ColumnInfo, ColumnType};
    use crate::value::Value;

    struct TestDialect;
    impl Dialect for TestDialect {
        fn quote_identifier(&self, ident: &str) -> String {
            format!("`{ident}`")
        }
        fn quote_string(&self, s: &str) -> String {
            format!("'{s}'")
        }
        fn limit_clause(&self, _limit: Option<u64>, _offset: Option<u64>) -> String {
            String::new()
        }
        fn parse_column_type(&self, _raw: &str) -> Option<ColumnType> {
            None
        }
        fn display_type_name(&self, ct: &ColumnType) -> String {
            format!("{:?}", ct.base)
        }
    }

    fn col(name: &str) -> ColumnInfo {
        ColumnInfo {
            name: name.into(),
            type_name: "text".into(),
            column_type: None,
            nullable: None,
            primary_key: None,
            default: None,
            comment: None,
        }
    }

    fn sample() -> ResultSet {
        ResultSet {
            columns: vec![col("id"), col("name")],
            rows: vec![
                vec![Value::I64(1), Value::Str("a,b".into())],
                vec![Value::I64(2), Value::Null],
            ],
            truncated: false,
        }
    }

    #[test]
    fn csv_escapes_commas() {
        let csv = to_csv(&sample());
        assert_eq!(csv, "id,name\r\n1,\"a,b\"\r\n2,NULL\r\n");
    }

    #[test]
    fn json_is_plain() {
        let json = to_json(&sample());
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v[0]["name"], serde_json::json!("a,b"));
        assert_eq!(v[1]["name"], serde_json::Value::Null);
    }

    #[test]
    fn markdown_table() {
        let md = to_markdown(&sample());
        assert!(md.starts_with("| id | name |\n| --- | --- |\n"));
        assert!(md.contains("| 1 | a,b |"));
    }

    #[test]
    fn insert_sql_quotes() {
        let d = TestDialect;
        let sql = to_insert_sql(&d, "t", &sample());
        assert_eq!(
            sql,
            "INSERT INTO `t` (`id`, `name`) VALUES (1, 'a,b');\nINSERT INTO `t` (`id`, `name`) VALUES (2, NULL);\n"
        );
    }
}
