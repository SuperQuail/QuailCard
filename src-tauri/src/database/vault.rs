use sqlx::SqliteConnection;

use super::{now_timestamp, Database};
use crate::{error::CommandError, models::VaultEnvelope};

impl Database {
    /// 查询当前认证加密保险库密文记录。
    pub async fn get_vault_envelope(&self) -> Result<Option<VaultEnvelope>, CommandError> {
        sqlx::query_as::<_, VaultEnvelope>(
            "SELECT format_version, protection_mode, kdf_salt, kdf_iterations, nonce, ciphertext
             FROM credential_vault WHERE id = 1",
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(CommandError::from)
    }

    /// 单独保存密码保护模式变化后的保险库密文记录。
    pub async fn save_vault_envelope(&self, envelope: &VaultEnvelope) -> Result<(), CommandError> {
        let mut transaction = self.pool.begin().await?;
        upsert_vault_envelope(&mut transaction, envelope).await?;
        transaction.commit().await?;
        Ok(())
    }

    /// 重置保险库并清除所有供应商的失效凭据引用。
    pub async fn reset_credential_vault(
        &self,
        envelope: &VaultEnvelope,
    ) -> Result<(), CommandError> {
        let mut transaction = self.pool.begin().await?;
        upsert_vault_envelope(&mut transaction, envelope).await?;
        sqlx::query(
            "UPDATE providers
             SET secret_ref = NULL, auth_type = NULL, oauth_account_id = NULL,
                 has_api_key = 0, status = 'untested', updated_at = ?",
        )
        .bind(now_timestamp())
        .execute(&mut *transaction)
        .await?;
        transaction.commit().await?;
        Ok(())
    }
}

/// 在调用方事务内插入或替换唯一保险库密文记录。
pub(super) async fn upsert_vault_envelope(
    connection: &mut SqliteConnection,
    envelope: &VaultEnvelope,
) -> Result<(), CommandError> {
    sqlx::query(
        "INSERT INTO credential_vault (
            id, format_version, protection_mode, kdf_salt, kdf_iterations,
            nonce, ciphertext, updated_at
         ) VALUES (1, ?, ?, ?, ?, ?, ?, ?)
         ON CONFLICT(id) DO UPDATE SET
            format_version = excluded.format_version,
            protection_mode = excluded.protection_mode,
            kdf_salt = excluded.kdf_salt,
            kdf_iterations = excluded.kdf_iterations,
            nonce = excluded.nonce,
            ciphertext = excluded.ciphertext,
            updated_at = excluded.updated_at",
    )
    .bind(envelope.format_version)
    .bind(&envelope.protection_mode)
    .bind(&envelope.kdf_salt)
    .bind(envelope.kdf_iterations)
    .bind(&envelope.nonce)
    .bind(&envelope.ciphertext)
    .bind(now_timestamp())
    .execute(connection)
    .await?;
    Ok(())
}
