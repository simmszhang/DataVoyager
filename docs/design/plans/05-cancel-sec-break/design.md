# #5 取消秒断 — 设计文档

> 状态：评审需重写（3 阻断项已修订，待复审） · 优先级 P1 · 规模：大 · 关联缺陷：#21（全局锁）、#23（sticky cancel）、#34（DML 取消）· 依赖共享契约：S1（并发模型，本方案回写）、S5（错误形状）

## 1. 现状与影响

- `state.rs:26`：`connections: tokio::sync::Mutex<HashMap<u64, ActiveConnection>>` 单把全局锁；`commands.rs` 的 `execute_query_stream(457-478)/execute_query(294-308)/export_result(769-785)/list_databases/tables/columns(251-284)/begin/commit/rollback/set_autocommit(522-554)/execute_edit(600-614)/run_ddl(660-674)` 都在 `guard` 存活期 `await` 网络 I/O；`cancel_query(503-509)` 也抢同一把锁（#21，review S1）。
- `state.rs:19` + `commands.rs:123`：取消令牌建连接时创建一次，`cancel_query` 置 true 后全仓库无 reset（#23，review S3）。
- `lib.rs:284-288`：取消检查只在 SELECT 分支（批间检查），DML 无取消检查（#34，review D4）。
- 取消返回 `Err(Cancelled)` 后 drop 结果流，慢查询排空很久（#5，drain）。
- **影响**：一次慢查询串行化**所有**连接的一切操作；运行中「取消」抢不到锁失效；取消后连接被毒化；DML 无法取消；「停止」不即时。

## 2. 目标与成功标准

1. 任意连接慢查询不再串行化其它连接（锁不跨 `await`，且 future 保持 `Send`）。
2. `cancel_query` 运行时即时生效，不抢连接锁。
3. 取消令牌按查询实例创建，取消后连接不被毒化。
4. 真·秒断：取消即关闭底层 socket（服务端中止查询），不排空（`SELECT SLEEP(60)` 也即时）。
5. DML 可取消（#34），且取消不影响正常路径的 `affected_rows`/`last_insert_id`。
6. 成功标准：连接 A 慢查询期间连接 B 正常；运行中取消在百毫秒级返回 `Cancelled`；取消后同连接再查询成功（自动重连）；`SELECT SLEEP(60)`/长 DML 可即时取消；INSERT 取消后 `last_insert_id` 仍正确（非取消路径）。

## 3. 方案对比

### 方案 A：per-connection 锁 + 查询实例 token + 关闭 socket 秒断（推荐）
- S1：注册表 + `futures::lock::Mutex` per-connection 锁（Send guard）；`cancel_query` 读 token 的 `Arc`；token 按查询实例创建；取消时 `Option<Conn>::take()` + drop 关 socket，连接毒化后自动重连。
- **优点**：一处解决 #21/#23/#5/#34；机制经 mysql_async 源码核实可编译。**缺点**：改造所有命令持锁范式。

### 方案 B：仅 per-connection 锁，保留 drain
- 修 #21/#23，但取消仍是 drain。
- **缺点**：#5「秒断」未达成，返工。

### 方案 C：`KILL QUERY` 控制连接
- 用独立连接发 `KILL QUERY <thread_id>` 中断查询。
- **缺点**：需额外连接 + 用户需 KILL/CONNECTION_ADMIN 权限；mysql_async `Conn` 未暴露易用的 `connection_id` 查询面。仅作 fallback。

**推荐 A**。

## 4. 推荐方案详细设计

### 4.1 状态重构（`src-tauri/src/state.rs`，回写 S1）

```rust
pub struct ActiveConnection {
    pub id: u64,
    pub name: String,
    pub driver_id: String,
    pub project_id: String,
    pub database: String,
    pub server_version: String,
    pub params: ConnectParams,       // 新增：重连所需（secrets 仅存内存）
    pub needs_reconnect: bool,       // 新增：秒断后标记毒化
    pub conn: Box<dyn Connection + Send>,
    // 删除 per-connection 的 cancel 字段
}

pub struct AppState {
    pub registry: DriverRegistry,
    // 外层注册表：std::sync::Mutex（只做 get/clone/insert/remove，绝不跨 await）
    pub connections: std::sync::Mutex<HashMap<u64, Arc<futures::lock::Mutex<ActiveConnection>>>>,
    // 查询实例 token 注册表："{conn_id}:{query_id}" -> Arc<CancellationToken>；Arc 包裹以便 QueryTokenGuard 持有
    pub query_tokens: Arc<std::sync::Mutex<HashMap<String, Arc<CancellationToken>>>>,
    pub next_id: AtomicU64,
    pub config: tokio::sync::Mutex<AppConfig>,   // 保持不变
    pub config_path: PathBuf,
    pub history: HistoryStore,
}
```

> **为何 `futures::lock::Mutex`**：`tokio::sync::MutexGuard` 非 `Send`，跨 `.await` 持锁会使命令 future 变 `!Send`，Tauri 2 `#[tauri::command]`（经 `tokio::spawn`）要求 `Future + Send`，会编译失败。`futures::lock::MutexGuard` 是 `Send`，可安全跨 await 持有 per-connection 锁。外层注册表因从不跨 await，用 `std::sync::Mutex` 即可。依赖：src-tauri 显式声明 `futures`。

