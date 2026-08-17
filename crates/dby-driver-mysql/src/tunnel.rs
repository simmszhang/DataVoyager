//! SSH 隧道：russh 本地端口转发（MySQL 驱动通过隧道连接远端数据库）。

use std::sync::Arc;

use dby_core::driver::SshOptions;
use dby_core::error::{DbError, Result};

fn ssh_err(e: russh::Error) -> DbError {
    DbError::Other(e.to_string())
}

/// 客户端 handler：M1 spike 接受任意主机密钥（M2 做指纹确认）。
struct ClientHandler;

impl russh::client::Handler for ClientHandler {
    type Error = russh::Error;

    async fn check_server_key(
        &mut self,
        _server_public_key: &russh::keys::PublicKey,
    ) -> std::result::Result<bool, Self::Error> {
        Ok(true)
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
    let mut handle = russh::client::connect(config, (ssh.host.clone(), ssh.port), ClientHandler)
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
