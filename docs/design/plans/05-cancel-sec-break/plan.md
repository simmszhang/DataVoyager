# #5 取消秒断 — 实现计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 全局锁改为 per-connection 锁、取消令牌按查询实例创建、取消即关闭 socket 秒断（SELECT+DML）、取消后自动重连。

**Architecture:** 连接注册表 + `futures::lock::Mutex` per-connection 锁（S1，Send guard）；查询实例 token 注册表（`Arc` 包裹供 RAII guard 持有）；`CancellationToken` 用 `watch`（无丢失唤醒）；取消时「外层 `select!` 竞速 → drop 查询 future 释放借用 → `Option<Conn>::take()` 关 socket」秒断 + `needs_reconnect` 自动重连。

**Tech Stack:** Rust（dby-core / src-tauri / mysql_async 0.37 / tokio / futures）、Tauri 2。

**Spec:** `docs/design/plans/05-cancel-sec-break/design.md`（修订版，含已核对的 mysql_async 0.37 语义）

## Global Constraints

- 外层注册表 `std::sync::Mutex` 只做同步 get/clone/insert/remove，绝不跨 `await`；per-connection 锁 `futures::lock::Mutex`（guard Send）可跨 await。
- token 按查询实例创建，key = `"{conn_id}:{query_id}"`；用 Drop guard 保证全路径注销。
- **秒断机制**：取消 → 外层 `tokio::select!` 让查询 future（含 `qr` 借用）被 drop → `self.conn.take()` 后 `drop(conn)` 关 socket（服务端中止，无 drain）；**不调用** `Conn::disconnect`（它按值消费且只用于优雅关闭）。
- 错误形状遵循 S5（`Cancelled` 带 kind）。
- CI 门禁：`cargo fmt --check`、`clippy -D warnings`、`cargo test -p dby-core -p dby-driver-mysql`。

---

### Task 1: `CancellationToken` 用 `watch`

**Files:**
- Modify: `crates/dby-core/src/query.rs`
- Modify: `crates/dby-core/Cargo.toml`（`tokio = { version = "1", features = ["sync"] }`）

**Interfaces:**
- Produces: `CancellationToken::cancelled(&self) -> impl Future<Output=()>`（watch 实现，无丢失唤醒）

- [ ] **Step 1: 写失败测试（含「先 cancel 后首 poll」回归）**

```rust
#[tokio::test]
async fn cancelled_resolves_when_cancel_before_or_after_poll() {
    // 场景 A：先 cancel 再 poll cancelled() → 立即返回
    let t = CancellationToken::new();
    t.cancel();
    tokio::time::timeout(std::time::Duration::from_secs(1), t.cancelled()).await.unwrap();
    // 场景 B：poll 中 cancel → notified
    let t2 = CancellationToken::new();
    let fut = t2.cancelled();
    tokio::pin!(fut);
    tokio::select! { _ = &mut fut => panic!("不应提前返回"), _ = tokio::time::sleep(Duration::from_millis(10)) => {} }
    t2.cancel();
    tokio::time::timeout(std::time::Duration::from_secs(1), fut).await.unwrap();
    assert!(t2.is_cancelled());
}
```

- [ ] **Step 2: 运行确认失败**

Run: `cargo test -p dby-core query::tests::cancelled_resolves_when_cancel_before_or_after_poll`
Expected: FAIL（`cancelled` 不存在；dby-core 无 tokio sync）

- [ ] **Step 3: 最小实现**

按 design §4.3：

