# #24 SSH 隧道生命周期 — 实现计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** SSH 隧道 drop 时完整释放（abort accept 循环 + 关闭 listener + 关闭会话 + abort 转发任务），direct-tcpip 错误不再被吞。

**Architecture:** `SshTunnel` 持 `CancellationToken`（#5 的 `cancelled()`）+ `JoinSet`；accept 循环用 `select!` 竞速取消；`Drop` cancel + abort；转发错误写 `last_error` 槽（`pub(crate)`）+ `log::warn!`，MySQL connect 失败时附带根因。

**Tech Stack:** Rust（russh / tokio / dby-core CancellationToken）。

**Spec:** `docs/design/plans/24-ssh-tunnel-lifecycle/design.md`

## Global Constraints

- `SshTunnel` drop 后不得残留 SSH 会话、本地端口、tokio 任务。
- direct-tcpip/转发错误必须可见（`log::warn!`）。
- CI 门禁：`cargo fmt --check`、`clippy -D warnings`、`cargo test -p dby-driver-mysql`。

---

### Task 1: `SshTunnel` 结构 + `Drop` 改造

**Files:**
- Modify: `crates/dby-driver-mysql/src/tunnel.rs`

**Interfaces:**
- Consumes: `dby_core::query::CancellationToken::cancelled()`（#5 同批先行；若 #5 未合入则内联 `tokio::sync::watch::Receiver<bool>` 等价的取消信号——**裸 `Notify` 有丢失唤醒，禁用**）
- Produces: `SshTunnel { local_port, cancel, _handle, task, forwards, pub(crate) last_error: Arc<Mutex<Option<String>>> }` + `impl Drop`

- [ ] **Step 1: 写失败测试（本地 listener + 注入 fake forward，无 sshd）**

```rust
#[tokio::test]
async fn drop_aborts_accept_loop_and_forwards() {
    // 本地 TcpListener + fake forward（记录调用）；drop SshTunnel 后断言 accept 任务结束、listener 释放、forwards 空
}
```

- [ ] **Step 2: 运行确认失败**

Run: `cargo test -p dby-driver-mysql tunnel::tests::drop_aborts_accept_loop_and_forwards`
Expected: FAIL（当前 `Drop` 只 detach）

- [ ] **Step 3: 最小实现**

按 design 4.1 改 `SshTunnel`，实现 `Drop`（cancel + abort accept 任务 + `forwards.abort_all()`）；`start_tunnel` 构造新字段。

- [ ] **Step 4: 运行确认通过**

Run: `cargo test -p dby-driver-mysql tunnel::tests::drop_aborts_accept_loop_and_forwards`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/dby-driver-mysql/src/tunnel.rs
git commit -m "fix(ssh): abort tunnel tasks and release resources on drop (#24)"
```

---

### Task 2: 可取消 accept 循环 + forward 抽象（boxed future，可测试）

**Files:**
- Modify: `crates/dby-driver-mysql/src/tunnel.rs`

**Interfaces:**
- Produces: `run_accept_loop(listener, cancel, forwards, forward: Arc<ForwardConn>)`；`type ForwardConn = dyn Fn(TcpStream) -> Pin<Box<dyn Future<Output=()> + Send + 'static>> + Send + Sync`

- [ ] **Step 1: 写失败测试（注入 fake forward，无 SSH Handle）**

```rust
#[tokio::test]
async fn accept_loop_exits_on_cancel() {
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
    let cancel = CancellationToken::new();
    let forwards = Arc::new(std::sync::Mutex::new(JoinSet::new()));
    let forward: Arc<ForwardConn> = Arc::new(|_socket| Box::pin(async {})); // fake
    let h = run_accept_loop(listener, cancel.clone(), forwards, forward);
    cancel.cancel();
    tokio::time::timeout(Duration::from_secs(1), h).await.unwrap().unwrap(); // 循环应退出
}
```

- [ ] **Step 2: 运行确认失败**

Run: `cargo test -p dby-driver-mysql tunnel::tests::accept_loop_exits_on_cancel`
Expected: FAIL（循环当前不响应取消）

- [ ] **Step 3: 最小实现**

按 design §4.2：`forward` 返回 **boxed future**（内部**不再** `tokio::spawn`，由 `forwards.lock().unwrap().spawn(forward(socket))` 恰好 spawn 一次——否则 `JoinSet::spawn(JoinHandle)` 双重 spawn：不编译（`Output` 不匹配）且 `abort_all` 只命中包装任务、真转发任务被 detach 泄漏）。`run_accept_loop` 用 `tokio::select!` 竞速 `cancel.cancelled()` 与 `listener.accept()`。

- [ ] **Step 4: 运行确认通过**

Run: `cargo test -p dby-driver-mysql tunnel::tests::accept_loop_exits_on_cancel`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/dby-driver-mysql/src/tunnel.rs
git commit -m "feat(ssh): cancellable accept loop with boxed-future forward abstraction (#24)"
```

---

### Task 3: direct-tcpip 错误回传（#36 后半，last_error 槽 + MySQL connect 附带）

**Files:**
- Modify: `crates/dby-driver-mysql/src/tunnel.rs`、`crates/dby-driver-mysql/src/lib.rs`（`MysqlDriver::connect` 失败读槽）
- Modify: `crates/dby-driver-mysql/Cargo.toml`（增 `log = "0.4"`）

**Interfaces:**
- Produces: forward 失败写 `last_error` 槽（`pub(crate)`，`lib.rs` 跨模块读）+ `log::warn!`；`MysqlDriver::connect` 的 `Conn::new` 失败时附带 `（SSH 转发失败：…）`

- [ ] **Step 1: 最小实现**

forward 任务内 `channel_open_direct_tcpip` 返回 `Err(e)` → `log::warn!("SSH direct-tcpip failed: {e}")` + `*last_error.lock().unwrap() = Some(e.to_string())`；`copy_bidirectional` 失败 `if let Err(e) = ... { log::warn!(...) }`（非 `let _`）。`MysqlDriver::connect` 的 `Conn::new(opts).await.map_err(...)` 内读 `ssh.last_error` 并拼进 `DbError::Database`。

> 注：当前壳层/驱动未接线 logger，`log::warn!` 暂为 no-op；**根因回传以 `last_error` 槽为主通道**。

- [ ] **Step 2: 运行确认通过**

Run: `cargo clippy --workspace --all-targets -- -D warnings`
Expected: PASS

- [ ] **Step 3: Commit**

```bash
git add crates/dby-driver-mysql/src/tunnel.rs crates/dby-driver-mysql/src/lib.rs crates/dby-driver-mysql/Cargo.toml
git commit -m "fix(ssh): surface direct-tcpip root cause via last_error slot (#36)"
```

---

### Task 4: 集成测试（`#[ignore]`）

**Files:**
- Create: `crates/dby-driver-mysql/tests/ssh_lifecycle.rs`

**Interfaces:**
- Consumes: `start_tunnel` / `SshTunnel` drop 语义

- [ ] **Step 1: 写集成测试**

```rust
#[tokio::test]
#[ignore] // 需真实 sshd
async fn repeated_connect_disconnect_does_not_leak() {
    // 循环 connect → drop 连接 N 次；断言本地端口可重新绑定、无残留
}
```

- [ ] **Step 2: 运行确认（起 sshd 后）**

Run: `cargo test -p dby-driver-mysql --test ssh_lifecycle -- --ignored --nocapture`
Expected: PASS

- [ ] **Step 3: Commit**

```bash
git add crates/dby-driver-mysql/tests/ssh_lifecycle.rs
git commit -m "test(ssh): lifecycle leak regression (#24)"
```
