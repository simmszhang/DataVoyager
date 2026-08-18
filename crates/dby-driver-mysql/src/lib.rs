//! MySQL 驱动：实现 `dby_core::driver::{Driver, Connection}`（异步、流式）。

mod conv;
mod dialect;
mod tunnel;

use async_trait::async_trait;
use dby_core::dialect::Dialect;
use dby_core::driver::{Capabilities, ConnectParams, Connection, Driver};
use dby_core::error::{DbError, Result};
use dby_core::metadata::{
    ColumnInfo, ForeignKeyInfo, IndexInfo, ProcedureInfo, TableInfo, TriggerInfo,
};
use dby_core::query::{ExecOpts, ResultSink, StreamEvent};
use dby_core::value::Value;

use mysql_async::prelude::Queryable;
use mysql_async::{Conn, OptsBuilder, Row, SslOpts};

pub use dialect::MysqlDialect;
pub use tunnel::probe_host_key;

/// 每批推送的行数（降低 IPC 频率）。
const BATCH_ROWS: usize = 100;

fn db_err(e: mysql_async::Error) -> DbError {
    DbError::Database(e.to_string())
}

pub struct MysqlDriver;

#[async_trait]
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
            supports_cancel: true, // 流式 + 取消已支持
            supports_data_edit: true,
        }
    }

    fn dialect(&self) -> &dyn Dialect {
        &MysqlDialect
    }

    async fn connect(&self, params: &ConnectParams) -> Result<Box<dyn Connection + Send>> {
        // SSH 隧道：本地端口转发到远端 MySQL
        let (host, port, ssh) = match &params.ssh {
            Some(ssh) if ssh.enabled => {
                let t = tunnel::start_tunnel(ssh, &params.host, params.port).await?;
                ("127.0.0.1".to_string(), t.local_port, Some(t))
            }
            _ => (params.host.clone(), params.port, None),
        };

        let mut opts = OptsBuilder::default()
            .ip_or_hostname(host)
            .tcp_port(port)
            .user(Some(params.user.clone()))
            .pass(params.password.clone())
            .db_name(params.database.clone());
        if let Some(ssl) = &params.ssl {
            if ssl.enabled {
                // M1：verify_cert=false 接受自签名（内网常见）；ca_path/客户端证书 M2
                let ssl_opts = if ssl.verify_cert {
                    SslOpts::default()
                } else {
                    SslOpts::default().with_danger_accept_invalid_certs(true)
                };
                opts = opts.ssl_opts(Some(ssl_opts));
            }
        }
        let conn = Conn::new(opts).await.map_err(db_err)?;
        let (major, minor, patch) = conn.server_version();
        Ok(Box::new(MysqlConnection {
            conn,
            version: format!("{major}.{minor}.{patch}"),
            _ssh: ssh,
        }))
    }
}

pub struct MysqlConnection {
    conn: Conn,
    version: String,
    _ssh: Option<tunnel::SshTunnel>,
}

#[async_trait]
impl Connection for MysqlConnection {
    async fn ping(&mut self) -> Result<()> {
        self.conn.ping().await.map_err(db_err)
    }

    fn server_version(&self) -> String {
        self.version.clone()
    }

    async fn catalogs(&mut self) -> Result<Vec<String>> {
        Ok(vec![])
    }

    async fn schemas(&mut self, _catalog: Option<&str>) -> Result<Vec<String>> {
        let rows: Vec<Row> = self.conn.query("SHOW DATABASES").await.map_err(db_err)?;
        Ok(rows
            .into_iter()
            .filter_map(|r| conv::row_string(&r, 0))
            .collect())
    }