```rust
#[derive(Clone)]
pub struct CancellationToken {
    flag: Arc<AtomicBool>,                       // 快速无锁轮询
    tx: tokio::sync::watch::Sender<bool>,
    rx: tokio::sync::watch::Receiver<bool>,
}
impl CancellationToken {
    pub fn new() -> Self { let (tx, rx) = tokio::sync::watch::channel(false); Self { flag: Arc::new(AtomicBool::new(false)), tx, rx } }
    pub fn cancel(&self) { self.flag.store(true, Ordering::SeqCst); let _ = self.tx.send(true); }
    pub fn is_cancelled(&self) -> bool { self.flag.load(Ordering::SeqCst) }
    pub async fn cancelled(&self) {
        if self.is_cancelled() { return; }
        let mut rx = self.rx.clone();
        if *rx.borrow() { return; }
        let _ = rx.changed().await; // watch 存最新值：后订阅可见，无 Notify 丢失唤醒
    }
}
```

> **不用 `Notify`**：`notify_waiters` 只唤醒已注册 waiter、不存 permit，先 cancel 后首 poll 会永久挂起。watch 存最新值语义正确。

- [ ] **Step 4: 运行确认通过**

Run: `cargo test -p dby-core query::tests::cancelled_resolves_when_cancel_before_or_after_poll`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/dby-core/src/query.rs crates/dby-core/Cargo.toml
git commit -m "feat(core): watch-based CancellationToken::cancelled() (#34)"
```

> 注：与 #62（驱动 dev-deps 缺 tokio `time`）无关，勿并入。

---

### Task 2: `AppState` 注册表重构 + `ActiveConnection` 扩展

**Files:**
- Modify: `src-tauri/src/state.rs`

**Interfaces:**
- Produces: `connections: std::sync::Mutex<HashMap<u64, Arc<futures::lock::Mutex<ActiveConnection>>>>`、`query_tokens: Arc<std::sync::Mutex<HashMap<String, Arc<CancellationToken>>>>`、`ActiveConnection { params, needs_reconnect }`（删 `cancel`）

- [ ] **Step 1: 编译驱动（靠编译失败暴露调用点）**

Run: `cargo check`
Expected: FAIL（`connections.insert` / `active.cancel` 等调用点类型不匹配）

- [ ] **Step 2: 最小实现**

按 design §4.1；`open_session` 构造 `Arc::new(futures::lock::Mutex::new(active))` 并填 `params`、`needs_reconnect: false`；`query_tokens` 为 `Arc::new(Mutex::new(HashMap::new()))`（供 guard 克隆持有）。src-tauri 显式加 `futures` 依赖。

- [ ] **Step 3: 运行确认通过**

Run: `cargo check`
Expected: PASS（待 Task 3 修完所有命令后整体通过）

- [ ] **Step 4: Commit（与 Task 3 合并提交）**

---

### Task 3: 命令持锁范式改造（#21）

**Files:**
- Modify: `src-tauri/src/commands.rs`

**Interfaces:**
- Consumes: `connections` 注册表（Task 2）

- [ ] **Step 1: 逐命令改为「取 Arc → 锁单连接」**

```rust
#[tauri::command]
pub async fn list_databases(state: State<'_, Arc<AppState>>, id: u64) -> Result<Vec<String>> {
    let entry = state.connections.lock().unwrap().get(&id).cloned() // std::sync::Mutex 同步锁，只借 Arc
        .ok_or_else(|| DbError::ConnectionNotFound(id.to_string()))?;
    let mut active = entry.lock().await;   // futures::lock::Mutex：Send guard，可跨 await
    ensure_connected(&state, &mut active).await?;
    active.conn.schemas(None).await
}
```

覆盖：`list_databases/list_tables/list_columns/list_connections/execute_query/execute_query_stream/export_result/begin/commit/rollback/set_autocommit/execute_edit/run_ddl/disconnect/delete_project`（遍历连接者逐连接加锁）。

- [ ] **Step 2: 运行确认失败→通过**

Run: `cargo check` → 逐处修到通过；再 `cargo clippy --workspace --all-targets -- -D warnings`
Expected: 最终 PASS

- [ ] **Step 3: Commit**

```bash
git add src-tauri/src/state.rs src-tauri/src/commands.rs
git commit -m "refactor(shell): futures::lock per-connection locks, no lock across await (#21)"
```

---

### Task 4: 查询实例 token + `cancel_query` 免锁（#23）

**Files:**
- Modify: `src-tauri/src/commands.rs`

**Interfaces:**
- Consumes: `query_tokens`（Task 2）、`CancellationToken`（Task 1）
- Produces: `QueryTokenGuard`（Drop 全路径注销）

- [ ] **Step 1: 写失败测试**

```rust
#[tokio::test]
async fn cancel_query_hits_only_matching_connection() {
    // 注入 id=1 与 id=2 两个 token，cancel_query(1) 只 cancel 前缀 "1:" 的 token
}
```

- [ ] **Step 2: 运行确认失败**

Run: `cargo test -p dby --bin dby cancel_query_hits_only_matching_connection`
Expected: FAIL

- [ ] **Step 3: 最小实现**

```rust
struct QueryTokenGuard { map: Arc<std::sync::Mutex<HashMap<String, Arc<CancellationToken>>>>, key: String }
impl Drop for QueryTokenGuard { fn drop(&mut self) { self.map.lock().unwrap().remove(&self.key); } }

