# #48 CSP + ACL 门控 — 实现计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 启用 CSP、破坏性命令服务端二次确认、危险分析升级 tokenizer、Warn 级落地，收敛 XSS 攻击面。

**Architecture:** `tauri.conf.json` 显式 CSP；壳层 `guard_dangerous` 对破坏性命令复检（**含任意 SQL 的 `execute_query`/`execute_query_stream`/`export_result`**）；`danger.rs` 状态机跳过字符串/注释；分级：Dangerous = DROP/TRUNCATE/ALTER/RENAME + DELETE 无 WHERE；Warn = UPDATE 无 WHERE。

**Tech Stack:** Rust（dby-core danger / src-tauri）、Tauri 2、React 19/TS。

**Spec:** `docs/design/plans/48-csp-acl/design.md`

## Global Constraints

- CSP 一旦启用，`script-src 'self'` 不允许引入 `unsafe-eval`（CodeMirror 6 无需）。
- `analyze_danger` 分级固定：`Dangerous` = DROP/TRUNCATE/ALTER/RENAME（**RENAME 需新增到关键词清单**，现 `danger.rs:35-48` 无）+ **DELETE 无 WHERE**；`Warn` = **UPDATE 无 WHERE**；`Safe` = 其余。
- 错误形状遵循 S5。
- CI 门禁：`cargo fmt --check`、`clippy -D warnings`、`cargo test -p dby-core`、`pnpm build`。

---

### Task 1: 启用 CSP

**Files:**
- Modify: `src-tauri/tauri.conf.json`

- [ ] **Step 1: 写配置**

```json
"security": {
  "csp": "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; font-src 'self'; connect-src 'self' ipc: http://ipc.localhost"
}
```

- [ ] **Step 2: 运行确认（dev 实测）**

Run: `pnpm tauri dev`
Expected: 连接/查询/流式/导出全链路无 CSP 报错（若 IPC origin 与版本不符，按 design 4.1 微调 `connect-src`）

- [ ] **Step 3: Commit**

```bash
git add src-tauri/tauri.conf.json
git commit -m "security: enable explicit CSP (#48)"
```

---

### Task 2: 危险分析 tokenizer（#12）

**Files:**
- Modify: `crates/dby-core/src/danger.rs`

**Interfaces:**
- Produces: `analyze_danger(sql) -> DangerLevel`（跳过字符串/注释；Warn 承载 DELETE/UPDATE 无 WHERE）

- [ ] **Step 1: 写失败测试**

```rust
#[test]
fn keywords_inside_strings_and_comments_are_ignored() {
    assert_eq!(analyze_danger("SELECT 'drop' FROM t"), DangerLevel::Safe);
    assert_eq!(analyze_danger("SELECT 'delete from x'"), DangerLevel::Safe);
    assert_eq!(analyze_danger("SELECT 1 /* drop table t */"), DangerLevel::Safe);
    assert_eq!(analyze_danger("SELECT 1 -- drop\n"), DangerLevel::Safe);
    assert!(analyze_danger("DROP TABLE t").is_dangerous());
}

#[test]
fn delete_without_where_stays_dangerous_update_is_warn() {
    assert!(analyze_danger("DELETE FROM users").is_dangerous());  // 不可逆数据删除：不降级
    assert_eq!(analyze_danger("UPDATE users SET x = 1"), DangerLevel::Warn);
    assert_eq!(analyze_danger("DELETE FROM users WHERE id = 1"), DangerLevel::Safe);
    assert!(analyze_danger("RENAME TABLE a TO b").is_dangerous()); // RENAME 需新增到关键词清单
}
```

- [ ] **Step 2: 运行确认失败**

Run: `cargo test -p dby-core danger::tests::keywords_inside_strings_and_comments_are_ignored`
Expected: FAIL（当前 `SELECT 'drop'` 被判 dangerous；`DELETE` 无 WHERE 判 Dangerous 而非 Warn）

- [ ] **Step 3: 最小实现**

在 `analyze_danger` 内，对每个 `stmt` 用状态机扫描：跟踪 `in_single/in_double/in_backtick/in_line_comment/in_block_comment`，仅在外层按词边界匹配关键词；**关键词清单需新增 `RENAME`**（现 `danger.rs:35-48` 仅有 DROP/TRUNCATE/ALTER/DELETE/UPDATE，否则 rename_table 门控 no-op）；`DROP/TRUNCATE/ALTER/RENAME` → Dangerous；`DELETE 无 WHERE` → Dangerous、`UPDATE 无 WHERE` → Warn；输出 `Dangerous(reasons)`/`Warn`/`Safe`。**既有测试反转**：`danger.rs:99-100` 现在自证 `SELECT 'drop'` 被判危险，须反转为 Safe。

