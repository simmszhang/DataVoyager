//! SSH 隧道：russh 本地端口转发（MySQL 驱动通过隧道连接远端数据库）。

use std::sync::{Arc, Mutex};

use dby_core::driver::SshOptions;
use dby_core::error::{DbError, Result};

fn ssh_err(e: russh::Error) -> DbError {
    DbError::Other(e.to_string())
}

/// 把 `Error::UnknownKey`（主机指纹未知或不匹配）映射为面向用户的 `Config` 错误：
/// `expected` 为空（未知主机）提示先确认指纹；否则提示不匹配（可能中间人攻击）并给出期望与实际指纹。
fn map_unknown_key_error(expected: Option<&str>, observed: Option<String>) -> DbError {
    match expected {
        None => DbError::Config("需先确认 SSH 主机指纹".to_string()),
        Some(exp) => DbError::Config(format!(
            "SSH 主机指纹不匹配（可能中间人攻击）：期望 {exp}，实际 {}",
            observed.unwrap_or_default()
        )),
    }
}

/// SSH kex（`connect`）阶段超时阈值：设计定死恰好 10 秒。
///
/// `russh::client::Config` 无 `connection_timeout` 字段，超时只能靠
/// `tokio::time::timeout` 包裹 `connect` 实现。认证（`authenticate_*`）阶段
/// 超时不在此函数覆盖范围（design §4.6 注，归后续项）。
fn ssh_connect_timeout() -> std::time::Duration {
    std::time::Duration::from_secs(10)
}

/// 计算 SSH 主机公钥的 OpenSSH 风格 SHA-256 指纹（`SHA256:<无 padding base64>`）。
///
/// `ClientHandler`/`ProbeHandler` 在 `check_server_key` 中调用本函数完成 TOFU 比对。
pub fn fingerprint(key: &russh::keys::PublicKey) -> String {
    key.fingerprint(russh::keys::HashAlg::Sha256).to_string()
}

/// 纯字符串比对：`expected` 与 `actual` 指纹是否一致（TOFU 校验核心，独立成纯函数便于单测）。
fn matches_fp(expected: &str, actual: &str) -> bool {
    expected == actual
}

/// 客户端 handler：按 `expected` 指纹校验主机密钥（TOFU）。
/// 校验结果总是写入 `observed` 槽，供失败后向用户展示实际指纹（Task 7）。
struct ClientHandler {
    expected: Option<String>,
    observed: Arc<Mutex<Option<String>>>,
}

impl russh::client::Handler for ClientHandler {
    type Error = russh::Error;

    async fn check_server_key(
        &mut self,
        server_public_key: &russh::keys::PublicKey,
    ) -> std::result::Result<bool, Self::Error> {
        let fp = fingerprint(server_public_key);
        *self.observed.lock().unwrap() = Some(fp.clone());
        match &self.expected {
            Some(exp) => Ok(matches_fp(exp, &fp)), // 不匹配 Ok(false) → russh 转 Err(Error::UnknownKey)
            None => Ok(false),                     // 未知主机：拒绝
        }
    }
}

/// 探针 handler：`check_server_key` 恒放行（`Ok(true)`）并把指纹写入 `slot`，
/// 由探针任务随后读取并断开（不认证）。
struct ProbeHandler {
    slot: Arc<Mutex<Option<String>>>,
}

impl russh::client::Handler for ProbeHandler {
    type Error = russh::Error;

    async fn check_server_key(
        &mut self,
        server_public_key: &russh::keys::PublicKey,
    ) -> std::result::Result<bool, Self::Error> {
        *self.slot.lock().unwrap() = Some(fingerprint(server_public_key));
        Ok(true) // 放行完成 kex；探针随后读槽并断开（不认证）
    }
}

