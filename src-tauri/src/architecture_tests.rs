//! 分层依赖门禁：把 SOLID 的依赖倒置从约定变成会失败的测试。
//!
//! 只做静态文本扫描，不编译被检查代码；违规时输出文件与行号。

use std::fs;
use std::path::{Path, PathBuf};

/// 递归收集目录下全部 Rust 源文件。
fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            rust_files(&path, out);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            out.push(path);
        }
    }
}

/// 返回命中任一禁用子串的文件与行号；注释行不参与判断，避免文档措辞误报。
fn violations(files: &[PathBuf], forbidden: &[&str]) -> Vec<String> {
    let mut found = Vec::new();
    for file in files {
        let Ok(text) = fs::read_to_string(file) else {
            continue;
        };
        for (index, line) in text.lines().enumerate() {
            let trimmed = line.trim_start();
            if trimmed.starts_with("//") {
                continue;
            }
            if forbidden.iter().any(|needle| line.contains(needle)) {
                found.push(format!("{}:{}  {}", file.display(), index + 1, trimmed));
            }
        }
    }
    found
}

/// 源文件是否位于给定目录（含子目录）。
fn under(path: &Path, dir: &Path) -> bool {
    path.starts_with(dir)
}

/// 源文件是否属于 Agent 用例层；P4 会从单文件迁移为 services/agent/ 目录。
fn is_agent_layer(path: &Path, root: &Path) -> bool {
    let services = root.join("services");
    path == services.join("agent.rs") || under(path, &services.join("agent"))
}

#[test]
/// ai/llm 是供应商中立层，不得触碰应用、存储、桌面与具体资源实现。
fn llm_layer_stays_provider_neutral() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    rust_files(&root.join("ai").join("llm"), &mut files);
    let found = violations(
        &files,
        &[
            "services::",
            "storage::",
            "tauri::",
            "dictionary::",
            "vaultfs::",
            "rusqlite",
        ],
    );
    assert!(found.is_empty(), "ai/llm 违反分层:\n{}", found.join("\n"));
}

#[test]
/// Agent 用例层只依赖端口与领域类型，不直接持有 HTTP、桌面运行时或数据库。
fn agent_usecase_ignores_transport_and_ui() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut all = Vec::new();
    rust_files(&root, &mut all);
    let files: Vec<PathBuf> = all
        .into_iter()
        .filter(|path| is_agent_layer(path, &root))
        .collect();
    let found = violations(&files, &["reqwest::", "tauri::", "rusqlite"]);
    assert!(
        found.is_empty(),
        "Agent 用例层违反分层:\n{}",
        found.join("\n")
    );
}

#[test]
/// ai 层不得反向依赖桌面运行时与数据库。
fn ai_layer_never_reaches_ui_or_database() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    rust_files(&root.join("ai"), &mut files);
    let found = violations(&files, &["tauri::", "rusqlite"]);
    assert!(found.is_empty(), "ai 层违反分层:\n{}", found.join("\n"));
}
