//! MySQL 驱动：实现 `dby_core::driver::{Driver, Connection}`。

mod conv;
mod dialect;

use dby_core::dialect::Dialect;
use dby_core::driver::{Capabilities, ConnectParams, Connection, Driver};
use dby_core::error::{DbError, Result};
use dby_core::metadata::{
    ColumnInfo, ForeignKeyInfo, IndexInfo, ProcedureInfo, TableInfo, TriggerInfo,
};
use dby_core::query::{ExecOpts, QueryOutput, ResultSet};
use dby_core::value::Value;

use mysql::prelude::Queryable;
use mysql::{Conn, OptsBuilder, Row};

pub use dialect::MysqlDialect;

const DEFAULT_MAX_ROWS: usize = 2000;

fn db_err(e: mysql::Error) -> DbError {
    DbError::Database(e.to_string())
}

pub struct MysqlDriver;

impl Driver for MysqlDriver {
    fn id(&self) -> &'static str {
        "mysql"
    }

    fn display_name(&self) -> &'static str {
        "MySQL"
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            supports_sql: true,
            supports_transactions: true,
            supports_catalogs: false, // MySQL 无 catalog 概念
            supports_schemas: true,   // 数据库即 schema
            supports_procedures: true,
            supports_cancel: false, // 同步连接暂不支持取消（M1 流式引擎引入）
            supports_data_edit: true,
        }
    }

    fn dialect(&self) -> &dyn Dialect {
        &MysqlDialect
    }

    fn connect(&self, params: &ConnectParams) -> Result<Box<dyn Connection + Send>> {
        let opts = OptsBuilder::new()
            .ip_or_hostname(Some(params.host.clone()))
            .tcp_port(params.port)
            .user(Some(params.user.clone()))
            .pass(params.password.clone())
            .db_name(params.database.clone());
        let conn = Conn::new(opts).map_err(db_err)?;
        let (major, minor, patch) = conn.server_version();
        Ok(Box::new(MysqlConnection {
            conn,
            version: format!("{major}.{minor}.{patch}"),
        }))
    }
}

pub struct MysqlConnection {
    conn: Conn,
    version: String,
}

impl Connection for MysqlConnection {
    fn ping(&mut self) -> Result<()> {
        self.conn.ping().map_err(db_err)
    }

    fn server_version(&self) -> String {
        self.version.clone()
    }

    fn catalogs(&mut self) -> Result<Vec<String>> {
        Ok(vec![])
    }

    fn schemas(&mut self, _catalog: Option<&str>) -> Result<Vec<String>> {
        let rows: Vec<Row> = self.conn.query("SHOW DATABASES").map_err(db_err)?;
        Ok(rows
            .into_iter()
            .filter_map(|r| conv::row_string(&r, 0))
            .collect())
    }

    fn tables(&mut self, schema: &str) -> Result<Vec<TableInfo>> {
        let sql = "SELECT TABLE_NAME, TABLE_TYPE, TABLE_COMMENT \
                   FROM information_schema.TABLES WHERE TABLE_SCHEMA = ? ORDER BY TABLE_NAME";
        let rows: Vec<Row> = self.conn.exec(sql, (schema,)).map_err(db_err)?;
        Ok(rows
            .into_iter()
            .map(|r| TableInfo {
                name: conv::row_string(&r, 0).unwrap_or_default(),
                kind: conv::row_string(&r, 1).map(|t| t.to_lowercase()),
                comment: conv::row_string(&r, 2),
            })
            .collect())
    }

    fn columns(&mut self, schema: &str, table: &str) -> Result<Vec<ColumnInfo>> {
        let sql = "SELECT COLUMN_NAME, COLUMN_TYPE, IS_NULLABLE, COLUMN_KEY, COLUMN_DEFAULT, COLUMN_COMMENT \
                   FROM information_schema.COLUMNS \
                   WHERE TABLE_SCHEMA = ? AND TABLE_NAME = ? ORDER BY ORDINAL_POSITION";
        let rows: Vec<Row> = self.conn.exec(sql, (schema, table)).map_err(db_err)?;
        Ok(rows
            .into_iter()
            .map(|r| ColumnInfo {
                name: conv::row_string(&r, 0).unwrap_or_default(),
                type_name: conv::row_string(&r, 1).unwrap_or_default(),
                nullable: conv::row_string(&r, 2).map(|v| v == "YES"),
                primary_key: conv::row_string(&r, 3).map(|v| v == "PRI"),
                default: conv::row_string(&r, 4),
                comment: conv::row_string(&r, 5),
            })
            .collect())
    }

    fn indexes(&mut self, schema: &str, table: &str) -> Result<Vec<IndexInfo>> {
        let sql = format!(
            "SHOW INDEX FROM {} FROM {}",
            MysqlDialect.quote_identifier(table),
            MysqlDialect.quote_identifier(schema)
        );
        let rows: Vec<Row> = self.conn.query(sql).map_err(db_err)?;
        // 列：Table, Non_unique, Key_name, Seq_in_index, Column_name, ...
        let mut indexes: Vec<IndexInfo> = Vec::new();
        for r in &rows {
            let name = conv::row_string(r, 2).unwrap_or_default();
            let unique = conv::row_string(r, 1).map(|v| v == "0").unwrap_or(false);
            let column = conv::row_string(r, 4).unwrap_or_default();
            if let Some(ix) = indexes.iter_mut().find(|i| i.name == name) {
                ix.columns.push(column);
            } else {
                indexes.push(IndexInfo {
                    name,
                    unique,
                    columns: vec![column],
                });
            }
        }
        Ok(indexes)
    }

