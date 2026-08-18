# #24 SSH 隧道生命周期 — 设计文档

> 状态：评审有条件通过（3 阻断项已修订，待复审） · 优先级 P1 · 规模：大 · 关联缺陷：#36 后半（direct-tcpip 失败被吞）· 依赖共享契约：S5（错误形状）、#5（`CancellationToken::cancelled()`）

## 1. 现状与影响

- `tunnel.rs:62-85`：accept 循环是 `tokio::spawn` 任务，闭包持有 `listener` 与 `Arc<Handle>`；`SshTunnel` drop 时只 drop 了 `JoinHandle`（**detach，不 abort**）和自己的 `Arc` 克隆，但循环任务仍持最后的 `Arc<Handle>` + `listener`，`accept()` 永不返回 Err，循环永不退出（review D7，🔴）。
- `tunnel.rs:70-83`：每个转发连接又 `tokio::spawn` 一个任务，`channel_open_direct_tcpip(...).await?` 的错误随 `JoinHandle` 被丢弃（`tunnel.rs:78-79`），目标 MySQL 不可达时只见通用错误、看不到根因（#36 后半，review D8）。
- **影响**：每次「连接→断开」泄漏 1 个 SSH 会话 + 1 个本地临时端口 + 1 个 tokio 任务；反复重连耗尽端口/FD/内存；direct-tcpip 失败根因不可见。

## 2. 目标与成功标准

1. `SshTunnel` drop 时：abort accept 循环、关闭 listener、关闭 SSH 会话、abort 全部 per-forward 任务。
2. accept 循环可被取消信号唤醒退出（不依赖 `accept()` 返回 Err）。
3. direct-tcpip 内层错误**被回传并附带到连接失败**（非仅 log）。
4. accept 循环可脱离真实 `Handle` 单测（可注入 forward 抽象）。
5. 成功标准：反复「连接→断开」不泄漏端口/任务/会话；断开后端口立即释放；目标 MySQL 不可达时连接错误含 direct-tcpip 根因。

## 3. 方案对比

### 方案 A：`CancellationToken` + `JoinSet` + 显式 `Drop` + 错误槽（推荐）
- accept 循环 `tokio::select!` 竞速 `accept()` 与取消；per-forward 任务收进 `JoinSet`；`Drop` cancel + abort + 释放 `Arc<Handle>`；direct-tcpip 失败写入共享错误槽供 MySQL 连接失败附带根因。
- **优点**：彻底可取消 + 根因可见。**缺点**：需改 `SshTunnel` 结构。

### 方案 B：`Notify` + 逐任务 JoinHandle 列表
- **缺点**：需手动锁 Vec，`JoinSet` 更省心。

### 方案 C：仅 abort accept 循环，不追 per-forward
- **缺点**：forward 任务仍持 `Arc<Handle>`，SSH 会话不释放，否决。

**推荐 A**。

## 4. 推荐方案详细设计

### 4.1 结构改造（`crates/dby-driver-mysql/src/tunnel.rs`）

```rust
pub struct SshTunnel {
    pub local_port: u16,
    cancel: dby_core::query::CancellationToken,
    _handle: Arc<russh::client::Handle<ClientHandler>>,
    task: tokio::task::JoinHandle<()>,
    forwards: Arc<std::sync::Mutex<tokio::task::JoinSet<()>>>,
    pub(crate) last_error: Arc<std::sync::Mutex<Option<String>>>, // direct-tcpip 首个失败根因（lib.rs 跨模块读取，须 pub(crate)）
}

impl Drop for SshTunnel {
    fn drop(&mut self) {
        self.cancel.cancel();          // 唤醒 accept 循环
        self.task.abort();             // 停止 accept 循环（连同 listener 一起 drop）
        if let Ok(mut f) = self.forwards.lock() { f.abort_all(); }
        // _handle 的 Arc 随结构 drop 释放 → 关闭 SSH 会话
    }
}
```

> `cancel.cancelled()` 依赖 #5 的 `CancellationToken::cancelled()`（`tokio::sync::watch` 实现）；若 #5 未落地，本方案内联 `tokio::sync::watch::Receiver<bool>` 等价的取消信号（见 §9）。

### 4.2 可取消 accept 循环 + forward 抽象（可测试）

把「转发一个连接」抽象为闭包，使 `run_accept_loop` 无需真实 `Handle` 即可单测：

```rust
use std::future::Future;

type ForwardConn = dyn Fn(tokio::net::TcpStream)
    -> std::pin::Pin<Box<dyn Future<Output = ()> + Send + 'static>> + Send + Sync;

fn run_accept_loop(
    listener: tokio::net::TcpListener,
    cancel: dby_core::query::CancellationToken,
    forwards: Arc<std::sync::Mutex<tokio::task::JoinSet<()>>>,
    forward: Arc<ForwardConn>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        loop {
            tokio::select! {
                biased;
                _ = cancel.cancelled() => break,
                accepted = listener.accept() => match accepted {
                    Ok((socket, _)) => {
                        forwards.lock().unwrap().spawn(forward(socket)); // forward 返回 boxed future，由 JoinSet 真正 spawn 一次
                    }
                    Err(_) => break,
                },
            }
        }
    })
}
```

