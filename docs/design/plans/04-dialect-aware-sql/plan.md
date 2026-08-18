# #4 方言感知 SQL 生成 — 实现计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 前端不再生成 SQL；表浏览 SQL 由 `Dialect` 在 dby-core 生成；节点 key 结构化；`split_statements` 并入 `Dialect`；元数据 SQL 参数化。

**Architecture:** `build_table_select(dialect, table, limit)` 收口 dby-core + 壳层命令；节点 key 用 JSON 编码；`Dialect::split_statements` 默认实现。

**Tech Stack:** Rust（dby-core / src-tauri / dby-driver-mysql）、React 19/TS。

**Spec:** `docs/design/plans/04-dialect-aware-sql/design.md`

## Global Constraints

- 前端禁止硬编码任何 SQL 引号/`LIMIT`（AGENTS.md 铁律）。
- SQL 生成走 `Dialect.quote_identifier`/`limit_clause`。
- CI 门禁：`cargo fmt --check`、`clippy -D warnings`、`cargo test -p dby-core`、`pnpm build`。

---

### Task 1: `build_table_select` + 单测

**Files:**
- Modify: `crates/dby-core/src/query.rs`（或新 `browse.rs`）

**Interfaces:**
- Produces: `pub fn build_table_select(dialect: &dyn Dialect, table: &str, limit: Option<u64>) -> String`

- [ ] **Step 1: 写失败测试**

```rust
#[test]
fn build_table_select_uses_dialect_quoting() {
    struct Pg; // 双引号方言
    impl Dialect for Pg { /* quote_identifier -> "x"; limit_clause -> LIMIT n */ }
    assert_eq!(build_table_select(&Pg, "users", Some(100)), "SELECT * FROM \"users\" LIMIT 100");
    assert_eq!(build_table_select(&MysqlDialect, "users", Some(100)), "SELECT * FROM `users` LIMIT 100");
}
```

- [ ] **Step 2: 运行确认失败**

Run: `cargo test -p dby-core query::tests::build_table_select_uses_dialect_quoting`
Expected: FAIL（函数不存在）

- [ ] **Step 3: 最小实现**

按 design 4.1 实现。

- [ ] **Step 4: 运行确认通过**

Run: `cargo test -p dby-core query::tests::build_table_select_uses_dialect_quoting`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/dby-core/src/query.rs
git commit -m "feat(core): dialect-aware table-browse SQL builder (#4)"
```

---

### Task 2: 壳层命令 + 前端接入

**Files:**
- Modify: `src-tauri/src/commands.rs`（`build_table_select`）、`src-tauri/src/lib.rs`（注册）
- Modify: `src/api.ts`、`src/App.tsx`（`handleOpenTable` 改调命令）

**Interfaces:**
- Consumes: `build_table_select`（Task 1）
- Produces: `api.buildTableSelect(id, table): Promise<string>`

- [ ] **Step 1: 写失败（编译）**

`handleOpenTable` 改为 async 调 `api.buildTableSelect`；先写 `api.ts` 的封装。

- [ ] **Step 2: 运行确认失败**

Run: `pnpm build`
Expected: FAIL（命令未注册）

- [ ] **Step 3: 最小实现**

壳层命令按 S1 范式（`std::sync::Mutex` 外层 + `futures::lock::Mutex` per-connection）：

```rust
#[tauri::command]
pub async fn build_table_select(state: State<'_, Arc<AppState>>, id: u64, table: String) -> Result<String> {
    let entry = state.connections.lock().unwrap().get(&id).cloned()
        .ok_or_else(|| DbError::ConnectionNotFound(id.to_string()))?;
    let active = entry.lock().await; // 只读 driver_id
    let driver = state.registry.resolve(&active.driver_id)?;
    Ok(dby_core::query::build_table_select(driver.dialect(), &table, Some(100)))
}
```

前端 `handleOpenTable` async 调用 `api.buildTableSelect(connId, table)` 填入 `query`（加 `try/catch`）。

- [ ] **Step 4: 运行确认通过**

Run: `cargo test` + `pnpm build`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/commands.rs src-tauri/src/lib.rs src/api.ts src/App.tsx
git commit -m "feat(shell): route table-browse SQL generation through dialect (#4)"
```

