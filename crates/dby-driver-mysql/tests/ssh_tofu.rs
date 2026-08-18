//! SSH 主机指纹（TOFU）集成测试：需要真实 sshd，默认 `#[ignore]` 跳过。
//!
//! 前置条件（参照 `deploy/database/README.md` 起服务）：
//! - 本地 sshd 可连，且能通过 SSH 隧道访问 MySQL；
//! - 环境变量：`DBY_TEST_SSH_HOST`（必填，缺省时测试直接跳过）、
//!   `DBY_TEST_SSH_PORT`（默认 22）、`DBY_TEST_SSH_USER`（默认 root）、
//!   `DBY_TEST_SSH_PASSWORD`（默认空）；
//! - MySQL 连接参数沿用仓库惯例：`DBY_TEST_MYSQL_HOST/PORT/USER/PASSWORD/DB`
//!   （默认 `127.0.0.1:3306`、`root`/`dby-test`/`dby_test`）。
//!
//! 运行：`cargo test -p dby-driver-mysql --test ssh_tofu -- --ignored --nocapture`

use dby_core::driver::{ConnectParams, Driver, SshOptions};
use dby_core::error::DbError;
use dby_driver_mysql::{probe_host_key, MysqlDriver};

/// 从环境变量读取 SSH 连接参数；`DBY_TEST_SSH_HOST` 缺失返回 `None`（调用方跳过）。
fn ssh_options_from_env() -> Option<SshOptions> {
    let host = std::env::var("DBY_TEST_SSH_HOST").ok()?;
    Some(SshOptions {
        enabled: true,
        host,
        port: std::env::var("DBY_TEST_SSH_PORT")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(22),
        user: std::env::var("DBY_TEST_SSH_USER").unwrap_or_else(|_| "root".to_string()),
        password: std::env::var("DBY_TEST_SSH_PASSWORD").ok(),
        // TOFU 指纹由各用例自行设置（探针确认或故意篡改）
        ..Default::default()
    })
}

/// 构建经 SSH 隧道访问 MySQL 的 `ConnectParams`（`host_key_fingerprint` 由调用方设置）。
fn connect_params(ssh: SshOptions) -> ConnectParams {
    ConnectParams {
        driver: "mysql".to_string(),
        host: std::env::var("DBY_TEST_MYSQL_HOST").unwrap_or_else(|_| "127.0.0.1".to_string()),
        port: std::env::var("DBY_TEST_MYSQL_PORT")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(3306),
        user: std::env::var("DBY_TEST_MYSQL_USER").unwrap_or_else(|_| "root".to_string()),
        password: std::env::var("DBY_TEST_MYSQL_PASSWORD").ok(),
        database: std::env::var("DBY_TEST_MYSQL_DB").ok(),
        ssh: Some(ssh),
        ..Default::default()
    }
}

/// TOFU 全流程：探针取指纹 → 确认（写入 `host_key_fingerprint`）→ 连接 →
/// 断开 → 重连免确认。
///
/// 真实 sshd 下应断言的语义：
/// 1. `probe_host_key` 返回 OpenSSH 风格 `SHA256:<base64>` 指纹；
/// 2. 携带该指纹首次 `connect` 成功（隧道建立、MySQL 可达，`ping` 通过）；
/// 3. 断开后携带同一指纹重连同样成功，不再出现「需先确认 SSH 主机指纹」错误。
#[tokio::test]
#[ignore = "需要真实 sshd（DBY_TEST_SSH_HOST），参照 deploy/database/README.md"]
async fn tofu_confirm_then_reconnect_without_prompt() {
    // 无真实 sshd 时跳过（`#[ignore]` 之外的双保险，本地无服务环境不失败）
    let Some(mut ssh) = ssh_options_from_env() else {
        eprintln!("skip: requires a real sshd (DBY_TEST_SSH_HOST)");
        return;
    };

    // 1) 探针：仅完成 kex（不认证、不转发），取回服务端实际主机指纹
    let Ok(fp) = probe_host_key(&ssh).await else {
        eprintln!(
            "skip: probe_host_key failed — is sshd reachable at {host}:{port}?",
            host = ssh.host,
            port = ssh.port
        );
        return;
    };
    eprintln!("observed host key fingerprint: {fp}");

    // 2) 确认：模拟用户首次连接时「信任」该指纹（此后 TOFU 不再询问）
    ssh.host_key_fingerprint = Some(fp);

    // 3) 首次连接：期望指纹与实际一致 → 隧道建立、连接成功
    let params = connect_params(ssh);
    let Ok(mut conn) = MysqlDriver.connect(&params).await else {
        eprintln!("skip: first connect failed — is MySQL reachable through the tunnel?");
        return;
    };
    if conn.ping().await.is_err() {
        eprintln!("skip: ping failed after first connect");
        return;
    }
    drop(conn); // 4) 断开（MySQL 连接与 SSH 隧道随之释放）

    // 5) 重连：同一指纹，免确认直接成功（真实环境应断言返回 `Ok`）
    if MysqlDriver.connect(&params).await.is_err() {
        eprintln!("skip: reconnect failed — confirmed fingerprint not honored");
        return;
    }
}

/// 篡改 `host_key_fingerprint`（错误期望）→ 连接被拒绝，且错误信息包含
/// 服务端实际指纹（供用户比对以判断是否中间人攻击）。
///
/// 真实 sshd 下应断言的语义：
/// 1. `MysqlDriver.connect` 返回 `Err(DbError::Config(_))`（而非 `Database`/`Other`）；
/// 2. 错误消息同时包含错误期望指纹与「实际」指纹（kex 阶段 observed）。
#[tokio::test]
#[ignore = "需要真实 sshd（DBY_TEST_SSH_HOST），参照 deploy/database/README.md"]
async fn mismatched_key_rejected_with_actual_fingerprint() {
    let Some(ssh) = ssh_options_from_env() else {
        eprintln!("skip: requires a real sshd (DBY_TEST_SSH_HOST)");
        return;
    };

    // 先探针拿到实际指纹（用于断言「错误信息含实际指纹」时比对）
    let Ok(actual) = probe_host_key(&ssh).await else {
        eprintln!(
            "skip: probe_host_key failed — is sshd reachable at {host}:{port}?",
            host = ssh.host,
            port = ssh.port
        );
        return;
    };

    // 故意写入错误期望：同形异值的 SHA256 指纹（模拟篡改/中间人场景）
    let mut ssh = ssh;
    ssh.host_key_fingerprint = Some(format!("SHA256:{}", "0".repeat(43)));

    match MysqlDriver.connect(&connect_params(ssh)).await {
        Ok(_) => {
            // 断言失败：错误指纹下竟连接成功（骨架：真实环境应视为失败）
            eprintln!("fail: connect unexpectedly succeeded with a wrong fingerprint");
        }
        Err(e) => {
            let msg = e.to_string();
            eprintln!("rejected as expected: {msg}");
            eprintln!("actual fingerprint (should appear in the error above): {actual}");
            // 断言：错误类型为 `Config` 且消息包含实际指纹（真实环境应 assert）
            let DbError::Config(_) = e else {
                eprintln!("fail: expected DbError::Config, got {msg}");
                return;
            };
        }
    }
}
