//! MySQL 驱动集成测试（需真实 MySQL，默认忽略）。
//!
//! 运行方式见 `deploy/database/README.md`：
//! `cargo test -p dby-driver-mysql --test mysql_integration -- --ignored --nocapture`

use dby_core::driver::{execute_buffered, ConnectParams, Driver};
use dby_core::error::DbError;
use dby_core::query::{CancellationToken, ExecOpts, ResultSink, StreamEvent};
use dby_driver_mysql::MysqlDriver;

fn env(name: &str, default: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| default.to_string())
}

fn params() -> ConnectParams {
    ConnectParams {
        driver: "mysql".to_string(),
        host: env("DBY_TEST_MYSQL_HOST", "127.0.0.1"),
        port: env("DBY_TEST_MYSQL_PORT", "3306").parse().unwrap_or(3306),
        user: env("DBY_TEST_MYSQL_USER", "root"),
        password: Some(env("DBY_TEST_MYSQL_PASSWORD", "dby-test")),
        database: Some(env("DBY_TEST_MYSQL_DB", "dby_test")),
        ..Default::default()
    }
}

struct CountingSink {
    rows: usize,
}
impl ResultSink for CountingSink {
    fn on_event(&mut self, ev: StreamEvent) {
        if let StreamEvent::Rows(rows) = ev {
            self.rows += rows.len();
        }
    }
}

#[tokio::test]
#[ignore = "requires MySQL; see deploy/database/README.md"]
async fn metadata_crud_transaction() {
    let driver = MysqlDriver;
    let mut conn = driver.connect(&params()).await.expect("connect failed");

    conn.ping().await.expect("ping failed");

    let schemas = conn.schemas(None).await.expect("schemas failed");
    assert!(!schemas.is_empty(), "expected at least one database");

    // DDL + DML + 查询（缓冲路径）
    execute_buffered(
        conn.as_mut(),
        Some("dby_test"),
        "DROP TABLE IF EXISTS dby_it",
        &ExecOpts::default(),
    )
    .await
    .unwrap();
    execute_buffered(
        conn.as_mut(),
        Some("dby_test"),
        "CREATE TABLE dby_it (id INT PRIMARY KEY, name VARCHAR(64))",
        &ExecOpts::default(),
    )
    .await
    .unwrap();
    execute_buffered(
        conn.as_mut(),
        Some("dby_test"),
        "INSERT INTO dby_it (id, name) VALUES (1, 'alice')",
        &ExecOpts::default(),
    )
    .await
    .unwrap();

    let out = execute_buffered(
        conn.as_mut(),
        Some("dby_test"),
        "SELECT id, name FROM dby_it",
        &ExecOpts::default(),
    )
    .await
    .unwrap();
    let rs = out.first_result_set().unwrap();
    assert_eq!(rs.rows.len(), 1);
    assert_eq!(rs.rows[0][0].to_display_string(), "1");
    assert_eq!(rs.rows[0][1].to_display_string(), "alice");

    // 事务：回滚
    conn.begin().await.unwrap();
    execute_buffered(
        conn.as_mut(),
        Some("dby_test"),
        "INSERT INTO dby_it (id, name) VALUES (2, 'bob')",
        &ExecOpts::default(),
    )
    .await
    .unwrap();
    conn.rollback().await.unwrap();
    let out = execute_buffered(
        conn.as_mut(),
        Some("dby_test"),
        "SELECT COUNT(*) AS c FROM dby_it",
        &ExecOpts::default(),
    )
    .await
    .unwrap();
    assert_eq!(out.first_result_set().unwrap().rows[0][0].to_display_string(), "1");

    // 事务：提交
    conn.begin().await.unwrap();
    execute_buffered(
        conn.as_mut(),
        Some("dby_test"),
        "INSERT INTO dby_it (id, name) VALUES (3, 'carol')",
        &ExecOpts::default(),
    )
    .await
    .unwrap();
    conn.commit().await.unwrap();
    let out = execute_buffered(
        conn.as_mut(),
        Some("dby_test"),
        "SELECT COUNT(*) AS c FROM dby_it",
        &ExecOpts::default(),
    )
    .await
    .unwrap();
    assert_eq!(out.first_result_set().unwrap().rows[0][0].to_display_string(), "2");

    execute_buffered(conn.as_mut(), Some("dby_test"), "DROP TABLE dby_it", &ExecOpts::default())
        .await
        .unwrap();
}

