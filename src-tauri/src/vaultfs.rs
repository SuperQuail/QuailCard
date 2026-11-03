use std::{
    path::{Component, Path, PathBuf},
    sync::RwLock,
};

use crate::error::CommandError;

mod agent;
mod attachment_actions;
mod attachments;
mod config;
pub(crate) mod documents;
mod remove;

pub(crate) use attachment_actions::{
    attachment_file_hints, read_attachment_bytes, write_replacing,
};
pub(crate) use remove::remover_for;

use self::documents::{strip_known_extension, DocumentKind};

#[cfg(test)]
mod attachment_actions_tests;
#[cfg(test)]
mod attachments_tests;

/// 管理 Vault 根目录并封装全部文件操作。
///
/// 前端只提交 Vault 相对路径；所有路径先净化再约束在根目录内，
/// 杜绝目录穿越与绝对路径访问。
pub struct VaultState {
    root: RwLock<Option<PathBuf>>,
}

impl VaultState {
    /// 创建未选择 Vault 的状态实例。
    pub fn new() -> Self {
        Self {
            root: RwLock::new(None),
        }
    }

    /// 设置 Vault 根目录并确保其存在。
    pub fn set_root(&self, path: PathBuf) -> Result<PathBuf, CommandError> {
        let canonical = path
            .canonicalize()
            .map_err(|_| CommandError::new("VAULT_INVALID", "所选文件夹不存在"))?;
        if !canonical.is_dir() {
            return Err(CommandError::new("VAULT_INVALID", "所选路径不是文件夹"));
        }
        *self
            .root
            .write()
            .map_err(|_| CommandError::new("INTERNAL_ERROR", "保险库状态锁失效"))? =
            Some(canonical.clone());
        Ok(canonical)
    }

    /// 返回当前 Vault 根目录。
    pub fn root(&self) -> Result<Option<PathBuf>, CommandError> {
        self.root
            .read()
            .map(|guard| guard.clone())
            .map_err(|_| CommandError::new("INTERNAL_ERROR", "保险库状态锁失效"))
    }

    /// 将相对路径解析为根目录内的绝对路径。
    fn resolve(&self, relative: &str) -> Result<PathBuf, CommandError> {
        self.safe_path(relative)
    }