/// 探针：仅完成 SSH kex（不认证、不转发），返回主机公钥指纹后立即断开。
///
/// `russh::client::connect` 在 kex 完成（`check_server_key` 已把指纹写入槽）后才返回；
/// 返回的 handle 直接 drop 即断开连接。
pub async fn probe_host_key(ssh: &SshOptions) -> Result<String> {
    let slot = Arc::new(Mutex::new(None));
    let config = Arc::new(russh::client::Config::default());
    let _handle = russh::client::connect(
        config,
        (ssh.host.clone(), ssh.port),
        ProbeHandler { slot: slot.clone() },
    )
    .await
    .map_err(ssh_err)?;
    let fp = slot
        .lock()
        .unwrap()
        .take()
        .ok_or_else(|| DbError::Other("未能取得主机指纹".to_string()))?;
    Ok(fp)
}

/// 活跃的 SSH 隧道：持有 SSH 会话与转发任务，MySQL 连接存续期间保持存活。
pub struct SshTunnel {
    pub local_port: u16,
    _handle: Arc<russh::client::Handle<ClientHandler>>,
    _task: tokio::task::JoinHandle<()>,
}

/// 认证方式：`private_key` 存在时优先公钥认证，否则回退密码认证。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AuthKind {
    PublicKey,
    Password,
}

/// 纯选择函数：`private_key` 存在 → `PublicKey`，否则 `Password`（供单测 + `start_tunnel` 分支）。
fn pick_auth(ssh: &SshOptions) -> AuthKind {
    if ssh.private_key.is_some() {
        AuthKind::PublicKey
    } else {
        AuthKind::Password
    }
}

