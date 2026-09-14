//! settings.toml：界面与行为设置的容错加载与写透。
//!
//! 新增设置一律在对应小节增加可选字段；废弃设置直接删除字段，
//! 下次保存该键即从文件中消失（自清洁），无需任何迁移步骤。

use serde::{Deserialize, Serialize};

use super::{envelope, Storage};
use crate::{error::CommandError, paths, video::models::VideoSettings};

/// 设置文件名，位于应用配置目录。
pub(crate) const FILE_NAME: &str = "settings.toml";

/// 界面字号支持的四种合法档位。
const SUPPORTED_FONT_SIZES: [&str; 4] = ["compact", "standard", "comfortable", "large"];
/// 界面字号缺失或损坏时的默认档位。
const DEFAULT_FONT_SIZE: &str = "comfortable";
/// 历史打开 Vault 的最大记录数量。
const MAX_RECENT_VAULTS: usize = 10;

/// settings.toml 的完整结构；小节即模块，删除小节等于舍弃对应功能状态。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub(crate) struct SettingsFile {
    /// 信封版本，缺失按当前版本解析。
    pub(crate) format_version: u64,
    /// 界面相关设置。
    pub(crate) ui: UiSettings,
    /// Vault 相关状态。
    pub(crate) vault: VaultSettings,
    /// 视频转笔记设置。
    pub(crate) video: VideoSettings,
    pub(crate) agent: crate::agent_execution_settings::AgentExecutionSettings,
}

impl Default for SettingsFile {
    /// 缺文件时的默认设置：舒适字号、空历史。
    fn default() -> Self {
        Self {
            format_version: envelope::CURRENT_FORMAT_VERSION,
            ui: UiSettings::default(),
            vault: VaultSettings::default(),
            video: VideoSettings::default(),
            agent: crate::agent_execution_settings::AgentExecutionSettings::default(),
        }
    }
}

/// 界面小节：字号与 AI 评分开关。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub(crate) struct UiSettings {
    /// 界面字号档位。
    pub(crate) font_size: String,
    /// 问答卡是否启用 AI 判分。
    pub(crate) ai_grading_enabled: bool,
}

impl Default for UiSettings {
    /// 缺省使用舒适字号、关闭 AI 判分。
    fn default() -> Self {
        Self {
            font_size: DEFAULT_FONT_SIZE.to_string(),
            ai_grading_enabled: false,
        }
    }
}

/// Vault 小节：最近打开路径。
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub(crate) struct VaultSettings {
    /// 最近打开的 Vault 绝对路径，最新在前。
    pub(crate) recent_vaults: Vec<String>,
}

impl Storage {
    /// 根请求开始时捕获执行额度，运行中的树不跟随配置变化。
    pub(crate) async fn agent_execution_settings(
        &self,
    ) -> Result<crate::agent_execution_settings::AgentExecutionSettings, CommandError> {
        let settings = self.inner.settings.lock().await.agent.clone();
        settings.validate()?;
        Ok(settings)
    }

    /// 读取持久化的界面字号，缺失或非法时返回默认舒适档。
    pub async fn get_font_size(&self) -> String {
        let stored = self.inner.settings.lock().await.ui.font_size.clone();
        if SUPPORTED_FONT_SIZES.contains(&stored.as_str()) {
            stored
        } else {
            DEFAULT_FONT_SIZE.to_string()
        }
    }

    /// 校验并持久化界面字号档位，非法值直接拒绝。
    pub async fn set_font_size(&self, font_size: &str) -> Result<(), CommandError> {
        if !SUPPORTED_FONT_SIZES.contains(&font_size) {
            return Err(CommandError::validation("不支持的界面字号档位"));
        }
        let mut settings = self.inner.settings.lock().await;
        settings.ui.font_size = font_size.to_string();
        self.persist_settings(&settings)
    }

    /// 读取"使用问答时启用 AI 评分"设置，默认关闭。
    pub async fn get_ai_grading_enabled(&self) -> Result<bool, CommandError> {
        Ok(self.inner.settings.lock().await.ui.ai_grading_enabled)
    }

    /// 持久化"使用问答时启用 AI 评分"设置。
    pub async fn set_ai_grading_enabled(&self, enabled: bool) -> Result<(), CommandError> {
        let mut settings = self.inner.settings.lock().await;
        settings.ui.ai_grading_enabled = enabled;
        self.persist_settings(&settings)
    }

    /// 读取历史打开的 Vault 路径列表（最新在前，最多 10 个）。
    pub async fn get_recent_vaults(&self) -> Result<Vec<String>, CommandError> {
        // 兼容早期版本写入的扩展长度路径：读取时统一还原，界面不再出现 `\\?\` 前缀。
        let guard = self.inner.settings.lock().await;
        Ok(guard
            .vault
            .recent_vaults
            .iter()
            .map(paths::simplified)
            .collect())
    }

