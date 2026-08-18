# #22 SSH 凭据存储 — 实现计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** SSH 密码/私钥只进 OS 钥匙串、`list_saved_connections` 脱敏、按需持久化、级联清理、补更新命令，消除明文落盘与前端泄漏。

**Architecture:** `SshOptions` 加 `skip_serializing`；keyring 三键（`{id}`/`{id}:ssh`/`{id}:ssh_key`）；新增 `SavedConnectionView` 脱敏视图；`connect` 增 `save`/`remember_password`；级联清理 + `update_saved_connection`。

**Tech Stack:** Rust（dby-core / src-tauri / keyring / serde）、React 19/TS。

**Spec:** `docs/design/plans/22-ssh-credential-storage/design.md`

## Global Constraints

- secret 一律不落 `config.json`、不进任何 IPC 返回体。
- keyring key 约定固定：`{config_id}`（MySQL）、`{config_id}:ssh`（SSH 密码）、`{config_id}:ssh_key`（SSH 私钥）。
- 写错误非静默：`log::warn!` + 需要时返回 `DbError::Storage`，不阻断连接成功。
- CI 门禁：`cargo fmt --check`、`clippy -D warnings`、`cargo test -p dby-core`、`pnpm build`。

---

### Task 1: `SshOptions` 加 `skip_serializing` + 单测

**Files:**
- Modify: `crates/dby-core/src/driver.rs`

**Interfaces:**
- Produces: `SshOptions.password/private_key` 序列化时被省略

- [ ] **Step 1: 写失败测试**

```rust
#[test]
fn ssh_options_never_serialize_secrets() {
    let s = SshOptions { enabled: true, host: "h".into(), port: 22, user: "u".into(),
        password: Some("pw".into()), private_key: Some("k".into()), host_key_fingerprint: None };
    let json = serde_json::to_string(&s).unwrap();
    assert!(!json.contains("password"));
    assert!(!json.contains("private_key"));
    assert!(!json.contains("pw"));
    let back: SshOptions = serde_json::from_str(&json).unwrap();
    assert_eq!(back.password, None); // 反序列化默认 None（secret 不落盘后读不到）
}
```

- [ ] **Step 2: 运行确认失败**

Run: `cargo test -p dby-core driver::tests::ssh_options_never_serialize_secrets`
Expected: FAIL（当前序列化会带出 password/private_key）

- [ ] **Step 3: 最小实现**

在 `SshOptions` 的 `password`、`private_key` 字段上加 `#[serde(default, skip_serializing)]`（保留 `Deserialize`）。

- [ ] **Step 4: 运行确认通过**

Run: `cargo test -p dby-core driver::tests::ssh_options_never_serialize_secrets`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/dby-core/src/driver.rs
git commit -m "fix(core): never serialize SSH password/private_key (#22)"
```

---

### Task 2: keyring 三键 helper + 单测

**Files:**
- Modify: `src-tauri/src/state.rs`（或新增 `src-tauri/src/secrets.rs`）

**Interfaces:**
- Produces: `fn secret_key(config_id: &str, kind: SecretKind) -> String`

- [ ] **Step 1: 写失败测试**

```rust
#[test]
fn secret_keys_are_stable() {
    assert_eq!(secret_key("abc", SecretKind::MysqlPassword), "abc");
    assert_eq!(secret_key("abc", SecretKind::SshPassword), "abc:ssh");
    assert_eq!(secret_key("abc", SecretKind::SshPrivateKey), "abc:ssh_key");
}
```

- [ ] **Step 2: 运行确认失败**

Run: `cargo test -p dby --bin dby secret_keys_are_stable`（或对应的 lib test target）
Expected: FAIL（函数不存在）

- [ ] **Step 3: 最小实现**

```rust
pub enum SecretKind { MysqlPassword, SshPassword, SshPrivateKey }
pub fn secret_key(config_id: &str, kind: SecretKind) -> String {
    match kind {
        SecretKind::MysqlPassword => config_id.to_string(),
        SecretKind::SshPassword => format!("{config_id}:ssh"),
        SecretKind::SshPrivateKey => format!("{config_id}:ssh_key"),
    }
}
```

- [ ] **Step 4: 运行确认通过**

Run: 同上
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/secrets.rs
git commit -m "feat(shell): add keyring secret key scheme for SSH (#22)"
```

