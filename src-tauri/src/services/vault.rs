use super::AppServices;
use crate::{error::CommandError, models::VaultStatus, storage::Storage};

impl AppServices {
    /// 查询凭据保险库的保护模式和锁定状态。
    pub async fn get_vault_status(&self, storage: &Storage) -> Result<VaultStatus, CommandError> {
        self.vault.status(storage).await
    }

    /// 使用用户密码解锁当前保险库会话。
    pub async fn unlock_vault(
        &self,
        storage: &Storage,
        password: &str,
    ) -> Result<VaultStatus, CommandError> {
        self.vault.unlock(storage, password).await
    }

    /// 设置或更换保险库密码并重新加密全部凭据。
    pub async fn set_vault_password(
        &self,
        storage: &Storage,
        password: &str,
    ) -> Result<VaultStatus, CommandError> {
        self.vault.set_password(storage, password).await
    }

    /// 移除保险库密码并恢复默认保护模式。
    pub async fn remove_vault_password(
        &self,
        storage: &Storage,
    ) -> Result<VaultStatus, CommandError> {
        self.vault.remove_password(storage).await
    }

    /// 清除会话内密码派生密钥。
    pub async fn lock_vault(&self, storage: &Storage) -> Result<VaultStatus, CommandError> {
        self.oauth.ensure_no_pending_attempt().await?;
        self.vault.lock(storage).await
    }

    /// 丢弃全部供应商凭据并恢复默认保护模式。
    pub async fn reset_vault(&self, storage: &Storage) -> Result<VaultStatus, CommandError> {
        self.oauth.ensure_no_pending_attempt().await?;
        self.vault.reset(storage).await
    }
}