    /// 读取笔记文件内容与修改时间。
    pub fn read_note(&self, relative: &str) -> Result<(String, i64), CommandError> {
        let path = self.resolve(relative)?;
        if !path.is_file() {
            return Err(CommandError::new("NOTE_NOT_FOUND", "笔记文件不存在"));
        }
        let bytes = std::fs::read(&path)?;
        // 支持带 BOM 的 UTF-8，读取后剥离。
        let content = String::from_utf8(bytes)
            .or_else(|error| {
                let mut bytes = error.into_bytes();
                if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
                    bytes.drain(0..3);
                }
                String::from_utf8(bytes)
            })
            .map_err(|_| CommandError::new("FILE_ENCODING", "笔记不是有效的 UTF-8 文本"))?;
        let mtime = std::fs::metadata(&path)?
            .modified()
            .ok()
            .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|duration| duration.as_secs() as i64)
            .unwrap_or_default();
        Ok((content, mtime))
    }

    /// 写入笔记文件（UTF-8、LF、无 BOM），返回新修改时间。
    pub fn write_note(&self, relative: &str, content: &str) -> Result<i64, CommandError> {
        let path = self.resolve(relative)?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let normalized = content.replace("\r\n", "\n");
        crate::storage::envelope::write_atomic(&path, normalized.as_bytes())?;
        let mtime = std::fs::metadata(&path)?
            .modified()
            .ok()
            .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|duration| duration.as_secs() as i64)
            .unwrap_or_default();
        Ok(mtime)
    }

    /// 新建 Markdown 笔记文件并返回相对路径。
    pub fn create_note(&self, folder: &str, title: &str) -> Result<String, CommandError> {
        let title = sanitize_file_name(title, DocumentKind::Markdown)?;
        let relative = join_relative(folder, &title)?;
        let path = self.resolve(&relative)?;
        if path.exists() {
            return Err(CommandError::validation("同名笔记已存在"));
        }
        let heading = strip_known_extension(&title);
        self.write_note(&relative, &format!("# {heading}\n"))?;
        Ok(relative)
    }

    /// 新建单词组文件并返回相对路径；内容由用例层生成并已通过校验。
    ///
    /// 允许与同名 Markdown 笔记并存：两者扩展名不同，标题可以相同。
    pub fn create_word_group_file(
        &self,
        folder: &str,
        title: &str,
        content: &str,
    ) -> Result<String, CommandError> {
        let title = sanitize_file_name(title, DocumentKind::WordGroup)?;
        let relative = join_relative(folder, &title)?;
        let path = self.resolve(&relative)?;
        if path.exists() {
            return Err(CommandError::validation("同名单词组已存在"));
        }
        self.write_note(&relative, content)?;
        Ok(relative)
    }

    /// 新建文件夹。
    pub fn create_folder(&self, relative: &str) -> Result<(), CommandError> {
        let path = self.resolve(relative)?;
        if path.exists() {
            return Err(CommandError::validation("同名文件夹已存在"));
        }
        std::fs::create_dir_all(&path)?;
        Ok(())
    }

    /// 重命名笔记文件；扩展名跟随旧文件（.md 仍是 .md，.qword 仍是 .qword）。
    pub fn rename_note(&self, old: &str, new: &str) -> Result<String, CommandError> {
        let source = self.resolve(old)?;
        if !source.is_file() {
            return Err(CommandError::new("NOTE_NOT_FOUND", "笔记文件不存在"));
        }
        // 旧路径扩展名决定新名字补什么后缀；未知扩展名按 Markdown 处理，
        // 保持历史上"改名总落到 .md"的兜底行为。
        let kind = DocumentKind::of_path(old).unwrap_or(DocumentKind::Markdown);
        let new_relative = normalize_rename_target(new, Some(kind))?;
        let target = self.resolve(&new_relative)?;
        if target.exists() {
            return Err(CommandError::validation("同名笔记已存在"));
        }
        std::fs::rename(&source, &target)?;
        Ok(new_relative)
    }

    /// 重命名或移动文件夹。
    pub fn rename_folder(&self, old: &str, new: &str) -> Result<String, CommandError> {
        let source = self.resolve(old)?;
        if !source.is_dir() {
            return Err(CommandError::new("FOLDER_NOT_FOUND", "文件夹不存在"));
        }
        // 文件夹名不补文档扩展名：旧实现会追加 .md，导致改名后目录名末尾多出后缀。
        let new_relative = normalize_rename_target(new, None)?;
        // 拖拽可以把文件夹放进任意目录；移进自身子树会让 fs::rename 失败并返回误导性错误。
        ensure_folder_move(old, &new_relative)?;
        let target = self.resolve(&new_relative)?;
        if target.exists() {
            return Err(CommandError::validation("同名文件夹已存在"));
        }
        std::fs::rename(&source, &target)?;
        Ok(new_relative)
    }

    /// 扫描 Vault 中全部 Markdown 笔记与单词组文件，返回相对路径、内容与修改时间。
    pub fn scan(&self) -> Result<Vec<(String, String, i64)>, CommandError> {
        let root = self
            .root()?
            .ok_or_else(|| CommandError::new("VAULT_NOT_OPEN", "请先选择 Vault 文件夹"))?;
        let mut files = Vec::new();
        collect_document_files(&root, &root, &mut files)?;
        Ok(files)
    }
}

/// 净化相对路径：拒绝绝对路径、父目录与盘符。
pub(crate) fn sanitize_relative(relative: &str) -> Result<PathBuf, CommandError> {
    let path = Path::new(relative.trim());
    if relative.trim().is_empty() {
        return Err(CommandError::validation("路径不能为空"));
    }
    // 反斜杠只在 Windows 上是分隔符，在 Unix 上会被当作普通字符绕过组件检查；
    // 前端契约是仅提交 `/` 分隔的相对路径，因此跨平台统一拒绝 `\`。
    if relative.contains('\\')
        || path.is_absolute()
        || path.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        return Err(CommandError::validation("路径包含非法字符"));
    }
    Ok(path.to_path_buf())
}