/// 建立隧道：连 SSH → 认证 → 本地临时端口转发到 `target_host:target_port`。
pub async fn start_tunnel(
    ssh: &SshOptions,
    target_host: &str,
    target_port: u16,
) -> Result<SshTunnel> {
    let config = Arc::new(russh::client::Config::default());
    let observed = Arc::new(Mutex::new(None));
    let handler = ClientHandler {
        expected: ssh.host_key_fingerprint.clone(),
        observed: observed.clone(),
    };
    // 仅包裹 kex（connect）阶段；认证（authenticate_*）阶段超时归后续项（design §4.6 注）。
    let connect_result = tokio::time::timeout(
        ssh_connect_timeout(),
        russh::client::connect(config, (ssh.host.clone(), ssh.port), handler),
    )
    .await;
    let mut handle = match connect_result {
        Err(_) => return Err(DbError::Other("SSH 连接超时".to_string())),
        Ok(Err(russh::Error::UnknownKey)) => {
            // handler 已把实际指纹写入 observed 槽；按 expected 区分「未知主机」与「指纹不匹配」
            let actual = observed.lock().unwrap().take();
            return Err(map_unknown_key_error(
                ssh.host_key_fingerprint.as_deref(),
                actual,
            ));
        }
        Ok(Err(e)) => return Err(ssh_err(e)),
        Ok(Ok(h)) => h,
    };
    let auth = match pick_auth(ssh) {
        AuthKind::PublicKey => {
            // PEM 字符串（非文件路径）；加密私钥 passphrase 暂不支持（无 passphrase 入参）
            let pem = ssh
                .private_key
                .as_deref()
                .expect("pick_auth(PublicKey) 时 private_key 必为 Some");
            let key = russh::keys::decode_secret_key(pem, None)
                .map_err(|e| DbError::Other(format!("SSH 私钥解析失败: {e}")))?;
            let hash_alg = handle
                .best_supported_rsa_hash()
                .await
                .map_err(ssh_err)?
                .flatten(); // Result<Option<Option<HashAlg>>, Error> → Option<HashAlg>；RSA 取 rsa-sha2-256/512，非 RSA 忽略
            let key_with_alg = russh::keys::PrivateKeyWithHashAlg::new(Arc::new(key), hash_alg);
            handle
                .authenticate_publickey(ssh.user.clone(), key_with_alg)
                .await
                .map_err(ssh_err)?
        }
        AuthKind::Password => handle
            .authenticate_password(ssh.user.clone(), ssh.password.clone().unwrap_or_default())
            .await
            .map_err(ssh_err)?,
    };
    if !matches!(auth, russh::client::AuthResult::Success) {
        return Err(DbError::Other("SSH 认证失败".to_string()));
    }

    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0u16))
        .await
        .map_err(|e| DbError::Other(e.to_string()))?;
    let local_port = listener
        .local_addr()
        .map_err(|e| DbError::Other(e.to_string()))?
        .port();

    let handle = Arc::new(handle);
    let h = handle.clone();
    let target_host = target_host.to_string();
    let task = tokio::spawn(async move {
        loop {
            let (mut socket, _) = match listener.accept().await {
                Ok(x) => x,
                Err(_) => break,
            };
            let h2 = h.clone();
            let th = target_host.clone();
            tokio::spawn(async move {
                let channel = h2
                    .channel_open_direct_tcpip(
                        th,
                        target_port as u32,
                        "127.0.0.1".to_string(),
                        0,
                    )
                    .await
                    .map_err(ssh_err)?;
                let mut stream = channel.into_stream();
                let _ = tokio::io::copy_bidirectional(&mut socket, &mut stream).await;
                Ok::<(), DbError>(())
            });
        }
    });

    Ok(SshTunnel {
        local_port,
        _handle: handle,
        _task: task,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use russh::client::Handler;
    use std::sync::{Arc, Mutex};

    /// 固定 Ed25519 公钥（与 russh 自带测试同一把密钥），指纹由
    /// `ssh-keygen -lf` 实测：SHA256:ldyiXa1JQakitNU5tErauu8DvWQ1dZ7aXu+rm7KQuog
    const TEST_PUBLIC_KEY: &str =
        "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAILagOJFgwaMNhBWQINinKOXmqS4Gh5NgxgriXwdOoINJ";
    /// 第二把固定 Ed25519 公钥（取自 russh 自带测试数据），指纹与
    /// `TEST_PUBLIC_KEY` 不同，用于「指纹不匹配」用例。
    const TEST_PUBLIC_KEY_B: &str =
        "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIJdD7y3aLq454yWBdwLWbieU1ebz9/cu7/QEXn9OIeZJ";

    /// `pick_auth`：有 `private_key` 时选 `PublicKey`。
    #[test]
    fn pick_auth_with_private_key_selects_public_key() {
        let ssh = SshOptions {
            private_key: Some("-----BEGIN OPENSSH PRIVATE KEY-----".to_string()),
            ..Default::default()
        };
        assert_eq!(pick_auth(&ssh), AuthKind::PublicKey);
    }

    /// `pick_auth`：无 `private_key` 时选 `Password`。
    #[test]
    fn pick_auth_without_private_key_selects_password() {
        let ssh = SshOptions::default();
        assert_eq!(pick_auth(&ssh), AuthKind::Password);
    }

    #[test]
    fn fingerprint_matches_openssh_form() {
        let key = russh::keys::PublicKey::from_openssh(TEST_PUBLIC_KEY)
            .expect("fixed test key must parse");
        let fp = fingerprint(&key);
        // 与 `ssh-keygen -lf` 输出逐字符一致
        assert_eq!(fp, "SHA256:ldyiXa1JQakitNU5tErauu8DvWQ1dZ7aXu+rm7KQuog");
        assert!(fp.starts_with("SHA256:"));
        // SHA-256 无 padding base64 = 43 字符
        assert_eq!(fp.len(), "SHA256:".len() + 43);
    }

    /// `ClientHandler` 三种场景：预期匹配 → `Ok(true)`；预期不符 → `Ok(false)` 且
    /// observed 槽记录实际指纹；无预期（未知主机）→ `Ok(false)` 且 observed 仍记录实际指纹。
    #[tokio::test]
    async fn client_handler_compares_and_records_observed() {
        let key_a = russh::keys::PublicKey::from_openssh(TEST_PUBLIC_KEY)
            .expect("fixed test key A must parse");
        let key_b = russh::keys::PublicKey::from_openssh(TEST_PUBLIC_KEY_B)
            .expect("fixed test key B must parse");
        let fp_a = fingerprint(&key_a);
        let fp_b = fingerprint(&key_b);
        assert_ne!(fp_a, fp_b, "test keys must have distinct fingerprints");

        // 1) expected = Some(A)，实际为 A → Ok(true)，observed 记录 A
        let observed = Arc::new(Mutex::new(None));
        let mut handler = ClientHandler {
            expected: Some(fp_a.clone()),
            observed: observed.clone(),
        };
        assert!(handler
            .check_server_key(&key_a)
            .await
            .expect("no handler error"));
        assert_eq!(observed.lock().unwrap().as_deref(), Some(fp_a.as_str()));

        // 2) expected = Some(A)，实际为 B → Ok(false)，observed 记录 B
        let observed = Arc::new(Mutex::new(None));
        let mut handler = ClientHandler {
            expected: Some(fp_a.clone()),
            observed: observed.clone(),
        };
        assert!(!handler
            .check_server_key(&key_b)
            .await
            .expect("no handler error"));
        assert_eq!(observed.lock().unwrap().as_deref(), Some(fp_b.as_str()));

        // 3) expected = None（未知主机）→ Ok(false)，observed 记录实际指纹
        let observed = Arc::new(Mutex::new(None));
        let mut handler = ClientHandler {
            expected: None,
            observed: observed.clone(),
        };
        assert!(!handler
            .check_server_key(&key_a)
            .await
            .expect("no handler error"));
        assert_eq!(observed.lock().unwrap().as_deref(), Some(fp_a.as_str()));
    }

    /// `ProbeHandler`：`check_server_key` 恒 `Ok(true)` 且把实际指纹写入槽
    /// （`probe_host_key` 依赖的路径：放行完成 kex，探针随后读槽）。
    #[tokio::test]
    async fn probe_handler_writes_slot_and_accepts() {
        let key = russh::keys::PublicKey::from_openssh(TEST_PUBLIC_KEY)
            .expect("fixed test key must parse");
        let fp = fingerprint(&key);
        let slot = Arc::new(Mutex::new(None));
        let mut handler = ProbeHandler { slot: slot.clone() };
        assert!(handler
            .check_server_key(&key)
            .await
            .expect("no handler error"));
        assert_eq!(slot.lock().unwrap().as_deref(), Some(fp.as_str()));
    }

    /// `ssh_connect_timeout`：kex 阶段超时阈值，设计定死恰好 10 秒。
    #[test]
    fn ssh_connect_timeout() {
        assert_eq!(
            super::ssh_connect_timeout(),
            std::time::Duration::from_secs(10)
        );
    }

    /// `map_unknown_key_error`：expected=None（未知主机）→「需先确认」提示；
    /// expected=Some(X) 且 observed=Some(Y)（指纹不匹配）→ 错误信息同时包含期望与实际指纹。
    #[test]
    fn maps_unknown_key_error_by_expected() {
        // 未知主机（expected=None）→ 需先确认 SSH 主机指纹
        let err = map_unknown_key_error(None, Some("SHA256:actual".to_string()));
        match err {
            DbError::Config(msg) => assert_eq!(msg, "需先确认 SSH 主机指纹"),
            other => panic!("expected DbError::Config, got {other:?}"),
        }

        // 指纹不匹配（expected=Some(X)，observed=Some(Y)）→ 信息同时包含 X 与 Y
        let err = map_unknown_key_error(Some("SHA256:expected"), Some("SHA256:actual".to_string()));
        match err {
            DbError::Config(msg) => {
                assert!(
                    msg.contains("SHA256:expected"),
                    "message must contain the expected fingerprint: {msg}"
                );
                assert!(
                    msg.contains("SHA256:actual"),
                    "message must contain the actual fingerprint: {msg}"
                );
            }
            other => panic!("expected DbError::Config, got {other:?}"),
        }
    }
}
