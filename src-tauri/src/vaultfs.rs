use std::{
    path::{Component, Path, PathBuf},
    sync::RwLock,
};

use crate::error::CommandError;

mod attachments;
mod config;

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
        let root = self
            .root()?
            .ok_or_else(|| CommandError::new("VAULT_NOT_OPEN", "请先选择 Vault 文件夹"))?;
        let relative = sanitize_relative(relative)?;
        let absolute = root.join(&relative);
        // 双保险：即使净化的路径被符号链接指向外部，也拒绝越界。
        let parent = absolute
            .parent()
            .unwrap_or(&root)
            .canonicalize()
            .unwrap_or_else(|_| root.clone());
        if !parent.starts_with(&root) {
            return Err(CommandError::validation("路径超出 Vault 范围"));
        }
        Ok(absolute)
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
        std::fs::write(&path, normalized)?;
        let mtime = std::fs::metadata(&path)?
            .modified()
            .ok()
            .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|duration| duration.as_secs() as i64)
            .unwrap_or_default();
        Ok(mtime)
    }

    /// 新建笔记文件并返回相对路径。
    pub fn create_note(&self, folder: &str, title: &str) -> Result<String, CommandError> {
        let title = sanitize_title(title)?;
        let relative = if folder.trim().is_empty() {
            title.clone()
        } else {
            format!("{}/{}", sanitize_relative(folder)?.to_string_lossy(), title)
        };
        let path = self.resolve(&relative)?;
        if path.exists() {
            return Err(CommandError::validation("同名笔记已存在"));
        }
        self.write_note(&relative, &format!("# {}\n", title.trim_end_matches(".md")))?;
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

    /// 重命名笔记文件。
    pub fn rename_note(&self, old: &str, new: &str) -> Result<String, CommandError> {
        let source = self.resolve(old)?;
        if !source.is_file() {
            return Err(CommandError::new("NOTE_NOT_FOUND", "笔记文件不存在"));
        }
        let new_relative = normalize_rename_target(new)?;
        let target = self.resolve(&new_relative)?;
        if target.exists() {
            return Err(CommandError::validation("同名笔记已存在"));
        }
        std::fs::rename(&source, &target)?;
        Ok(new_relative)
    }

    /// 删除笔记文件。
    pub fn delete_note(&self, relative: &str) -> Result<(), CommandError> {
        let path = self.resolve(relative)?;
        if !path.is_file() {
            return Err(CommandError::new("NOTE_NOT_FOUND", "笔记文件不存在"));
        }
        std::fs::remove_file(&path)?;
        Ok(())
    }

    /// 重命名文件夹。
    pub fn rename_folder(&self, old: &str, new: &str) -> Result<String, CommandError> {
        let source = self.resolve(old)?;
        if !source.is_dir() {
            return Err(CommandError::new("FOLDER_NOT_FOUND", "文件夹不存在"));
        }
        let new_relative = normalize_rename_target(new)?;
        let target = self.resolve(&new_relative)?;
        if target.exists() {
            return Err(CommandError::validation("同名文件夹已存在"));
        }
        std::fs::rename(&source, &target)?;
        Ok(new_relative)
    }

    /// 删除文件夹及其全部内容。
    pub fn delete_folder(&self, relative: &str) -> Result<(), CommandError> {
        let path = self.resolve(relative)?;
        if !path.is_dir() {
            return Err(CommandError::new("FOLDER_NOT_FOUND", "文件夹不存在"));
        }
        std::fs::remove_dir_all(&path)?;
        Ok(())
    }

    /// 扫描 Vault 中全部 .md 文件，返回相对路径、内容与修改时间。
    pub fn scan(&self) -> Result<Vec<(String, String, i64)>, CommandError> {
        let root = self
            .root()?
            .ok_or_else(|| CommandError::new("VAULT_NOT_OPEN", "请先选择 Vault 文件夹"))?;
        let mut files = Vec::new();
        collect_markdown_files(&root, &root, &mut files)?;
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

/// 净化笔记/文件夹新名称并强制 .md 后缀。
fn normalize_rename_target(new: &str) -> Result<String, CommandError> {
    let path = sanitize_relative(new)?;
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_default();
    let name = sanitize_title(&name)?;
    let parent = path
        .parent()
        .map(|parent| parent.to_string_lossy().to_string())
        .filter(|parent| !parent.is_empty() && parent != ".");
    Ok(match parent {
        Some(parent) => format!("{parent}/{name}"),
        None => name,
    })
}

/// 净化文件名：去除路径分隔符与 Windows 非法字符，确保 .md 后缀。
fn sanitize_title(title: &str) -> Result<String, CommandError> {
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
    if !name.to_lowercase().ends_with(".md") {
        name.push_str(".md");
    }
    Ok(name)
}

/// 递归收集目录树中的 Markdown 文件。
fn collect_markdown_files(
    root: &Path,
    directory: &Path,
    files: &mut Vec<(String, String, i64)>,
) -> Result<(), CommandError> {
    for entry in std::fs::read_dir(directory)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            collect_markdown_files(root, &path, files)?;
        } else if path
            .extension()
            .map(|extension| extension.eq_ignore_ascii_case("md"))
            .unwrap_or(false)
        {
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
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

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
    /// 文件名净化会移除非法字符并补上后缀。
    fn sanitizes_titles() {
        assert_eq!(sanitize_title("我的:笔记").unwrap(), "我的笔记.md");
        assert_eq!(sanitize_title("已经.md").unwrap(), "已经.md");
        assert!(sanitize_title("   ").is_err());
    }
}
