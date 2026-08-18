# #9 SSH 主机密钥 TOFU — 实现计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** SSH 隧道改为 TOFU 主机指纹校验 + 私钥认证 + 连接超时，消除 MITM 风险。

**Architecture:** 指纹作为 `SshOptions` 非敏感字段随 `ConnectionConfig` 持久化；探针/连接双 handler（`ProbeHandler` 写槽 + `ClientHandler` 比对）；新增只读探针命令 `probe_host_key` 供前端首连确认；`russh::keys`（russh 0.62 内置）实现私钥认证。

**Tech Stack:** Rust（russh 0.62.6 / ssh-key / tokio time）、Tauri 2、React 19/TS。

**Spec:** `docs/design/plans/09-ssh-host-key-tofu/design.md`（修订版，含已核对的 russh 0.62.6 API）

## Global Constraints

- 指纹用 `PublicKey::fingerprint(HashAlg::Sha256)`（= `Default::default()`），**不引入 sha2/base64ct**。
- `SshOptions.password/private_key` 的 `skip_serializing` + keyring 归 #22（S3），本方案只加非敏感 `host_key_fingerprint`，且与 #22 **同批合入**。
- 私钥认证：`decode_secret_key(pem, None)`（PEM 字符串，非文件路径）+ `best_supported_rsa_hash().await?.flatten()` + `PrivateKeyWithHashAlg::new`；无独立 `russh-keys` 依赖（russh 0.62 已折叠进 `russh::keys`）。
- 超时用 `tokio::time::timeout` 包裹 `connect`（`Config` 无 `connection_timeout` 字段）；驱动 tokio 补 `"time"` feature。
- 错误形状遵循 S5：`DbError` 序列化 `{"kind","message"}`。
- CI 门禁：`cargo fmt --check`、`cargo clippy --workspace --all-targets -- -D warnings`、`cargo test -p dby-core -p dby-driver-mysql`。

---

### Task 1: 指纹计算 helper + 单测

**Files:**
- Create: `crates/dby-driver-mysql/src/tunnel.rs`（新增 `fingerprint` 与 `#[cfg(test)]`）

**Interfaces:**
- Produces: `fn fingerprint(key: &ssh_key::PublicKey) -> String`（标准 OpenSSH `SHA256:…`）

- [ ] **Step 1: 写失败测试**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use russh::keys::HashAlg;

    #[test]
    fn fingerprint_matches_openssh_form() {
        // 用固定公钥断言与 `ssh-keygen -lf` 输出**逐字符一致**（非仅前缀/长度）
        // 生成测试密钥：russh::keys::PrivateKey::random(...) 或内置固定密钥
        let fp = fingerprint(&key);
        assert!(fp.starts_with("SHA256:"));
        assert_eq!(fp.len(), "SHA256:".len() + 43); // SHA-256 无 padding base64 = 43 字符
        // 可选：与 ssh-keygen -lf <pubkey> 输出比对（本地手动）
    }
}
```

- [ ] **Step 2: 运行确认失败**

Run: `cargo test -p dby-driver-mysql tunnel::tests::fingerprint_matches_openssh_form`
Expected: FAIL（`fingerprint` 未定义）

- [ ] **Step 3: 最小实现**

```rust
use russh::keys::HashAlg;

fn fingerprint(key: &ssh_key::PublicKey) -> String {
    key.fingerprint(HashAlg::Sha256).to_string() // ssh-key 官方 API：Display 即 "SHA256:<无padding base64>"
}
```

- [ ] **Step 4: 运行确认通过**

Run: `cargo test -p dby-driver-mysql tunnel::tests::fingerprint_matches_openssh_form`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/dby-driver-mysql/src/tunnel.rs
git commit -m "feat(ssh): OpenSSH-style SHA256 host key fingerprint via ssh-key API"
```

---

### Task 2: 探针/连接双 handler

**Files:**
- Modify: `crates/dby-driver-mysql/src/tunnel.rs`（`ProbeHandler` + `ClientHandler`）

**Interfaces:**
- Produces: `ProbeHandler { slot: Arc<Mutex<Option<String>>> }`（`check_server_key` 恒 `Ok(true)` + 写槽）；`ClientHandler { expected: Option<String>, observed: Arc<Mutex<Option<String>>> }`（匹配→`Ok(true)`、不匹配/无预期→`Ok(false)` + 写槽）