- [ ] **Step 4: 运行确认通过**

Run: `cargo test -p dby-core danger::tests::`
Expected: 全部 PASS（含既有 `drop_and_truncate_are_dangerous`、`delete_update_without_where_is_dangerous` 需改为断言 Warn——同步修正既有断言）

- [ ] **Step 5: Commit**

```bash
git add crates/dby-core/src/danger.rs
git commit -m "feat(core): tokenizer-aware danger analysis + warn level (#12/#51)"
```

---

### Task 3: 服务端 `guard_dangerous` + 破坏性命令 `confirmed`

**Files:**
- Modify: `src-tauri/src/commands.rs`（`guard_dangerous` + **`execute_query/execute_query_stream/export_result/drop_database/drop_table/rename_table/delete_project`** 加 `confirmed`；**不**给 `execute_edit`——它恒带 WHERE、danger 复检是 no-op，门控归 #26）
- Modify: `src-tauri/src/lib.rs`（无新命令，签名变更）
- Modify: `src/api.ts`、`src/App.tsx`（传 `confirmed`）

**Interfaces:**
- Produces: `fn guard_dangerous(sql: &str, confirmed: bool) -> Result<()>`

- [ ] **Step 1: 写失败测试**

```rust
#[test]
fn guard_rejects_dangerous_without_confirm() {
    assert!(guard_dangerous("DROP TABLE t", false).is_err());
    assert!(guard_dangerous("DROP TABLE t", true).is_ok());
    assert!(guard_dangerous("SELECT 1", false).is_ok());
    assert!(guard_dangerous("UPDATE t SET x=1", false).is_ok()); // Warn 非 Dangerous，走前端提示
}
```

- [ ] **Step 2: 运行确认失败**

Run: `cargo test -p dby --bin dby guard_rejects_dangerous_without_confirm`
Expected: FAIL（函数不存在）

- [ ] **Step 3: 最小实现**

按 design §4.2 实现 `guard_dangerous`；门控范围：**任意 SQL 路径** `execute_query`/`execute_query_stream`（手打 SQL，`confirmed` 前端确认后传 true）、`export_result`（服务端重跑任意 SQL，同样拦截）、schema 命令 `drop_database/drop_table/rename_table`、非 SQL 破坏命令 `delete_project`；命令签名加 `confirmed: bool`。前端确认弹窗通过后传 `true`，`App.tsx`/`api.ts` 相应调用点更新。

- [ ] **Step 4: 运行确认通过**

Run: `cargo test` + `pnpm build`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/commands.rs src/api.ts src/App.tsx
git commit -m "security: server-side danger recheck + confirmed flag on destructive commands (#48)"
```

---

### Task 4: 前端 Warn 处置（#51）

**Files:**
- Modify: `src/App.tsx`（`handleRun` 分支）

**Interfaces:**
- Consumes: `api.analyzeDanger` 返回 `{level:"warn"} | {level:"dangerous",reasons} | {level:"safe"}`

- [ ] **Step 1: 写失败测试（手工验证项，无自动化）**

`handleRun` 分支伪代码：

```ts
if (danger.level === "dangerous") { setPendingDanger({ sql, reasons: danger.reasons }); return; }
if (danger.level === "warn") { setPendingWarn({ sql }); return; } // 轻量确认
await runQuery(activeId, sql);
```

- [ ] **Step 2: 运行确认失败**

Run: `pnpm build`
Expected: 先确认 `api.ts` 的 `DangerLevel` 类型含 `{level:"warn"}`（已含），再实现分支

- [ ] **Step 3: 最小实现**

新增 `pendingWarn` 状态 + 轻量确认弹窗（文案「缺少 WHERE，可能影响大量行」），确认后 `runQuery`。

- [ ] **Step 4: 运行确认通过**

Run: `pnpm build`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add src/App.tsx
git commit -m "fix(frontend): handle warn-level danger with lightweight confirm (#51)"
```

---

### Task 5: 脱敏回归验证 + XSS 冒烟

**Files:**
- 无源码改动（依赖 #22 的 `list_saved_connections` 脱敏）

- [ ] **Step 1: 验证 list_saved_connections 返回体无 secret**

Run: `pnpm tauri dev` → 连接一个含 SSH 的库 → 前端 console 打印 `await api.listSavedConnections(null)`，断言无 `password`/`private_key` 字段。

- [ ] **Step 2: 验证 CSP 阻断注入**

在编辑器粘贴并运行无害的 `<img src=x onerror=alert(1)>` 场景，或用 devtools 注入 `fetch` 越权命令；确认被 CSP 阻断、无命令越权。

- [ ] **Step 3: Commit（如无改动则跳过）**

```bash
git add -A && git commit -m "test: verify CSP and credential sanitization (#48/#22)"
```
