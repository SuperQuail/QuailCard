//! Agent 单词组适配器：读写 .qword，并在写入后刷新索引与卡片。
//!
//! 契约：结构化字段与 id 规则在领域层（word_group）与用例层校验；适配器只负责
//! 真实文件读取，以及文件写入后的派生数据同步——卡片是派生数据，同步失败必须
//! 让模型知道"文件已保存、可以重试"，不能伪装成功。
use crate::{
    error::CommandError,
    services::agent_ports::{AgentFuture, AgentWordGroups},
    storage::Storage,
    vaultfs::{documents::DocumentKind, VaultState},
};
use tauri::Manager;

pub(crate) struct WordGroupsAdapter {
    pub app: tauri::AppHandle,
}

impl AgentWordGroups for WordGroupsAdapter {
    /// 读取现有单词组原文；文件不存在返回 None，交给调用方判断是新建还是冲突。
    fn load<'a>(&'a self, path: &'a str) -> AgentFuture<'a, Option<String>> {
        Box::pin(async move {
            let vault = self.app.state::<VaultState>();
            let target = vault.agent_document_path(path)?;
            match std::fs::read_to_string(&target) {
                Ok(content) => Ok(Some(content)),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
                Err(error) => Err(error.into()),
            }
        })
    }

    /// 写入成功后刷新检索索引与卡片；文件已由仓库原子写入，这里失败只提示重试。
    fn sync<'a>(&'a self, path: &'a str, content: &'a str) -> AgentFuture<'a, ()> {
        Box::pin(async move {
            if DocumentKind::of_path(path) != Some(DocumentKind::WordGroup) {
                return Err(CommandError::validation("不是单词组文件"));
            }
            // 写盘前已校验过一次；这里再解析一次，保证卡片只按合法文档生成。
            let document = crate::word_group::parse(content)?;
            let vault = self.app.state::<VaultState>();
            let target = vault.agent_document_path(path)?;
            let mtime = std::fs::metadata(&target)?
                .modified()
                .ok()
                .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|duration| duration.as_secs() as i64)
                .unwrap_or_default();
            let storage = self.app.state::<Storage>();
            storage.upsert_note_index(path, content, mtime).await?;
            if let Err(error) = storage.sync_word_group_cards(path, &document) {
                eprintln!("AGENT_WORD_GROUP_SYNC_ERROR(detail): {error} path={path}");
                return Err(CommandError::new(
                    "WORD_GROUP_SYNC_ERROR",
                    "单词组已保存，但卡片同步失败，请重试",
                ));
            }
            Ok(())
        })
    }
}
