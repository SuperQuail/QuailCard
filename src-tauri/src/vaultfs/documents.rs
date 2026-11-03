//! 文档类型：Markdown 笔记与单词组（.qword）的扩展名与标题约定。
//!
//! 扩展名是前后端共享的契约：扫描、索引、卡片命名空间与改名补全都从这里
//! 取规范值，避免各处硬编码后缀导致行为分叉。

/// Markdown 笔记的规范扩展名（小写、含点）。
pub(crate) const MARKDOWN_EXTENSION: &str = ".md";

/// 单词组文件的规范扩展名（小写、含点）。
pub(crate) const WORD_GROUP_EXTENSION: &str = ".qword";

/// Vault 中受支持的文档类型。
///
/// 单词组是 JSON 文档，只物化为卡片；它没有 frontmatter、不参与双链，
/// 因此需要与 Markdown 在索引、链接解析等处区分处理。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DocumentKind {
    /// Markdown 笔记：参与标签、双链与全文搜索。
    Markdown,
    /// 单词组：按页写词，保存时物化为听写卡。
    WordGroup,
}

impl DocumentKind {
    /// 按扩展名识别文档类型；大小写不敏感，未知扩展名返回 None。
    pub(crate) fn of_path(path: &str) -> Option<Self> {
        let name = file_name(path);
        // 以最后一个点切分，避免把「a.b.qword」误判成无扩展名文件；
        // 纯点开头的隐藏文件（如「.qword」）不算文档，与 Path::extension 一致。
        let (stem, extension) = name.rsplit_once('.')?;
        if stem.is_empty() {
            return None;
        }
        if extension.eq_ignore_ascii_case("qword") {
            Some(Self::WordGroup)
        } else if extension.eq_ignore_ascii_case("md") {
            Some(Self::Markdown)
        } else {
            None
        }
    }

    /// 返回该类型使用的规范扩展名（小写、含点）。
    pub(crate) fn extension(self) -> &'static str {
        match self {
            Self::Markdown => MARKDOWN_EXTENSION,
            Self::WordGroup => WORD_GROUP_EXTENSION,
        }
    }

    /// 是否为 Markdown 笔记；单词组不参与标签与双链。
    pub(crate) fn is_markdown(self) -> bool {
        self == Self::Markdown
    }
}

/// 从文档路径推导显示标题：取文件名并去掉已知扩展名。
pub(crate) fn title_from_path(path: &str) -> String {
    strip_known_extension(file_name(path)).to_string()
}

/// 去掉文件名上的已知文档扩展名；未知扩展名原样保留。
pub(crate) fn strip_known_extension(name: &str) -> &str {
    let Some((stem, extension)) = name.rsplit_once('.') else {
        return name;
    };
    if stem.is_empty() {
        return name;
    }
    if extension.eq_ignore_ascii_case("md") || extension.eq_ignore_ascii_case("qword") {
        stem
    } else {
        name
    }
}

/// 取路径的最后一段（文件名）；同时兼容 / 与 Windows 反斜杠。
fn file_name(path: &str) -> &str {
    path.rsplit(['/', '\\']).next().unwrap_or(path)
}

#[cfg(test)]
#[path = "documents_tests.rs"]
mod tests;
