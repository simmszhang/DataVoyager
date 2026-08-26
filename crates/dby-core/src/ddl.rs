//! DDL 语句生成（方言感知）：建/删库、建/改/删表。

use serde::Deserialize;

use crate::dialect::Dialect;

/// 建表时的列定义（来自前端建表对话框）。
#[derive(Debug, Clone, Deserialize)]
pub struct ColumnDef {
    pub name: String,
    pub type_name: String,
    #[serde(default = "default_nullable")]
    pub nullable: bool,
    #[serde(default)]
    pub primary_key: bool,
}

fn default_nullable() -> bool {
    true
}

pub fn build_create_database(dialect: &dyn Dialect, name: &str) -> String {
    format!("CREATE DATABASE {};", dialect.quote_identifier(name))
}

pub fn build_drop_database(dialect: &dyn Dialect, name: &str) -> String {
    format!("DROP DATABASE {};", dialect.quote_identifier(name))
}

pub fn build_create_table(dialect: &dyn Dialect, table: &str, columns: &[ColumnDef]) -> String {
    let defs = columns
        .iter()
        .map(|c| {
            let mut def = format!("{} {}", dialect.quote_identifier(&c.name), c.type_name);
            if !c.nullable {
                def.push_str(" NOT NULL");
            }
            if c.primary_key {
                def.push_str(" PRIMARY KEY");
            }
            def
        })
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "CREATE TABLE {} ({});",
        dialect.quote_identifier(table),
        defs
    )
}

pub fn build_rename_table(dialect: &dyn Dialect, old_name: &str, new_name: &str) -> String {
    format!(
        "RENAME TABLE {} TO {};",
        dialect.quote_identifier(old_name),
        dialect.quote_identifier(new_name)
    )
}

pub fn build_drop_table(dialect: &dyn Dialect, table: &str) -> String {
    format!("DROP TABLE {};", dialect.quote_identifier(table))
}

/// 生成 DROP VIEW 语句
pub fn build_drop_view(dialect: &dyn Dialect, name: &str) -> String {
    format!("DROP VIEW {};", dialect.quote_identifier(name))
}

/// 生成 DROP FUNCTION/PROCEDURE 语句
pub fn build_drop_routine(dialect: &dyn Dialect, kind: &str, name: &str) -> String {
    // kind: "FUNCTION" | "PROCEDURE"
    format!("DROP {} {};", kind, dialect.quote_identifier(name))
}

/// 生成 DROP TRIGGER 语句
pub fn build_drop_trigger(dialect: &dyn Dialect, name: &str) -> String {
    format!("DROP TRIGGER {};", dialect.quote_identifier(name))
}

/// 生成 TRUNCATE TABLE 语句
pub fn build_truncate_table(dialect: &dyn Dialect, name: &str) -> String {
    format!("TRUNCATE TABLE {};", dialect.quote_identifier(name))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metadata::ColumnType;

    struct TestDialect;
    impl Dialect for TestDialect {
        fn quote_identifier(&self, ident: &str) -> String {
            format!("`{ident}`")
        }
        fn quote_string(&self, s: &str) -> String {
            format!("'{s}'")
        }
        fn limit_clause(&self, _l: Option<u64>, _o: Option<u64>) -> String {
            String::new()
        }
        fn parse_column_type(&self, _raw: &str) -> Option<ColumnType> {
            None
        }
        fn display_type_name(&self, ct: &ColumnType) -> String {
            format!("{:?}", ct.base)
        }
    }

    #[test]
    fn create_and_drop_database() {
        let d = TestDialect;
        assert_eq!(build_create_database(&d, "mydb"), "CREATE DATABASE `mydb`;");
        assert_eq!(build_drop_database(&d, "mydb"), "DROP DATABASE `mydb`;");
    }

    #[test]
    fn create_table_with_columns() {
        let d = TestDialect;
        let cols = vec![
            ColumnDef {
                name: "id".into(),
                type_name: "INT".into(),
                nullable: false,
                primary_key: true,
            },
            ColumnDef {
                name: "name".into(),
                type_name: "VARCHAR(64)".into(),
                nullable: true,
                primary_key: false,
            },
        ];
        assert_eq!(
            build_create_table(&d, "users", &cols),
            "CREATE TABLE `users` (`id` INT NOT NULL PRIMARY KEY, `name` VARCHAR(64));"
        );
    }

    #[test]
    fn rename_and_drop_table() {
        let d = TestDialect;
        assert_eq!(build_rename_table(&d, "a", "b"), "RENAME TABLE `a` TO `b`;");
        assert_eq!(build_drop_table(&d, "t"), "DROP TABLE `t`;");
    }

    #[test]
    fn drop_view_and_routine() {
        let d = TestDialect;
        assert_eq!(build_drop_view(&d, "v1"), "DROP VIEW `v1`;");
        assert_eq!(
            build_drop_routine(&d, "FUNCTION", "fn1"),
            "DROP FUNCTION `fn1`;"
        );
        assert_eq!(
            build_drop_routine(&d, "PROCEDURE", "sp1"),
            "DROP PROCEDURE `sp1`;"
        );
        assert_eq!(build_drop_trigger(&d, "trg1"), "DROP TRIGGER `trg1`;");
        assert_eq!(build_truncate_table(&d, "t1"), "TRUNCATE TABLE `t1`;");
    }
}
