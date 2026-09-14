//! Vault 内视频任务存储：任务记录与两级转录缓存。
//!
//! 新目录：<Vault>/.quailcard/agent/.video/；旧 video/ 仅做只读兼容。
//! - tasks/<taskId>/{task.json, transcript.json, audio_p*.m4s, video_p*.m4s, shots/*.jpg}
//! - pages/<videoKey>-<cid>.json（分 P 级转录音频缓存，跨任务复用）
//!
//! 转录可再生，损坏按重建处理；单条任务记录损坏只影响该任务。

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::{envelope, now_timestamp};

#[path = "video_cleanup.rs"]
mod cleanup;
#[path = "video_paths.rs"]
mod paths;
#[cfg(test)]
#[path = "video_tests.rs"]
mod tests;
use crate::{error::CommandError, video::transcript::Transcript};

/// 任务记录文件名。
const TASK_FILE: &str = "task.json";
/// 转录文件名。
const TRANSCRIPT_FILE: &str = "transcript.json";
/// 当前记录格式版本。
const FORMAT_VERSION: u64 = 1;

/// 任务发起来源：用户手动发起与 Agent 自动发起必须分开，Agent 任务不进用户历史。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum TaskOrigin {
    /// 用户在工作区手动发起；旧记录没有该字段，按此处理。
    #[default]
    User,
    /// 学习 Agent 通过视频工具自动发起。
    Agent,
}

/// 单个视频任务的持久化记录。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub(crate) struct VideoTaskRecord {
    pub format_version: u64,
    pub task_id: String,
    /// 规范化复用键，分 P 参与。
    pub cache_key: String,
    pub source_url: String,
    pub title: String,
    pub owner: String,
    pub duration: f64,
    pub quality: Option<u32>,
    pub pages: Vec<u32>,
    pub state: String,
    pub step: String,
    pub progress: u8,
    pub transcript_source: String,
    pub note_path: Option<String>,
    pub error: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
    /// 发起来源；缺失该字段的旧记录按用户手动任务读取。
    pub origin: TaskOrigin,
}

impl VideoTaskRecord {
    /// 新建任务记录：状态为排队中，时间戳统一取自存储层。
    pub(crate) fn new(task_id: &str, cache_key: &str, source_url: &str) -> VideoTaskRecord {
        let now = now_timestamp();
        VideoTaskRecord {
            format_version: FORMAT_VERSION,
            task_id: task_id.to_string(),
            cache_key: cache_key.to_string(),
            source_url: source_url.to_string(),
            state: "queued".to_string(),
            step: "准备中".to_string(),
            created_at: now,
            updated_at: now,
            ..Default::default()
        }
    }
}

/// 转录文件信封。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct TranscriptFile {
    format_version: u64,
    language: String,
    source: String,
    segments: Vec<crate::video::transcript::Segment>,
}

/// Vault 内视频任务存储。
#[derive(Clone)]
pub(crate) struct VideoStorage {
    root: PathBuf,
}

impl VideoStorage {
    /// 只绑定根目录，实际访问时交给 vaultfs 校验，不创建未打开的知识库。
    pub(crate) fn new(vault_root: &Path) -> Self {
        Self {
            root: vault_root.to_path_buf(),
        }
    }

    /// 仅在隔离命名空间创建任务目录，不覆盖旧卡片镜像。
    pub(crate) fn task_dir(&self, task_id: &str) -> Result<PathBuf, CommandError> {
        paths::component(task_id)?;
        self.create_dir(&format!("tasks/{task_id}"))
    }

    /// 内部记录也校验最终文件，防止文件级链接绕过。
    fn task_file(&self, task_id: &str) -> Result<PathBuf, CommandError> {
        self.task_dir(task_id)?;
        self.current(&format!("tasks/{task_id}/{TASK_FILE}"))
    }

    /// 转录写入路径独立校验，不信任已创建的父目录。
    fn transcript_file(&self, task_id: &str) -> Result<PathBuf, CommandError> {
        self.task_dir(task_id)?;
        self.current(&format!("tasks/{task_id}/{TRANSCRIPT_FILE}"))
    }

