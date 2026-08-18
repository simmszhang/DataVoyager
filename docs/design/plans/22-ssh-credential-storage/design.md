# #22 SSH 凭据存储 — 设计文档

> 状态：评审有条件通过（5 阻断项已修订，待复审） · 优先级 P0 · 规模：大 · 关联缺陷：#6（保存开关）、#39（写错误）、#41（级联清理）、#43（半失败）、#63（update 命令）· 依赖共享契约：S3（凭据存储与脱敏）、S5（错误形状）

## 1. 现状与影响

- `commands.rs:88-94`：`connect` 把 `params.ssh.clone()`（含 `password`/`private_key`）写入 `ConnectionConfig` 并 `save` 到 `config.json`；仅 MySQL `password` 进钥匙串（`96-99`）。
- `commands.rs:174-185`：`list_saved_connections` 返回完整 `Vec<ConnectionConfig>`，SSH 密码明文发到前端。
- `commands.rs:210`：`reconnect` 从 config 取 `ssh`（明文）。
- **影响**：违背 architecture §5「密码/SSH 凭据 → OS 钥匙串」承诺；SSH 凭据明文落盘 + WebView 可见（review S2，🔴）。
- 连带：`connect` 每次自动持久化、无「保存连接」开关（#6）；`set_secret/delete_secret` 失败 `let _` 静默（#39，`commands.rs:98,232`）及 `history.record` 失败静默（`commands.rs:323,328,491,496` 等）；`delete_project` 只校验活跃连接、遗留孤儿 `ConnectionConfig` 与钥匙串（#41，`commands.rs:369-382`）；连接配置无更新命令、`color` 死字段（#63）；`connect` 半失败留孤儿连接（#43，`commands.rs:126` 先 insert、`94` 后 save）。

## 2. 目标与成功标准

1. SSH 密码/私钥与 MySQL 密码一样只进 OS 钥匙串，绝不落 `config.json`、绝不跨 IPC 返回前端。
2. `list_saved_connections` 返回脱敏视图（无任何 secret；含 `has_ssh/ssh_host/ssh_port/ssh_user`）。
3. `reconnect` 从钥匙串读取 secrets；删除连接/项目时级联清理钥匙串条目。
4. `connect` 提供「保存连接/记住密码」开关，按需持久化。
5. 写错误不再静默（至少 log/告警），持久化失败不留孤儿连接。
6. 提供 `update_saved_connection`；明确 `color`/`params` 语义。
7. 旧 `config.json` 明文一次性迁移进钥匙串并清除（幂等）。
8. 成功标准：`config.json` 全库 grep 不到 SSH 密码/私钥；`list_saved_connections` 返回体不含 secret 字段；旧配置迁移后明文被清除。

## 3. 方案对比

### 方案 A：`skip_serializing` + keyring 三键 + 脱敏视图（推荐）
- `SshOptions.password/private_key` 加 `#[serde(skip_serializing)]`（保留反序列化）；keyring key 约定 `{config_id}` / `{config_id}:ssh` / `{config_id}:ssh_key`；`list_saved_connections` 返回新脱敏视图。
- **优点**：最小改动、与现有 keyring 机制一致、单一事实源。**缺点**：需一次性迁移旧明文并清除。

### 方案 B：独立凭据库（如 SQLite secrets 表）
- 所有凭据进独立加密存储。
- **优点**：可扩展。**缺点**：过度设计，OS 钥匙串已是系统级加密存储，违背 YAGNI。

### 方案 C：仅脱敏、落盘仍明文
- 只改 `list_saved_connections` 脱敏，config 仍存明文。
- **缺点**：不满足架构承诺，明文落盘风险仍在，否决。

**推荐 A**。

## 4. 推荐方案详细设计

### 4.1 数据结构

`crates/dby-core/src/driver.rs` 的 `SshOptions`：

