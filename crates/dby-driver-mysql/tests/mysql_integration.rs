//! MySQL 驱动集成测试（需真实 MySQL，默认忽略）。
//!
//! 运行方式见 `deploy/database/README.md`：
//! `cargo test -p dby-driver-mysql --test mysql_integration -- --ignored --nocapture`

use dby_core::driver::{execute_buffered, ConnectParams, Driver};
use dby_core::error::DbError;
use dby_core::query::{CancellationToken, CollectingSink, ExecOpts, ResultSink, StreamEvent};
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
    assert_eq!(
        out.first_result_set().unwrap().rows[0][0].to_display_string(),
        "1"
    );

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
    assert_eq!(
        out.first_result_set().unwrap().rows[0][0].to_display_string(),
        "2"
    );

    execute_buffered(
        conn.as_mut(),
        Some("dby_test"),
        "DROP TABLE dby_it",
        &ExecOpts::default(),
    )
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

    // 长查询：递归 CTE 产生大量行（MySQL 8.0），取消后连接毒化（秒断，不可复用）。
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
    // #5 秒断：取消即关 socket，连接毒化 —— ping 报 ConnectionNotFound（不再可复用）。
    assert!(
        matches!(conn.ping().await, Err(DbError::ConnectionNotFound(_))),
        "cancel must poison the connection (sec-break)"
    );
    // 自动重连（壳层 ensure_connected 的驱动级等价）：新连接 SELECT 1 成功。
    let mut conn2 = driver.connect(&params()).await.expect("reconnect failed");
    let out = execute_buffered(
        conn2.as_mut(),
        None,
        "SELECT 1 AS one",
        &ExecOpts::default(),
    )
    .await
    .unwrap();
    assert_eq!(
        out.first_result_set().unwrap().rows[0][0].to_display_string(),
        "1"
    );
}

/// #5 秒断即时性：`SELECT SLEEP(60)` 取消必须在 <2s 返回（非 drain 排空），
/// 取消即关 socket 毒化连接；重连后同连接 SELECT 1 成功（壳层 `ensure_connected` 的驱动级等价）。
///
/// 环境守卫：未设置 `DBY_TEST_MYSQL_*` 时跳过（CI 无 MySQL；`--ignored` 手动运行时
/// 未配置环境也不误连默认值）。
#[tokio::test]
#[ignore = "requires MySQL; see deploy/database/README.md"]
async fn select_sleep_cancel_is_prompt_and_reconnects() {
    if std::env::var("DBY_TEST_MYSQL_HOST").is_err()
        && std::env::var("DBY_TEST_MYSQL_PORT").is_err()
        && std::env::var("DBY_TEST_MYSQL_PASSWORD").is_err()
    {
        eprintln!("skip: DBY_TEST_MYSQL_* 未设置（无真实 MySQL）");
        return;
    }

    let driver = MysqlDriver;
    let mut conn = driver.connect(&params()).await.expect("connect failed");

    // SELECT SLEEP(60)：100ms 后取消；断言 Cancelled 且耗时 < 2s（秒断，非 drain）。
    let cancel = CancellationToken::new();
    let cancel_clone = cancel.clone();
    let opts = ExecOpts {
        cancel: Some(cancel_clone),
        ..Default::default()
    };
    let canceller = tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        cancel.cancel();
    });
    let started = std::time::Instant::now();
    let mut sink = CountingSink { rows: 0 };
    let res = conn
        .execute_stream(None, "SELECT SLEEP(60)", &opts, &mut sink)
        .await;
    let elapsed = started.elapsed();
    canceller.await.unwrap();
    assert!(
        matches!(res, Err(DbError::Cancelled)),
        "expected cancellation, got {res:?}"
    );
    assert!(
        elapsed < std::time::Duration::from_secs(2),
        "cancel must be prompt (<2s, sec-break not drain), took {elapsed:?}"
    );

    // 取消即关 socket：连接毒化，ping 报 ConnectionNotFound。
    assert!(
        matches!(conn.ping().await, Err(DbError::ConnectionNotFound(_))),
        "cancel must poison the connection"
    );

    // 自动重连：新连接 SELECT 1 成功。
    let mut conn2 = driver.connect(&params()).await.expect("reconnect failed");
    let out = execute_buffered(
        conn2.as_mut(),
        None,
        "SELECT 1 AS one",
        &ExecOpts::default(),
    )
    .await
    .unwrap();
    assert_eq!(
        out.first_result_set().unwrap().rows[0][0].to_display_string(),
        "1"
    );
}