---

### Task 3: 脱敏视图 `SavedConnectionView`

**Files:**
- Modify: `src-tauri/src/commands.rs`（新增结构体 + 改 `list_saved_connections`）
- Modify: `src/api.ts`（`SavedConnection` 收窄 + `api.listSavedConnections`）

**Interfaces:**
- Produces: `SavedConnectionView { id, project_id, name, driver, host, port, user, database, has_ssh, ssh_host, ssh_port, ssh_user, color }`

- [ ] **Step 1: 写失败测试**

```rust
#[test]
fn saved_view_has_no_secret() {
    let v = SavedConnectionView { /* ...全部字段填非敏感值 */ };
    let json = serde_json::to_string(&v).unwrap();
    assert!(!json.contains("password"));
    assert!(!json.contains("private_key"));
}
```

- [ ] **Step 2: 运行确认失败**

Run: `cargo test -p dby --bin dby saved_view_has_no_secret`
Expected: FAIL

- [ ] **Step 3: 最小实现**

按 design 4.3 定义 `SavedConnectionView`；`list_saved_connections` 从 `ConnectionConfig` 构造脱敏视图返回 `Vec<SavedConnectionView>`。

- [ ] **Step 4: 运行确认通过 + 前端类型同步**

Run: `cargo test`（通过）+ `pnpm build`（`src/api.ts` 的 `SavedConnection` 改为视图字段）
Expected: 两者 PASS

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/commands.rs src/api.ts
git commit -m "feat(shell): return sanitized saved-connection view (#22)"
```

---

### Task 4: `connect` 保存开关 + secrets 入 keyring（#6）

**Files:**
- Modify: `src-tauri/src/commands.rs`（`connect`）
- Modify: `src/components/ConnectionDialog.tsx`、`src/App.tsx`（传 `save`/`remember_password`）

**Interfaces:**
- Consumes: `secret_key`（Task 2）
- Produces: `connect(params, project_id, save, remember_password)`

- [ ] **Step 1: 写失败测试**

```rust
// 壳层命令涉及 keyring，用 mock/特征隔离：抽 store_secrets(&config_id, &params) 供单测注入
#[test]
fn connect_save_false_writes_nothing() { /* 注入 fake secrets backend，断言未调用 */ }
```

- [ ] **Step 2: 运行确认失败**

Run: `cargo test -p dby --bin dby connect_save_false_writes_nothing`
Expected: FAIL

- [ ] **Step 3: 最小实现**

按 design §4.4（**secrets 先存、config 后存**，避免「config 已存但 secret 缺失」半失败态；`persist_config` 在 `save` 失败时回滚内存 `push`）：

```rust
if remember_password {
    if let Err(e) = store_secrets(&config_id, &params) {
        state.connections.lock().await.remove(&resp.id); // 断开不留孤儿；未存 config，可安全失败
        return Err(e);
    }
}
if let Err(e) = persist_config(state.inner(), config).await {
    if remember_password { delete_secrets(&config_id); } // 回滚已存 secrets
    state.connections.lock().await.remove(&resp.id);
    return Err(e);
}
```

`save=false` 不持久化（不写 config、不写 keyring）。`ConnectionDialog` 加「保存连接」「记住密码」两复选框（默认勾选），`api.connect` 传 `save`/`remember_password`。

- [ ] **Step 4: 运行确认通过**

Run: `cargo test` + `pnpm build`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/commands.rs src/components/ConnectionDialog.tsx src/App.tsx
git commit -m "feat(shell): add save/remember-password toggles to connect (#6/#22)"
```

---

### Task 5: `reconnect` 从 keyring 读 secret + `params` 回填（#63）

**Files:**
- Modify: `src-tauri/src/commands.rs`（`reconnect`）

**Interfaces:**
- Consumes: `secret_key`
- Produces: `reconnect(config_id)` 用 keyring secret 建连

- [ ] **Step 1: 写失败测试**

抽 `build_params_from_config(&config, &secrets) -> ConnectParams`，断言 ssh.password/private_key 来自 secrets、`params` 回填。

- [ ] **Step 2: 运行确认失败**

Run: `cargo test -p dby --bin dby build_params_from_config_*`
Expected: FAIL

- [ ] **Step 3: 最小实现**

