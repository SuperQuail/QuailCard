//! 拆卡准备适配器；只做笔记读取与已有校验，不再驱动独立的生成执行器。
use crate::{
    agent_bridge,
    ai::{validate_generation_input, GenerationSession},
    card_generation::note_content_hash,
    error::CommandError,
    models::{GenerationContext, GenerationInput, ProviderConfig},
    services::agent_ports::{AgentFuture, AgentLearning, AgentRepository, PreparedGeneration},
    storage::Storage,
};
use tauri::Manager;

pub(crate) struct LearningAdapter {
    pub app: tauri::AppHandle,
    pub config: ProviderConfig,
}

impl AgentLearning for LearningAdapter {
    /// 读取笔记并复用拆卡对话框的全部生成前校验：领域输入、磁盘快照与视觉能力。
    fn prepare<'a>(&'a self, path: &'a str, kind: &'a str) -> AgentFuture<'a, PreparedGeneration> {
        Box::pin(async move {
            let repository = agent_bridge::files(&self.app)?;
            let note = repository.read(path)?;
            let content = note["content"].as_str().unwrap_or("").replace("\r\n", "\n");
            // 采纳校验使用同一份领域摘要函数，两处实现不会漂移。
            let hash = note_content_hash(&content);
            let root = self
                .app
                .state::<crate::vaultfs::VaultState>()
                .root()?
                .ok_or_else(|| CommandError::validation("知识库已关闭"))?
                .to_string_lossy()
                .to_string();
            // 卡片类型决定学习方式，映射与拆卡对话框保持一致。
            let modes = [("qa", "ai-review"), ("vocabulary", "dictation")];
            let mode = modes
                .iter()
                .find(|entry| entry.0 == kind)
                .ok_or_else(|| CommandError::validation("不支持的卡片类型"))?
                .1;
            let input = GenerationInput {
                type_id: kind.into(),
                study_mode_id: mode.into(),
                note_title: path
                    .rsplit('/')
                    .next()
                    .unwrap_or(path)
                    .trim_end_matches(".md")
                    .into(),
                source_text: content,
                images: vec![],
                // -1 表示不设上限：结束条件由 finish_generation 给出。
                requested_count: -1,
                context: Some(GenerationContext {
                    vault_path: root.clone(),
                    note_path: path.into(),
                    note_hash: hash.clone(),
                    selection: None,
                }),
            };
            validate_generation_input(&input)?;
            let storage = self.app.state::<Storage>();
            let snapshot = storage.validate_generation_context(&input)?;
            // 材料在 Agent 侧没有图片，这个校验与拆卡对话框保持同一条规则。
            if !input.images.is_empty() && !self.config.supports_vision {
                return Err(CommandError::validation(
                    "当前供应商未启用图片输入，请更换模型或在模型设置中开启",
                ));
            }
            let session =
                GenerationSession::prepared(&input, snapshot.note_content, &snapshot.cards);
            Ok(PreparedGeneration {
                path: path.into(),
                kind: kind.into(),
                expected_vault_path: root,
                expected_note_hash: hash,
                input,
                session,
            })
        })
    }
}
