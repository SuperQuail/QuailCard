//! 应用文件存储层：TOML 配置 + JSON 数据文件的唯一文件 IO 出口。
//!
//! ## 文件布局
//! - 应用配置目录：settings.toml（界面/行为设置）、providers.toml（供应商）、
//!   vault.bin（加密凭据保险库密文）；
//! - Vault 内 .quailcard/：镜像笔记目录树，每篇笔记一个卡片 JSON
//!   （卡片 + 调度状态 + 复习历史同文件），另有顶层 config.json 存附件目录。
//!
//! ## 演化契约（防迁移四律，新增依赖或字段时必须遵守）
//! 1. 读容忍：缺字段取默认值、未知字段忽略、禁止 deny_unknown_fields；
//! 2. 写完整：每次保存写出当前完整格式，废弃字段随保存自然消失；
//! 3. 只增不改：永不改名、删除或改类型已有字段，演化只允许新增可选字段；
//! 4. 版本熔断：文件 formatVersion 高于当前应用时拒绝打开，防止旧程序
//!    在保存时把看不懂的新字段写丢。
//!
//! ## 损坏策略
//! - 配置文件（settings/providers）损坏：备份为 .corrupt-<时间戳> 后重建默认；
//! - 卡片与保险库文件损坏：报错拒开，绝不自动重置（不可再生数据）。

mod card_files;
mod card_records;
mod cards;
pub(crate) mod envelope;
mod helpers;
mod notes;
mod providers;
mod providers_file;
mod review;
mod search;
mod settings;
mod vault_file;

use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use tokio::sync::Mutex;

use crate::error::CommandError;

/// 应用级文件存储：聚合 TOML 配置、加密保险库文件与 Vault 内卡片缓存。
///
/// 克隆共享同一内部状态；各存储域由独立锁串行化写入，
/// 所有落盘均通过临时文件 + rename 原子替换。锁只在同步代码段内持有，
/// 绝不跨 await，避免死锁与运行时阻塞。
#[derive(Clone)]
pub struct Storage {
    inner: Arc<StorageInner>,
}

/// 存储共享状态：配置目录与各存储域的内存镜像。
struct StorageInner {
    /// 应用级配置目录，settings.toml / providers.toml / vault.bin 所在地。
    config_dir: PathBuf,
    /// 全局设置内存镜像，写透到 settings.toml。
    settings: Mutex<settings::SettingsFile>,
    /// 供应商配置内存镜像，写透到 providers.toml。
    providers: Mutex<providers_file::ProvidersFile>,
    /// Vault 内卡片缓存（.quailcard/**/*.json 的内存镜像）。
    cards: cards::CardStore,
    /// 笔记内存索引，由 Vault 扫描重建，不再持久化。
    notes: notes::NoteIndex,
}

impl Storage {
    /// 打开（或首次创建）应用级文件存储。
    ///
    /// 配置文件缺失或损坏（可再生）时播种默认内容；不可再生文件
    /// （卡片、保险库）不在启动路径上，由各自命令首次访问时加载。
    pub fn open(config_dir: &Path) -> Result<Self, CommandError> {
        std::fs::create_dir_all(config_dir)?;
        let settings_path = config_dir.join(settings::FILE_NAME);
        let settings = match envelope::load_toml(
            &settings_path,
            &envelope::CorruptPolicy::BackupAndRegenerate,
        )? {
            Some(loaded) => loaded,
            None => {
                let seeded = settings::SettingsFile::default();
                envelope::save_toml(&settings_path, &seeded)?;
                seeded
            }
        };
        let providers_path = config_dir.join(providers_file::FILE_NAME);
        let providers = match envelope::load_toml(
            &providers_path,
            &envelope::CorruptPolicy::BackupAndRegenerate,
        )? {
            Some(loaded) => loaded,
            None => {
                let seeded = providers_file::seed_providers_file();
                envelope::save_toml(&providers_path, &seeded)?;
                seeded
            }
        };
        Ok(Self {
            inner: Arc::new(StorageInner {
                config_dir: config_dir.to_path_buf(),
                settings: Mutex::new(settings),
                providers: Mutex::new(providers),
                cards: cards::CardStore::default(),
                notes: notes::NoteIndex::default(),
            }),
        })
    }
}

impl Storage {
    /// 返回应用级配置目录，供设置界面展示数据位置。
    pub fn config_dir(&self) -> &Path {
        &self.inner.config_dir
    }

    /// 返回当前 Vault 的卡片数据目录；未打开 Vault 时为 None。
    pub fn cards_dir(&self) -> Option<PathBuf> {
        self.inner.cards.cards_dir()
    }
}

/// 返回当前 UTC Unix 秒数，作为各存储域统一的时间来源。
pub(crate) fn now_timestamp() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

/// 笔记摘要的统一排序键：先标题后路径，保证列表顺序稳定。
pub(super) fn notes_summary_sort_key(title: &str, path: &str) -> (String, String) {
    // SQLite 的 BINARY 排序按字节比较；这里统一小写近似标题排序，
    // 并以路径作次级键避免同名标题抖动。
    (title.to_lowercase(), path.to_string())
}

#[cfg(test)]
pub(crate) mod testutil {
    use super::*;

    /// 测试用临时目录句柄，Drop 时递归删除目录树。
    pub(crate) struct TempDir(PathBuf);

    impl TempDir {
        /// 在系统临时目录下创建唯一的测试目录。
        pub(crate) fn new() -> Self {
            let path =
                std::env::temp_dir().join(format!("quailcard-test-{}", uuid::Uuid::now_v7()));
            std::fs::create_dir_all(&path).expect("创建测试目录失败");
            Self(path)
        }

        /// 返回目录路径。
        pub(crate) fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TempDir {
        /// 测试结束后清理全部临时文件。
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// 创建带独立配置目录与 Vault 目录的存储实例，供各模块测试复用。
    ///
    /// 返回顺序为 (存储, 配置目录, Vault 目录)；两个临时目录句柄必须由
    /// 调用方持有，Drop 之前存储一直可用。
    pub(crate) async fn test_storage() -> (Storage, TempDir, TempDir) {
        let config = TempDir::new();
        let vault = TempDir::new();
        let storage = Storage::open(config.path()).expect("打开测试存储失败");
        storage
            .open_vault(vault.path(), &[])
            .await
            .expect("初始化测试 Vault 失败");
        (storage, config, vault)
    }
}