按 design §4.5 落地；`ConnectionConfig` 增 `params: HashMap<String,String>`（#63）。**缺 secret 判定**：`config.ssh.enabled && ssh_password.is_none() && ssh_private_key.is_none()` → `DbError::Config("该连接未保存 SSH 凭据，请重新连接并输入")`（kind=config，前端据此打开连接对话框补录）。

- [ ] **Step 4: 运行确认通过**

Run: `cargo test`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/commands.rs crates/dby-core/src/config.rs
git commit -m "feat(shell): reconnect reads SSH secrets from keyring and restores params (#22/#63)"
```

---

### Task 6: 级联清理 + 写错误非静默（#41/#39）

**Files:**
- Modify: `src-tauri/src/commands.rs`（`delete_saved_connection`、`delete_project`）

**Interfaces:**
- Consumes: `secret_key`
- Produces: 删除连接/项目时清理对应 keyring 三键

- [ ] **Step 1: 写失败测试**

```rust
#[test]
fn delete_project_rejects_or_cascades_saved_connections() { /* 断言存在同项目 config.connections 时行为 */ }
```

- [ ] **Step 2: 运行确认失败**

Run: `cargo test -p dby --bin dby delete_project_*`
Expected: FAIL

- [ ] **Step 3: 最小实现**

`delete_saved_connection` 删 3 个 keyring 条目；`delete_project` 校验 `config.connections`（非仅活跃），删除后级联清理；所有 `let _ =` 改为 `if let Err(e) = ... { log::warn!(...) }`。

- [ ] **Step 4: 运行确认通过**

Run: `cargo test` + `cargo clippy -D warnings`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/commands.rs
git commit -m "fix(shell): cascade keyring cleanup on delete + non-silent write errors (#41/#39)"
```

---

### Task 7: `update_saved_connection`（#63）

**Files:**
- Modify: `src-tauri/src/commands.rs`、`src-tauri/src/lib.rs`（注册）
- Modify: `src/api.ts`

**Interfaces:**
- Produces: `update_saved_connection(config_id, update: UpdateSavedConnection) -> Result<()>`

- [ ] **Step 1: 写失败测试**

```rust
#[test]
fn update_saved_connection_changes_name_and_color() { /* 构造 config，调用后断言 name/color 变更且未引入新 id */ }
```

- [ ] **Step 2: 运行确认失败**

Run: `cargo test -p dby --bin dby update_saved_connection_*`
Expected: FAIL

- [ ] **Step 3: 最小实现**

```rust
#[derive(Deserialize)]
pub struct UpdateSavedConnection { pub name: Option<String>, pub color: Option<String>, pub ssh: Option<SshOptions> }
```

按 design 4.7 实现；`color` 由前端传真实值。

- [ ] **Step 4: 运行确认通过**

Run: `cargo test` + `pnpm build`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/commands.rs src-tauri/src/lib.rs src/api.ts
git commit -m "feat(shell): add update_saved_connection and wire color (#63)"
```

---

### Task 8: 旧明文迁移 + 端到端验证

**Files:**
- Modify: `src-tauri/src/lib.rs`（setup 时迁移）

**Interfaces:**
- Consumes: `secret_key`
- Produces: 首次启动把旧 config 中 ssh 明文迁入 keyring 并清除

- [ ] **Step 1: 写失败测试**

```rust
#[test]
fn migrate_clears_legacy_ssh_plaintext() { /* 构造含 ssh.password 的旧 config，migrate 后 config 无 secret 且 keyring 有值 */ }
```

- [ ] **Step 2: 运行确认失败**

Run: `cargo test -p dby --bin dby migrate_*`
Expected: FAIL

- [ ] **Step 3: 最小实现**

setup 中 `migrate_legacy_secrets(config) -> usize`：**先写 keyring、成功后清空字段**（失败**不** `take()`、保留明文下次重试），单条失败 `log::warn!` 跳过不中断，返回迁移数；`migrated > 0` 则 `cfg.save()` 重写清除明文。

- [ ] **Step 4: 运行确认通过（手工）**

Run: `cargo test` + 手工：预置旧 config → 启动 → 检查 `config.json` 无 secret、keyring 有值
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/lib.rs
git commit -m "feat(shell): migrate legacy SSH plaintext into keyring on startup (#22)"
```