```rust
// 注意：skip_serializing 不影响 Debug；禁止对 SshOptions 使用 {:?} 打印（会泄密）。
// 如需日志，只打印 host/port/user。
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SshOptions {
    pub enabled: bool,
    pub host: String,
    #[serde(default = "default_ssh_port")]
    pub port: u16,
    pub user: String,
    #[serde(default, skip_serializing)]
    pub password: Option<String>,
    #[serde(default, skip_serializing)]
    pub private_key: Option<String>,
    #[serde(default)]
    pub host_key_fingerprint: Option<String>, // #9 新增，非敏感
}
```

`crates/dby-core/src/config.rs` 的 `ConnectionConfig` 增 `params` 字段（#63）：

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionConfig {
    // ...既有字段不变
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub params: std::collections::HashMap<String, String>, // 驱动特定参数，重连回填
}
```

### 4.2 钥匙串 key 约定（新建 `src-tauri/src/secrets.rs`）

把 `commands.rs:152-171` 的 `set_secret/get_secret/delete_secret` **迁入**新 `secrets` 模块并扩展三键：

```rust
pub enum SecretKind { MysqlPassword, SshPassword, SshPrivateKey }

pub fn secret_key(config_id: &str, kind: SecretKind) -> String {
    match kind {
        SecretKind::MysqlPassword => config_id.to_string(),
        SecretKind::SshPassword => format!("{config_id}:ssh"),
        SecretKind::SshPrivateKey => format!("{config_id}:ssh_key"),
    }
}