    /// 媒体名必须为单个文件名，拒绝路径穿越和 Windows 别名。
    pub(crate) fn media_file(&self, task_id: &str, name: &str) -> Result<PathBuf, CommandError> {
        paths::component(name)?;
        if name.eq_ignore_ascii_case(TASK_FILE) || name.eq_ignore_ascii_case(TRANSCRIPT_FILE) {
            return Err(CommandError::validation("媒体文件不能覆盖任务记录"));
        }
        self.task_dir(task_id)?;
        self.current(&format!("tasks/{task_id}/{name}"))
    }

    /// 截图目录受相同的祖先与链接校验约束。
    pub(crate) fn shots_dir(&self, task_id: &str) -> Result<PathBuf, CommandError> {
        paths::component(task_id)?;
        self.create_dir(&format!("tasks/{task_id}/shots"))
    }

    /// 截图输出文件也必须校验最终目标，调用方不能自行拼接绕过链接检查。
    pub(crate) fn shot_file(&self, task_id: &str, name: &str) -> Result<PathBuf, CommandError> {
        paths::component(name)?;
        self.shots_dir(task_id)?;
        self.current(&format!("tasks/{task_id}/shots/{name}"))
    }

    /// 任务结束后仅清除新目录中可确认的临时媒体，不删除记录或递归目录。
    pub(crate) fn cleanup_task_media(&self, task_id: &str) -> Result<(), CommandError> {
        paths::component(task_id)?;
        cleanup::task_media(&self.root, task_id)
    }

    /// 删除整条任务：记录、转录、生成媒体与空目录；未知内容保留。
    pub(crate) fn remove_task(&self, task_id: &str) -> Result<(), CommandError> {
        paths::component(task_id)?;
        cleanup::task_all(&self.root, task_id)
    }

    /// 写出完整当前格式；旧目录永远不被覆盖或迁移。
    pub(crate) fn save_task(&self, record: &VideoTaskRecord) -> Result<(), CommandError> {
        let mut record = record.clone();
        record.format_version = FORMAT_VERSION;
        envelope::save_json(&self.task_file(&record.task_id)?, &record)
    }

    /// 优先读新记录；旧记录只读且要求标识匹配，不把卡片当作任务。
    pub(crate) fn load_task(&self, task_id: &str) -> Result<Option<VideoTaskRecord>, CommandError> {
        paths::component(task_id)?;
        let relative = format!("tasks/{task_id}/{TASK_FILE}");
        let file = self.current(&relative)?;
        let record: Option<VideoTaskRecord> = if file.try_exists()? {
            envelope::load_json(&file, &envelope::CorruptPolicy::BackupAndRegenerate)?
        } else {
            paths::legacy_json(&self.root, &relative, "taskId")?
        };
        Ok(record.filter(|record| record.task_id == task_id))
    }

    /// 保存整任务转录；错误直接交给调用方，不能伪装完成。
    pub(crate) fn save_transcript(
        &self,
        task_id: &str,
        transcript: &Transcript,
    ) -> Result<(), CommandError> {
        envelope::save_json(&self.transcript_file(task_id)?, &to_file(transcript))
    }

    /// 缺失的新转录可读旧任务转录，但必须先确认旧任务身份。
    pub(crate) fn load_transcript(
        &self,
        task_id: &str,
    ) -> Result<Option<Transcript>, CommandError> {
        paths::component(task_id)?;
        let relative = format!("tasks/{task_id}/{TRANSCRIPT_FILE}");
        let path = self.current(&relative)?;
        let file: Option<TranscriptFile> = if path.try_exists()? {
            envelope::load_json(&path, &envelope::CorruptPolicy::BackupAndRegenerate)?
        } else if self.load_task(task_id)?.is_some() {
            paths::legacy_json(&self.root, &relative, "segments")?
        } else {
            None
        };
        Ok(file.map(from_file))
    }

    /// 缓存身份由调用方含来源、模型和语言；摘要保持完整键的区分度。
    pub(crate) fn save_page_transcript(
        &self,
        video_key: &str,
        cid: u64,
        transcript: &Transcript,
    ) -> Result<(), CommandError> {
        self.create_dir("pages")?;
        envelope::save_json(&self.page_file(video_key, cid)?, &to_file(transcript))
    }