- [ ] **Step 1: 写失败测试**

```rust
#[tokio::test]
async fn client_handler_compares_and_records_observed() {
    // expected=Some(A)：A 匹配→Ok(true)；B 不匹配→Ok(false) 且 observed 槽有 B 的指纹；
    // expected=None：→Ok(false)（拒绝未知，russh 转 Error::UnknownKey）
}
```

> 注：`check_server_key(&mut self, &ssh_key::PublicKey)` 是 async trait 方法，`#[tokio::test]` 包裹；比对逻辑抽纯函数 `matches_fp(expected, actual) -> bool` 便于单测。

- [ ] **Step 2: 运行确认失败**

Run: `cargo test -p dby-driver-mysql tunnel::tests::client_handler_*`
Expected: FAIL（handler 未定义）

- [ ] **Step 3: 最小实现**

```rust
use std::sync::{Arc, Mutex};
use russh::client::Handler;

struct ProbeHandler { slot: Arc<Mutex<Option<String>>> }
impl Handler for ProbeHandler {
    type Error = russh::Error;
    async fn check_server_key(&mut self, key: &ssh_key::PublicKey) -> Result<bool, Self::Error> {
        *self.slot.lock().unwrap() = Some(fingerprint(key));
        Ok(true) // 放行完成 kex；探针随后读槽并断开（不认证）
    }
}

struct ClientHandler { expected: Option<String>, observed: Arc<Mutex<Option<String>>> }
impl Handler for ClientHandler {
    type Error = russh::Error;
    async fn check_server_key(&mut self, key: &ssh_key::PublicKey) -> Result<bool, Self::Error> {
        let fp = fingerprint(key);
        *self.observed.lock().unwrap() = Some(fp.clone());
        match &self.expected {
            Some(exp) => Ok(&fp == exp), // 不匹配 Ok(false) → russh 转 Err(Error::UnknownKey)
            None => Ok(false),           // 未知主机：拒绝
        }
    }
}
```

- [ ] **Step 4: 运行确认通过**

Run: `cargo test -p dby-driver-mysql tunnel::tests::client_handler_*`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/dby-driver-mysql/src/tunnel.rs
git commit -m "feat(ssh): probe/connect handlers with TOFU key comparison"
```

---

### Task 3: `SshOptions` 增 `host_key_fingerprint`

**Files:**
- Modify: `crates/dby-core/src/driver.rs`（`SshOptions`）
- Test: `crates/dby-core/src/driver.rs`

**Interfaces:**
- Produces: `SshOptions.host_key_fingerprint: Option<String>`（`#[serde(default)]`；**不加** `skip_serializing`——那是 #22 的活）

- [ ] **Step 1: 写失败测试**

```rust
#[test]
fn ssh_options_fingerprint_roundtrips() {
    let s = SshOptions { enabled: true, host: "h".into(), port: 22, user: "u".into(),
        password: None, private_key: None, host_key_fingerprint: Some("SHA256:x".into()) };
    let json = serde_json::to_string(&s).unwrap();
    assert!(json.contains("host_key_fingerprint"));
    let back: SshOptions = serde_json::from_str(&json).unwrap();
    assert_eq!(back.host_key_fingerprint.as_deref(), Some("SHA256:x"));
}
```

- [ ] **Step 2: 运行确认失败**

Run: `cargo test -p dby-core driver::tests::ssh_options_fingerprint_roundtrips`
Expected: FAIL（字段不存在）

- [ ] **Step 3: 最小实现**

```rust
    /// TOFU 已信任主机指纹（非敏感，可落盘）
    #[serde(default)]
    pub host_key_fingerprint: Option<String>,
```

- [ ] **Step 4: 运行确认通过**

