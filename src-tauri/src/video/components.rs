//! 外部组件（ffmpeg / whisper-cli）解析与自检。
//!
//! 解析顺序：用户指定路径 → 应用数据目录中的可选加速组件 → 资源目录 →
//! 开发期源码资源目录 → 系统 PATH。任何一步都只返回真实存在的文件。

#[cfg(test)]
#[path = "components/resource_tests.rs"]
mod resource_tests;
mod runtime;
pub(crate) use runtime::{cpu_fallback, probe_whisper};

use std::path::{Path, PathBuf};

/// 需要外部程序的组件种类。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Component {
    /// 媒体解码与抽帧。
    Ffmpeg,
    /// 本地语音识别。
    Whisper,
}

impl Component {
    /// 组件在磁盘上的文件名。
    pub(crate) fn file_name(self) -> &'static str {
        match (self, cfg!(windows)) {
            (Component::Ffmpeg, true) | (Component::Whisper, true) => {
                if self == Component::Ffmpeg {
                    "ffmpeg.exe"
                } else {
                    "whisper-cli.exe"
                }
            }
            (Component::Ffmpeg, false) => "ffmpeg",
            (Component::Whisper, false) => "whisper-cli",
        }
    }

    /// 资源目录中的子目录名。
    pub(crate) fn folder(self) -> &'static str {
        match self {
            Component::Ffmpeg => "ffmpeg",
            Component::Whisper => "whisper",
        }
    }

    /// 可选加速包在应用数据目录中的位置。
    pub(crate) fn accelerated_folder(self) -> Option<&'static str> {
        match self {
            Component::Ffmpeg => None,
            Component::Whisper => Some("components/whisper-vulkan"),
        }
    }
}

/// 解析结果：真实路径与来源标记（用于界面展示）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Resolved {
    pub path: PathBuf,
    pub source: &'static str,
}

/// 按优先级解析组件路径；找不到返回 None。
pub(crate) fn resolve(
    component: Component,
    override_path: Option<&str>,
    resource_dir: Option<&Path>,
    data_dir: Option<&Path>,
    manifest_dir: Option<&Path>,
) -> Option<Resolved> {
    let name = component.file_name();
    let mut candidates: Vec<(PathBuf, &'static str)> = Vec::new();
    if let Some(path) = override_path.map(str::trim).filter(|path| !path.is_empty()) {
        candidates.push((PathBuf::from(path), "用户指定"));
    }
    if let Some(folder) = component.accelerated_folder() {
        if let Some(data) = data_dir {
            candidates.push((data.join(folder).join(name), "加速组件"));
        }
    }
    if let Some(resource) = resource_dir {
        candidates.push((resource.join(component.folder()).join(name), "随包组件"));
    }
    if let Some(manifest) = manifest_dir {
        candidates.push((
            manifest
                .join("resources")
                .join(component.folder())
                .join(name),
            "开发目录",
        ));
    }
    for (path, source) in candidates {
        if path.is_file() {
            ensure_executable(&path);
            return Some(Resolved { path, source });
        }
    }
    find_in_path(name).map(|path| {
        ensure_executable(&path);
        Resolved {
            path,
            source: "系统环境",
        }
    })
}

/// 在系统 PATH 中查找可执行文件。
fn find_in_path(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|folder| folder.join(name))
        .find(|candidate| candidate.is_file())
}

/// Unix 上补齐可执行位；Windows 无需处理。
pub(crate) fn ensure_executable(path: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(metadata) = std::fs::metadata(path) {
            let mut permissions = metadata.permissions();
            if permissions.mode() & 0o111 == 0 {
                permissions.set_mode(permissions.mode() | 0o755);
                let _ = std::fs::set_permissions(path, permissions);
            }
        }
    }
    #[cfg(not(unix))]
    {
        let _ = path;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    /// 文件名按平台区分。
    fn file_names_follow_platform() {
        if cfg!(windows) {
            assert_eq!(Component::Ffmpeg.file_name(), "ffmpeg.exe");
            assert_eq!(Component::Whisper.file_name(), "whisper-cli.exe");
        } else {
            assert_eq!(Component::Ffmpeg.file_name(), "ffmpeg");
        }
    }

    #[test]
    /// 用户指定路径优先于随包目录。
    fn override_wins_over_bundled() {
        let dir = std::env::temp_dir().join(format!("qc-components-{}", uuid::Uuid::now_v7()));
        std::fs::create_dir_all(&dir).unwrap();
        let custom = dir.join("custom-ffmpeg");
        std::fs::write(&custom, b"x").unwrap();
        let resolved = resolve(
            Component::Ffmpeg,
            Some(custom.to_str().unwrap()),
            None,
            None,
            None,
        );
        assert_eq!(resolved.unwrap().source, "用户指定");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    /// 不存在的路径不会命中，避免把坏配置当成功。
    fn missing_path_is_ignored() {
        let resolved = resolve(
            Component::Ffmpeg,
            Some("C:/not-exist/ffmpeg.exe"),
            None,
            None,
            Some(Path::new("Z:/none")),
        );
        assert!(resolved.is_none() || resolved.unwrap().source != "用户指定");
    }
}