// 每个执行命令开头：
let key = format!("{id}:{query_id}");
let token = CancellationToken::new();
state.query_tokens.lock().unwrap().insert(key.clone(), token.clone());
let _guard = QueryTokenGuard { map: state.query_tokens.clone(), key };
let opts = ExecOpts { cancel: Some(token), ..Default::default() };

#[tauri::command]
pub async fn cancel_query(state: State<'_, Arc<AppState>>, id: u64) -> Result<()> {
    let tokens = state.query_tokens.lock().unwrap();
    let prefix = format!("{id}:"); // 提到循环外
    for (k, t) in tokens.iter() { if k.starts_with(&prefix) { t.cancel(); } }
    Ok(())
}
```

- [ ] **Step 4: 运行确认通过**

Run: `cargo test` + `cargo clippy -D warnings`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/commands.rs
git commit -m "feat(shell): per-query cancel tokens with RAII guard, lock-free cancel_query (#23)"
```

---

### Task 5: 秒断 + 自动重连（#5，SELECT 即时取消）

**Files:**
- Modify: `crates/dby-driver-mysql/src/lib.rs`（`MysqlConnection.conn: Option<Conn>` + `execute_stream` 外层 `select!`）
- Modify: `src-tauri/src/commands.rs`（`ensure_connected` + 捕获 `Cancelled` 置 `needs_reconnect`）
- Modify: `src/App.tsx`（取消后状态提示）

**Interfaces:**
- Produces: `ensure_connected(state, active) -> Result<()>`；`MysqlConnection.conn: Option<Conn>`

- [ ] **Step 1: 写失败测试（集成 `#[ignore]`）**

```rust
#[tokio::test]
#[ignore]
async fn select_sleep_cancel_is_prompt_and_reconnects() {
    // SELECT SLEEP(60)；100ms 后 cancel_query；断言返回 Cancelled 耗时 < 2s（秒断，非 drain）；
    // 再次同连接 SELECT 1 成功（自动重连）
}
```

- [ ] **Step 2: 运行确认（起 MySQL 后）**

Run: `cargo test -p dby-driver-mysql --test mysql_integration -- --ignored select_sleep_cancel_is_prompt_*`
Expected: FAIL（当前取消要等排空/无重连）

- [ ] **Step 3: 最小实现**

驱动（design §4.5）：