#[tokio::test]
#[ignore = "requires MySQL; see deploy/database/README.md"]
async fn ddl_database_and_table() {
    use dby_core::ddl;
    let driver = MysqlDriver;
    let mut conn = driver.connect(&params()).await.expect("connect failed");
    let dialect = driver.dialect();

    // 建库
    execute_buffered(
        conn.as_mut(),
        None,
        &ddl::build_create_database(dialect, "dby_ddl_test"),
        &ExecOpts::default(),
    )
    .await
    .unwrap();

    // 建表
    let cols = vec![
        ddl::ColumnDef {
            name: "id".into(),
            type_name: "INT".into(),
            nullable: false,
            primary_key: true,
        },
        ddl::ColumnDef {
            name: "name".into(),
            type_name: "VARCHAR(64)".into(),
            nullable: true,
            primary_key: false,
        },
    ];
    execute_buffered(
        conn.as_mut(),
        Some("dby_ddl_test"),
        &ddl::build_create_table(dialect, "t1", &cols),
        &ExecOpts::default(),
    )
    .await
    .unwrap();

    // 改表名
    execute_buffered(
        conn.as_mut(),
        Some("dby_ddl_test"),
        &ddl::build_rename_table(dialect, "t1", "t2"),
        &ExecOpts::default(),
    )
    .await
    .unwrap();
    let tables = conn.tables("dby_ddl_test").await.unwrap();
    assert!(tables.iter().any(|t| t.name == "t2"));

    // 删表 + 删库
    execute_buffered(
        conn.as_mut(),
        Some("dby_ddl_test"),
        &ddl::build_drop_table(dialect, "t2"),
        &ExecOpts::default(),
    )
    .await
    .unwrap();
    execute_buffered(
        conn.as_mut(),
        None,
        &ddl::build_drop_database(dialect, "dby_ddl_test"),
        &ExecOpts::default(),
    )
    .await
    .unwrap();
}

#[tokio::test]
#[ignore = "requires MySQL 8.0; see deploy/database/README.md"]
async fn streaming_cancel_and_reuse() {
    let driver = MysqlDriver;
    let mut conn = driver.connect(&params()).await.expect("connect failed");

    // 长查询：递归 CTE 产生大量行（MySQL 8.0），取消后连接仍可复用。
    let cancel = CancellationToken::new();
    let cancel_clone = cancel.clone();
    let opts = ExecOpts {
        cancel: Some(cancel_clone),
        ..Default::default()
    };
    let canceller = tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        cancel.cancel();
    });
    let mut sink = CountingSink { rows: 0 };
    let res = conn
        .execute_stream(
            None,
            // 交叉连接产生大量行、每行 1ms 睡眠（足够慢让取消生效）
            "SELECT SLEEP(0.001) FROM information_schema.tables a, information_schema.tables b, information_schema.tables c LIMIT 5000",
            &opts,
            &mut sink,
        )
        .await;
    canceller.await.unwrap();
    assert!(
        matches!(res, Err(DbError::Cancelled)),
        "expected cancellation, got {res:?}"
    );
    // 取消后连接可复用
    conn.ping().await.expect("connection should be reusable after cancel");
    let out = execute_buffered(conn.as_mut(), None, "SELECT 1 AS one", &ExecOpts::default())
        .await
        .unwrap();
    assert_eq!(out.first_result_set().unwrap().rows[0][0].to_display_string(), "1");
}
