//! 认证加密保险库：密文持久化于 vault.bin，会话内管理密码派生密钥。

use std::sync::Arc;

use tokio::sync::{Mutex, OwnedMutexGuard, RwLock};
use zeroize::Zeroizing;

use crate::{
    error::CommandError,
    models::VaultEnvelope,
    storage::Storage,
    vault_crypto::{
        decrypt_payload_with_key, derive_default_key, derive_password_key, encrypt_new_default,
        encrypt_new_password, encrypt_payload, invalid_envelope, validate_credential,
        validate_envelope, validate_password, VaultPayload, KEY_LENGTH, MODE_DEFAULT,
        MODE_PASSWORD,
    },
};

/// 管理 vault.bin 认证加密保险库和会话内密码密钥。
#[derive(Clone)]
pub struct EncryptedVault {
    unlocked_password_key: Arc<RwLock<Option<Zeroizing<[u8; KEY_LENGTH]>>>>,
    operation_lock: Arc<Mutex<()>>,
}

impl Default for EncryptedVault {
    /// 创建默认的进程内保险库状态。
    fn default() -> Self {
        Self::new()
    }
}

impl EncryptedVault {
    /// 创建尚未加载持久化密文记录的保险库服务。
    pub fn new() -> Self {
        Self {
            unlocked_password_key: Arc::new(RwLock::new(None)),
            operation_lock: Arc::new(Mutex::new(())),
        }
    }

    /// 首次启动时创建使用默认保护的空保险库。
    pub async fn initialize(&self, storage: &Storage) -> Result<(), CommandError> {
        let _guard = self.lock_operations().await;
        if storage.get_vault_envelope().await?.is_some() {
            return Ok(());
        }
        let envelope = encrypt_new_default(&VaultPayload::empty())?;
        storage.save_vault_envelope(&envelope).await
    }

    /// 获取串行化凭据和保护模式变更的操作锁。
    pub async fn lock_operations(&self) -> OwnedMutexGuard<()> {
        self.operation_lock.clone().lock_owned().await
    }

    /// 读取指定随机引用对应的供应商凭据。
    pub async fn get_credential(
        &self,
        storage: &Storage,
        secret_ref: &str,
    ) -> Result<String, CommandError> {
        let envelope = require_envelope(storage).await?;
        let payload = self.decrypt_payload(&envelope).await?;
        payload.credentials.get(secret_ref).cloned().ok_or_else(|| {
            CommandError::new(
                "PROVIDER_CREDENTIAL_MISSING",
                "加密保险库中不存在供应商凭据",
            )
        })
    }

    /// 在内存载荷中写入新凭据、移除旧引用并生成待提交的新密文记录。
    pub async fn prepare_set_credential(
        &self,
        storage: &Storage,
        secret_ref: &str,
        credential: &str,
        old_secret_ref: Option<&str>,
    ) -> Result<VaultEnvelope, CommandError> {
        validate_credential(secret_ref, credential)?;
        let current = require_envelope(storage).await?;
        let mut payload = self.decrypt_payload(&current).await?;
        payload
            .credentials
            .insert(secret_ref.to_string(), credential.to_string());
        if let Some(old_ref) = old_secret_ref.filter(|old_ref| *old_ref != secret_ref) {
            payload.credentials.remove(old_ref);
        }
        self.encrypt_existing_mode(&payload, &current).await
    }

    /// 在内存中删除凭据并生成待提交的新密文记录。
    pub async fn prepare_delete_credential(
        &self,
        storage: &Storage,
        secret_ref: &str,
    ) -> Result<VaultEnvelope, CommandError> {
        let current = require_envelope(storage).await?;
        let mut payload = self.decrypt_payload(&current).await?;
        payload.credentials.remove(secret_ref);
        self.encrypt_existing_mode(&payload, &current).await
    }

    /// 返回当前保护模式和会话锁定状态。
    pub async fn status(
        &self,
        storage: &Storage,
    ) -> Result<crate::models::VaultStatus, CommandError> {
        let envelope = require_envelope(storage).await?;
        let locked = envelope.protection_mode == MODE_PASSWORD
            && self.unlocked_password_key.read().await.is_none();
        Ok(crate::models::VaultStatus {
            protection_mode: envelope.protection_mode,
            locked,
        })
    }

    /// 使用用户密码验证并解锁密码保护保险库。
    pub async fn unlock(
        &self,
        storage: &Storage,
        password: &str,
    ) -> Result<crate::models::VaultStatus, CommandError> {
        validate_password(password)?;
        let _guard = self.lock_operations().await;
        let envelope = require_envelope(storage).await?;
        if envelope.protection_mode != MODE_PASSWORD {
            return self.status(storage).await;
        }
        let key = derive_password_key(&envelope, password)?;
        decrypt_payload_with_key(&envelope, &key).map_err(|_| {
            CommandError::new("VAULT_PASSWORD_INVALID", "密码错误或保险库数据已损坏")
        })?;
        *self.unlocked_password_key.write().await = Some(key);
        self.status(storage).await
    }