    /// 记录一次打开的 Vault：去重后移到最前并限制数量。
    pub async fn record_recent_vault(&self, path: &str) -> Result<(), CommandError> {
        let mut settings = self.inner.settings.lock().await;
        let list = &mut settings.vault.recent_vaults;
        // 历史记录可能存过 verbatim 写法；写入时整体自清洁，避免同一目录出现两条。
        for item in list.iter_mut() {
            *item = paths::simplified(&*item);
        }
        let path = paths::simplified(path);
        list.retain(|item| *item != path);
        list.insert(0, path);
        list.truncate(MAX_RECENT_VAULTS);
        self.persist_settings(&settings)
    }

    /// 读取视频转笔记设置，缺失字段已在解析时取默认值。
    pub async fn get_video_settings(&self) -> VideoSettings {
        self.inner.settings.lock().await.video.clone()
    }

    /// 保存视频转笔记设置；写入前不改变其他小节。
    pub async fn set_video_settings(&self, settings: &VideoSettings) -> Result<(), CommandError> {
        settings.validate()?;
        let mut current = self.inner.settings.lock().await;
        let mut next = current.clone();
        next.video = settings.clone();
        self.persist_settings(&next)?;
        *current = next;
        Ok(())
    }

    /// 把设置内存镜像写透到 settings.toml。
    fn persist_settings(&self, settings: &SettingsFile) -> Result<(), CommandError> {
        envelope::save_toml(&self.inner.config_dir.join(FILE_NAME), settings)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::testutil;

    #[tokio::test]
    /// 非法视频设置不能覆盖此前已保存的配置，重开后仍保持原值。
    async fn rejects_invalid_video_settings_without_mutation() {
        let (storage, config, _vault) = testutil::test_storage().await;
        let original = storage.get_video_settings().await;
        let mut changed = original.clone();
        changed.note_folder = "../outside".into();
        assert!(storage.set_video_settings(&changed).await.is_err());
        assert_eq!(storage.get_video_settings().await, original);
        let reopened = Storage::open(config.path()).unwrap();
        assert_eq!(reopened.get_video_settings().await, original);
    }

    #[tokio::test]
    /// 新存储应通过默认播种获得舒适字号。
    async fn defaults_to_comfortable_font_size() {
        let (storage, _config, _vault) = testutil::test_storage().await;
        assert_eq!(storage.get_font_size().await, "comfortable");
    }

    #[tokio::test]
    /// 四种合法字号均应可写入并重新读取。
    async fn persists_all_supported_font_sizes() {
        let (storage, _config, _vault) = testutil::test_storage().await;
        for value in SUPPORTED_FONT_SIZES {
            storage.set_font_size(value).await.unwrap();
            assert_eq!(storage.get_font_size().await, value);
        }
    }

    #[tokio::test]
    /// 非法字号应被拒绝且不改变已保存的值。
    async fn rejects_invalid_font_size() {
        let (storage, _config, _vault) = testutil::test_storage().await;
        storage.set_font_size("comfortable").await.unwrap();
        let error = storage.set_font_size("oversized").await.unwrap_err();
        assert_eq!(error.code, "VALIDATION_ERROR");
        assert_eq!(storage.get_font_size().await, "comfortable");
    }

    #[tokio::test]
    /// 最近 Vault 去重、置前并限制在 10 条以内。
    async fn records_recent_vaults_with_dedup() {
        let (storage, _config, _vault) = testutil::test_storage().await;
        for index in 0..12 {
            storage
                .record_recent_vault(&format!("C:\\vault-{index}"))
                .await
                .unwrap();
        }
        storage.record_recent_vault("C:\\vault-11").await.unwrap();
        let list = storage.get_recent_vaults().await.unwrap();
        assert_eq!(list.len(), 10);
        assert_eq!(list[0], "C:\\vault-11");
        // 循环写入 12 个后截断保留 vault-11..vault-2，置前的 vault-11 之外首位是 vault-10。
        assert_eq!(list[1], "C:\\vault-10");
        assert!(!list.contains(&"C:\\vault-0".to_string()));
    }

    #[cfg(windows)]
    #[tokio::test]
    /// verbatim 写法与普通写法指向同一目录：入库后只保留一条且不再带前缀。
    async fn normalizes_verbatim_recent_vaults() {
        let (storage, _config, _vault) = testutil::test_storage().await;
        storage.record_recent_vault(r"\\?\C:\vault").await.unwrap();
        storage.record_recent_vault(r"C:\vault").await.unwrap();
        assert_eq!(
            storage.get_recent_vaults().await.unwrap(),
            vec![r"C:\vault".to_string()]
        );
    }
}