// set_secret/get_secret/delete_secret 复用 keyring::Entry::new("dby", key)，签名不变
```

### 4.3 脱敏视图（`src-tauri/src/commands.rs`）

```rust
#[derive(Serialize)]
pub struct SavedConnectionView {
    pub id: String,
    pub project_id: String,
    pub name: String,
    pub driver: String,
    pub host: String,
    pub port: u16,
    pub user: String,
    pub database: Option<String>,
    pub has_ssh: bool,
    pub ssh_host: Option<String>,
    pub ssh_port: Option<u16>,   // 非敏感，供 #9 TOFU 指纹确认展示 host:port
    pub ssh_user: Option<String>,
    pub color: Option<String>,
}
```

`list_saved_connections` 返回 `Vec<SavedConnectionView>`（前端 `SavedConnection` 同步收窄，review S2 契约漂移一并修复）。

### 4.4 `connect` 保存开关（#6/#43）

```rust
#[tauri::command]
pub async fn connect(
    state: State<'_, Arc<AppState>>,
    params: ConnectParams,
    project_id: Option<String>,
    save: bool,               // 是否保存连接
    remember_password: bool,  // 是否记住密码（save=true 生效；一个开关覆盖 MySQL 密码 + SSH 密码 + SSH 私钥三 secret）
) -> Result<ConnectResponse> {
    let project_id = state.resolve_project_id(project_id).await;
    let name = auto_name(&params);
    let resp = open_session(state.inner(), &params, project_id.clone(), name.clone()).await?;
    if save {
        let config_id = uuid::Uuid::new_v4().to_string();
        let config = ConnectionConfig {
            id: config_id.clone(), project_id, name, driver: params.driver.clone(),
            host: params.host.clone(), port: params.port, user: params.user.clone(),
            database: params.database.clone(), ssl: params.ssl.clone(),
            ssh: params.ssh.clone(), color: None, params: params.params.clone(),
        };
        // 先存 secrets（remember_password=true）再存 config，避免「config 已存但 secret 缺失」半失败态
        if remember_password {
            if let Err(e) = store_secrets(&config_id, &params) {
                state.connections.lock().await.remove(&resp.id); // 断开不留孤儿（#43）；未存 config，可安全失败
                return Err(e);
            }
        }
        if let Err(e) = persist_config(state.inner(), config).await {
            // config 保存失败：回滚已存 secrets + 断开（不留半失败态）
            if remember_password { delete_secrets(&config_id); }
            state.connections.lock().await.remove(&resp.id);
            return Err(e);
        }
    }
    Ok(resp)
}
```

`persist_config(state, config) -> Result<()>`：`cfg.connections.push(config)` + `cfg.save(...)`；**`save` 失败时回滚 `push`**（`cfg.connections.retain(|c| c.id != config.id)` 后再返回 Err），避免内存出现未持久化的幽灵配置。
`store_secrets(config_id, params) -> Result<()>`：写 `{config_id}` / `{config_id}:ssh` / `{config_id}:ssh_key` 三键（secret 为 None 或空串则跳过，不写）。
`delete_secrets(config_id)`：删上述三键（回滚用）。

前端 `api.connect` 增 `save`/`remember_password` 两参；`ConnectionDialog` 增「保存连接」「记住密码」两个复选框（默认均勾选）。

### 4.5 `reconnect` 从钥匙串读 secret（缺 secret 报错）

```rust
#[tauri::command]
pub async fn reconnect(state: State<'_, Arc<AppState>>, config_id: String) -> Result<ConnectResponse> {
    let config = { /* 取 config.clone()，不存在则 DbError::Config */ };
    let password = get_secret(&secret_key(&config_id, SecretKind::MysqlPassword)).ok();
    let ssh_password = get_secret(&secret_key(&config_id, SecretKind::SshPassword)).ok();
    let ssh_private_key = get_secret(&secret_key(&config_id, SecretKind::SshPrivateKey)).ok();

    // 缺 secret 判定：需要密码/SSH 但 keyring 无条目 → 报错，引导前端走 connect 补录
    if config.ssh.as_ref().map(|s| s.enabled).unwrap_or(false)
        && ssh_password.is_none() && ssh_private_key.is_none() {
        return Err(DbError::Config("该连接未保存 SSH 凭据，请重新连接并输入".to_string()));
    }
    // MySQL 密码缺失不强制（可能无密码库），由驱动认证失败自然报错；空密码边缘见 §7

    let params = ConnectParams {
        driver: config.driver.clone(), host: config.host.clone(), port: config.port,
        user: config.user.clone(), password, database: config.database.clone(),
        ssl: config.ssl.clone(),
        ssh: config.ssh.clone().map(|mut s| { s.password = ssh_password; s.private_key = ssh_private_key; s }),
        params: config.params.clone(), // #63：恢复驱动参数
    };
    open_session(state.inner(), &params, config.project_id.clone(), config.name.clone()).await
}
```

前端：`reconnect` 收到 `Config("该连接未保存…")` 时，打开 `ConnectionDialog` 预填该连接非敏感字段，用户输入凭据后走 `connect`（save=true 时覆盖持久化）。

### 4.6 级联清理（#41）

- `delete_saved_connection`：删 config + 删 `{config_id}` / `{config_id}:ssh` / `{config_id}:ssh_key` 三个 keyring 条目。
- `delete_project`：**保持「拒绝」**（与前端 `App.tsx:332` 文案「项目下连接需先删除」一致）——校验 `config.connections`（不只活跃 `connections`），有同项目连接则返回 `DbError::Config`，不级联删除（避免误删其它项目在用凭据）。

### 4.7 更新命令（#63）

```rust
#[derive(Deserialize)]
pub struct UpdateSavedConnection {
    pub name: Option<String>,
    pub color: Option<String>,
    pub ssh: Option<SshOptions>, // 只允许更新非敏感字段；secret 走 store_secrets 单独更新
}

