# #9 SSH 主机密钥 TOFU — 设计文档

> 状态：评审需重写（5 阻断项已修订，待复审） · 优先级 P0 · 规模：大 · 关联缺陷：#8（私钥认证）、#36 前半（SSH 连接超时）· 依赖共享契约：S3（凭据存储，由 #22 负责，本方案与之同批合入）、S5（错误形状）

## 1. 现状与影响

- `tunnel.rs:12-24`：`ClientHandler::check_server_key` 恒返回 `Ok(true)`，接受任意主机密钥。
- **影响**：SSH 链路可被中间人（MITM）替换主机密钥而静默窃听/篡改，违背「生产级安全」（defects #9，architecture §12）。
- `tunnel.rs:43-46`：仅 `authenticate_password`；`SshOptions.private_key`（`driver.rs:69`）已建模未实现 → 生产环境禁密码则无法连接（#8）。
- `tunnel.rs:39-42`：`russh::client::connect` 无超时 → SSH 主机黑洞/丢包时长时间挂起（#36 前半，review D8）。
- `commands.rs:66-102`：`connect` 把 `params.ssh`（含密码）写入 `config.json`；`commands.rs:210`：`reconnect` 从 `config.ssh` 读回（明文）→ 属 #22（S3），本方案不改，但与 #22 **同批合入**（见 §7/§9）。

## 2. 目标与成功标准

1. 首次连接展示 `SHA256` 主机指纹，用户确认后才建立隧道（TOFU）。
2. 已信任指纹持久化；后续连接自动校验，不匹配即拒绝并报可读错误（含实际指纹）。
3. 支持私钥认证（`russh::keys::decode_secret_key` + `PrivateKeyWithHashAlg`），RSA 用 `rsa-sha2-256/512`。
4. SSH 连接设超时（`tokio::time::timeout`），超时返回带 kind 的 `DbError`。
5. `test_connection`（测试连接）同样走指纹校验/TOFU。
6. 成功标准：MITM（指纹不匹配）被拒且错误含实际指纹；正常重连免二次确认；私钥可登录；黑洞主机在超时内返回；指纹与 `ssh-keygen -lf` 一致。

## 3. 方案对比

### 方案 A：探针命令 + 指纹存 `SshOptions`（推荐）
- 只读探针命令 `probe_host_key(params) -> String` 连 SSH 取公钥算指纹即断开（不认证、不转发）；`connect` 的 handler 按 `SshOptions.host_key_fingerprint` 比对。
- **优点**：指纹作为非敏感字段存 `SshOptions`，单一事实源，随连接删除/级联清理。**缺点**：需额外探针命令。

### 方案 B：两段式 `connect`（错误里带指纹）
- 未知指纹做成独立 error，前端从错误里拿指纹再重试。
- **缺点**：`russh::Error` 无法承载自定义指纹字符串（见 §4.3），需额外通道回传，不干净。

### 方案 C：独立 `known_hosts` 文件（OpenSSH 风格）
- **缺点**：与 `ConnectionConfig` 分离，删除连接时指纹残留（与 #41 割裂）。

**推荐 A**。

## 4. 推荐方案详细设计

### 4.1 数据结构（`crates/dby-core/src/driver.rs`）

```rust
pub struct SshOptions {
    pub enabled: bool,
    pub host: String,
    #[serde(default = "default_ssh_port")]
    pub port: u16,
    pub user: String,
    pub password: Option<String>,   // 字段本身已有；skip_serializing + keyring 归 #22（S3），本方案不落地
    pub private_key: Option<String>,
    /// TOFU 已信任主机指纹（OpenSSH 风格 "SHA256:<base64>"；非敏感，可落 config.json）
    #[serde(default)]
    pub host_key_fingerprint: Option<String>,
}
```

> **S3 边界**：`password/private_key` 的 `skip_serializing` + keyring 由 #22 落地；本方案**必须与 #22 同批合入**，否则本方案若先落地会导致 `reconnect` 静默丢 SSH 密码。

### 4.2 指纹计算（`crates/dby-driver-mysql/src/tunnel.rs`）

用官方 API，**不引入 sha2/base64ct**：

```rust
use russh::keys::HashAlg;

fn fingerprint(key: &ssh_key::PublicKey) -> String {
    key.fingerprint(HashAlg::Sha256).to_string() // 标准 OpenSSH "SHA256:…"（无 padding）
}
```

> `ssh_key::PublicKey` 即 `russh::keys::PublicKey`（`keys/mod.rs:78` re-export）；`fingerprint(Default::default())` 的 `Default` 即 `HashAlg::Sha256`。

### 4.3 handler：探针与连接分离

```rust
use std::sync::{Arc, Mutex};
use russh::client::Handler;

/// 探针 handler：接受任意密钥（Ok(true)），把指纹写入共享槽供探针命令读取。
struct ProbeHandler { slot: Arc<Mutex<Option<String>>> }

impl Handler for ProbeHandler {
    type Error = russh::Error;
    async fn check_server_key(&mut self, key: &ssh_key::PublicKey) -> Result<bool, Self::Error> {
        *self.slot.lock().unwrap() = Some(fingerprint(key));
        Ok(true) // 放行以完成 kex；探针随后读槽并断开（不认证）
    }
}

/// 连接 handler：按预期指纹比对，指纹写入共享槽供错误映射回传实际值。
struct ClientHandler {
    expected: Option<String>,
    observed: Arc<Mutex<Option<String>>>,
}

impl Handler for ClientHandler {
    type Error = russh::Error;
    async fn check_server_key(&mut self, key: &ssh_key::PublicKey) -> Result<bool, Self::Error> {
        let fp = fingerprint(key);
        *self.observed.lock().unwrap() = Some(fp.clone());
        match &self.expected {
            Some(exp) => Ok(&fp == exp), // 匹配放行；不匹配 Ok(false) → russh 转 Err(Error::UnknownKey)
            None => Ok(false),           // 未知主机：拒绝（前端应先走探针确认）
        }
    }
}
```

