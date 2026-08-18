# #45 数值精度 — 实现计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `I64/U64` 与 `last_insert_id`/`id` 跨 IPC 以字符串承载，前端字符串渲染，消除 >2^53 精度丢失。

**Architecture:** 自定义 `Value` serde（I64/U64 `v` 为字符串）；`last_insert_id`/`id` 字符串化；前端 `i64/u64` 字符串。

**Tech Stack:** Rust（dby-core serde / src-tauri / dby-driver-mysql）、React 19/TS。

**Spec:** `docs/design/plans/45-numeric-precision/design.md`

## Global Constraints

- `I64/U64` 的 envelope `v` 必须为十进制字符串。
- 前端不得对 `i64/u64` 做 `Number()` 转换。
- CI 门禁：`cargo fmt --check`、`clippy -D warnings`、`cargo test -p dby-core -p dby-driver-mysql`、`pnpm build`。

---

### Task 1: `Value` 自定义 serde（I64/U64 字符串化）

**Files:**
- Modify: `crates/dby-core/src/value.rs`

**Interfaces:**
- Produces: `Value::I64/U64` 序列化 `{"t","v"}` 且 `v` 为字符串，往返无损

- [ ] **Step 1: 写失败测试**

```rust
#[test]
fn i64_serializes_as_string() {
    assert_eq!(serde_json::to_value(Value::I64(9223372036854775807)).unwrap(),
        serde_json::json!({"t":"i64","v":"9223372036854775807"}));
    let back: Value = serde_json::from_str("{\"t\":\"i64\",\"v\":\"9223372036854775807\"}").unwrap();
    assert_eq!(back, Value::I64(9223372036854775807));
    let u = Value::U64(18446744073709551615);
    assert_eq!(serde_json::to_value(&u).unwrap(), serde_json::json!({"t":"u64","v":"18446744073709551615"}));
}
```

- [ ] **Step 2: 运行确认失败**

Run: `cargo test -p dby-core value::tests::i64_serializes_as_string`
Expected: FAIL（当前 `v` 是 number）

- [ ] **Step 3: 最小实现**

按 design 4.1 手写 `Serialize`/`Deserialize`（`I64/U64` 内容字符串化；其余变体保持原 tag/content 语义）。更新既有 `serializes_as_tagged_envelope` 断言（`Value::I64(42)` 现为 `{"t":"i64","v":"42"}`）。

- [ ] **Step 4: 运行确认通过**

Run: `cargo test -p dby-core value::tests::`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/dby-core/src/value.rs
git commit -m "fix(core): serialize I64/U64 as strings to avoid >2^53 loss (#45)"
```

---

### Task 2: `to_json_value` 大整数输出字符串

**Files:**
- Modify: `crates/dby-core/src/value.rs`

- [ ] **Step 1: 写失败测试**

```rust
#[test]
fn to_json_value_keeps_bigint_as_string() {
    assert_eq!(Value::I64(9223372036854775807).to_json_value(), serde_json::json!("9223372036854775807"));
    assert_eq!(Value::U64(18446744073709551615).to_json_value(), serde_json::json!("18446744073709551615"));
}
```

- [ ] **Step 2: 运行确认失败**

Run: `cargo test -p dby-core value::tests::to_json_value_keeps_bigint_as_string`
Expected: FAIL（当前是 number）

- [ ] **Step 3: 最小实现**

`to_json_value` 的 `I64/U64` 分支改为 `serde_json::Value::String(i.to_string())`。

- [ ] **Step 4: 运行确认通过**

Run: `cargo test -p dby-core value::tests::to_json_value_keeps_bigint_as_string`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/dby-core/src/value.rs
git commit -m "fix(core): export bigint as string in JSON (#45)"
```

---

### Task 3: `last_insert_id` 字符串化（`id` 保持 number）

**Files:**
- Modify: `crates/dby-core/src/query.rs`（`QueryOutput.last_insert_id`、`StreamEvent::Affected.last_insert_id`、`CollectingSink.last_insert_id` → `Option<String>`）
- Modify: `crates/dby-driver-mysql/src/lib.rs`（`qr.last_insert_id().map(|v| v.to_string())`）

**Interfaces:**
- Produces: `last_insert_id: Option<String>`（`ConnectResponse.id`/`ConnectionSummary.id` **保持 `u64`**，是 `alloc_id()` 内部计数器、非 DB 值）

- [ ] **Step 1: 编译驱动**

Run: `cargo check`
Expected: FAIL（`last_insert_id` 类型变化暴露所有消费点）

- [ ] **Step 2: 最小实现**

逐处改 `QueryOutput`/`StreamEvent::Affected`/`CollectingSink` 的 `last_insert_id` 类型为 `Option<String>`；驱动 `qr.last_insert_id().map(|v| v.to_string())`。**不改** `ConnectResponse.id`/`ConnectionSummary.id`/`alloc_id()`。

- [ ] **Step 3: 运行确认通过**

Run: `cargo check` + `cargo test -p dby-core -p dby-driver-mysql`
Expected: PASS

- [ ] **Step 4: Commit**

```bash
git add crates/dby-core/src/query.rs crates/dby-driver-mysql/src/lib.rs
git commit -m "fix: stringify last_insert_id over IPC (#45)"
```

---

### Task 4: 前端字符串渲染

**Files:**
- Modify: `src/api.ts`（`CellValue.i64/u64.v: string`、`last_insert_id: string | null`）
- Modify: `src/App.tsx`（`displayCell` 直接返回 `v`）、`src/components/ResultsGrid.tsx`

**Interfaces:**
- Consumes: 字符串化的 `v`/`last_insert_id`（`ConnectResponse.id` 保持 number，前端 `workspaces: Record<number,…>` 不动）

- [ ] **Step 1: 写失败（TS 编译）**

Run: `pnpm build`
Expected: FAIL（`CellValue.i64/u64.v` 类型不匹配）

- [ ] **Step 2: 最小实现**

`displayCell` 对 `i64/u64` 返回 `v`（字符串）；`last_insert_id` 类型改 `string | null`；`toCellValue`（`App.tsx:14-20`）不再 `Number()`、改为按列类型提交（联动 #1）。`store.ts` 的 key 类型不动（`id` 仍 number）。

- [ ] **Step 3: 运行确认通过**

Run: `pnpm build`
Expected: PASS

- [ ] **Step 4: Commit**

```bash
git add src/api.ts src/App.tsx src/components/ResultsGrid.tsx src/store.ts
git commit -m "fix(frontend): render bigint as string, no Number() coercion (#45)"
```

---

### Task 5: 集成回归（`#[ignore]`）

**Files:**
- Create: `crates/dby-driver-mysql/tests/precision.rs`

**Interfaces:**
- Consumes: 字符串化 `Value`/`last_insert_id`

- [ ] **Step 1: 写集成测试**

```rust
#[tokio::test]
#[ignore]
async fn bigint_roundtrips_without_precision_loss() {
    // 建 BIGINT 主键表插入 9223372036854775807；SELECT 回读断言 Value::I64(9223372036854775807)；
    // 断言 last_insert_id 字符串等于该值
}
```

- [ ] **Step 2: 运行确认通过**

Run: `cargo test -p dby-driver-mysql --test precision -- --ignored`
Expected: PASS

- [ ] **Step 3: Commit**

```bash
git add crates/dby-driver-mysql/tests/precision.rs
git commit -m "test: bigint precision roundtrip (#45)"
```
