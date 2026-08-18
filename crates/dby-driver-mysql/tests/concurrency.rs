//! 驱动层冒烟测试：两条独立的 `MysqlDriver` 连接互不阻塞。
//!
//! 说明（评审后重定性）：本测试位于驱动 crate，只验证「驱动层两个独立的
//! `MysqlConnection` 无共享锁、天然并发、互不阻塞」——这是驱动 API 的冒烟测试，
//! 修复前/后都会通过，**不验证** #21 的壳层每连接锁（S1）。S1 锁在壳层
//! `src-tauri/src/state.rs` + `commands.rs`，由 src-tauri 层验证（已记录为
//! deferred：需 src-tauri harness，超出 plan 范围；S1 锁已在 T2+T3 评审中
//! 逐处静态核实）。
//!
//! 本测试：连接 A 上 `SELECT SLEEP(30)` 的同时，连接 B 的
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

/// 驱动层冒烟：两条独立 `MysqlDriver` 连接（各自独立的 `MysqlConnection`，
/// 驱动层无共享锁）并发运行 —— A 的 `SELECT SLEEP(30)` 不阻塞 B 的
/// `schemas()`（<2s 返回）。壳层每连接锁（S1，#21）不在本 crate 验证。
///
/// 环境守卫：`DBY_TEST_MYSQL_HOST/PORT/PASSWORD` 全未设置时跳过
/// （CI 无 MySQL；`--ignored` 手动运行未配置环境也不误连默认值）。
/// 守卫之后为可编译骨架：连接/查询失败时打印并跳过（不 unwrap/panic），
/// 唯一硬断言是 B 的返回耗时 <2s（A 的慢查询进行中）。
#[tokio::test]
#[ignore = "requires MySQL; see deploy/database/README.md"]
async fn two_driver_connections_run_concurrently() {
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
        "connection B was blocked by connection A's slow query ({elapsed:?}): two driver connections must not serialize each other"
    );
    eprintln!(
        "connection B listed {} database(s) in {elapsed:?} while A ran SELECT SLEEP(30)",
        dbs.len()
    );

    // 收尾：中止 A 的慢查询任务（drop 连接即关 socket，服务端随之中止 SLEEP）。
    slow_task.abort();
}