    async fn tables(&mut self, schema: &str) -> Result<Vec<TableInfo>> {
        let sql = "SELECT TABLE_NAME, TABLE_TYPE, TABLE_COMMENT \
                   FROM information_schema.TABLES WHERE TABLE_SCHEMA = ? ORDER BY TABLE_NAME";
        let rows: Vec<Row> = self.conn.exec(sql, (schema,)).await.map_err(db_err)?;
        Ok(rows
            .into_iter()
            .map(|r| TableInfo {
                name: conv::row_string(&r, 0).unwrap_or_default(),
                kind: conv::row_string(&r, 1).map(|t| t.to_lowercase()),
                comment: conv::row_string(&r, 2),
            })
            .collect())
    }

    async fn columns(&mut self, schema: &str, table: &str) -> Result<Vec<ColumnInfo>> {
        let sql = "SELECT COLUMN_NAME, COLUMN_TYPE, IS_NULLABLE, COLUMN_KEY, COLUMN_DEFAULT, COLUMN_COMMENT \
                   FROM information_schema.COLUMNS \
                   WHERE TABLE_SCHEMA = ? AND TABLE_NAME = ? ORDER BY ORDINAL_POSITION";
        let rows: Vec<Row> = self.conn.exec(sql, (schema, table)).await.map_err(db_err)?;
        Ok(rows
            .into_iter()
            .map(|r| ColumnInfo {
                name: conv::row_string(&r, 0).unwrap_or_default(),
                type_name: conv::row_string(&r, 1).unwrap_or_default(),
                column_type: None,
                nullable: conv::row_string(&r, 2).map(|v| v == "YES"),
                primary_key: conv::row_string(&r, 3).map(|v| v == "PRI"),
                default: conv::row_string(&r, 4),
                comment: conv::row_string(&r, 5),
            })
            .collect())
    }

