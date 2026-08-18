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

/// 标识符字符（词边界切分用：字母/数字/下划线）。
fn is_ident_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

/// 分析 SQL 的危险等级：逐句扫描，跳过字符串字面量与注释，仅在外层按词边界匹配关键词。
pub fn analyze_danger(sql: &str) -> DangerLevel {
    let mut reasons: Vec<String> = Vec::new();
    // UPDATE 无 WHERE 可回滚，降级为 Warn（不进入 reasons）
    let mut update_without_where = false;

    for stmt in split_statements(sql) {
        let bytes = stmt.as_bytes();
        let mut i = 0usize;
        let mut in_single = false;
        let mut in_double = false;
        let mut in_backtick = false;
        let mut in_line_comment = false;
        let mut in_block_comment = false;
        // 本条语句内的关键词出现情况（WHERE 只认外层词）
        let mut has_delete = false;
        let mut has_update = false;
        let mut has_where = false;

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
                b'-' if next == Some(b'-') => in_line_comment = true,
                b'/' if next == Some(b'*') => in_block_comment = true,
                _ if c.is_ascii_alphabetic() || c == b'_' => {
                    let start = i;
                    while i < bytes.len() && is_ident_byte(bytes[i]) {
                        i += 1;
                    }
                    match stmt[start..i].to_uppercase().as_str() {
                        "DROP" => reasons.push("包含 DROP 语句".to_string()),
                        "TRUNCATE" => reasons.push("包含 TRUNCATE 语句".to_string()),
                        "ALTER" => reasons.push("包含 ALTER 语句".to_string()),
                        "RENAME" => reasons.push("包含 RENAME 语句".to_string()),
                        "DELETE" => has_delete = true,
                        "UPDATE" => has_update = true,
                        "WHERE" => has_where = true,
                        _ => {}
                    }
                    continue;
                }
                _ => {}
            }
            i += 1;
        }

        // DELETE 无 WHERE：不可逆数据删除，保持 Dangerous；UPDATE 无 WHERE：降级 Warn
        if has_delete && !has_where {
            reasons.push("DELETE 缺少 WHERE 条件".to_string());
        }
        if has_update && !has_where {
            update_without_where = true;
        }
    }

    if !reasons.is_empty() {
        reasons.sort();
        reasons.dedup();
        DangerLevel::Dangerous(reasons)
    } else if update_without_where {
        DangerLevel::Warn
    } else {
        DangerLevel::Safe
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
        // DELETE 无 WHERE：不可逆数据删除，保持 Dangerous
        assert!(analyze_danger("DELETE FROM users").is_dangerous());
        // UPDATE 无 WHERE：可回滚，降级为 Warn
        assert_eq!(analyze_danger("UPDATE users SET x = 1"), DangerLevel::Warn);
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
        // 字符串字面量里的 DROP 被跳过，不触发危险判定
        assert_eq!(analyze_danger("SELECT 'drop' FROM t"), DangerLevel::Safe);
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

    #[test]
    fn keywords_inside_strings_and_comments_are_ignored() {
        assert_eq!(analyze_danger("SELECT 'drop' FROM t"), DangerLevel::Safe);
        assert_eq!(analyze_danger("SELECT 'delete from x'"), DangerLevel::Safe);
        assert_eq!(
            analyze_danger("SELECT 1 /* drop table t */"),
            DangerLevel::Safe
        );
        assert_eq!(analyze_danger("SELECT 1 -- drop\n"), DangerLevel::Safe);
        assert!(analyze_danger("DROP TABLE t").is_dangerous());
    }

    #[test]
    fn delete_without_where_stays_dangerous_update_is_warn() {
        assert!(analyze_danger("DELETE FROM users").is_dangerous()); // 不可逆数据删除：不降级
        assert_eq!(analyze_danger("UPDATE users SET x = 1"), DangerLevel::Warn);
        assert_eq!(
            analyze_danger("DELETE FROM users WHERE id = 1"),
            DangerLevel::Safe
        );
        assert!(analyze_danger("RENAME TABLE a TO b").is_dangerous()); // RENAME 需新增到关键词清单
    }
}