    fn foreign_keys(&mut self, schema: &str, table: &str) -> Result<Vec<ForeignKeyInfo>> {
        let sql = "SELECT CONSTRAINT_NAME, COLUMN_NAME, REFERENCED_TABLE_NAME, REFERENCED_COLUMN_NAME \
                   FROM information_schema.KEY_COLUMN_USAGE \
                   WHERE TABLE_SCHEMA = ? AND TABLE_NAME = ? AND REFERENCED_TABLE_NAME IS NOT NULL \
                   ORDER BY CONSTRAINT_NAME, ORDINAL_POSITION";
        let rows: Vec<Row> = self.conn.exec(sql, (schema, table)).map_err(db_err)?;
        let mut fks: Vec<ForeignKeyInfo> = Vec::new();
        for r in &rows {
            let name = conv::row_string(&r, 0).unwrap_or_default();
            let column = conv::row_string(&r, 1).unwrap_or_default();
            let ref_table = conv::row_string(&r, 2).unwrap_or_default();
            let ref_col = conv::row_string(&r, 3).unwrap_or_default();
            if let Some(fk) = fks.iter_mut().find(|f| f.name == name) {
                fk.columns.push(column);
                fk.referenced_columns.push(ref_col);
            } else {
                fks.push(ForeignKeyInfo {
                    name,
                    columns: vec![column],
                    referenced_table: ref_table,
                    referenced_columns: vec![ref_col],
                });
            }
        }
        Ok(fks)
    }

    fn triggers(&mut self, schema: &str, table: &str) -> Result<Vec<TriggerInfo>> {
        let sql = "SELECT TRIGGER_NAME, ACTION_TIMING, EVENT_MANIPULATION \
                   FROM information_schema.TRIGGERS \
                   WHERE TRIGGER_SCHEMA = ? AND EVENT_OBJECT_TABLE = ? ORDER BY TRIGGER_NAME";
        let rows: Vec<Row> = self.conn.exec(sql, (schema, table)).map_err(db_err)?;
        Ok(rows
            .into_iter()
            .map(|r| TriggerInfo {
                name: conv::row_string(&r, 0).unwrap_or_default(),
                timing: conv::row_string(&r, 1).unwrap_or_default(),
                event: conv::row_string(&r, 2).unwrap_or_default(),
            })
            .collect())
    }

    fn procedures(&mut self, schema: &str) -> Result<Vec<ProcedureInfo>> {
        let sql = "SELECT ROUTINE_NAME, ROUTINE_TYPE FROM information_schema.ROUTINES \
                   WHERE ROUTINE_SCHEMA = ? ORDER BY ROUTINE_NAME";
        let rows: Vec<Row> = self.conn.exec(sql, (schema,)).map_err(db_err)?;
        Ok(rows
            .into_iter()
            .map(|r| ProcedureInfo {
                name: conv::row_string(&r, 0).unwrap_or_default(),
                kind: conv::row_string(&r, 1).unwrap_or_default(),
            })
            .collect())
    }

    fn table_ddl(&mut self, schema: &str, table: &str) -> Result<String> {
        let sql = format!(
            "SHOW CREATE TABLE {}.{}",
            MysqlDialect.quote_identifier(schema),
            MysqlDialect.quote_identifier(table)
        );
        let rows: Vec<Row> = self.conn.query(sql).map_err(db_err)?;
        rows.into_iter()
            .next()
            .and_then(|r| conv::row_string(&r, 1))
            .ok_or_else(|| DbError::Database(format!("no DDL for {schema}.{table}")))
    }

    fn execute(&mut self, schema: Option<&str>, sql: &str, opts: &ExecOpts) -> Result<QueryOutput> {
        if let Some(db) = schema {
            if !db.is_empty() {
                self.conn.select_db(db).map_err(db_err)?;
            }
        }
        let max_rows = opts.max_rows.unwrap_or(DEFAULT_MAX_ROWS);
        let mut qr = self.conn.query_iter(sql).map_err(db_err)?;
        let mut output = QueryOutput::default();

        let columns: Vec<ColumnInfo> = qr
            .columns()
            .as_ref()
            .iter()
            .map(|c| ColumnInfo {
                name: c.name_str().to_string(),
                type_name: MysqlDialect.display_type_name(&format!("{:?}", c.column_type())),
                nullable: None,
                primary_key: None,
                default: None,
                comment: None,
            })
            .collect();

        if columns.is_empty() {
            output.affected_rows = qr.affected_rows();
            output.last_insert_id = qr.last_insert_id();
        } else {
            let mut rows: Vec<Vec<Value>> = Vec::new();
            let mut truncated = false;
            for row in qr.by_ref() {
                let row = row.map_err(db_err)?;
                if rows.len() < max_rows {
                    rows.push(row_to_values(&row));
                } else {
                    truncated = true;
                }
            }
            output.result_sets.push(ResultSet {
                columns,
                rows,
                truncated,
            });
        }
        Ok(output)
    }

    fn begin(&mut self) -> Result<()> {
        self.conn.query_drop("START TRANSACTION").map_err(db_err)
    }

    fn commit(&mut self) -> Result<()> {
        self.conn.query_drop("COMMIT").map_err(db_err)
    }

    fn rollback(&mut self) -> Result<()> {
        self.conn.query_drop("ROLLBACK").map_err(db_err)
    }

    fn cancel(&self) -> Result<()> {
        Err(DbError::Unsupported(
            "同步 MySQL 驱动暂不支持取消（M1 流式引擎引入）".to_string(),
        ))
    }
}

fn row_to_values(row: &Row) -> Vec<Value> {
    (0..row.len())
        .map(|i| match row.get::<mysql::Value, usize>(i) {
            Some(v) => conv::mysql_value_to_dby(&v),
            None => Value::Null,
        })
        .collect()
}