---

### Task 3: 节点 key 结构化（#3）

**Files:**
- Modify: `src/components/SchemaTree.tsx`

**Interfaces:**
- Produces: `NodeKey`（union）+ `keyOf`/`parseKey`（JSON 编码）

- [ ] **Step 1: 最小实现**

按 design 4.2 把 `c:5`/`d:5:库`/`t:5:库:表` 替换为 `JSON.stringify(NodeKey)`；节点点击回调 `JSON.parse` 解构。

- [ ] **Step 2: 运行确认通过**

Run: `pnpm build`
Expected: PASS（树展开/点表仍正常）

- [ ] **Step 3: Commit**

```bash
git add src/components/SchemaTree.tsx
git commit -m "fix(frontend): structured tree node keys instead of colon-join (#3)"
```

---

### Task 4: `split_statements` 并入 `Dialect`（#59）

**Files:**
- Modify: `crates/dby-core/src/dialect.rs`（trait 增 `split_statements` 默认实现 + 自由函数改薄封装）

**Interfaces:**
- Produces: `Dialect::split_statements(&self, sql) -> Vec<&str>`（默认实现 = `generic_split_statements`）；自由函数 `split_statements(sql)` 保留为薄封装（委托 `generic_split_statements`）

> **边界**：**不改 `analyze_danger` 签名**（`danger.rs:29` 仍是 `sql: &str`）——方言化切分推迟到未来与 #48 的 tokenizer 协调；本方案只交付 `Dialect::split_statements` 能力。

- [ ] **Step 1: 写失败测试**

```rust
#[test]
fn dialect_split_statements_default_matches_free_fn() {
    assert_eq!(TestDialect.split_statements("SELECT 'a;b'; SELECT 1"), vec!["SELECT 'a;b'", "SELECT 1"]);
    assert_eq!(split_statements("SELECT 'a;b'; SELECT 1"), vec!["SELECT 'a;b'", "SELECT 1"]); // 薄封装同源
}
```

- [ ] **Step 2: 运行确认失败**

Run: `cargo test -p dby-core dialect::tests::dialect_split_statements_default_matches_free_fn`
Expected: FAIL

- [ ] **Step 3: 最小实现**

把现有 `split_statements` 逻辑抽为 `generic_split_statements`；`Dialect::split_statements` 默认实现委托它；自由函数 `split_statements(sql)` 改为薄封装（委托 `generic_split_statements`），`danger.rs` 等既有调用点无感。未来 Postgres 驱动可覆盖 trait 方法。

- [ ] **Step 4: 运行确认通过**

Run: `cargo test -p dby-core` + `cargo check`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/dby-core/src/dialect.rs
git commit -m "refactor(core): move split_statements into Dialect default method (#59)"
```

---

### Task 5: 元数据 SQL 参数化标注（#61）

**Files:**
- Modify: `crates/dby-driver-mysql/src/lib.rs`（`indexes`/`table_ddl` 注释标注例外）

**Interfaces:**
- 无签名变化

- [ ] **Step 1: 最小实现**

`indexes`/`table_ddl` 保留 `quote_identifier` 拼接，加注释 `// 非参数化例外：SHOW INDEX/SHOW CREATE TABLE 不支持占位符；标识符已 quote_identifier 转义`。

- [ ] **Step 2: 运行确认通过**

Run: `cargo clippy --workspace --all-targets -- -D warnings`
Expected: PASS

- [ ] **Step 3: Commit**

```bash
git add crates/dby-driver-mysql/src/lib.rs
git commit -m "docs(mysql): mark non-parameterizable metadata SQL exceptions (#61)"
```
