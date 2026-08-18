//! vault.bin：加密凭据保险库密文的持久化。
//!
//! 文件内容为信封 JSON，字节字段以 base64 编码；密文本身由
//! vault_crypto 的认证加密保证完整性，这里只负责容错加载与原子写。
//! 该文件属于不可再生数据：损坏时直接报错，绝不自动重置。

use super::{envelope, envelope::CorruptPolicy, Storage};
use crate::{error::CommandError, models::VaultEnvelope};

/// 保险库密文文件名，位于应用配置目录。
pub(crate) const FILE_NAME: &str = "vault.bin";

/// 损坏时使用的安全错误码与提示，不暴露密文细节。
const CORRUPT: CorruptPolicy = CorruptPolicy::Reject {
    code: "VAULT_FILE_CORRUPT",
    message: "凭据保险库文件已损坏，为防止数据丢失已停止加载，请手动处理后重试",
};

impl Storage {
    /// 查询当前认证加密保险库密文记录，文件不存在返回 None。
    pub async fn get_vault_envelope(&self) -> Result<Option<VaultEnvelope>, CommandError> {
        envelope::load_json(&self.inner.config_dir.join(FILE_NAME), &CORRUPT)
    }

    /// 原子保存保险库密文记录。
    pub async fn save_vault_envelope(
        &self,
        envelope_value: &VaultEnvelope,
    ) -> Result<(), CommandError> {
        envelope::save_json(&self.inner.config_dir.join(FILE_NAME), envelope_value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::testutil;

    #[tokio::test]
    /// 密文记录写入后可完整读回，None 与 Some 语义清晰。
    async fn vault_envelope_roundtrip() {
        let (storage, _config, _vault) = testutil::test_storage().await;
        assert!(storage
            .get_vault_envelope()
            .await
            .expect("读取空保险库失败")
            .is_none());
        let envelope = crate::models::VaultEnvelope {
            format_version: 1,
            protection_mode: "default".to_string(),
            kdf_salt: vec![1, 2, 3, 4],
            kdf_iterations: None,
            nonce: vec![5, 6, 7],
            ciphertext: vec![8, 9],
        };
        storage
            .save_vault_envelope(&envelope)
            .await
            .expect("保存保险库失败");
        assert_eq!(
            storage.get_vault_envelope().await.expect("读取保险库失败"),
            Some(envelope)
        );
    }

    #[tokio::test]
    /// 损坏的保险库文件被拒绝加载而非静默重建。
    async fn corrupt_vault_file_is_rejected() {
        let (storage, config, _vault) = testutil::test_storage().await;
        std::fs::write(config.path().join(FILE_NAME), b"not json").expect("写损坏文件失败");
        let error = storage
            .get_vault_envelope()
            .await
            .expect_err("损坏文件不应加载");
        assert_eq!(error.code, "VAULT_FILE_CORRUPT");
    }
}