/// 纯字符串净化：Agent 只操作受支持的文档（Markdown 笔记与单词组）。
///
/// 与 `agent_document_path` 共用同一规则，写入范围校验与真实文件访问不会出现
/// 两套标准；扩展名判定统一走 [crate::vaultfs::documents::DocumentKind]。
pub(crate) fn sanitize_agent_document(relative: &str) -> Result<String, CommandError> {
    if relative.trim() != relative
        || crate::vaultfs::documents::DocumentKind::of_path(relative).is_none()
        || relative
            .split(['/', '\\'])
            .any(|part| part.starts_with('.'))
    {
        return Err(CommandError::validation(
            "Agent 只能操作 Markdown 笔记或单词组",
        ));
    }
    let path = sanitize_relative(relative)?;
    Ok(path.to_string_lossy().replace('\\', "/"))
}

/// 纯字符串净化：写入授权的目录前缀不能是隐藏目录或内部数据目录。
///
/// 目录本身不要求存在；存在性由存储层在父级授权时校验。
pub(crate) fn sanitize_agent_dir(relative: &str) -> Result<String, CommandError> {
    let path = sanitize_relative(relative)?;
    let text = path.to_string_lossy().replace('\\', "/");
    if text.is_empty()
        || text
            .split('/')
            .any(|part| part.is_empty() || part.starts_with('.'))
    {
        return Err(CommandError::validation("目录路径无效或不可授权"));
    }
    Ok(text)
}

/// 校验文件夹移动目标：不能原地不动，也不能移进自身或其子目录。
fn ensure_folder_move(old: &str, new: &str) -> Result<(), CommandError> {
    let old = old.trim_end_matches('/');
    if new == old || new.starts_with(&format!("{old}/")) {
        return Err(CommandError::validation("不能把文件夹移动到自身或其子目录"));
    }
    Ok(())
}

/// 净化改名目标：kind 为 Some 时补全对应文档扩展名，None 表示文件夹名原样保留。
fn normalize_rename_target(new: &str, kind: Option<DocumentKind>) -> Result<String, CommandError> {
    let path = sanitize_relative(new)?;
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_default();
    let name = match kind {
        Some(kind) => sanitize_file_name(&name, kind)?,
        None => sanitize_plain_name(&name)?,
    };
    let parent = path
        .parent()
        .map(|parent| parent.to_string_lossy().to_string())
        .filter(|parent| !parent.is_empty() && parent != ".");
    Ok(match parent {
        Some(parent) => format!("{parent}/{name}"),
        None => name,
    })
}

/// 把文件夹与文件名拼成 Vault 相对路径；空文件夹表示 Vault 根目录。
fn join_relative(folder: &str, name: &str) -> Result<String, CommandError> {
    let folder = folder.trim();
    if folder.is_empty() {
        return Ok(name.to_string());
    }
    Ok(format!(
        "{}/{name}",
        sanitize_relative(folder)?.to_string_lossy()
    ))
}

/// 净化文件名并补全指定文档扩展名；已带同一扩展名（大小写不敏感）时不重复追加。
fn sanitize_file_name(title: &str, kind: DocumentKind) -> Result<String, CommandError> {
    let mut name = sanitize_plain_name(title)?;
    let extension = kind.extension();
    if !name.to_lowercase().ends_with(extension) {
        name.push_str(extension);
    }
    Ok(name)
}

/// 净化单个文件或文件夹名称：去除路径分隔符与 Windows 非法字符，限长 100 字符。
///
/// 文件夹改名复用同一套字符规则，但绝不追加任何文档扩展名。
fn sanitize_plain_name(title: &str) -> Result<String, CommandError> {
    let mut name: String = title
        .trim()
        .chars()
        .filter(|character| {
            !matches!(
                character,
                '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|'
            )
        })
        .take(100)
        .collect();
    name = name.trim().to_string();
    if name.is_empty() {
        return Err(CommandError::validation("名称不能为空"));
    }
    Ok(name)
}