> `check_server_key` 返回 `Ok(false)` 会被 russh 转成 `Err(Error::UnknownKey)`（`client/mod.rs:1721-1724`）；`russh::Error` 无 `Closed` 变体、无法承载指纹字符串，故用 `observed` 共享槽回传。

### 4.4 探针命令 + TOFU 流

```rust
// commands.rs
#[tauri::command]
pub async fn probe_host_key(params: ConnectParams) -> Result<String> {
    let slot = Arc::new(Mutex::new(None));
    let config = Arc::new(russh::client::Config::default());
    let handle = russh::client::connect(config, (params.ssh.host, params.ssh.port), ProbeHandler { slot: slot.clone() })
        .await.map_err(ssh_err)?; // 完成 kex 即返回（check_server_key 已把指纹写槽）
    Ok(slot.lock().unwrap().take().unwrap_or_else(|| "未知".into()))
    // handle drop 即断开，不认证、不转发
}
```

TOFU 流：`ConnectionDialog` 勾选 SSH 后，点「连接/测试连接」→ 先 `probe_host_key` → 弹窗显示指纹 → 用户确认 → `connect`/`test_connection` 携带 `host_key_fingerprint` → 壳层持久化到 `SshOptions.host_key_fingerprint`。

`test_connection`：同样走指纹校验——`ConnectParams.ssh.host_key_fingerprint` 存在则校验、否则返回「需先确认指纹」（前端先探针）。

### 4.5 私钥认证（#8）

```rust
let auth = match &ssh.private_key {
    Some(pem) => {
        let key = russh::keys::decode_secret_key(pem, None) // PEM 字符串（非文件路径；加密私钥 passphrase 暂不支持，见 §7）
            .map_err(|e| DbError::Other(format!("SSH 私钥解析失败: {e}")))?;
        let hash_alg = handle.best_supported_rsa_hash().await.map_err(ssh_err)?.flatten(); // Result<Option<Option<HashAlg>>, Error> → Option<HashAlg>；RSA 取 rsa-sha2-256/512，非 RSA 忽略
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

依赖：**无需独立 `russh-keys`**（russh 0.62 已把 keys 折叠进 `russh::keys`）。

### 4.6 连接超时（#36 前半）

```rust
let config = Arc::new(russh::client::Config::default()); // 无 connection_timeout 字段
let mut handle = tokio::time::timeout(
    std::time::Duration::from_secs(10),
    russh::client::connect(config, (ssh.host.clone(), ssh.port), handler),
).await
    .map_err(|_| DbError::Other("SSH 连接超时".to_string()))?
    .map_err(ssh_err)?;
```

依赖：`dby-driver-mysql/Cargo.toml` 的 tokio 补 `"time"` feature（当前 `["net","io-util","rt"]`，见 Cargo.toml:16）。

> 超时仅覆盖 `connect`（kex）阶段；`authenticate_*` 阶段超时（若服务端 kex 后挂起）归后续项，不在本方案（#36 前半仅「连接超时」）。

## 5. 错误处理（遵循 S5）

- `check_server_key` 返回 `Ok(false)` → russh `Err(Error::UnknownKey)`；`start_tunnel` 捕获后按 `expected` 区分两条消息：

  ```rust
  // connect 返回 Err(Error::UnknownKey) 后：
  match &handler.expected {
      None => Err(DbError::Config("需先确认 SSH 主机指纹".to_string())), // 前端据此先探针
      Some(exp) => {
          let actual = handler.observed.lock().unwrap().take();
          Err(DbError::Config(format!("SSH 主机指纹不匹配（可能中间人攻击）：期望 {exp}，实际 {}", actual.unwrap_or_default())))
      }
  }
  ```

- 超时/认证/私钥解析失败：`DbError::Other` 带可读信息（kind 化后前端可本地化）。

## 6. 测试策略

- **单元**：`fingerprint()` 对固定公钥断言与 `ssh-keygen -lf` 输出**逐字符一致**（`SHA256:` + 43 字符无 padding，非仅前缀/长度）。
- **单元（handler）**：`ClientHandler` 三路（匹配→Ok(true)、不匹配→Ok(false) 且槽有实际 fp、无预期→Ok(false)）；`ProbeHandler` 写槽 + Ok(true)。
- **集成（`#[ignore]`，需真实 sshd）**：首次「探针→确认→连接」；重连免确认；篡改密钥被拒且错误含实际指纹；私钥登录（含 RSA 键）；黑洞主机在超时内返回。
- **覆盖缺口**：当前 SSH 完全无测试（subsystems §15）。

## 7. 回归风险与影响面

- `SshOptions` 增字段：前端 `ConnectParams.ssh`（`src/api.ts`）同步 `host_key_fingerprint`。
- **行为变更**：SSH 连接从「静默接受」变「首连需确认一次」；`test_connection` 一并收紧。
- **与 #22 强绑定**：`skip_serializing` + keyring 由 #22 落地，本方案与 #22 **同批合入**，否则 reconnect 丢 SSH 密码。
- 与 #24 共享 `tunnel.rs`：本方案改 handler/认证/超时；accept 循环生命周期（abort/关闭）由 #24 负责，行不重叠。

## 8. 关联缺陷处置

- #9：4.2/4.3/4.4；#8：4.5；#36 前半：4.6。

## 9. 与其它方案组的依赖

- 依赖 #22（S3）同批合入；与 #24 共享 `tunnel.rs`（改动互不重叠）；依赖 S5（错误形状）。