    /// 复合缓存键不回退旧缓存，避免跨来源或模型误用。
    pub(crate) fn load_page_transcript(
        &self,
        video_key: &str,
        cid: u64,
    ) -> Result<Option<Transcript>, CommandError> {
        let path = self.page_file(video_key, cid)?;
        let file: Option<TranscriptFile> = if path.try_exists()? {
            envelope::load_json(&path, &envelope::CorruptPolicy::BackupAndRegenerate)?
        } else if paths::legacy_key(video_key) {
            paths::legacy_json(
                &self.root,
                &format!("pages/{video_key}-{cid}.json"),
                "segments",
            )?
        } else {
            None
        };
        Ok(file.map(from_file))
    }

    /// 摘要避免净化删除字符导致不同视频键落在同一个缓存文件。
    fn page_file(&self, video_key: &str, cid: u64) -> Result<PathBuf, CommandError> {
        use sha2::{Digest, Sha256};
        self.current(&format!(
            "pages/{:x}-{cid}.json",
            Sha256::digest(video_key.as_bytes())
        ))
    }

    /// 列出两代任务；新记录遮蔽同 id 旧记录，坏任务不阻断整个历史。
    pub(crate) fn list_tasks(&self) -> Result<Vec<VideoTaskRecord>, CommandError> {
        let mut ids = std::collections::BTreeSet::new();
        for namespace in [paths::CURRENT, paths::LEGACY] {
            let dir = paths::resolve(&self.root, namespace, "tasks")?;
            let entries = match std::fs::read_dir(dir) {
                Ok(entries) => entries,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(error) => return Err(error.into()),
            };
            for entry in entries {
                let entry = entry?;
                if entry.file_type()?.is_dir() {
                    if let Some(id) = entry
                        .file_name()
                        .to_str()
                        .filter(|id| paths::component(id).is_ok())
                    {
                        ids.insert(id.to_owned());
                    }
                }
            }
        }
        let mut records = Vec::new();
        for id in ids {
            match self.load_task(&id) {
                Ok(Some(record)) => records.push(record),
                Ok(None) => {}
                Err(_) => eprintln!("VIDEO_TASK_SKIPPED: 无法读取单个视频任务"),
            }
        }
        records.sort_by(|a, b| {
            b.updated_at
                .cmp(&a.updated_at)
                .then(a.task_id.cmp(&b.task_id))
        });
        Ok(records)
    }

    /// 旧复用契约仅用于兼容性测试；生产流程使用模型与来源隔离的分 P 缓存。
    #[cfg(test)]
    pub(crate) fn find_reusable(&self, cache_key: &str) -> Result<Option<String>, CommandError> {
        for record in self.list_tasks()? {
            if record.cache_key == cache_key
                && self
                    .load_transcript(&record.task_id)
                    .ok()
                    .flatten()
                    .is_some()
            {
                return Ok(Some(record.task_id));
            }
        }
        Ok(None)
    }

    /// 仅按终态记录更新时间清理过期媒体；保留历史、缓存、运行中任务和所有旧目录。
    pub(crate) fn cleanup(&self, keep_days: u64) -> Result<(), CommandError> {
        let seconds = keep_days.saturating_mul(86_400).min(i64::MAX as u64) as i64;
        let cutoff = now_timestamp().saturating_sub(seconds);
        for record in self.list_tasks()? {
            if record.updated_at < cutoff
                && matches!(record.state.as_str(), "completed" | "failed" | "cancelled")
            {
                self.cleanup_task_media(&record.task_id)?;
            }
        }
        Ok(())
    }

    /// 每次操作都重新校验祖先和最终目标，不缓存未经验证的绝对路径。
    fn current(&self, relative: &str) -> Result<PathBuf, CommandError> {
        paths::resolve(&self.root, paths::CURRENT, relative)
    }

    /// 创建前后均校验目录，避免复用之前已被替换成链接的路径。
    fn create_dir(&self, relative: &str) -> Result<PathBuf, CommandError> {
        let path = self.current(relative)?;
        std::fs::create_dir_all(&path)?;
        self.current(relative)
    }
}

/// 领域转录转成文件结构。
fn to_file(transcript: &Transcript) -> TranscriptFile {
    TranscriptFile {
        format_version: FORMAT_VERSION,
        language: transcript.language.clone(),
        source: transcript.source.clone(),
        segments: transcript.segments.clone(),
    }
}

/// 文件结构还原成领域转录。
fn from_file(file: TranscriptFile) -> Transcript {
    Transcript {
        language: file.language,
        source: file.source,
        segments: file.segments,
    }
}
