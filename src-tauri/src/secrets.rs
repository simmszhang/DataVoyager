use dby_core::error::{DbError, Result};

/// 钥匙串凭据的种类：MySQL 密码 / SSH 密码 / SSH 私钥。
///
/// SSH 多键读写将在 #22 后续任务中接入命令层，当前仅测试引用，
/// 非测试构建下暂为 dead code，故显式允许。
#[allow(dead_code)]
pub enum SecretKind {
    MysqlPassword,
    SshPassword,
    SshPrivateKey,
}

/// 每种凭据在钥匙串中使用的稳定键名：
/// - MySQL 密码：`{config_id}`
/// - SSH 密码：`{config_id}:ssh`
/// - SSH 私钥：`{config_id}:ssh_key`
#[allow(dead_code)]
pub fn secret_key(config_id: &str, kind: SecretKind) -> String {
    match kind {
        SecretKind::MysqlPassword => config_id.to_string(),
        SecretKind::SshPassword => format!("{config_id}:ssh"),
        SecretKind::SshPrivateKey => format!("{config_id}:ssh_key"),
    }
}

pub fn set_secret(key: &str, value: &str) -> Result<()> {
    keyring::Entry::new("dby", key)
        .map_err(|e| DbError::Other(format!("keychain error: {e}")))?
        .set_password(value)
        .map_err(|e| DbError::Other(format!("keychain error: {e}")))
}

pub fn get_secret(key: &str) -> Result<String> {
    keyring::Entry::new("dby", key)
        .map_err(|e| DbError::Other(format!("keychain error: {e}")))?
        .get_password()
        .map_err(|e| DbError::Other(format!("keychain error: {e}")))
}

pub fn delete_secret(key: &str) -> Result<()> {
    keyring::Entry::new("dby", key)
        .map_err(|e| DbError::Other(format!("keychain error: {e}")))?
        .delete_credential()
        .map_err(|e| DbError::Other(format!("keychain error: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secret_keys_are_stable() {
        assert_eq!(secret_key("abc", SecretKind::MysqlPassword), "abc");
        assert_eq!(secret_key("abc", SecretKind::SshPassword), "abc:ssh");
        assert_eq!(secret_key("abc", SecretKind::SshPrivateKey), "abc:ssh_key");
    }
}
