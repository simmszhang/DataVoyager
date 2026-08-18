//! #21 并发回归测试：慢查询不得串行化其他连接。
//!
//! 历史缺陷 #21：全局锁把**所有**连接串行化 —— 连接 A 的长查询进行期间，
//! 连接 B 的任何操作都要排队等 A 结束。修复方案 S1：改为每连接独立锁
//! （连接 A 的慢查询不再阻塞连接 B）。
//!
//! 本测试验证：连接 A 上 `SELECT SLEEP(30)` 的同时，连接 B 的
//! `schemas()`（SHOW DATABASES，即 list_databases）必须在 <2s 内返回。
//!
//! 需要真实 MySQL，默认 `#[ignore]` 跳过；运行方式同其他集成测试：
//! `cargo test -p dby-driver-mysql --test concurrency -- --ignored --nocapture`
//! （连接参数环境变量：`DBY_TEST_MYSQL_HOST/PORT/USER/PASSWORD/DB`）

use std::time::{Duration, Instant};

use dby_core::driver::{ConnectParams, Driver};
use dby_core::query::{CollectingSink, ExecOpts};
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

/// 回归 #21：连接 A 的慢查询不得阻塞连接 B（每连接独立锁，S1）。
///
/// 环境守卫：`DBY_TEST_MYSQL_HOST/PORT/PASSWORD` 全未设置时跳过
/// （CI 无 MySQL；`--ignored` 手动运行未配置环境也不误连默认值）。
/// 守卫之后为可编译骨架：连接/查询失败时打印并跳过（不 unwrap/panic），
/// 唯一硬断言是 B 的返回耗时 <2s（A 的慢查询进行中）。
#[tokio::test]
#[ignore = "requires MySQL; see deploy/database/README.md"]
async fn slow_query_does_not_block_other_connection() {
    if std::env::var("DBY_TEST_MYSQL_HOST").is_err()
        && std::env::var("DBY_TEST_MYSQL_PORT").is_err()
        && std::env::var("DBY_TEST_MYSQL_PASSWORD").is_err()
    {
        eprintln!("skip: DBY_TEST_MYSQL_* 未设置（无真实 MySQL）");
        return;
    }

    let driver = MysqlDriver;
    let Ok(mut conn_a) = driver.connect(&params()).await else {
        eprintln!("skip: connection A failed — is MySQL reachable?");
        return;
    };
    let Ok(mut conn_b) = driver.connect(&params()).await else {
        eprintln!("skip: connection B failed — is MySQL reachable?");
        return;
    };

    // 连接 A：独立任务里跑 30s 慢查询（远超 B 的 2s 阈值）。
    let slow_task = tokio::spawn(async move {
        let mut sink = CollectingSink::new(None);
        conn_a
            .execute_stream(None, "SELECT SLEEP(30)", &ExecOpts::default(), &mut sink)
            .await
    });

    // 等 A 的慢查询真正开始，再计时 B 的 list_databases（否则 B 可能先完成，测试无意义）。
    tokio::time::sleep(Duration::from_millis(200)).await;

    // 连接 B：慢查询进行中列数据库，<2s 返回即证明未被 A 串行化。
    let started = Instant::now();
    let Ok(dbs) = conn_b.schemas(None).await else {
        eprintln!("skip: schemas on connection B failed");
        return;
    };
    let elapsed = started.elapsed();
    assert!(
        elapsed < Duration::from_secs(2),
        "connection B was blocked by connection A's slow query ({elapsed:?}): per-connection locks (#21) broken"
    );
    eprintln!(
        "connection B listed {} database(s) in {elapsed:?} while A ran SELECT SLEEP(30)",
        dbs.len()
    );

    // 收尾：中止 A 的慢查询任务（drop 连接即关 socket，服务端随之中止 SLEEP）。
    slow_task.abort();
}
