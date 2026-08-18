//! SSH 隧道：russh 本地端口转发（MySQL 驱动通过隧道连接远端数据库）。

use std::sync::{Arc, Mutex};

use dby_core::driver::SshOptions;
use dby_core::error::{DbError, Result};

fn ssh_err(e: russh::Error) -> DbError {
    DbError::Other(e.to_string())
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
/// 由探针任务随后读取并断开（不认证）。Task 4 之前无生产调用点，故允许 dead_code。
#[allow(dead_code)]
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

/// 活跃的 SSH 隧道：持有 SSH 会话与转发任务，MySQL 连接存续期间保持存活。
pub struct SshTunnel {
    pub local_port: u16,
    _handle: Arc<russh::client::Handle<ClientHandler>>,
    _task: tokio::task::JoinHandle<()>,
}

/// 建立隧道：连 SSH → 认证 → 本地临时端口转发到 `target_host:target_port`。
pub async fn start_tunnel(
    ssh: &SshOptions,
    target_host: &str,
    target_port: u16,
) -> Result<SshTunnel> {
    let config = Arc::new(russh::client::Config::default());
    let handler = ClientHandler {
        expected: ssh.host_key_fingerprint.clone(),
        observed: Arc::new(Mutex::new(None)),
    };
    let mut handle = russh::client::connect(config, (ssh.host.clone(), ssh.port), handler)
        .await
        .map_err(ssh_err)?;
    let auth = handle
        .authenticate_password(ssh.user.clone(), ssh.password.clone().unwrap_or_default())
        .await
        .map_err(ssh_err)?;
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
}