#[tauri::command]
pub async fn update_saved_connection(
    state: State<'_, Arc<AppState>>, config_id: String, update: UpdateSavedConnection,
) -> Result<()> {
    // 找到 config，按 Some 字段覆盖，cfg.save；color 由前端传入真实值
}
```

前端编辑入口（本方案范围）：在「已保存」列表加「编辑」按钮 → 弹窗改 name/color（重命名/着色）；`params`/`ssh` 更新留待 M2（#47 能力矩阵落地后）。

### 4.8 旧明文迁移（`src-tauri/src/lib.rs` setup 时）

```rust
pub fn migrate_legacy_secrets(config: &mut AppConfig) -> usize {
    let mut migrated = 0usize;
    for c in config.connections.iter_mut() {
        if let Some(ssh) = c.ssh.as_mut() {
            let mut cleared = false;
            // 先写 keyring、成功后清除明文（take）；失败保留明文不丢，log 告警跳过
            if let Some(pw) = ssh.password.as_deref() {
                match set_secret(&secret_key(&c.id, SecretKind::SshPassword), pw) {
                    Ok(()) => { ssh.password = None; cleared = true; }
                    Err(e) => log::warn!("迁移 SSH 密码失败（保留明文）: {e}"),
                }
            }
            if let Some(k) = ssh.private_key.as_deref() {
                match set_secret(&secret_key(&c.id, SecretKind::SshPrivateKey), k) {
                    Ok(()) => { ssh.private_key = None; cleared = true; }
                    Err(e) => log::warn!("迁移 SSH 私钥失败（保留明文）: {e}"),
                }
            }
            if cleared { migrated += 1; }
        }
    }
    migrated
}
```

- **时机**：`setup` 里 `AppConfig::load` 成功后、`save` 前调用；`migrated > 0` 则 `cfg.save()` 重写（清除明文）。
- **幂等**：二次运行 `password/private_key` 已为 None → no-op。
- **失败不丢明文**：`set_secret` 失败时**不** `take()`（明文保留在 config），`log::warn!` 跳过该连接、继续其余；函数返回 `usize`（无 `?`，不因单条失败中断启动）。

## 5. 错误处理（遵循 S5）

- `connect` 持久化失败：断开已建会话（`connections.remove(resp.id)`）+ 返回 `DbError::Storage`（#43/#39）。
- `store_secrets` 单键失败：`log::warn!` 记录，返回 `DbError::Storage`（触发上面的断开+返回），不静默。
- `reconnect` 缺 secret：`DbError::Config("该连接未保存 SSH 凭据，请重新连接并输入")`（kind=config，前端据 kind 引导补录）。
- 迁移单条失败：`log::warn!` 跳过，不阻断启动。

## 6. 测试策略

- **单元**：`secret_key()` 三种 key 的确定性；`SavedConnectionView` 序列化**不含** secret；`SshOptions` 序列化不含 `password/private_key`、反序列化仍可读。
- **单元（迁移）**：`migrate_legacy_secrets` 从旧 config 迁出明文 → config 无 secret + keyring 有值；二次运行 no-op（幂等）；单条失败跳过。
- **集成/手工**：connect(save=false) 不产生 config/keyring；connect(save=true) 后 `config.json` 无 secret、`list_saved_connections` 无 secret；reconnect 用 keyring secret 成功；reconnect 缺 secret 报错；delete_saved_connection 三键级联清理；delete_project 后 keyring 无残留。

## 7. 回归风险与影响面

- **契约变更**：`list_saved_connections` 返回类型从 `ConnectionConfig` → `SavedConnectionView`，前端 `SavedConnection` 同步（review S2）。
- **迁移**：见 §4.8；`skip_serializing` 只阻止新写入，存量明文靠迁移清除。
- **空密码边缘**：MySQL 无密码连接时 `store_secrets` 跳过（不写空串），reconnect 时 `password=None` 仍可连（MySQL 允许空密码）；仅 SSH 缺凭据才强制报错。
- **同类 secret**：`SslOptions.client_key`（`driver.rs:56`）当前为死字段（M2 mTLS 才用），但属同类私钥——在 M2 接 mTLS 时必须同样 keyring 化，本方案显式标注避免遗漏。
- 与 #9 边界：`host_key_fingerprint` 属非敏感、仍落盘；本方案只处理 secrets。
- 与 #48 联动：`list_saved_connections` 脱敏是 #48「凭据读取」攻击面的前置收敛。

## 8. 关联缺陷处置

- #6：4.4 保存开关；#39：§5 非静默；#41：4.6 级联清理；#43：4.4 持久化失败断开；#63：4.7 更新命令 + `color`/`params` 语义。

## 9. 与其它方案组的依赖

- 提供共享契约 S3 的落地实现，供 #9（私钥）、#48（脱敏）引用。
- 与 #5（S1 并发）独立，但 commands 改动同样遵循「锁不跨 await」范式。