Run: `cargo test -p dby-core driver::tests::ssh_options_fingerprint_roundtrips`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/dby-core/src/driver.rs
git commit -m "feat(core): add non-secret host_key_fingerprint to SshOptions"
```

---

### Task 4: `probe_host_key` 命令 + 前端 TOFU 弹窗

**Files:**
- Modify: `src-tauri/src/commands.rs`（新增 `probe_host_key`）、`src-tauri/src/lib.rs`（注册）
- Modify: `src/api.ts`（`probeHostKey`）、`src/components/ConnectionDialog.tsx`（探针→确认→connect 携带指纹）、`src/App.tsx`（`test_connection` 也走指纹校验）

**Interfaces:**
- Consumes: `ConnectParams.ssh`、`ProbeHandler`
- Produces: `probe_host_key(params: ConnectParams) -> Result<String>`；前端 `api.probeHostKey(params): Promise<string>`

- [ ] **Step 1: 写失败测试（前端手动，无自动化单测）**

```ts
if (params.ssh?.enabled && !params.ssh.host_key_fingerprint) {
  const fp = await api.probeHostKey(params);
  setPendingFingerprint({ fp, params });
  return; // 弹窗确认
}
await api.connect(params, projectId);
```

- [ ] **Step 2: 运行确认失败**

Run: `pnpm build`
Expected: FAIL（`probeHostKey` 未定义 / 命令未注册）

- [ ] **Step 3: 最小实现**

`commands.rs`：

```rust
#[tauri::command]
pub async fn probe_host_key(params: ConnectParams) -> Result<String> {
    let slot = Arc::new(std::sync::Mutex::new(None));
    let config = Arc::new(russh::client::Config::default());
    let ssh = params.ssh.as_ref().ok_or_else(|| DbError::Config("未配置 SSH".to_string()))?;
    let _handle = russh::client::connect(config, (ssh.host.clone(), ssh.port), ProbeHandler { slot: slot.clone() })
        .await.map_err(ssh_err)?; // 完成 kex 即返回（check_server_key 已写槽）；handle 非 mut（避免 clippy unused_mut）
    Ok(slot.lock().unwrap().take().ok_or_else(|| DbError::Other("未能取得主机指纹".to_string()))?)
    // handle drop 即断开，不认证、不转发
}
```

`lib.rs` 注册 `commands::probe_host_key`；`api.ts` 加 `probeHostKey`；`test_connection` 同样校验 `ssh.host_key_fingerprint`（无则返回「需先确认指纹」）。

- [ ] **Step 4: 运行确认通过**

Run: `pnpm build` && `cargo clippy --workspace --all-targets -- -D warnings`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/commands.rs src-tauri/src/lib.rs src/api.ts src/components/ConnectionDialog.tsx src/App.tsx
git commit -m "feat(ssh): host-key probe command and TOFU confirmation flow"
```

---

### Task 5: 私钥认证（#8）

**Files:**
- Modify: `crates/dby-driver-mysql/src/tunnel.rs`（`start_tunnel` 认证分支）
- Modify: `crates/dby-driver-mysql/Cargo.toml`（**不加** `russh-keys`；russh 0.62 已含 `russh::keys`；补 tokio `"time"` 见 Task 6）

**Interfaces:**
- Consumes: `SshOptions.private_key`（PEM 字符串）
- Produces: 认证成功/失败的 `AuthResult` 分支

- [ ] **Step 1: 写失败测试（逻辑层）**

抽纯函数 `fn pick_auth(ssh) -> AuthKind`（`PublicKey`/`Password`），测「有 private_key 选 PublicKey、否则 Password」。

- [ ] **Step 2: 运行确认失败**

Run: `cargo test -p dby-driver-mysql tunnel::tests::pick_auth_*`
Expected: FAIL

- [ ] **Step 3: 最小实现**

按 design §4.5：

```rust
let auth = match &ssh.private_key {
    Some(pem) => {
        let key = russh::keys::decode_secret_key(pem, None) // PEM 字符串（非文件路径；加密私钥 passphrase 暂不支持）
            .map_err(|e| DbError::Other(format!("SSH 私钥解析失败: {e}")))?;
        let hash_alg = handle.best_supported_rsa_hash().await.map_err(ssh_err)?.flatten(); // Result<Option<Option<HashAlg>>,Error> → Option<HashAlg>
        let key_with_alg = russh::keys::PrivateKeyWithHashAlg::new(std::sync::Arc::new(key), hash_alg);
        handle.authenticate_publickey(ssh.user.clone(), key_with_alg).await.map_err(ssh_err)?
    }
    None => handle.authenticate_password(ssh.user.clone(), ssh.password.clone().unwrap_or_default())
        .await.map_err(ssh_err)?,
};
if !matches!(auth, russh::client::AuthResult::Success) {
    return Err(DbError::Other("SSH 认证失败".to_string()));
}
```

- [ ] **Step 4: 运行确认通过**

