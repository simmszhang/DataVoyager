# #28 多结果集协议 — 实现计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `StreamEvent` 增加 `ResultSetEnd/Truncated/Done/Error`；`CollectingSink` 分桶；驱动遍历全部结果集；channel 发终止事件且 send 失败取消。

**Architecture:** 显式边界/终止事件（S4）；`CollectingSink` 按 `ResultSetEnd` 分桶；驱动用 `columns()+next()` 遍历（**无 `next_result_set()`**，按列空/非空判别）；`ChannelSink` 持 cancel token。

**Tech Stack:** Rust（dby-core / dby-driver-mysql / src-tauri）、React 19/TS。

**Spec:** `docs/design/plans/28-multi-resultset/design.md`

## Global Constraints

- 流式终止事件（`Done`/`Error`）必须经 channel 发送，与 `invoke` 返回值解耦收尾。
- `channel.send` 失败必须触发取消（#42）。
- CI 门禁：`cargo fmt --check`、`clippy -D warnings`、`cargo test -p dby-core -p dby-driver-mysql`、`pnpm build`。

---

### Task 1: `StreamEvent` 扩展

**Files:**
- Modify: `crates/dby-core/src/query.rs`

**Interfaces:**
- Produces: `StreamEvent::{ResultSetEnd, Truncated, Done, Error{message}}`

- [ ] **Step 1: 写失败测试**

```rust
#[test]
fn result_set_end_and_done_serialize() {
    assert_eq!(serde_json::to_value(StreamEvent::ResultSetEnd).unwrap()["event"], "result_set_end");
    assert_eq!(serde_json::to_value(StreamEvent::Done).unwrap()["event"], "done"); // unit 变体无 data
    assert_eq!(serde_json::to_value(StreamEvent::Error{kind:"cancelled".into(), message:"x".into()}).unwrap()["data"]["message"], "x");
    assert_eq!(serde_json::to_value(StreamEvent::Error{kind:"cancelled".into(), message:"x".into()}).unwrap()["data"]["kind"], "cancelled");
}
```

- [ ] **Step 2: 运行确认失败**

Run: `cargo test -p dby-core query::tests::result_set_end_and_done_serialize`
Expected: FAIL（变体不存在）

- [ ] **Step 3: 最小实现**

按 design §4.1 加变体（`Done` 为 unit、`Error { kind: String, message: String }` 携带 kind 对齐 S5；serde `rename_all="snake_case"` 得到 `done`/`error`）。

- [ ] **Step 4: 运行确认通过**

Run: `cargo test -p dby-core query::tests::result_set_end_and_done_serialize`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/dby-core/src/query.rs
git commit -m "feat(core): add ResultSetEnd/Truncated/Done/Error stream events (#28)"
```

---

### Task 2: `CollectingSink` 分桶

**Files:**
- Modify: `crates/dby-core/src/query.rs`

**Interfaces:**
- Produces: `CollectingSink` 多结果集分桶 + `into_output` 结算残留

- [ ] **Step 1: 写失败测试**

```rust
#[test]
fn collecting_sink_buckets_multiple_result_sets() {
    let mut sink = CollectingSink::new(Some(100));
    sink.on_event(StreamEvent::Columns(vec![col("a")]));
    sink.on_event(StreamEvent::Rows(vec![vec![Value::I64(1)]]));
    sink.on_event(StreamEvent::ResultSetEnd);
    sink.on_event(StreamEvent::Columns(vec![col("b")]));
    sink.on_event(StreamEvent::Rows(vec![vec![Value::I64(2)]]));
    let out = sink.into_output();
    assert_eq!(out.result_sets.len(), 2);
    assert_eq!(out.result_sets[0].columns[0].name, "a");
    assert_eq!(out.result_sets[1].columns[0].name, "b");
}
```

- [ ] **Step 2: 运行确认失败**

Run: `cargo test -p dby-core query::tests::collecting_sink_buckets_multiple_result_sets`
Expected: FAIL（当前只产 1 组）

- [ ] **Step 3: 最小实现**

按 design 4.2 改造 `CollectingSink`（`result_sets: Vec<ResultSet>` + `current` builder + `Truncated/ResultSetEnd` 处理）。

- [ ] **Step 4: 运行确认通过**

Run: `cargo test -p dby-core query::tests::collecting_sink_*`
Expected: PASS（含既有 truncate 用例）

- [ ] **Step 5: Commit**

```bash
git add crates/dby-core/src/query.rs
git commit -m "feat(core): bucket multi-result-set into QueryOutput (#28)"
```

---

### Task 3: 驱动遍历多结果集（columns 空/非空判别）

**Files:**
- Modify: `crates/dby-driver-mysql/src/lib.rs`（`execute_stream`）

**Interfaces:**
- Consumes: `StreamEvent::ResultSetEnd`（Task 1）
- Produces: `loop` 遍历，`columns()` 空/非空判别，`None` 分支 `next()?` 冒中途错误

- [ ] **Step 1: 写失败测试（集成 `#[ignore]`）**