/// #33：元数据路径（columns()）与查询结果路径（execute_stream()）的列类型必须一致。
///
/// 历史缺陷：同一列在两条路径分别报 `int`/`MYSQL_TYPE_LONG`（base 不一致），
/// R6 后 unsigned 整数两路径均取 U 族。建表含 int unsigned / decimal(10,2) /
/// datetime(6) / tinyint(1)，分别走两条路径，断言 `column_type.base` 与
/// `unsigned` 一致（设计 §4.3「统一」定义），并回归 type_name 不再 int vs long。
#[tokio::test]
#[ignore = "requires MySQL; see deploy/database/README.md"]
async fn metadata_and_query_result_type_names_agree() {
    use dby_core::metadata::ColumnTypeBase as B;

    let driver = MysqlDriver;
    let mut conn = driver.connect(&params()).await.expect("connect failed");

    execute_buffered(
        conn.as_mut(),
        Some("dby_test"),
        "DROP TABLE IF EXISTS dby_tt",
        &ExecOpts::default(),
    )
    .await
    .unwrap();
    execute_buffered(
        conn.as_mut(),
        Some("dby_test"),
        "CREATE TABLE dby_tt (\
         a INT UNSIGNED, \
         b DECIMAL(10,2), \
         c DATETIME(6), \
         d TINYINT(1))",
        &ExecOpts::default(),
    )
    .await
    .unwrap();

    // 元数据路径
    let meta = conn.columns("dby_test", "dby_tt").await.unwrap();
    assert_eq!(meta.len(), 4);

    // 查询结果路径
    let mut sink = CollectingSink::new(None);
    conn.execute_stream(
        Some("dby_test"),
        "SELECT a, b, c, d FROM dby_tt",
        &ExecOpts::default(),
        &mut sink,
    )
    .await
    .unwrap();
    let output = sink.into_output();
    let query = &output.first_result_set().unwrap().columns;
    assert_eq!(query.len(), 4);

    // base 与 unsigned 跨路径一致（不再 int vs long 不一致）
    for (m, q) in meta.iter().zip(query) {
        let mt = m.column_type.as_ref().expect("metadata column_type");
        let qt = q.column_type.as_ref().expect("query column_type");
        assert_eq!(mt.base, qt.base, "base mismatch for {}", m.name);
        assert_eq!(mt.unsigned, qt.unsigned, "unsigned mismatch for {}", m.name);
    }

    // R6：int unsigned → U 族（而非 I32 + 标志），type_name 同源生成
    assert_eq!(meta[0].column_type.as_ref().unwrap().base, B::U32);
    assert!(meta[0].column_type.as_ref().unwrap().unsigned);
    assert_eq!(meta[0].type_name, "int unsigned");
    assert_eq!(query[0].type_name, "int unsigned");

    // decimal / datetime / tinyint(1) 的 base 断言（R6 不涉及的列保持原映射）
    assert_eq!(meta[1].column_type.as_ref().unwrap().base, B::Decimal);
    assert_eq!(meta[2].column_type.as_ref().unwrap().base, B::DateTime);
    assert_eq!(meta[3].column_type.as_ref().unwrap().base, B::Bool);
    assert!(!meta[3].column_type.as_ref().unwrap().unsigned);

    execute_buffered(
        conn.as_mut(),
        Some("dby_test"),
        "DROP TABLE dby_tt",
        &ExecOpts::default(),
    )
    .await
    .unwrap();
}