Run: `cargo test -p dby-driver-mysql tunnel::tests::pick_auth_*`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/dby-driver-mysql/src/tunnel.rs
git commit -m "feat(ssh): publickey auth via decode_secret_key + best_supported_rsa_hash (#8)"
```

---

### Task 6: SSH 连接超时（#36 前半）

**Files:**
- Modify: `crates/dby-driver-mysql/src/tunnel.rs`（`start_tunnel`）
- Modify: `crates/dby-driver-mysql/Cargo.toml`（tokio features 补 `"time"`）

**Interfaces:**
- Produces: 连接超时（kex 阶段）用 `tokio::time::timeout` 包裹 `connect`

- [ ] **Step 1: 写失败测试（逻辑层）**

抽 `fn ssh_connect_timeout() -> Duration`（10s），断言值。

- [ ] **Step 2: 运行确认失败**

Run: `cargo test -p dby-driver-mysql tunnel::tests::ssh_connect_timeout`
Expected: FAIL

- [ ] **Step 3: 最小实现**

```rust
let mut handle = tokio::time::timeout(
    std::time::Duration::from_secs(10),
    russh::client::connect(config, (ssh.host.clone(), ssh.port), handler),
).await
    .map_err(|_| DbError::Other("SSH 连接超时".to_string()))?
    .map_err(ssh_err)?;
```

> 注意：`russh::client::Config` **无** `connection_timeout` 字段（`client/mod.rs:2107-2132`），超时只能靠 `tokio::time::timeout` 包裹。`authenticate_*` 阶段超时归后续项（design §4.6 注）。

- [ ] **Step 4: 运行确认通过**

Run: `cargo test -p dby-driver-mysql tunnel::tests::ssh_connect_timeout` + `cargo check`
Expected: PASS（`time` feature 已显式声明）

- [ ] **Step 5: Commit**

```bash
git add crates/dby-driver-mysql/src/tunnel.rs crates/dby-driver-mysql/Cargo.toml
git commit -m "feat(ssh): connection timeout via tokio::time::timeout (#36)"
```

---

### Task 7: 错误映射（§5 区分不匹配/未知）

**Files:**
- Modify: `crates/dby-driver-mysql/src/tunnel.rs`（`start_tunnel` 捕获 `Error::UnknownKey` 后按 `expected` 区分）

**Interfaces:**
- Produces: 指纹不匹配 → `DbError::Config("…期望 …，实际 …")`（读 `observed` 槽）；指纹未知 → `DbError::Config("需先确认 SSH 主机指纹")`

- [ ] **Step 1: 写失败测试**

```rust
#[test]
fn maps_unknown_key_error_by_expected() {
    // expected=None → "需先确认"；expected=Some(X) 且 observed=Some(Y) → 含期望与实际指纹
}
```

- [ ] **Step 2: 运行确认失败 → 通过**

Run: `cargo test -p dby-driver-mysql tunnel::tests::maps_unknown_key_error_by_expected`
Expected: 初始 FAIL → PASS

- [ ] **Step 3: Commit**

```bash
git add crates/dby-driver-mysql/src/tunnel.rs
git commit -m "feat(ssh): distinguish unknown vs mismatched host key errors (#9)"
```

---

### Task 8: 集成测试（`#[ignore]`）

**Files:**
- Create: `crates/dby-driver-mysql/tests/ssh_tofu.rs`（`#[ignore]`，需真实 sshd）

**Interfaces:**
- Consumes: `probe_host_key`、`connect` 的指纹校验

- [ ] **Step 1: 写集成测试骨架**

```rust
#[tokio::test]
#[ignore] // 需真实 sshd，参照 deploy/database/README.md 起服务
async fn tofu_confirm_then_reconnect_without_prompt() { /* 探针→确认→连接→重连免确认 */ }

#[tokio::test]
#[ignore]
async fn mismatched_key_rejected_with_actual_fingerprint() { /* 篡改 expected → 拒绝且错误含实际指纹 */ }
```

- [ ] **Step 2: 运行确认（本地起 sshd 后）**

Run: `cargo test -p dby-driver-mysql --test ssh_tofu -- --ignored --nocapture`
Expected: PASS

- [ ] **Step 3: Commit**

```bash
git add crates/dby-driver-mysql/tests/ssh_tofu.rs
git commit -m "test(ssh): sshd TOFU integration tests (#9)"
```