```rust
#[tokio::test]
#[ignore]
async fn call_procedure_yields_two_result_sets() {
    // 建存储过程返回 2 个 SELECT；execute_buffered 后断言 result_sets.len()==2
}

#[tokio::test]
#[ignore]
async fn consecutive_dml_yields_both_affected() {
    // "UPDATE t SET x=1 WHERE id=1; UPDATE t SET x=2 WHERE id=2;" → 两个 Affected 都发出（不丢第二集）
}

#[tokio::test]
#[ignore]
async fn mid_stream_error_is_surfaced() {
    // "SELECT 1; BLABLA;" → 第二集 server 错误经 next()? 冒给调用方（不静默吞）
}
```

- [ ] **Step 2: 运行确认失败**

Run: `cargo test -p dby-driver-mysql --test mysql_integration -- --ignored call_procedure_yields_two_result_sets`
Expected: FAIL（当前只读首个）

- [ ] **Step 3: 最小实现**

按 design §4.3（**mysql_async 0.37 无 `next_result_set()`**；DML 的 `columns()` 返回 `Some(空列)`（`helpers.rs:69-74`），且空列集 `next()` 也推进）：

```rust
let mut qr = self.conn.query_iter(sql).await.map_err(db_err)?;
loop {
    match qr.columns() {
        Some(cols) if !cols.is_empty() => {
            // SELECT：发 Columns + 分批 Rows，集尾 next() 自动 next_set
            // ...（发 ResultSetEnd）
        }
        Some(_) => {
            // DML（空列）：发 Affected，再 next() 推进（连续 DML 不丢集）
            sink.on_event(StreamEvent::Affected { affected_rows: qr.affected_rows(), last_insert_id: qr.last_insert_id() });
            qr.next().await.map_err(db_err)?;
        }
        None => {
            // 无 pending；可能隐藏中途 server 错误，next()? 冒错后 break
            qr.next().await.map_err(db_err)?;
            break;
        }
    }
}
```

- [ ] **Step 4: 运行确认通过**

Run: 同上三用例
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/dby-driver-mysql/src/lib.rs
git commit -m "feat(mysql): iterate all result sets via columns/next discrimination (#28)"
```

---

### Task 4: `ChannelSink` 终止事件 + send 失败取消（#42）

**Files:**
- Modify: `src-tauri/src/commands.rs`（`ChannelSink` + `execute_query_stream`）
- Modify: `src/App.tsx`、`src/api.ts`（处理新事件）

**Interfaces:**
- Consumes: `CancellationToken`（#5）、`StreamEvent::{Done,Error}`（Task 1）

- [ ] **Step 1: 写失败测试（单元）**

```rust
#[test]
fn channel_sink_cancels_on_send_failure() {
    // 用一个已关闭的 Channel 或 mock send 返回 Err，断言 cancel token 被置位
}
```

- [ ] **Step 2: 运行确认失败**

Run: `cargo test -p dby --bin dby channel_sink_cancels_on_send_failure`
Expected: FAIL

- [ ] **Step 3: 最小实现**

按 design 4.4：`ChannelSink` 持 `cancel`；`send` 失败即 cancel；`execute_query_stream` 末尾发 `Done`/`Error`。前端 `onmessage` 覆盖 `result_set_end/truncated/done/error`。

- [ ] **Step 4: 运行确认通过**

Run: `cargo test` + `pnpm build`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/commands.rs src/App.tsx src/api.ts
git commit -m "feat(shell): terminal stream events + cancel on channel close (#42)"
```

---

### Task 5: 前端多结果集展示（最小落地）

**Files:**
- Modify: `src/App.tsx`、`src/components/ResultsGrid.tsx`、`src/store.ts`

**Interfaces:**
- Consumes: `done/error/result_set_end/truncated`

- [ ] **Step 1: 最小实现**

流式结果暂只展示第一组（后续结果集在 `StreamResult` 增 `result_sets` 后切换）；本任务保证 `truncated` 落到 UI、`done` 复位 `running`、`error` 落 `ws.error`。

- [ ] **Step 2: 运行确认通过**

Run: `pnpm build`
Expected: PASS

- [ ] **Step 3: Commit**

```bash
git add src/App.tsx src/components/ResultsGrid.tsx src/store.ts
git commit -m "feat(frontend): handle terminal/truncated stream events (#28/#42)"
```
