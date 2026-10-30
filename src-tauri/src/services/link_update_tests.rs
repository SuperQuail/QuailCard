//! 改名联动改写的测试：规划、写法选择、卡片锚点平移与失败隔离。

use std::path::PathBuf;

use uuid::Uuid;

use super::link_update::{apply, plan};
use crate::storage::{links::Edit, testutil, Storage};
use crate::vaultfs::VaultState;

/// 隔离的临时 Vault：Drop 时递归清理。
struct TestVault(PathBuf);

impl TestVault {
    /// 用 UUID 隔离并发测试目录。
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!("quailcard-link-update-{}", Uuid::now_v7()));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    /// 构造已选择此目录的 VaultState。
    fn state(&self) -> VaultState {
        let state = VaultState::new();
        state.set_root(self.0.clone()).unwrap();
        state
    }

    /// 写入一篇笔记并返回相对路径。
    fn write(&self, relative: &str, content: &str) -> String {
        let path = self.0.join(relative);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(&path, content).unwrap();
        relative.to_string()
    }

    /// 读取笔记正文。
    fn read(&self, relative: &str) -> String {
        std::fs::read_to_string(self.0.join(relative)).unwrap()
    }
}

impl Drop for TestVault {
    /// 测试结束后尽力删除临时目录。
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// 建好索引的存储：把 Vault 里现有笔记写入内存索引。
async fn indexed(storage: &Storage, vault: &TestVault, files: &[(&str, &str)]) {
    for (path, content) in files {
        vault.write(path, content);
        storage.upsert_note_index(path, content, 1).await.unwrap();
    }
}

#[tokio::test]
/// 只有能解析到被移动笔记的链接才会进入改写计划。
async fn plan_collects_only_links_into_moved_note() {
    let (storage, _config, _vault_dir) = testutil::test_storage().await;
    let vault = TestVault::new();
    indexed(
        &storage,
        &vault,
        &[
            ("目标.md", "# 目标"),
            ("引用.md", "看 [[目标]] 与 [[无关]]"),
            ("无关.md", "# 无关"),
        ],
    )
    .await;
    let pending = plan(&storage, "目标.md").await;
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].note_path, "引用.md");
    assert_eq!(pending[0].resolved, "目标.md");
}

#[tokio::test]
/// 文件夹改名时，前缀下的所有笔记都会被识别为"被移动"。
async fn plan_covers_whole_folder() {
    let (storage, _config, _vault_dir) = testutil::test_storage().await;
    let vault = TestVault::new();
    indexed(
        &storage,
        &vault,
        &[
            ("旧/甲.md", "# 甲"),
            ("旧/乙.md", "# 乙"),
            ("引用.md", "[[旧/甲]] 和 [[旧/乙]]"),
        ],
    )
    .await;
    let pending = plan(&storage, "旧").await;
    assert_eq!(pending.len(), 2);
}

#[tokio::test]
/// 没有引用时计划为空，apply 立即返回空报告。
async fn plan_is_empty_without_references() {
    let (storage, _config, _vault_dir) = testutil::test_storage().await;
    let vault = TestVault::new();
    indexed(&storage, &vault, &[("目标.md", "# 目标")]).await;
    assert!(plan(&storage, "目标.md").await.is_empty());
}

#[tokio::test]
/// 改名后引用方的链接被改写，且标题与显示文本保持原样。
async fn apply_rewrites_referencing_note() {
    let (storage, _config, _vault_dir) = testutil::test_storage().await;
    let vault = TestVault::new();
    let state = vault.state();
    indexed(
        &storage,
        &vault,
        &[("旧名.md", "# 旧名"), ("引用.md", "看 [[旧名#小节|显示]]")],
    )
    .await;
    let pending = plan(&storage, "旧名.md").await;
    state.rename_note("旧名.md", "新名.md").unwrap();
    storage
        .rename_note_paths("旧名.md", "新名.md")
        .await
        .unwrap();
    let report = apply(&state, &storage, pending, "旧名.md", "新名.md")
        .await
        .unwrap();
    assert_eq!(report.updated, ["引用.md"]);
    assert!(report.failed.is_empty());
    assert_eq!(vault.read("引用.md"), "看 [[新名#小节|显示]]");
}