/// 递归收集目录树中的 Markdown 笔记与单词组文件。
fn collect_document_files(
    root: &Path,
    directory: &Path,
    files: &mut Vec<(String, String, i64)>,
) -> Result<(), CommandError> {
    for entry in std::fs::read_dir(directory)? {
        let entry = entry?;
        let path = entry.path();
        // 内部数据目录与链接不是用户文档，回收副本不能重新出现在列表中。
        if entry.file_name() == ".quailcard"
            || entry.file_type()?.is_symlink()
            || !path.canonicalize()?.starts_with(root)
        {
            continue;
        }
        if path.is_dir() {
            collect_document_files(root, &path, files)?;
            continue;
        }
        if DocumentKind::of_path(&entry.file_name().to_string_lossy()).is_none() {
            continue;
        }
        let bytes = std::fs::read(&path)?;
        let content = String::from_utf8(bytes)
            .or_else(|error| {
                let mut bytes = error.into_bytes();
                if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
                    bytes.drain(0..3);
                }
                String::from_utf8(bytes)
            })
            .unwrap_or_default();
        let mtime = std::fs::metadata(&path)?
            .modified()
            .ok()
            .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|duration| duration.as_secs() as i64)
            .unwrap_or_default();
        let relative = path
            .strip_prefix(root)
            .map(|path| path.to_string_lossy().replace('\\', "/"))
            .unwrap_or_default();
        files.push((relative, content, mtime));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::path::PathBuf;

    use uuid::Uuid;

    use super::*;

    /// 隔离的临时 Vault：用 UUID 避开并发测试，离开作用域时清理。
    struct TestVault(PathBuf);

    impl TestVault {
        /// 创建唯一的临时目录。
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!("quailcard-vaultfs-{}", Uuid::now_v7()));
            std::fs::create_dir_all(&path).unwrap();
            Self(path)
        }

        /// 构造已选择此目录的 VaultState。
        fn state(&self) -> VaultState {
            let state = VaultState::new();
            state.set_root(self.0.clone()).unwrap();
            state
        }

        /// 写入任意相对路径文件，自动创建父目录。
        fn write(&self, relative: &str, content: &str) {
            let path = self.0.join(relative);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, content).unwrap();
        }
    }

    impl Drop for TestVault {
        /// 测试结束后尽力删除临时目录。
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    /// 相对路径净化拒绝目录穿越。
    fn rejects_path_traversal() {
        assert!(sanitize_relative("..\\secret.md").is_err());
        assert!(sanitize_relative("../secret.md").is_err());
        assert!(sanitize_relative("C:\\secret.md").is_err());
        assert!(sanitize_relative("/etc/passwd").is_err());
        assert!(sanitize_relative("a/../../b.md").is_err());
        assert!(sanitize_relative("笔记/子目录/文件.md").is_ok());
    }

    #[test]
    /// 文件夹不能移进自身或自身子目录，同级前缀目录不受影响。
    fn rejects_folder_move_into_itself() {
        assert!(ensure_folder_move("英语", "英语").is_err());
        assert!(ensure_folder_move("英语", "英语/单词").is_err());
        assert!(ensure_folder_move("英语/", "英语/单词").is_err());
        assert!(ensure_folder_move("英语", "课程/英语").is_ok());
        assert!(ensure_folder_move("英语", "英语角").is_ok());
    }

    #[test]
    /// 文件名净化移除非法字符并按类型补扩展名；文件夹名不追加扩展名。
    fn sanitizes_names_by_kind() {
        assert_eq!(
            sanitize_file_name("我的:笔记", DocumentKind::Markdown).unwrap(),
            "我的笔记.md"
        );
        assert_eq!(
            sanitize_file_name("已经.md", DocumentKind::Markdown).unwrap(),
            "已经.md"
        );
        assert_eq!(
            sanitize_file_name("单词", DocumentKind::WordGroup).unwrap(),
            "单词.qword"
        );
        assert_eq!(
            sanitize_file_name("单词.QWORD", DocumentKind::WordGroup).unwrap(),
            "单词.QWORD"
        );
        assert_eq!(sanitize_plain_name("文件夹:名").unwrap(), "文件夹名");
        assert!(sanitize_plain_name("   ").is_err());
        assert!(sanitize_file_name("   ", DocumentKind::Markdown).is_err());
    }

    #[test]
    /// 扫描同时收集 .md 与 .qword（大小写不敏感），跳过内部目录与其他类型。
    fn scans_markdown_and_word_groups() {
        let vault = TestVault::new();
        vault.write("a.md", "# 笔记");
        vault.write("目录/b.qword", "{}");
        vault.write("目录/c.txt", "忽略");
        vault.write(".quailcard/config.json", "{}");
        vault.write("d.MD", "# 大写");
        vault.write("e.QWORD", "{}");
        let files = vault.state().scan().unwrap();
        let paths: BTreeSet<String> = files.into_iter().map(|(path, ..)| path).collect();
        assert_eq!(
            paths,
            BTreeSet::from([
                "a.md".to_string(),
                "目录/b.qword".to_string(),
                "d.MD".to_string(),
                "e.QWORD".to_string(),
            ])
        );
    }

    #[test]
    /// 新建单词组写盘并返回相对路径；重名拒绝，且允许与同名 .md 并存。
    fn creates_word_group_file() {
        let vault = TestVault::new();
        vault.write("单词.md", "# 同名笔记");
        let state = vault.state();
        let path = state
            .create_word_group_file("课程", " 单词 ", "{\"words\":[]}")
            .unwrap();
        assert_eq!(path, "课程/单词.qword");
        assert_eq!(
            std::fs::read_to_string(vault.0.join(&path)).unwrap(),
            "{\"words\":[]}"
        );
        assert_eq!(
            state
                .create_word_group_file("课程", "单词", "{}")
                .unwrap_err()
                .code,
            "VALIDATION_ERROR"
        );
        // 与 .md 同名不冲突：扩展名不同。
        assert!(state.create_word_group_file("", "单词", "{}").is_ok());
    }

    #[test]
    /// 新建 Markdown 笔记仍按 .md 补扩展名。
    fn creates_markdown_note() {
        let vault = TestVault::new();
        let state = vault.state();
        let path = state.create_note("", "我的笔记").unwrap();
        assert_eq!(path, "我的笔记.md");
        assert_eq!(
            std::fs::read_to_string(vault.0.join(&path)).unwrap(),
            "# 我的笔记\n"
        );
    }

    #[test]
    /// 改名保留旧文件扩展名：.md 仍是 .md，单词组仍是 .qword。
    fn rename_note_keeps_extension() {
        let vault = TestVault::new();
        vault.write("旧笔记.md", "# x");
        vault.write("旧单词.qword", "{}");
        std::fs::create_dir_all(vault.0.join("课程")).unwrap();
        let state = vault.state();
        assert_eq!(
            state.rename_note("旧笔记.md", "新笔记").unwrap(),
            "新笔记.md"
        );
        assert_eq!(
            state.rename_note("旧单词.qword", "课程/新单词").unwrap(),
            "课程/新单词.qword"
        );
        assert!(vault.0.join("新笔记.md").is_file());
        assert!(vault.0.join("课程/新单词.qword").is_file());
        assert!(!vault.0.join("旧单词.qword").exists());
    }

    #[test]
    /// 文件夹改名不再追加 .md（修复既有缺陷）。
    fn rename_folder_does_not_append_extension() {
        let vault = TestVault::new();
        std::fs::create_dir_all(vault.0.join("旧文件夹")).unwrap();
        std::fs::create_dir_all(vault.0.join("课程")).unwrap();
        let state = vault.state();
        assert_eq!(
            state.rename_folder("旧文件夹", "新文件夹").unwrap(),
            "新文件夹"
        );
        assert!(vault.0.join("新文件夹").is_dir());
        assert!(!vault.0.join("新文件夹.md").exists());
        assert_eq!(
            state.rename_folder("新文件夹", "课程/英语").unwrap(),
            "课程/英语"
        );
        assert!(vault.0.join("课程/英语").is_dir());
    }
}