    /// 将当前已解锁保险库重新加密为新的密码保护模式。
    pub async fn set_password(
        &self,
        storage: &Storage,
        password: &str,
    ) -> Result<crate::models::VaultStatus, CommandError> {
        validate_password(password)?;
        let _guard = self.lock_operations().await;
        let current = require_envelope(storage).await?;
        let payload = self.decrypt_payload(&current).await?;
        let (next, key) = encrypt_new_password(&payload, password)?;
        storage.save_vault_envelope(&next).await?;
        *self.unlocked_password_key.write().await = Some(key);
        self.status(storage).await
    }

    /// 将已解锁的密码保险库重新加密回默认保护模式。
    pub async fn remove_password(
        &self,
        storage: &Storage,
    ) -> Result<crate::models::VaultStatus, CommandError> {
        let _guard = self.lock_operations().await;
        let current = require_envelope(storage).await?;
        if current.protection_mode != MODE_PASSWORD {
            return self.status(storage).await;
        }
        let payload = self.decrypt_payload(&current).await?;
        let next = encrypt_new_default(&payload)?;
        storage.save_vault_envelope(&next).await?;
        *self.unlocked_password_key.write().await = None;
        self.status(storage).await
    }

    /// 清除会话内密码密钥并立即锁定保险库。
    pub async fn lock(
        &self,
        storage: &Storage,
    ) -> Result<crate::models::VaultStatus, CommandError> {
        let _guard = self.lock_operations().await;
        *self.unlocked_password_key.write().await = None;
        self.status(storage).await
    }

    /// 丢弃全部凭据并恢复默认保护，供忘记密码时重新配置。
    pub async fn reset(
        &self,
        storage: &Storage,
    ) -> Result<crate::models::VaultStatus, CommandError> {
        let _guard = self.lock_operations().await;
        let envelope = encrypt_new_default(&VaultPayload::empty())?;
        storage.reset_credential_vault(&envelope).await?;
        *self.unlocked_password_key.write().await = None;
        self.status(storage).await
    }

    /// 按照密文记录中的保护模式解密凭据映射。
    async fn decrypt_payload(
        &self,
        envelope: &VaultEnvelope,
    ) -> Result<VaultPayload, CommandError> {
        let key = self.key_for_envelope(envelope).await?;
        decrypt_payload_with_key(envelope, &key)
    }

    /// 使用原保护参数和全新 nonce 重新加密凭据映射。
    async fn encrypt_existing_mode(
        &self,
        payload: &VaultPayload,
        current: &VaultEnvelope,
    ) -> Result<VaultEnvelope, CommandError> {
        let key = self.key_for_envelope(current).await?;
        encrypt_payload(
            payload,
            &key,
            &current.protection_mode,
            current.kdf_salt.clone(),
            current.kdf_iterations,
        )
    }

    /// 根据默认密钥材料或会话内密码密钥确定当前解密密钥。
    async fn key_for_envelope(
        &self,
        envelope: &VaultEnvelope,
    ) -> Result<Zeroizing<[u8; KEY_LENGTH]>, CommandError> {
        validate_envelope(envelope)?;
        match envelope.protection_mode.as_str() {
            MODE_DEFAULT => derive_default_key(&envelope.kdf_salt),
            MODE_PASSWORD => {
                let keys = self.unlocked_password_key.read().await;
                let key = keys.as_ref().ok_or_else(|| {
                    CommandError::new("VAULT_LOCKED", "凭据保险库已锁定，请先输入密码解锁")
                })?;
                Ok(Zeroizing::new(**key))
            }
            _ => Err(invalid_envelope()),
        }
    }
}