真实 `forward`（`start_tunnel` 内构造）：

```rust
let forward: Arc<ForwardConn> = Arc::new(move |mut socket| {
    let h2 = handle.clone();
    let th = target_host.clone();
    let last_error = last_error.clone();
    Box::pin(async move {                 // 不内部 tokio::spawn：由 JoinSet::spawn 恰好 spawn 一次
        match h2.channel_open_direct_tcpip(th, target_port as u32, "127.0.0.1".to_string(), 0).await {
            Ok(channel) => {
                let mut stream = channel.into_stream();
                if let Err(e) = tokio::io::copy_bidirectional(&mut socket, &mut stream).await {
                    log::warn!("SSH 转发中断: {e}"); // 与 §5 一致，不静默
                }
            }
            Err(e) => {
                log::warn!("SSH direct-tcpip failed: {e}");
                *last_error.lock().unwrap() = Some(e.to_string());
            }
        }
    })
});
```

> **为何 forward 返回 boxed future 而非内部 `tokio::spawn`**：若 forward 内部 `tokio::spawn` 出真正的转发任务并返回 `JoinHandle`，`JoinSet::spawn(JoinHandle)` 会**再包一层任务**（且 `JoinHandle::Output=Result<(),JoinError>` 与 `JoinSet<()>::spawn` 的 `Output=()` 不匹配、不编译）；`Drop` 的 `abort_all()` 只 abort 包装任务，真正的转发任务被 detach、继续持有 `Arc<Handle>` → SSH 会话照旧泄漏。返回 boxed future 让 `JoinSet` 直接 spawn 转发任务本体，`abort_all` 才能命中并中止它。

### 4.3 direct-tcpip 错误回传（#36 后半，非仅 log）

`SshTunnel.last_error` 槽记录首个 direct-tcpip 失败根因；`MysqlDriver::connect`（`lib.rs:84-90`）在 `Conn::new(opts)` 连 `127.0.0.1:local_port` 失败时，读 `ssh.last_error` 并附带到返回的 `DbError`：

```rust
let conn = Conn::new(opts).await.map_err(|e| {
    let root = ssh.as_ref()
        .and_then(|t| t.last_error.lock().unwrap().clone())
        .map(|r| format!("（SSH 转发失败：{r}）"))
        .unwrap_or_default();
    DbError::Database(format!("连接 MySQL 失败：{e}{root}"))
})?;
```

> 尽力而为：forward 任务通常先于 `Conn::new` 失败写入槽；若竞态下槽尚未填充，退化为通用错误 + `log::warn`（已在 forward 任务内记录）。

## 5. 错误处理（遵循 S5）

- direct-tcpip 失败：`log::warn!` + 写 `last_error` 槽；MySQL 连接失败附带根因（§4.3）。
- `copy_bidirectional` 失败：`log::warn!`（正常断开，不附带）。
- 取消/断开：静默释放。

## 6. 测试策略

- **单元**（`run_accept_loop` + 本地 `TcpListener` + 注入 fake `forward`，无需 sshd/`Handle`）：`cancel()` 后循环退出、`listener` 释放；fake forward 失败写入 `last_error`；drop `SshTunnel` 后 `forwards` 为空（`abort_all` 生效）。
- **集成**（`#[ignore]`，需真实 sshd）：反复「连接→断开」N 次，断言端口不泄漏、SSH 会话计数不增长；目标 MySQL 不可达时连接错误含 direct-tcpip 根因。

## 7. 回归风险与影响面

- **依赖新增**：`dby-driver-mysql` 当前无 `log` 依赖（`Cargo.toml:9-16`），本方案 `log::warn!` 需新增 `log = "0.4"`。**当前壳层/驱动均未接线 logger，`log::warn!` 暂为 no-op**；根因回传以 `last_error` 槽为准（主通道），`log::warn!` 仅作未来接线 logger 后的补充。
- `SshTunnel` 字段变化：`MysqlConnection._ssh` 仅 drop 语义变化 + 新增 `last_error` 读取点（`connect` 失败路径）。
- 与 #9 共享 `tunnel.rs`：本方案只改「accept 循环 + Drop + forward 错误 + forward 抽象」，不改 handler/认证/超时（#9 负责）。
- `forward` 闭包抽象引入 `Arc<dyn Fn>` 间接层（仅测试需要，生产单路）。

## 8. 关联缺陷处置

- #24：4.1/4.2；#36 后半：4.3 direct-tcpip 根因回传（前半连接超时归 #9）。

## 9. 与其它方案组的依赖

- 复用 #5 的 `CancellationToken`（`dby_core::query`，含 `cancelled()`）；若 #5 未合入，本方案内联等价 `tokio::sync::watch` 取消信号。
- **合批排序**：#24 依赖 #5 的 `cancelled()`；#5 的自动重连又依赖 #24 防隧道泄漏——两者**必须同批合入**（#5 先行落地 `CancellationToken::cancelled()`，#24 紧随）；`start_tunnel`/`SshTunnel` 的合并形态（#9 的 handler + #24 的取消/Drop）以 #9 定稿为基准、本方案叠加。
- 与 #9 共享 `tunnel.rs`（改动行互不重叠）；依赖 S5。
