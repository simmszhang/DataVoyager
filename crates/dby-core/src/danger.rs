//! 危险操作分析：识别破坏性 SQL，供前端执行前确认。

use serde::Serialize;

use crate::dialect::split_statements;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "level", content = "reasons", rename_all = "snake_case")]
pub enum DangerLevel {
    Safe,
    Warn,
    Dangerous(Vec<String>),
}

impl DangerLevel {
    pub fn is_dangerous(&self) -> bool {
        matches!(self, DangerLevel::Dangerous(_))
    }
}

/// 大小写不敏感的关键词匹配（按非标识符字符切词比较）。
fn contains_keyword(upper_sql: &str, keyword: &str) -> bool {
    upper_sql
        .split(|c: char| !c.is_ascii_alphanumeric() && c != '_')
        .any(|w| w == keyword)
}

/// 分析 SQL 的危险等级：逐句（用方言无关的切分）检查破坏性关键词。
pub fn analyze_danger(sql: &str) -> DangerLevel {
    let mut reasons: Vec<String> = Vec::new();

    for stmt in split_statements(sql) {
        let upper = stmt.to_uppercase();

        if contains_keyword(&upper, "DROP") {
            reasons.push("包含 DROP 语句".to_string());
        }
        if contains_keyword(&upper, "TRUNCATE") {
            reasons.push("包含 TRUNCATE 语句".to_string());
        }
        if contains_keyword(&upper, "ALTER") {
            reasons.push("包含 ALTER 语句".to_string());
        }
        // DELETE / UPDATE 无 WHERE
        let is_delete = contains_keyword(&upper, "DELETE");
        let is_update = contains_keyword(&upper, "UPDATE");
        if (is_delete || is_update) && !contains_keyword(&upper, "WHERE") {
            reasons.push("DELETE/UPDATE 缺少 WHERE 条件".to_string());
        }
    }

    if reasons.is_empty() {
        DangerLevel::Safe
    } else {
        reasons.sort();
        reasons.dedup();
        DangerLevel::Dangerous(reasons)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn select_is_safe() {
        assert_eq!(analyze_danger("SELECT * FROM users"), DangerLevel::Safe);
        assert_eq!(analyze_danger("SHOW TABLES"), DangerLevel::Safe);
    }

    #[test]
    fn drop_and_truncate_are_dangerous() {
        assert!(analyze_danger("DROP TABLE users").is_dangerous());
        assert!(analyze_danger("TRUNCATE TABLE users").is_dangerous());
        assert!(analyze_danger("ALTER TABLE users ADD COLUMN x INT").is_dangerous());
    }

    #[test]
    fn delete_update_without_where_is_dangerous() {
        assert!(analyze_danger("DELETE FROM users").is_dangerous());
        assert!(analyze_danger("UPDATE users SET x = 1").is_dangerous());
    }

    #[test]
    fn delete_update_with_where_is_safe() {
        assert_eq!(analyze_danger("DELETE FROM users WHERE id = 1"), DangerLevel::Safe);
        assert_eq!(
            analyze_danger("UPDATE users SET x = 1 WHERE id = 1"),
            DangerLevel::Safe
        );
    }

    #[test]
    fn keyword_matching_is_case_insensitive_and_word_bound() {
        // 小写也识别
        assert!(analyze_danger("drop table t").is_dangerous());
        // "droplet" 不误报为 DROP
        assert_eq!(analyze_danger("SELECT droplet FROM t"), DangerLevel::Safe);
        // 字符串字面量里的 DROP 会被切词命中（简化策略，可接受）
        assert!(analyze_danger("SELECT 'drop' FROM t").is_dangerous());
    }

    #[test]
    fn multi_statement_collects_reasons() {
        let d = analyze_danger("SELECT 1; DROP TABLE a; TRUNCATE b");
        match d {
            DangerLevel::Dangerous(reasons) => {
                assert!(reasons.iter().any(|r| r.contains("DROP")));
                assert!(reasons.iter().any(|r| r.contains("TRUNCATE")));
            }
            _ => panic!("expected dangerous"),
        }
    }
}