/// #34：DML 取消必须与 SELECT 一样走外层 `select!`（秒断 #5）—— 长 UPDATE 取消返回
/// `Cancelled` 且 <2s（非 drain 排空）；非取消路径的 INSERT 仍保留 `last_insert_id`
/// （防 `query_drop` 回归：`query_drop` 不提供 affected_rows/last_insert_id）。
///
/// 环境守卫：未设置 `DBY_TEST_MYSQL_*` 时跳过（CI 无 MySQL；`--ignored` 手动运行时
/// 未配置环境也不误连默认值）。
#[tokio::test]
#[ignore = "requires MySQL; see deploy/database/README.md"]
async fn dml_can_be_cancelled_and_last_insert_id_kept() {
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
        "DROP TABLE IF EXISTS dby_dml_cancel",
        &ExecOpts::default(),
    )
    .await
    .unwrap();
    execute_buffered(
        conn.as_mut(),
        Some("dby_test"),
        "CREATE TABLE dby_dml_cancel (id INT AUTO_INCREMENT PRIMARY KEY, name VARCHAR(64))",
        &ExecOpts::default(),
    )
    .await
    .unwrap();
    execute_buffered(
        conn.as_mut(),
        Some("dby_test"),
        "INSERT INTO dby_dml_cancel (name) VALUES ('seed')",
        &ExecOpts::default(),
    )
    .await
    .unwrap();

    // 长 UPDATE（每行 SLEEP(60)）：100ms 后取消 → Cancelled（DML 与 SELECT 同走外层 select!）。
    let cancel = CancellationToken::new();
    let cancel_clone = cancel.clone();
    let opts = ExecOpts {
        cancel: Some(cancel_clone),
        ..Default::default()
    };
    let canceller = tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        cancel.cancel();
    });
    let started = std::time::Instant::now();
    let mut sink = CountingSink { rows: 0 };
    let res = conn
        .execute_stream(
            Some("dby_test"),
            "UPDATE dby_dml_cancel SET name = SLEEP(60) WHERE id = 1",
            &opts,
            &mut sink,
        )
        .await;
    let elapsed = started.elapsed();
    canceller.await.unwrap();
    assert!(
        matches!(res, Err(DbError::Cancelled)),
        "expected DML cancellation, got {res:?}"
    );
    assert!(
        elapsed < std::time::Duration::from_secs(2),
        "DML cancel must be prompt (<2s, sec-break not drain), took {elapsed:?}"
    );
    // #5 秒断：取消即关 socket，连接毒化。
    assert!(
        matches!(conn.ping().await, Err(DbError::ConnectionNotFound(_))),
        "cancel must poison the connection"
    );

    // 非取消路径：INSERT 的 last_insert_id 必须保留（query_iter 而非 query_drop）。
    let mut conn2 = driver.connect(&params()).await.expect("reconnect failed");
    let mut sink2 = CollectingSink::new(None);
    conn2
        .execute_stream(
            Some("dby_test"),
            "INSERT INTO dby_dml_cancel (name) VALUES ('after-cancel')",
            &ExecOpts::default(),
            &mut sink2,
        )
        .await
        .unwrap();
    let out = sink2.into_output();
    assert_eq!(out.affected_rows, 1);
    let lid = out
        .last_insert_id
        .expect("last_insert_id must be present on non-cancelled INSERT");
    // 返回值必须真的标识该行（若回归 query_drop，last_insert_id 为 None，此断言即失败）。
    let mut check = CollectingSink::new(None);
    conn2
        .execute_stream(
            Some("dby_test"),
            "SELECT id FROM dby_dml_cancel WHERE name = 'after-cancel'",
            &ExecOpts::default(),
            &mut check,
        )
        .await
        .unwrap();
    let check_out = check.into_output();
    let rs = check_out.first_result_set().expect("result set");
    assert_eq!(rs.rows.len(), 1);
    assert_eq!(rs.rows[0][0].to_display_string(), lid.to_string());

    execute_buffered(
        conn2.as_mut(),
        Some("dby_test"),
        "DROP TABLE dby_dml_cancel",
        &ExecOpts::default(),
    )
    .await
    .unwrap();
}

/// #28 多结果集：存储过程返回 2 个 SELECT → `result_sets.len()==2` 且各行归位
/// （驱动按 columns 空/非空判别遍历全部结果集，不再只读首个）。
#[tokio::test]
#[ignore = "requires MySQL; see deploy/database/README.md"]
async fn call_procedure_yields_two_result_sets() {
    let driver = MysqlDriver;
    let mut conn = driver.connect(&params()).await.expect("connect failed");

    execute_buffered(
        conn.as_mut(),
        Some("dby_test"),
        "DROP PROCEDURE IF EXISTS dby_multi_rs",
        &ExecOpts::default(),
    )
    .await
    .unwrap();
    execute_buffered(
        conn.as_mut(),
        Some("dby_test"),
        "CREATE PROCEDURE dby_multi_rs() BEGIN SELECT 1 AS a; SELECT 2 AS b; END",
        &ExecOpts::default(),
    )
    .await
    .unwrap();

    let out = execute_buffered(
        conn.as_mut(),
        Some("dby_test"),
        "CALL dby_multi_rs()",
        &ExecOpts::default(),
    )
    .await
    .unwrap();
    assert_eq!(out.result_sets.len(), 2, "CALL must yield 2 result sets");
    assert_eq!(out.result_sets[0].rows[0][0].to_display_string(), "1");
    assert_eq!(out.result_sets[1].rows[0][0].to_display_string(), "2");

    execute_buffered(
        conn.as_mut(),
        Some("dby_test"),
        "DROP PROCEDURE dby_multi_rs",
        &ExecOpts::default(),
    )
    .await
    .unwrap();
}