### 4.2 命令持锁范式（锁不跨「全局」await，Send 安全）

```rust
let entry = state.connections.lock().unwrap().get(&id).cloned()
    .ok_or_else(|| DbError::ConnectionNotFound(id.to_string()))?; // 只借 Arc，立刻放外层锁
let mut active = entry.lock().await;   // futures::lock::Mutex：Send guard，可跨 await
ensure_connected(&state, &mut active).await?; // 毒化重连（见 4.7）
active.conn.execute_stream(/* ... */).await
```

**覆盖命令**（所有触碰 `active.conn` 或遍历连接者）：`list_databases/list_tables/list_columns/execute_query/execute_query_stream/export_result/begin/commit/rollback/set_autocommit/execute_edit/run_ddl/disconnect`，以及**遍历连接的命令** `list_connections`（`commands.rs:243-248` 逐连接加锁 snapshot）、`delete_project`（`commands.rs:372-376` 逐连接加锁判 `project_id`）、`cancel_query`（读 `query_tokens`，不碰连接锁）。`open_session` 构造 `Arc::new(futures::lock::Mutex::new(active))`。

### 4.3 取消令牌（`crates/dby-core/src/query.rs`，扩展 S1）

```rust
#[derive(Clone)]
pub struct CancellationToken {
    flag: Arc<AtomicBool>,                          // 快速无锁轮询（批内检查）
    tx: tokio::sync::watch::Sender<bool>,
    rx: tokio::sync::watch::Receiver<bool>,
}

impl CancellationToken {
    pub fn new() -> Self {
        let (tx, rx) = tokio::sync::watch::channel(false);
        Self { flag: Arc::new(AtomicBool::new(false)), tx, rx }
    }
    pub fn cancel(&self) {
        self.flag.store(true, SeqCst);
        let _ = self.tx.send(true);   // watch 存最新值：后订阅者立即可见，无 Notify 丢失唤醒
    }
    pub fn is_cancelled(&self) -> bool { self.flag.load(SeqCst) }
    pub async fn cancelled(&self) {
        if self.is_cancelled() { return; }
        let mut rx = self.rx.clone();
        if *rx.borrow() { return; }
        let _ = rx.changed().await;   // 取消后立即就绪
    }
}
```

> 用 `watch` 而非 `Notify`：`Notify::notify_waiters` 只唤醒已注册 waiter、不存 permit，先 cancel 后 `cancelled()` 首 poll 会永久挂起；`watch` 存最新值，语义正确。dby-core 显式声明 `tokio = { features = ["sync"] }`（**与 #62 无关**：#62 是驱动 dev-deps 缺 `time`，见 §7）。

### 4.4 查询实例 token 注册/注销（RAII）

```rust
struct QueryTokenGuard { map: Arc<Mutex<HashMap<String, Arc<CancellationToken>>>>, key: String }
impl Drop for QueryTokenGuard { fn drop(&mut self) { self.map.lock().unwrap().remove(&self.key); } }

// 每个执行命令开头：
let query_id = uuid::Uuid::new_v4().to_string();
let key = format!("{id}:{query_id}");
let token = CancellationToken::new();
state.query_tokens.lock().unwrap().insert(key.clone(), token.clone());
let _guard = QueryTokenGuard { map: /* Arc 指向 query_tokens */, key }; // 任意路径（含 ? 提前返回）自动注销
let opts = ExecOpts { cancel: Some(token), ..Default::default() };
```

`cancel_query`（不抢连接锁）：

```rust
#[tauri::command]
pub async fn cancel_query(state: State<'_, Arc<AppState>>, id: u64) -> Result<()> {
    let tokens = state.query_tokens.lock().unwrap();
    let prefix = format!("{id}:"); // 提到循环外，避免逐键重算
    for (k, t) in tokens.iter() {
        if k.starts_with(&prefix) { t.cancel(); }
    }
    Ok(())
}
```

查询实例级 token 天然修复 #23。

### 4.5 秒断机制（#5，SELECT 即时取消）

`MysqlConnection.conn` 由 `Conn` 改 `Option<Conn>`（`lib.rs:94-98`）。`execute_stream` 结构：

```rust
async fn execute_stream(&mut self, schema, sql, opts, sink) -> Result<()> {
    // USE 前置（见 4.6 一并竞速）
    let cancelled = {
        let conn = self.conn.as_mut().ok_or_else(|| DbError::ConnectionNotFound("mysql".into()))?;
        tokio::select! {
            biased;
            _ = cancel_signal(opts) => true,                    // 取消命中
            res = run_query_stream(conn, sql, sink, opts) => return res, // 正常完成直接返回
        }
    };
    // 走到这里 = 取消命中；run_query_stream 的 future 已被 drop（qr 借用释放）
    if cancelled {
        if let Some(c) = self.conn.take() { drop(c); } // drop Conn 关闭 socket → 服务端中止，无 drain
        return Err(DbError::Cancelled);
    }
    Ok(())
}
```

