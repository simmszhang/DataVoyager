//! #45 数值精度 — BIGINT 往返无损集成测试（需真实 MySQL，默认忽略）。
//!
//! 覆盖：`BIGINT`（`9223372036854775807`，i64::MAX）与 `BIGINT UNSIGNED`
//! （`18446744073709551615`，u64::MAX）经驱动文本协议插入/回读全程无损
//! （`Value::I64/U64` 全量保持，不坍缩为 f64）；`last_insert_id` 以十进制
//! **字符串**返回（#45 修复，杜绝 >2^53 精度丢失）。
//!
//! 表结构说明（相对任务简报的偏差）：简报原定单列 `v BIGINT UNSIGNED`；按 #33
//! 语义，unsigned 列统一映射 U 族（`U64`），无法从该列产出 `Value::I64` 断言。
//! 故拆为 `v BIGINT`（有符号 → `I64`）+ `u BIGINT UNSIGNED`（无符号 → `U64`）
//! 两列，分别覆盖 i64::MAX / u64::MAX 两种 >2^53 往返。
//!
//! 运行方式见 `deploy/database/README.md`：
//! `cargo test -p dby-driver-mysql --test precision -- --ignored --nocapture`

use dby_core::driver::{execute_buffered, ConnectParams, Driver};
use dby_core::query::ExecOpts;
use dby_core::value::Value;
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

/// #45：BIGINT / BIGINT UNSIGNED 主键查询 + `last_insert_id` 超 2^53 不丢精度。
///
/// 断言点：
/// 1. `INSERT ... (9223372036854775807)` 后 `last_insert_id` 是**十进制字符串**
///    （非 number），且与 `SELECT id` 回读值一致；
/// 2. `SELECT v` 回读 `Value::I64(9223372036854775807)`（有符号 BIGINT 全量无损）；
/// 3. `18446744073709551615`（u64::MAX = BIGINT UNSIGNED 上限）被接受，
///    回读 `Value::U64(18446744073709551615)`。
///
/// 环境守卫：未设置 `DBY_TEST_MYSQL_*` 时跳过（CI 无 MySQL；`--ignored` 手动运行时
/// 未配置环境也不误连默认值）。
#[tokio::test]
#[ignore = "requires MySQL; see deploy/database/README.md"]
async fn bigint_roundtrips_without_precision_loss() {
    if std::env::var("DBY_TEST_MYSQL_HOST").is_err()
        && std::env::var("DBY_TEST_MYSQL_PORT").is_err()
        && std::env::var("DBY_TEST_MYSQL_PASSWORD").is_err()
    {
        eprintln!("skip: DBY_TEST_MYSQL_* 未设置（无真实 MySQL）");
        return;
    }

    let driver = MysqlDriver;
    let mut conn = driver.connect(&params()).await.expect("connect failed");

    execute_buffered(
        conn.as_mut(),
        Some("dby_test"),
        "DROP TABLE IF EXISTS dby_precision",
        &ExecOpts::default(),
    )
    .await
    .unwrap();
    execute_buffered(
        conn.as_mut(),
        Some("dby_test"),
        "CREATE TABLE dby_precision (id BIGINT PRIMARY KEY AUTO_INCREMENT, \
         v BIGINT NULL, u BIGINT UNSIGNED NULL)",
        &ExecOpts::default(),
    )
    .await
    .unwrap();

    // i64::MAX 插入有符号 BIGINT 列。
    let out = execute_buffered(
        conn.as_mut(),
        Some("dby_test"),
        "INSERT INTO dby_precision (v) VALUES (9223372036854775807)",
        &ExecOpts::default(),
    )
    .await
    .unwrap();
    assert_eq!(out.affected_rows, 1);
    // #45：last_insert_id 是十进制字符串（而非 number），否则 >2^53 静默丢精度。
    let lid = out
        .last_insert_id
        .expect("last_insert_id must be present on INSERT");
    assert!(
        !lid.is_empty() && lid.chars().all(|c| c.is_ascii_digit()),
        "last_insert_id must be a decimal string, got {lid:?}"
    );

    // u64::MAX = BIGINT UNSIGNED 上限：应被接受并无损回读为 U64。
    execute_buffered(
        conn.as_mut(),
        Some("dby_test"),
        "INSERT INTO dby_precision (u) VALUES (18446744073709551615)",
        &ExecOpts::default(),
    )
    .await
    .unwrap();

    // SELECT 回读：id 列（BIGINT 有符号）与 last_insert_id 一致；
    // v = i64::MAX → I64 全量无损；u = u64::MAX → U64 全量无损。
    let out = execute_buffered(
        conn.as_mut(),
        Some("dby_test"),
        "SELECT id, v, u FROM dby_precision ORDER BY id",
        &ExecOpts::default(),
    )
    .await
    .unwrap();
    let rs = out
        .first_result_set()
        .expect("SELECT must yield a result set");
    assert_eq!(rs.rows.len(), 2);
    assert_eq!(rs.rows[0][1], Value::I64(9223372036854775807));
    assert_eq!(rs.rows[0][2], Value::Null);
    assert_eq!(rs.rows[1][1], Value::Null);
    assert_eq!(rs.rows[1][2], Value::U64(18446744073709551615));
    assert!(
        matches!(rs.rows[0][0], Value::I64(_)),
        "id column must be I64, got {:?}",
        rs.rows[0][0]
    );
    assert_eq!(
        rs.rows[0][0].to_display_string(),
        lid,
        "id column must match last_insert_id"
    );

    execute_buffered(
        conn.as_mut(),
        Some("dby_test"),
        "DROP TABLE dby_precision",
        &ExecOpts::default(),
    )
    .await
    .unwrap();
}
