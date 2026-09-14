//! 视频转笔记的端口定义。

use crate::{error::CommandError, video::media::AsrEngine};

/// 笔记写入端口：只暴露落盘能力，不泄露文件系统细节。
pub(crate) trait VideoNotes: Send + Sync {
    /// 新建笔记并返回 Vault 内相对路径；同名自动加序号，不覆盖已有内容。
    fn write_note(&self, folder: &str, title: &str, content: &str) -> Result<String, CommandError>;
    /// 保存配图到附件目录，返回相对笔记目录可直接使用的路径。
    fn save_shot(
        &self,
        note_folder: &str,
        file_name: &str,
        bytes: &[u8],
    ) -> Result<String, CommandError>;
}

/// 媒体与语音能力集合，由组合根注入具体实现。
pub(crate) struct VideoTools<'a> {
    pub audio: &'a dyn crate::video::media::AudioExtractor,
    pub frames: &'a dyn crate::video::media::FrameExtractor,
    pub asr: &'a dyn AsrEngine,
}