> **为什么 drop `Conn`（而非 `QueryResult`）即秒断**：mysql_async 0.37 的 `QueryResult` drop **不排空**，隐式清理（读剩余行）只在 `Conn` **下次被查询或 drop** 时触发（`query_result/mod.rs:45-49`）；`Conn` 无 `Drop` 实现，drop `Conn` 直接关 socket，服务端见连接关闭即中止查询。故「取消 → drop 查询 future（释放 `&mut Conn` 借用）→ `self.conn.take()` 关 socket」是唯一无需 drain 的秒断路径。`QueryResult::drop_result`（`query_result/mod.rs:370`）恰恰是排空，禁用之。

`cancel_signal(opts)`：

```rust
async fn cancel_signal(opts: &ExecOpts) {
    if let Some(c) = &opts.cancel { c.cancelled().await } else { std::future::pending::<()>().await }
}
```

`run_query_stream(conn, sql, sink, opts)`：沿用现有「`query_iter` → 列/行分批推 sink」逻辑；批间 `is_cancelled()` 检查保留为**防御性**（正常路径下外层 `select!` 已即时处理，此处仅兜底）。无 `futures` 额外依赖（用 `std::future::pending`）。

### 4.6 DML 取消（#34，保留 affected/last_insert_id）

DML 与 SELECT 统一走外层 `select!`（4.5），`run_query_stream` 对无列结果集分支**仍用 `query_iter`**（非 `query_drop`），读 `qr.affected_rows()/last_insert_id()` 推 `Affected`。取消命中时外层 `select!` drop 该 future、take+drop `Conn` 中止 DML；正常完成时 `Affected` 值完整。`USE` 前置 `query_drop` 同样包进外层 `select!`（或作为 `run_query_stream` 的一部分被竞速）。

### 4.7 毒化自动重连

```rust
async fn ensure_connected(state: &AppState, active: &mut ActiveConnection) -> Result<()> {
    if active.needs_reconnect {
        let driver = state.registry.resolve(&active.driver_id)?;
        let conn = driver.connect(&active.params).await?;
        active.conn = conn;
        active.server_version = active.conn.server_version();
        active.needs_reconnect = false;
    }
    Ok(())
}
```

壳层各执行命令捕获 `DbError::Cancelled` 后置 `active.needs_reconnect = true` 并记录历史 status=cancelled；前端状态栏提示「已取消，连接将自动重连」。

## 5. 错误处理（遵循 S5）

- 取消：`DbError::Cancelled`（kind=`cancelled`），前端据 kind 区分「取消」与「失败」（联动 #29/S5）。
- 重连失败：`DbError::Database("重连失败: …")`。

## 6. 测试策略

- **单元（query）**：`cancelled()` 在 `cancel()` 前后均可立即就绪（**含「先 cancel 后首 poll」的丢失唤醒回归**）；token 克隆共享。
- **单元（state/commands）**：`cancel_query` 只触达 `{id}:` 前缀；`QueryTokenGuard` 全路径注销。
- **集成（`#[ignore]`）**：连接 A `SELECT SLEEP(60)` 期间连接 B `list_databases` 立即返回（不串行化）；`SELECT SLEEP(60)` 取消在百毫秒级返回（**秒断即时性，非仅 DML**）；取消后同连接再 `SELECT 1` 成功（自动重连）；长 DML 可取消；**INSERT（非取消路径）`last_insert_id` 仍正确**（防 #34 回归）。

## 7. 回归风险与影响面

- 所有命令持锁范式改变：改动面覆盖 `commands.rs` 全部连接命令 + `list_connections`/`delete_project`，需逐处回归。
- `MysqlConnection.conn` 改 `Option<Conn>`：所有 `self.conn.` 调用点需 `as_mut()`/`as_ref()`。
- 秒断后连接不可复用 → 前端处理「取消后自动重连」状态。
- **#24 前置依赖**：自动重连对 SSH 连接会重复 `start_tunnel`，而旧隧道因 #24（drop 只 detach 不 abort）泄漏——#24 必须与本方案同批（或先行）合入，否则取消越频繁泄漏越快；#24 落地前 SSH 连接取消后降级为手动重连 + 告警。
- **#62 更正**：#62（tokio `time` 传递启用）是 `dby-driver-mysql` **dev-deps** 缺 `time`（`Cargo.toml:19`），与 dby-core 增 `sync` 无关，需单独在驱动 dev-deps 显式加 `"time"`。
- `ExecOpts.cancel` 文档注释（`query.rs:17`）仍描述旧 drain 语义，随本方案更新。

## 8. 关联缺陷处置

- #21：4.1/4.2；#23：4.4；#5：4.5；#34：4.6；#24（前置）：§7。

## 9. 与其它方案组的依赖

- 回写共享契约 S1（`futures::lock::Mutex`）；是其余涉及 `commands.rs` 的方案（#22/#24/#28/#45/#48）的持锁范式基准。
- 与 #29（错误 kind）联动；#24 为同批前置。