/// #28 多结果集：连续 DML 语句两个 Affected 都发出（不丢第二集）。
/// 判别点：第二条 UPDATE 影响 2 行 → 顶层 `affected_rows` 必须是 2（最后结果集语义）；
/// 旧实现只读首个结果集时停留 1/0。DML 集不得产出 Columns（无结果集）。
#[tokio::test]
#[ignore = "requires MySQL; see deploy/database/README.md"]
async fn consecutive_dml_yields_both_affected() {
    let driver = MysqlDriver;
    let mut conn = driver.connect(&params()).await.expect("connect failed");

    execute_buffered(
        conn.as_mut(),
        Some("dby_test"),
        "DROP TABLE IF EXISTS dby_multi_dml",
        &ExecOpts::default(),
    )
    .await
    .unwrap();
    execute_buffered(
        conn.as_mut(),
        Some("dby_test"),
        "CREATE TABLE dby_multi_dml (id INT PRIMARY KEY, x INT)",
        &ExecOpts::default(),
    )
    .await
    .unwrap();
    execute_buffered(
        conn.as_mut(),
        Some("dby_test"),
        "INSERT INTO dby_multi_dml (id, x) VALUES (1, 0), (2, 0)",
        &ExecOpts::default(),
    )
    .await
    .unwrap();

    let mut sink = CollectingSink::new(None);
    conn.execute_stream(
        Some("dby_test"),
        "UPDATE dby_multi_dml SET x = 1 WHERE id = 1; \
         UPDATE dby_multi_dml SET x = 2 WHERE id IN (1, 2)",
        &ExecOpts::default(),
        &mut sink,
    )
    .await
    .unwrap();
    let out = sink.into_output();
    // 第二条 UPDATE 影响 2 行：顶层 affected_rows 取最后结果集（=2）；若第二集被丢弃则停留 1。
    assert_eq!(
        out.affected_rows, 2,
        "both DML sets must be consumed (top-level = last set)"
    );
    // DML 集不产 Columns：不得出现空列结果集。
    assert!(
        out.result_sets.is_empty(),
        "DML must not emit Columns/result sets"
    );

    // 两条 UPDATE 都在服务端生效（多语句语义）：第二条 `WHERE id IN (1,2)` 会把 id=1 也改回 2，
    // 故最终两行都是 x=2（若第二条未被消费，顶层 affected_rows 会停留 1，前面断言已拦截）。
    let check = execute_buffered(
        conn.as_mut(),
        Some("dby_test"),
        "SELECT x FROM dby_multi_dml ORDER BY id",
        &ExecOpts::default(),
    )
    .await
    .unwrap();
    let rs = check.first_result_set().unwrap();
    assert_eq!(rs.rows[0][0].to_display_string(), "2");
    assert_eq!(rs.rows[1][0].to_display_string(), "2");

    execute_buffered(
        conn.as_mut(),
        Some("dby_test"),
        "DROP TABLE dby_multi_dml",
        &ExecOpts::default(),
    )
    .await
    .unwrap();
}

/// #28 多结果集：第二集 server 错误必须经 `next()?` 冒给调用方（不静默吞）。
/// 旧实现只读首个结果集，`SELECT 1; BLABLA` 的错误留在连接里被静默吞掉（返回 Ok）。
#[tokio::test]
#[ignore = "requires MySQL; see deploy/database/README.md"]
async fn mid_stream_error_is_surfaced() {
    let driver = MysqlDriver;
    let mut conn = driver.connect(&params()).await.expect("connect failed");

    let mut sink = CollectingSink::new(None);
    let res = conn
        .execute_stream(
            Some("dby_test"),
            "SELECT 1 AS one; BLABLA",
            &ExecOpts::default(),
            &mut sink,
        )
        .await;

    // 第二集语法错误必须冒给调用方（不静默吞）。
    let err = res.expect_err("mid-stream server error must be surfaced");
    assert!(
        matches!(&err, DbError::Database(msg) if msg.contains("BLABLA")),
        "expected DbError::Database mentioning BLABLA, got {err:?}"
    );
    // 第一集已正常发出。
    let out = sink.into_output();
    assert_eq!(
        out.first_result_set().unwrap().rows[0][0].to_display_string(),
        "1"
    );
}