    async fn indexes(&mut self, schema: &str, table: &str) -> Result<Vec<IndexInfo>> {
        let sql = format!(
            "SHOW INDEX FROM {} FROM {}",
            MysqlDialect.quote_identifier(table),
            MysqlDialect.quote_identifier(schema)
        );
        let rows: Vec<Row> = self.conn.query(sql).await.map_err(db_err)?;
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

    async fn foreign_keys(&mut self, schema: &str, table: &str) -> Result<Vec<ForeignKeyInfo>> {
        let sql = "SELECT CONSTRAINT_NAME, COLUMN_NAME, REFERENCED_TABLE_NAME, REFERENCED_COLUMN_NAME \
                   FROM information_schema.KEY_COLUMN_USAGE \
                   WHERE TABLE_SCHEMA = ? AND TABLE_NAME = ? AND REFERENCED_TABLE_NAME IS NOT NULL \
                   ORDER BY CONSTRAINT_NAME, ORDINAL_POSITION";
        let rows: Vec<Row> = self.conn.exec(sql, (schema, table)).await.map_err(db_err)?;
        let mut fks: Vec<ForeignKeyInfo> = Vec::new();
        for r in &rows {
            let name = conv::row_string(r, 0).unwrap_or_default();
            let column = conv::row_string(r, 1).unwrap_or_default();
            let ref_table = conv::row_string(r, 2).unwrap_or_default();
            let ref_col = conv::row_string(r, 3).unwrap_or_default();
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

    async fn triggers(&mut self, schema: &str, table: &str) -> Result<Vec<TriggerInfo>> {
        let sql = "SELECT TRIGGER_NAME, ACTION_TIMING, EVENT_MANIPULATION \
                   FROM information_schema.TRIGGERS \
                   WHERE TRIGGER_SCHEMA = ? AND EVENT_OBJECT_TABLE = ? ORDER BY TRIGGER_NAME";
        let rows: Vec<Row> = self.conn.exec(sql, (schema, table)).await.map_err(db_err)?;
        Ok(rows
            .into_iter()
            .map(|r| TriggerInfo {
                name: conv::row_string(&r, 0).unwrap_or_default(),
                timing: conv::row_string(&r, 1).unwrap_or_default(),
                event: conv::row_string(&r, 2).unwrap_or_default(),
            })
            .collect())
    }

    async fn procedures(&mut self, schema: &str) -> Result<Vec<ProcedureInfo>> {
        let sql = "SELECT ROUTINE_NAME, ROUTINE_TYPE FROM information_schema.ROUTINES \
                   WHERE ROUTINE_SCHEMA = ? ORDER BY ROUTINE_NAME";
        let rows: Vec<Row> = self.conn.exec(sql, (schema,)).await.map_err(db_err)?;
        Ok(rows
            .into_iter()
            .map(|r| ProcedureInfo {
                name: conv::row_string(&r, 0).unwrap_or_default(),
                kind: conv::row_string(&r, 1).unwrap_or_default(),
            })
            .collect())
    }

    async fn table_ddl(&mut self, schema: &str, table: &str) -> Result<String> {
        let sql = format!(
            "SHOW CREATE TABLE {}.{}",
            MysqlDialect.quote_identifier(schema),
            MysqlDialect.quote_identifier(table)
        );
        let rows: Vec<Row> = self.conn.query(sql).await.map_err(db_err)?;
        rows.into_iter()
            .next()
            .and_then(|r| conv::row_string(&r, 1))
            .ok_or_else(|| DbError::Database(format!("no DDL for {schema}.{table}")))
    }

    async fn execute_stream(
        &mut self,
        schema: Option<&str>,
        sql: &str,
        opts: &ExecOpts,
        sink: &mut dyn ResultSink,
    ) -> Result<()> {
        if let Some(db) = schema {
            if !db.is_empty() {
                self.conn
                    .query_drop(format!("USE {}", MysqlDialect.quote_identifier(db)))
                    .await
                    .map_err(db_err)?;
            }
        }

        let mut qr = self.conn.query_iter(sql).await.map_err(db_err)?;
        if let Some(cols) = qr.columns() {
            let columns: Vec<ColumnInfo> = cols
                .iter()
                .map(|c| ColumnInfo {
                    name: c.name_str().to_string(),
                    type_name: format!("{:?}", c.column_type()),
                    column_type: None,
                    nullable: None,
                    primary_key: None,
                    default: None,
                    comment: None,
                })
                .collect();
            sink.on_event(StreamEvent::Columns(columns));

            let mut batch = Vec::with_capacity(BATCH_ROWS);
            while let Some(row) = qr.next().await.map_err(db_err)? {
                batch.push(row_to_values(&row));
                if batch.len() >= BATCH_ROWS {
                    sink.on_event(StreamEvent::Rows(std::mem::take(&mut batch)));
                }
                if let Some(tok) = &opts.cancel {
                    if tok.is_cancelled() {
                        return Err(DbError::Cancelled);
                    }
                }
            }
            if !batch.is_empty() {
                sink.on_event(StreamEvent::Rows(batch));
            }
        } else {
            sink.on_event(StreamEvent::Affected {
                affected_rows: qr.affected_rows(),
                last_insert_id: qr.last_insert_id(),
            });
        }
        Ok(())
    }

    async fn begin(&mut self) -> Result<()> {
        self.conn.query_drop("START TRANSACTION").await.map_err(db_err)
    }

    async fn commit(&mut self) -> Result<()> {
        self.conn.query_drop("COMMIT").await.map_err(db_err)
    }

    async fn rollback(&mut self) -> Result<()> {
        self.conn.query_drop("ROLLBACK").await.map_err(db_err)
    }

    async fn set_autocommit(&mut self, enabled: bool) -> Result<()> {
        let sql = if enabled {
            "SET autocommit = 1"
        } else {
            "SET autocommit = 0"
        };
        self.conn.query_drop(sql).await.map_err(db_err)
    }
}

fn row_to_values(row: &Row) -> Vec<Value> {
    (0..row.len())
        .map(|i| match row.get::<mysql_async::Value, usize>(i) {
            Some(v) => conv::mysql_value_to_dby(&v),
            None => Value::Null,
        })
        .collect()
}