/// 查询必须存在的唯一保险库密文记录。
async fn require_envelope(storage: &Storage) -> Result<VaultEnvelope, CommandError> {
    storage
        .get_vault_envelope()
        .await?
        .ok_or_else(|| CommandError::new("VAULT_NOT_INITIALIZED", "凭据保险库尚未初始化"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::testutil;
    use crate::vault_crypto::MODE_DEFAULT;

    /// 创建已经初始化默认保险库的存储实例；临时目录随返回值保持存活。
    async fn test_vault() -> (
        Storage,
        EncryptedVault,
        testutil::TempDir,
        testutil::TempDir,
    ) {
        let (storage, config, vault_dir) = testutil::test_storage().await;
        let vault_service = EncryptedVault::new();
        vault_service
            .initialize(&storage)
            .await
            .expect("初始化保险库失败");
        (storage, vault_service, config, vault_dir)
    }

    #[tokio::test]
    /// 默认保护可以读写凭据且密文文件不包含原文。
    async fn default_mode_encrypts_credentials() {
        let (storage, vault_service, _config, _dir) = test_vault().await;
        let credential = "sk-sensitive-value";
        let envelope = vault_service
            .prepare_set_credential(&storage, "secret-1", credential, None)
            .await
            .expect("加密凭据失败");
        assert!(!envelope
            .ciphertext
            .windows(credential.len())
            .any(|window| window == credential.as_bytes()));
        storage
            .save_vault_envelope(&envelope)
            .await
            .expect("保存保险库失败");
        assert_eq!(
            vault_service
                .get_credential(&storage, "secret-1")
                .await
                .expect("读取凭据失败"),
            credential
        );
    }

    #[tokio::test]
    /// 相同载荷重复加密会使用不同 nonce 和密文。
    async fn encryption_uses_fresh_nonce() {
        let (storage, vault_service, _config, _dir) = test_vault().await;
        let first = vault_service
            .prepare_set_credential(&storage, "secret-1", "value", None)
            .await
            .expect("第一次加密失败");
        let second = vault_service
            .prepare_set_credential(&storage, "secret-1", "value", None)
            .await
            .expect("第二次加密失败");
        assert_ne!(first.nonce, second.nonce);
        assert_ne!(first.ciphertext, second.ciphertext);
    }

    #[tokio::test]
    /// 密码保护在模拟重启的锁定后只接受正确密码。
    async fn password_mode_requires_unlock() {
        let (storage, vault_service, _config, _dir) = test_vault().await;
        let envelope = vault_service
            .prepare_set_credential(&storage, "secret-1", "oauth-token", None)
            .await
            .expect("加密凭据失败");
        storage
            .save_vault_envelope(&envelope)
            .await
            .expect("保存保险库失败");
        vault_service
            .set_password(&storage, "correct-password")
            .await
            .expect("设置密码失败");
        vault_service.lock(&storage).await.expect("锁定保险库失败");
        assert_eq!(
            vault_service
                .get_credential(&storage, "secret-1")
                .await
                .expect_err("锁定后不应读取凭据")
                .code,
            "VAULT_LOCKED"
        );
        assert_eq!(
            vault_service
                .unlock(&storage, "wrong-password")
                .await
                .expect_err("错误密码不应解锁")
                .code,
            "VAULT_PASSWORD_INVALID"
        );
        vault_service
            .unlock(&storage, "correct-password")
            .await
            .expect("正确密码应解锁");
        assert_eq!(
            vault_service
                .get_credential(&storage, "secret-1")
                .await
                .expect("解锁后读取凭据失败"),
            "oauth-token"
        );
        let status = vault_service
            .remove_password(&storage)
            .await
            .expect("移除密码失败");
        assert_eq!(status.protection_mode, MODE_DEFAULT);
        vault_service
            .lock(&storage)
            .await
            .expect("默认模式锁定调用失败");
        assert_eq!(
            vault_service
                .get_credential(&storage, "secret-1")
                .await
                .expect("恢复默认保护后读取凭据失败"),
            "oauth-token"
        );
    }

    #[tokio::test]
    /// 修改认证密文任意字节都会导致完整性校验失败。
    async fn tampering_is_rejected() {
        let (storage, vault_service, _config, _dir) = test_vault().await;
        let mut envelope = vault_service
            .prepare_set_credential(&storage, "secret-1", "value", None)
            .await
            .expect("加密凭据失败");
        let last = envelope.ciphertext.last_mut().expect("密文不能为空");
        *last ^= 0x01;
        storage
            .save_vault_envelope(&envelope)
            .await
            .expect("保存损坏测试密文失败");
        assert_eq!(
            vault_service
                .get_credential(&storage, "secret-1")
                .await
                .expect_err("损坏密文不应解密")
                .code,
            "VAULT_CRYPTO_ERROR"
        );
    }

    #[tokio::test]
    /// 忘记密码重置会同时清空保险库和供应商凭据元数据。
    async fn reset_clears_provider_credentials() {
        let (storage, vault_service, _config, _dir) = test_vault().await;
        let envelope = vault_service
            .prepare_set_credential(&storage, "secret-1", "value", None)
            .await
            .expect("加密凭据失败");
        storage
            .replace_provider_credential(
                "openai",
                Some("secret-1"),
                Some("api_key"),
                None,
                &envelope,
            )
            .await
            .expect("保存供应商凭据失败");
        vault_service
            .set_password(&storage, "correct-password")
            .await
            .expect("设置密码失败");
        vault_service.lock(&storage).await.expect("锁定保险库失败");
        let status = vault_service.reset(&storage).await.expect("重置保险库失败");
        assert_eq!(status.protection_mode, MODE_DEFAULT);
        assert!(!status.locked);
        assert!(
            !storage
                .get_provider_summary("openai")
                .await
                .expect("查询供应商失败")
                .has_credential
        );
        assert!(vault_service
            .get_credential(&storage, "secret-1")
            .await
            .is_err());
    }
}
