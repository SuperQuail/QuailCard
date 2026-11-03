use serde::{Deserialize, Serialize};

/// 打字背诵偏好：每词重复遍数与每页单词数。
///
/// 契约：这是 settings.toml 与前端共用的线格式；字段缺失按默认值解析，
/// 读取时容忍越界值并在使用前钳制，写入时由存储层拒绝非法值。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct PracticeSettings {
    /// 每个单词的重复遍数（不含最后的记忆遍）。
    pub repeats_per_word: u32,
    /// 每页包含的单词数量。
    pub words_per_page: u32,
}

impl Default for PracticeSettings {
    /// 默认五遍照打 + 一遍记忆、每页三个单词：一页 18 格，节奏适中。
    fn default() -> Self {
        Self {
            repeats_per_word: 5,
            words_per_page: 3,
        }
    }
}

impl PracticeSettings {
    /// 重复遍数的合法范围；下限 1 表示「只打一遍就进入记忆遍」。
    pub const MIN_REPEATS: u32 = 1;
    pub const MAX_REPEATS: u32 = 10;
    /// 每页单词数的合法范围；上限 6 避免一页格子过多、滚动过长。
    pub const MIN_WORDS: u32 = 1;
    pub const MAX_WORDS: u32 = 6;

    /// 是否落在合法范围内；存储层据此拒绝写入。
    pub fn is_valid(&self) -> bool {
        (Self::MIN_REPEATS..=Self::MAX_REPEATS).contains(&self.repeats_per_word)
            && (Self::MIN_WORDS..=Self::MAX_WORDS).contains(&self.words_per_page)
    }

    /// 把越界值夹到最近边界：settings.toml 被手改坏时仍然可用，不阻断启动。
    pub fn clamped(&self) -> Self {
        Self {
            repeats_per_word: self
                .repeats_per_word
                .clamp(Self::MIN_REPEATS, Self::MAX_REPEATS),
            words_per_page: self.words_per_page.clamp(Self::MIN_WORDS, Self::MAX_WORDS),
        }
    }
}
