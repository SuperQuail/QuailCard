use super::AppServices;
use crate::{database::Database, error::CommandError, models::VaultStatus};

impl AppServices {
    /// 查询凭据保险库的保护模式和锁定状态。
    pub async fn get_vault_status(&self, database: &Database) -> Result<VaultStatus, CommandError> {
        self.vault.status(database).await
    }

    /// 使用用户密码解锁当前保险库会话。
    pub async fn unlock_vault(
        &self,
        database: &Database,
        password: &str,
    ) -> Result<VaultStatus, CommandError> {
        self.vault.unlock(database, password).await
    }

    /// 设置或更换保险库密码并重新加密全部凭据。
    pub async fn set_vault_password(
        &self,
        database: &Database,
        password: &str,
    ) -> Result<VaultStatus, CommandError> {
        self.vault.set_password(database, password).await
    }

    /// 移除保险库密码并恢复默认保护模式。
    pub async fn remove_vault_password(
        &self,
        database: &Database,
    ) -> Result<VaultStatus, CommandError> {
        self.vault.remove_password(database).await
    }

    /// 清除会话内密码派生密钥。
    pub async fn lock_vault(&self, database: &Database) -> Result<VaultStatus, CommandError> {
        self.oauth.ensure_no_pending_attempt().await?;
        self.vault.lock(database).await
    }

    /// 丢弃全部供应商凭据并恢复默认保护模式。
    pub async fn reset_vault(&self, database: &Database) -> Result<VaultStatus, CommandError> {
        self.oauth.ensure_no_pending_attempt().await?;
        self.vault.reset(database).await
    }
}
