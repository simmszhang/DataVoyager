//! SQL 方言抽象 + 通用语句切分。

use crate::metadata::ColumnType;

/// 每驱动声明自己的 SQL 语法规则，供编辑器、SQL 生成（编辑/导出/过滤）使用。
pub trait Dialect: Send + Sync {
    /// 标识符引用（MySQL: `` ` ``，Postgres: `"`）。
    fn quote_identifier(&self, ident: &str) -> String;
    /// 字符串字面量转义。
    fn quote_string(&self, s: &str) -> String;
    /// 分页子句，例如 `LIMIT n OFFSET m`。
    fn limit_clause(&self, limit: Option<u64>, offset: Option<u64>) -> String;
    /// 原生类型字符串 → 结构化列类型（元数据路径；无法解析时返回 `None`）。
    fn parse_column_type(&self, raw: &str) -> Option<ColumnType>;
    /// 结构化列类型 → 展示名（由 `ColumnInfo.type_name` 消费，不再依赖 Debug 字符串）。
    fn display_type_name(&self, ct: &ColumnType) -> String;
    /// 按方言规则切分语句（默认实现 = `generic_split_statements` 的通用切分）。
    /// 未来 Postgres 等驱动可覆盖以支持 dollar-quoting 等方言差异。
    fn split_statements<'a>(&self, sql: &'a str) -> Vec<&'a str> {
        generic_split_statements(sql)
    }
}

/// 方言无关的通用语句切分：在单/双引号、反引号、行注释、块注释之外按 `;` 切分。
/// 供 [`Dialect::split_statements`] 默认实现与自由函数 [`split_statements`] 共用。
fn generic_split_statements(sql: &str) -> Vec<&str> {
    let bytes = sql.as_bytes();
    let mut out = Vec::new();
    let mut start = 0usize;
    let mut i = 0usize;
    let mut in_single = false;
    let mut in_double = false;
    let mut in_backtick = false;
    let mut in_line_comment = false;
    let mut in_block_comment = false;

    while i < bytes.len() {
        let c = bytes[i];
        let next = bytes.get(i + 1).copied();

        if in_line_comment {
            if c == b'\n' {
                in_line_comment = false;
            }
            i += 1;
            continue;
        }
        if in_block_comment {
            if c == b'*' && next == Some(b'/') {
                in_block_comment = false;
                i += 2;
                continue;
            }
            i += 1;
            continue;
        }
        if in_single {
            if c == b'\\' && next.is_some() {
                i += 2;
                continue;
            }
            if c == b'\'' {
                in_single = false;
            }
            i += 1;
            continue;
        }
        if in_double {
            if c == b'\\' && next.is_some() {
                i += 2;
                continue;
            }
            if c == b'"' {
                in_double = false;
            }
            i += 1;
            continue;
        }
        if in_backtick {
            if c == b'`' {
                in_backtick = false;
            }
            i += 1;
            continue;
        }

        match c {
            b'\'' => in_single = true,
            b'"' => in_double = true,
            b'`' => in_backtick = true,
            b'-' if next == Some(b'-') => {
                in_line_comment = true;
                i += 2;
                continue;
            }
            b'#' => {
                in_line_comment = true;
                i += 1;
                continue;
            }
            b'/' if next == Some(b'*') => {
                in_block_comment = true;
                i += 2;
                continue;
            }
            b';' => {
                let stmt = sql[start..i].trim();
                if !stmt.is_empty() {
                    out.push(stmt);
                }
                start = i + 1;
            }
            _ => {}
        }
        i += 1;
    }
    let tail = sql[start..].trim();
    if !tail.is_empty() {
        out.push(tail);
    }
    out
}

/// 朴素但正确的语句切分：在单/双引号、反引号、行注释、块注释之外按 `;` 切分。
/// 薄封装：委托 `generic_split_statements`，与 trait 默认方法同源，
/// 供 `danger.rs` 等既有调用点继续使用。
pub fn split_statements(sql: &str) -> Vec<&str> {
    generic_split_statements(sql)
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
                (None, Some(o)) => format!("LIMIT {o}, 18446744073709551615"),
                (None, None) => String::new(),
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
    fn split_respects_quotes_and_comments() {
        let sql = "SELECT 'a;b'; SELECT `x;y` FROM t; -- ; comment\nSELECT 1; /* ; */ SELECT 2";
        let parts = split_statements(sql);
        assert_eq!(
            parts,
            vec![
                "SELECT 'a;b'",
                "SELECT `x;y` FROM t",
                "-- ; comment\nSELECT 1",
                "/* ; */ SELECT 2"
            ]
        );
    }

    #[test]
    fn split_handles_single_statement() {
        assert_eq!(split_statements("SELECT 1"), vec!["SELECT 1"]);
        assert_eq!(split_statements(""), Vec::<&str>::new());
    }

    #[test]
    fn dialect_split_statements_default_matches_free_fn() {
        assert_eq!(
            TestDialect.split_statements("SELECT 'a;b'; SELECT 1"),
            vec!["SELECT 'a;b'", "SELECT 1"]
        );
        assert_eq!(
            split_statements("SELECT 'a;b'; SELECT 1"),
            vec!["SELECT 'a;b'", "SELECT 1"]
        ); // 薄封装同源
    }

    #[test]
    fn quote_identifier_escapes_backticks() {
        assert_eq!(TestDialect.quote_identifier("a`b"), "`a``b`");
    }
}