```rust
pub struct MysqlConnection { conn: Option<Conn>, version: String, _ssh: Option<SshTunnel> }

async fn execute_stream(&mut self, schema, sql, opts, sink) -> Result<()> {
    // USE 前置并入 run_query_stream
    let cancelled = {
        let conn = self.conn.as_mut().ok_or_else(|| DbError::ConnectionNotFound("mysql".into()))?;
        tokio::select! {
            biased;
            _ = cancel_signal(opts) => true,                                // 取消命中
            res = run_query_stream(conn, sql, sink, opts) => return res,     // 正常完成直接返回
        }
    };
    if cancelled {
        if let Some(c) = self.conn.take() { drop(c); } // drop Conn 关 socket → 服务端中止，无 drain
        return Err(DbError::Cancelled);
    }
    Ok(())
}

async fn cancel_signal(opts: &ExecOpts) {
    if let Some(c) = &opts.cancel { c.cancelled().await } else { std::future::pending::<()>().await }
}
```

> **为什么 drop `Conn` 而非 `disconnect`/`drop_result`**：mysql_async 0.37 `QueryResult` drop 不排空（隐式清理在 `Conn` 下次使用/drop 时，`query_result/mod.rs:45-49`）；`Conn` 无 `Drop` impl，drop `Conn` 直接关 socket 服务端中止；`Conn::disconnect(mut self)` 按值消费且是优雅关闭；`QueryResult::drop_result`（`:370`）是排空，禁用。外层 `select!` 让查询 future 被 drop（`qr` 借用释放）后，`self.conn.take()` 才可行。

壳层：各执行命令先 `ensure_connected`，捕获 `Cancelled` 置 `active.needs_reconnect = true` 并记录历史 status=cancelled。

- [ ] **Step 4: 运行确认通过**

Run: 同上集成测试
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/dby-driver-mysql/src/lib.rs src-tauri/src/commands.rs src/App.tsx
git commit -m "feat: sec-break cancel via socket close + auto-reconnect (#5)"
```

---

### Task 6: DML 取消（#34，统一走外层 select!）

**Files:**
- Modify: `crates/dby-driver-mysql/src/lib.rs`（`run_query_stream` 的 DML 分支）

**Interfaces:**
- Consumes: `CancellationToken::cancelled()`（Task 1）

- [ ] **Step 1: 写失败测试（集成 `#[ignore]`）**

```rust
#[tokio::test]
#[ignore]
async fn dml_can_be_cancelled_and_last_insert_id_kept() {
    // 长 UPDATE ... SLEEP；启动后 cancel_query → Cancelled；
    // 非取消路径 INSERT 的 last_insert_id 仍正确（防 query_drop 回归）
}
```

- [ ] **Step 2: 运行确认失败**

Run: `cargo test -p dby-driver-mysql --test mysql_integration -- --ignored dml_can_be_cancelled_*`
Expected: FAIL（当前 DML 无取消）

- [ ] **Step 3: 最小实现**

DML 与 SELECT 统一走外层 `select!`（Task 5 的 `run_query_stream`）；DML 分支**仍用 `query_iter`**（非 `query_drop`）以保留 `affected_rows/last_insert_id`；取消命中时外层 select! drop 该 future、take+drop Conn 中止 DML。

- [ ] **Step 4: 运行确认通过**

Run: 同上
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/dby-driver-mysql/src/lib.rs
git commit -m "feat(mysql): make DML cancellable via unified select! (#34)"
```

---

### Task 7: 并发回归测试（#21 验证）

**Files:**
- Create: `crates/dby-driver-mysql/tests/concurrency.rs`（`#[ignore]`）

**Interfaces:**
- Consumes: per-connection 锁（Task 3）

- [ ] **Step 1: 写集成测试**

```rust
#[tokio::test]
#[ignore]
async fn slow_query_does_not_block_other_connection() {
    // 连接 A 跑 SELECT SLEEP(30)；连接 B list_databases 应在 < 2s 返回
}
```

- [ ] **Step 2: 运行确认通过**

Run: `cargo test -p dby-driver-mysql --test concurrency -- --ignored`
Expected: PASS

- [ ] **Step 3: Commit**

```bash
git add crates/dby-driver-mysql/tests/concurrency.rs
git commit -m "test: slow query no longer serializes other connections (#21)"
```
