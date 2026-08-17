//! MySQL 方言。

use dby_core::dialect::Dialect;

#[derive(Debug, Default, Clone, Copy)]
pub struct MysqlDialect;

impl Dialect for MysqlDialect {
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
            (None, Some(o)) => format!("LIMIT {o}, 18446744073709551615"),
            (None, None) => String::new(),
        }
    }

    fn display_type_name(&self, raw: &str) -> String {
        raw.strip_prefix("MYSQL_TYPE_")
            .unwrap_or(raw)
            .to_lowercase()
    }
}
