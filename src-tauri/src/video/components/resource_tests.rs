//! 直接调用 Tauri 的资源映射器，防止 CPU 子目录被 glob 展平。
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

struct Scratch(PathBuf);
impl Drop for Scratch {
    /// 仅回收本测试在构建目录创建的唯一目录。
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
/// 主 GPU 与回退 CPU 可执行文件同名，打包后必须仍位于不同目录。
fn directory_mapping_preserves_cpu_fallback() {
    // Tauri 映射使用相对路径；Windows 绝对路径会被其 normalize 去掉盘符。
    let root =
        Scratch(PathBuf::from("target").join(format!("resource-test-{}", uuid::Uuid::now_v7())));
    let source = root.0.join("whisper");
    std::fs::create_dir_all(source.join("cpu")).unwrap();
    std::fs::write(source.join("whisper-cli.exe"), b"gpu").unwrap();
    std::fs::write(source.join("cpu").join("whisper-cli.exe"), b"cpu").unwrap();
    let mapping = HashMap::from([(source.to_string_lossy().to_string(), "whisper/".to_string())]);
    let paths = tauri::utils::resources::ResourcePaths::from_map(&mapping, true);
    let resources = paths.iter().map(|item| item.unwrap()).collect::<Vec<_>>();
    assert!(resources
        .iter()
        .any(|item| item.target() == Path::new("whisper/whisper-cli.exe")));
    assert!(resources
        .iter()
        .any(|item| item.target() == Path::new("whisper/cpu/whisper-cli.exe")));
    let config: serde_json::Value =
        serde_json::from_str(include_str!("../../../tauri.conf.json")).unwrap();
    assert_eq!(
        config["bundle"]["resources"]["resources/whisper/"],
        "whisper/"
    );
    assert!(config["bundle"]["resources"]
        .get("resources/whisper/*")
        .is_none());
}