#[tokio::test]
/// 改名后新标题不再唯一时，改写退化成相对路径写法，避免指向另一篇同名笔记。
async fn apply_uses_path_form_on_title_conflict() {
    let (storage, _config, _vault_dir) = testutil::test_storage().await;
    let vault = TestVault::new();
    let state = vault.state();
    indexed(
        &storage,
        &vault,
        &[
            ("甲/名.md", "# 名"),
            ("乙/名.md", "# 名"),
            ("引用.md", "[[甲/名]]"),
        ],
    )
    .await;
    let pending = plan(&storage, "甲/名.md").await;
    state.rename_note("甲/名.md", "乙/甲改名.md").unwrap();
    storage
        .rename_note_paths("甲/名.md", "乙/甲改名.md")
        .await
        .unwrap();
    let report = apply(&state, &storage, pending, "甲/名.md", "乙/甲改名.md")
        .await
        .unwrap();
    assert_eq!(report.updated, ["引用.md"]);
    // 原写法是路径形式，且新标题与另一篇重名 ⇒ 继续写路径（不带 .md）。
    assert_eq!(vault.read("引用.md"), "[[乙/甲改名]]");
}

#[tokio::test]
/// 被移动笔记不存在时不会误改其他笔记的内容。
async fn apply_skips_when_nothing_pending() {
    let (storage, _config, _vault_dir) = testutil::test_storage().await;
    let vault = TestVault::new();
    let state = vault.state();
    indexed(&storage, &vault, &[("引用.md", "没有链接")]).await;
    let report = apply(&state, &storage, Vec::new(), "旧.md", "新.md")
        .await
        .unwrap();
    assert!(report.updated.is_empty() && report.failed.is_empty());
    assert_eq!(vault.read("引用.md"), "没有链接");
}

#[tokio::test]
/// 卡片锚点：改写点之前不动、之后平移、被劈开时保持原样。
async fn shifts_card_sources_around_edits() {
    let (storage, _config, vault_dir) = testutil::test_storage().await;
    let note = "笔记.md";
    let content = "AAA [[旧]] BBB";
    storage.upsert_note_index(note, content, 1).await.unwrap();
    std::fs::write(vault_dir.path().join(note), content).unwrap();

    /// 构造一条锚点固定的来源卡片。
    fn card_input(note: &str, from: usize, to: usize) -> crate::models::CardInput {
        crate::models::CardInput {
            id: Some(format!("card-{from}")),
            note_path: note.to_string(),
            source_ref: None,
            source: Some(crate::models::CardSource {
                from,
                to,
                excerpt: "x".into(),
                prefix: String::new(),
                suffix: String::new(),
            }),
            kind: "qa".to_string(),
            front: "问".to_string(),
            back: "答".to_string(),
            detail: None,
            example: None,
            aliases: Vec::new(),
            rubric: Vec::new(),
        }
    }

    // 改写 [[旧]] → [[新的名字]]：目标文本区间 [5,6)，长度 1 → 4。
    let edit = Edit {
        from: 5,
        to: 6,
        new_len: 4,
    };
    storage.save_card(card_input(note, 0, 2)).await.unwrap();
    storage.save_card(card_input(note, 8, 10)).await.unwrap();
    storage.save_card(card_input(note, 5, 6)).await.unwrap();
    storage.shift_card_sources(note, &[edit]).await.unwrap();
    let cards = storage.list_note_cards(note).await.unwrap();
    let by_id: std::collections::HashMap<String, (usize, usize)> = cards
        .into_iter()
        .map(|card| {
            let source = card.source.unwrap();
            (card.id, (source.from, source.to))
        })
        .collect();
    assert_eq!(by_id["card-0"], (0, 2), "改写点之前的锚点不动");
    assert_eq!(by_id["card-8"], (11, 13), "改写点之后的锚点按增量平移");
    assert_eq!(by_id["card-5"], (5, 6), "与改写区间相交的锚点保持原样");
}
