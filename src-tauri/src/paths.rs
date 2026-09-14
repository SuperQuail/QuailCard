//! 路径文本出口：把系统调用使用的路径转成可展示、可持久化的写法。
//!
//! Windows 的 `std::fs::canonicalize` 返回扩展长度（verbatim）路径：资源目录、Vault 根目录
//! 这类先经 canonicalize 的路径都会带 `\\?\` 前缀。它本身合法（还能绕过 MAX_PATH），
//! 因此系统调用继续使用原路径，只在写给界面或配置文件的出口还原。

use std::path::Path;

/// 还原 Windows verbatim 前缀：`\\?\C:\x` → `C:\x`，`\\?\UNC\s\share` → `\\s\share`。
///
/// 卷标路径 `\\?\Volume{...}` 去掉前缀后不再是合法路径，原样保留；其他平台不做处理。
pub(crate) fn simplified(path: impl AsRef<Path>) -> String {
    #[cfg(windows)]
    {
        let text = path.as_ref().to_string_lossy();
        if let Some(rest) = text.strip_prefix(r"\\?\UNC\") {
            return format!(r"\\{rest}");
        }
        if let Some(rest) = text.strip_prefix(r"\\?\") {
            if rest.as_bytes().get(1) == Some(&b':') {
                return rest.to_string();
            }
        }
        text.into_owned()
    }
    #[cfg(not(windows))]
    {
        path.as_ref().to_string_lossy().into_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    /// 普通路径原样返回，避免不同平台风格被误改。
    fn keeps_plain_paths() {
        assert_eq!(simplified(Path::new("/usr/bin/ffmpeg")), "/usr/bin/ffmpeg");
        assert_eq!(
            simplified(Path::new("C:/tools/ffmpeg.exe")),
            "C:/tools/ffmpeg.exe"
        );
    }

    #[cfg(windows)]
    #[test]
    /// 扩展长度前缀还原为可粘贴路径；卷标路径去前缀后不再合法，必须原样保留。
    fn strips_verbatim_prefix() {
        assert_eq!(
            simplified(Path::new(r"\\?\C:\tools\ffmpeg.exe")),
            r"C:\tools\ffmpeg.exe"
        );
        assert_eq!(
            simplified(Path::new(r"\\?\UNC\server\share\ffmpeg.exe")),
            r"\\server\share\ffmpeg.exe"
        );
        assert_eq!(
            simplified(Path::new(r"\\?\Volume{9f0d}\ffmpeg.exe")),
            r"\\?\Volume{9f0d}\ffmpeg.exe"
        );
        // 字符串入口（配置文件中的历史路径）走同一套规则。
        assert_eq!(simplified(r"\\?\D:\Notes\Godot"), r"D:\Notes\Godot");
    }
}
