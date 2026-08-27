//! Test that tables() and views() return separate lists (defect #70).
//!
//! Run with: `cargo test -p dby-driver-mysql --test tables_views_separate -- --ignored --nocapture`

use dby_core::driver::{execute_buffered, ConnectParams, Driver};
use dby_core::query::ExecOpts;
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

#[tokio::test]
#[ignore = "requires MySQL; see deploy/database/README.md"]
async fn tables_and_views_are_separate() {
    let driver = MysqlDriver;
    let mut conn = driver.connect(&params()).await.expect("connect failed");

    // Create test table and view
    execute_buffered(
        conn.as_mut(),
        Some("dby_test"),
        "DROP TABLE IF EXISTS test_table_for_view",
        &ExecOpts::default(),
    )
    .await
    .ok();

    execute_buffered(
        conn.as_mut(),
        Some("dby_test"),
        "DROP VIEW IF EXISTS test_view2",
        &ExecOpts::default(),
    )
    .await
    .ok();

    execute_buffered(
        conn.as_mut(),
        Some("dby_test"),
        "CREATE TABLE test_table_for_view (id INT PRIMARY KEY, name VARCHAR(50))",
        &ExecOpts::default(),
    )
    .await
    .expect("create table failed");

    execute_buffered(
        conn.as_mut(),
        Some("dby_test"),
        "CREATE VIEW test_view2 AS SELECT * FROM test_table_for_view",
        &ExecOpts::default(),
    )
    .await
    .expect("create view failed");

    // Get tables and views
    let tables = conn.tables("dby_test").await.expect("tables failed");
    let views = conn.views("dby_test").await.expect("views failed");

    // Verify view is NOT in tables list
    let table_names: Vec<_> = tables.iter().map(|t| t.name.as_str()).collect();
    let view_names: Vec<_> = views.iter().map(|v| v.name.as_str()).collect();

    assert!(
        table_names.contains(&"test_table_for_view"),
        "test_table_for_view should be in tables"
    );
    assert!(
        !table_names.contains(&"test_view2"),
        "test_view2 should NOT be in tables (defect #70)"
    );
    assert!(
        view_names.contains(&"test_view2"),
        "test_view2 should be in views"
    );

    // Cleanup
    execute_buffered(
        conn.as_mut(),
        Some("dby_test"),
        "DROP VIEW test_view2",
        &ExecOpts::default(),
    )
    .await
    .ok();

    execute_buffered(
        conn.as_mut(),
        Some("dby_test"),
        "DROP TABLE test_table_for_view",
        &ExecOpts::default(),
    )
    .await
    .ok();
}
