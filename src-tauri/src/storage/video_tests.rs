//! 仅覆盖视频存储：兼容、缓存隔离、非破坏性清理与路径边界。
use super::*;
use crate::storage::testutil::TempDir;

/// 使用真实临时知识库，避免测试意外依赖构造函数创建根目录。
fn fixture() -> (TempDir, VideoStorage) {
    let root = TempDir::new();
    let storage = VideoStorage::new(root.path());
    (root, storage)
}

/// 小型转录使测试只关注存储契约。
fn transcript(source: &str) -> Transcript {
    Transcript {
        language: "zh".into(),
        source: source.into(),
        segments: vec![],
    }
}

/// 只在测试里直接构造历史布局，模拟旧版应用写出的文件。
fn legacy(root: &Path, relative: &str, value: &impl Serialize) -> PathBuf {
    let path = root.join(paths::LEGACY).join(relative);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, serde_json::to_vec(value).unwrap()).unwrap();
    path
}

#[test]
/// 新任务状态与时间戳保持既有契约。
fn record_starts_queued() {
    let record = VideoTaskRecord::new("t1", "key", "url");
    assert_eq!(record.state, "queued");
    assert_eq!(record.created_at, record.updated_at);
}

#[test]
/// 非法 id、分隔符、设备名和文件流均不能产生路径。
fn rejects_unsafe_components() {
    let (_root, storage) = fixture();
    for value in [
        "",
        ".",
        "..",
        "../outside",
        "a/b",
        "a\\b",
        "C:/outside",
        "CON",
        "a.",
        "a ",
        "a:stream",
    ] {
        assert!(storage.task_dir(value).is_err(), "{value}");
        assert!(storage.media_file("t1", value).is_err(), "{value}");
        assert!(storage.shots_dir(value).is_err(), "{value}");
        assert!(storage.load_task(value).is_err(), "{value}");
    }
    assert!(storage.media_file("t1", "task.json").is_err());
    assert!(storage.media_file("t1", "audio_p1.wav").is_ok());
}

#[test]
/// 读取缺失数据不创建目录，未知根目录也不会被隐式创建。
fn reads_are_non_creating() {
    let (root, storage) = fixture();
    assert!(storage.load_task("missing").unwrap().is_none());
    assert!(storage.load_transcript("missing").unwrap().is_none());
    assert!(storage.load_page_transcript("BV1", 42).unwrap().is_none());
    assert!(storage.list_tasks().unwrap().is_empty());
    assert!(!root.path().join(".quailcard").exists());
    let missing = root.path().join("missing");
    assert!(VideoStorage::new(&missing).task_dir("t1").is_err());
    assert!(!missing.exists());
}

#[test]
/// 摘要保留完整缓存身份，模型、来源与非法字符不会被删成相同键。
fn page_cache_identity_is_lossless() {
    let (_root, storage) = fixture();
    for key in [
        "BV1",
        "../BV1/..",
        "BV1:asr:model-a:zh",
        "BV1:asr:model-b:zh",
    ] {
        storage
            .save_page_transcript(key, 42, &transcript(key))
            .unwrap();
    }
    for key in [
        "BV1",
        "../BV1/..",
        "BV1:asr:model-a:zh",
        "BV1:asr:model-b:zh",
    ] {
        assert_eq!(
            storage
                .load_page_transcript(key, 42)
                .unwrap()
                .unwrap()
                .source,
            key
        );
    }
}

#[test]
/// 旧任务可展示与复用，后续保存只写隔离目录且不更改旧字节。
fn preserves_legacy_tasks_and_cards() {
    let (root, storage) = fixture();
    let mut old = VideoTaskRecord::new("old", "cache", "url");
    old.updated_at = 1;
    let path = legacy(root.path(), "tasks/old/task.json", &old);
    let original = std::fs::read(&path).unwrap();
    legacy(
        root.path(),
        "tasks/old/transcript.json",
        &to_file(&transcript("legacy")),
    );
    let card = legacy(
        root.path(),
        "tasks/card/task.json",
        &serde_json::json!({"cards":[], "taskId":"card"}),
    );
    let card_bytes = std::fs::read(&card).unwrap();
    assert!(storage.load_task("card").unwrap().is_none());
    assert_eq!(
        storage.find_reusable("cache").unwrap().as_deref(),
        Some("old")
    );
    old.title = "updated".into();
    storage.save_task(&old).unwrap();
    assert_eq!(storage.load_task("old").unwrap().unwrap().title, "updated");
    assert_eq!(storage.list_tasks().unwrap().len(), 1);
    assert_eq!(std::fs::read(path).unwrap(), original);
    assert_eq!(std::fs::read(card).unwrap(), card_bytes);
    assert!(storage
        .task_dir("old")
        .unwrap()
        .starts_with(root.path().canonicalize().unwrap().join(paths::CURRENT)));
}

#[test]
/// 旧分 P 缓存只读兼容；复合键不回退，真实卡片和源笔记优先。
fn legacy_pages_do_not_shadow_cards_or_scoped_keys() {
    let (root, storage) = fixture();
    let path = legacy(
        root.path(),
        "pages/BV1-42.json",
        &to_file(&transcript("legacy")),
    );
    let bytes = std::fs::read(&path).unwrap();
    assert_eq!(
        storage
            .load_page_transcript("BV1", 42)
            .unwrap()
            .unwrap()
            .source,
        "legacy"
    );
    assert!(storage
        .load_page_transcript("BV1:asr:model:zh", 42)
        .unwrap()
        .is_none());
    let note = root.path().join("video/pages/BV1-42.md");
    std::fs::create_dir_all(note.parent().unwrap()).unwrap();
    std::fs::write(note, "真实笔记").unwrap();
    assert!(storage.load_page_transcript("BV1", 42).unwrap().is_none());
    storage
        .save_page_transcript("BV1", 42, &transcript("new"))
        .unwrap();
    assert_eq!(std::fs::read(path).unwrap(), bytes);
}

#[test]
/// 单条未来版本与损坏记录不阻断历史，未来版本仍在单项读取时熔断。
fn history_is_sorted_and_isolates_bad_records() {
    let (root, storage) = fixture();
    for (id, time) in [("older", 1), ("newer", 2)] {
        let mut record = VideoTaskRecord::new(id, "key", "url");
        record.updated_at = time;
        storage.save_task(&record).unwrap();
    }
    let mut future = VideoTaskRecord::new("future", "key", "url");
    future.format_version = 99;
    legacy(root.path(), "tasks/future/task.json", &future);
    std::fs::write(storage.task_file("broken").unwrap(), "broken").unwrap();
    assert!(storage.load_task("future").is_err());
    let ids: Vec<_> = storage
        .list_tasks()
        .unwrap()
        .into_iter()
        .map(|r| r.task_id)
        .collect();
    assert_eq!(ids, ["newer", "older"]);
}

#[test]
/// 缓存损坏可重建，但不再将仅存在而不可解析的转录判为命中。
fn corrupt_transcript_is_missing_not_reusable() {
    let (_root, storage) = fixture();
    storage
        .save_task(&VideoTaskRecord::new("t1", "key", "url"))
        .unwrap();
    std::fs::write(storage.transcript_file("t1").unwrap(), "broken").unwrap();
    assert!(storage.find_reusable("key").unwrap().is_none());
    assert!(storage.load_transcript("t1").unwrap().is_none());
}

#[test]
/// 清理无权判断运行状态和旧目录归属，因此任何保留期都不能删除数据。
fn cleanup_never_removes_tasks_or_card_namespace() {
    let (root, storage) = fixture();
    let path = legacy(
        root.path(),
        "tasks/note/task.json",
        &serde_json::json!({"cards":[]}),
    );
    storage
        .save_task(&VideoTaskRecord::new("running", "key", "url"))
        .unwrap();
    storage.cleanup(0).unwrap();
    storage.cleanup(u64::MAX).unwrap();
    assert!(path.exists());
    assert!(storage.load_task("running").unwrap().is_some());
}

#[test]
/// 显式清理仅删除生成媒体，记录、任意文件、子目录与旧副本必须保留。
fn terminal_cleanup_is_exact_and_non_recursive() {
    let (root, storage) = fixture();
    storage
        .save_task(&VideoTaskRecord::new("t1", "key", "url"))
        .unwrap();
    storage.save_transcript("t1", &transcript("keep")).unwrap();
    let dir = storage.task_dir("t1").unwrap();
    for name in [
        "audio_p1.m4s",
        "video_p2.m4s",
        "audio_p1.wav",
        "note.md",
        "other.wav",
    ] {
        std::fs::write(dir.join(name), "media").unwrap();
    }
    let shot = storage.shot_file("t1", "shot-p1-000001.jpg").unwrap();
    std::fs::write(&shot, "shot").unwrap();
    std::fs::write(storage.shot_file("t1", "user.jpg").unwrap(), "keep").unwrap();
    std::fs::create_dir_all(dir.join("nested")).unwrap();
    std::fs::write(dir.join("nested/audio_p1.wav"), "keep").unwrap();
    let old = legacy(root.path(), "tasks/t1/audio_p1.wav", &"keep");
    storage.cleanup_task_media("t1").unwrap();
    assert!(!shot.exists());
    for name in ["audio_p1.m4s", "video_p2.m4s", "audio_p1.wav"] {
        assert!(!dir.join(name).exists());
    }
    for name in [
        "task.json",
        "transcript.json",
        "note.md",
        "other.wav",
        "shots/user.jpg",
        "nested/audio_p1.wav",
    ] {
        assert!(dir.join(name).exists());
    }
    assert!(old.exists());
    assert!(storage.cleanup_task_media("../outside").is_err());
}

#[test]
/// 旧记录缺少 origin 字段时按用户任务读取，新记录原样回读来源。
fn origin_defaults_to_user_and_round_trips() {
    let (_root, storage) = fixture();
    std::fs::write(
        storage.task_file("legacy").unwrap(),
        br#"{"formatVersion":1,"taskId":"legacy"}"#,
    )
    .unwrap();
    assert_eq!(
        storage.load_task("legacy").unwrap().unwrap().origin,
        TaskOrigin::User
    );
    let mut agent = VideoTaskRecord::new("agent", "key", "url");
    agent.origin = TaskOrigin::Agent;
    storage.save_task(&agent).unwrap();
    assert_eq!(
        storage.load_task("agent").unwrap().unwrap().origin,
        TaskOrigin::Agent
    );
}

#[test]
/// 删除整条任务只清受控内容：记录、转录、生成媒体与已清空的目录。
fn remove_task_deletes_controlled_content_only() {
    let (root, storage) = fixture();
    let mut record = VideoTaskRecord::new("t1", "key", "url");
    record.origin = TaskOrigin::Agent;
    storage.save_task(&record).unwrap();
    storage.save_transcript("t1", &transcript("gone")).unwrap();
    let dir = storage.task_dir("t1").unwrap();
    std::fs::write(dir.join("audio_p1.wav"), "media").unwrap();
    let shot = storage.shot_file("t1", "shot-p1-000001.jpg").unwrap();
    std::fs::write(&shot, "shot").unwrap();
    storage.remove_task("t1").unwrap();
    assert!(!dir.exists());
    assert!(storage.load_task("t1").unwrap().is_none());
    assert!(storage.load_transcript("t1").unwrap().is_none());
    // 未知文件使目录留存，绝不递归删除用户内容。
    storage.save_task(&record).unwrap();
    std::fs::write(dir.join("note.md"), "keep").unwrap();
    storage.remove_task("t1").unwrap();
    assert!(dir.join("note.md").exists());
    assert!(storage.load_task("t1").unwrap().is_none());
    assert!(storage.remove_task("../outside").is_err());
    // 旧命名空间永远不参与删除。
    let old = legacy(root.path(), "tasks/legacy/task.json", &record);
    storage.remove_task("legacy").unwrap();
    assert!(old.exists());
}

#[test]
/// 保留期只影响过期终态媒体，运行中或未过期任务及历史始终存在。
fn retention_preserves_active_and_recent_media() {
    let (_root, storage) = fixture();
    for (id, state, age) in [
        ("old", "completed", 3),
        ("recent", "completed", 0),
        ("active", "running", 3),
    ] {
        let mut record = VideoTaskRecord::new(id, "key", "url");
        record.state = state.into();
        record.updated_at -= age * 86_400;
        storage.save_task(&record).unwrap();
        std::fs::write(storage.media_file(id, "audio_p1.wav").unwrap(), "media").unwrap();
    }
    storage.cleanup(u64::MAX).unwrap();
    assert!(storage.media_file("old", "audio_p1.wav").unwrap().exists());
    storage.cleanup(1).unwrap();
    assert!(!storage.media_file("old", "audio_p1.wav").unwrap().exists());
    for id in ["recent", "active"] {
        assert!(storage.media_file(id, "audio_p1.wav").unwrap().exists());
    }
    assert_eq!(storage.list_tasks().unwrap().len(), 3);
}

/// 符号链接用例在 Windows 显式忽略；主动运行时任何创建失败都必须报错。
fn link(target: &Path, path: &Path, directory: bool) -> bool {
    #[cfg(windows)]
    let result = if directory {
        std::os::windows::fs::symlink_dir(target, path)
    } else {
        std::os::windows::fs::symlink_file(target, path)
    };
    #[cfg(unix)]
    let result = {
        let _ = directory;
        std::os::unix::fs::symlink(target, path)
    };
    result.expect("创建测试链接失败，需要符号链接权限");
    true
}

#[test]
/// 任务祖先链接不得让任何写入穿越知识库，包括知识库内的其他目录。
#[cfg_attr(windows, ignore = "requires symlink privilege")]
fn rejects_linked_task_directory() {
    let (root, storage) = fixture();
    let outside = TempDir::new();
    storage.create_dir("tasks").unwrap();
    let path = root.path().join(paths::CURRENT).join("tasks/linked");
    if !link(outside.path(), &path, true) {
        return;
    }
    assert!(storage.task_dir("linked").is_err());
    assert!(storage.media_file("linked", "audio.wav").is_err());
    assert!(storage.shots_dir("linked").is_err());
    assert!(storage.load_task("linked").is_err());
    storage.cleanup(0).unwrap();
    assert_eq!(std::fs::read_dir(outside.path()).unwrap().count(), 0);
}

#[test]
/// 最终媒体、任务与缓存链接也被拒绝，不能仅验证父目录。
#[cfg_attr(windows, ignore = "requires symlink privilege")]
fn rejects_linked_files_and_shots() {
    let (_root, storage) = fixture();
    let outside = TempDir::new();
    let target = outside.path().join("target");
    std::fs::write(&target, "unchanged").unwrap();
    let dir = storage.task_dir("t1").unwrap();
    if !link(&target, &dir.join("task.json"), false) {
        return;
    }
    assert!(storage
        .save_task(&VideoTaskRecord::new("t1", "key", "url"))
        .is_err());
    assert!(storage.load_task("t1").is_err());
    assert!(link(&target, &dir.join("audio.wav"), false));
    assert!(storage.media_file("t1", "audio.wav").is_err());
    assert!(link(outside.path(), &dir.join("shots"), true));
    assert!(storage.shots_dir("t1").is_err());
    storage.create_dir("pages").unwrap();
    let page = storage.page_file("BV1", 42).unwrap();
    assert!(link(&target, &page, false));
    assert!(storage.load_page_transcript("BV1", 42).is_err());
    assert!(storage
        .save_page_transcript("BV1", 42, &transcript("x"))
        .is_err());
    assert_eq!(std::fs::read_to_string(target).unwrap(), "unchanged");
}

#[cfg(windows)]
#[test]
/// 目录联接无需管理员符号链接权限，受控命名空间也必须拒绝被重定向。
fn rejects_managed_namespace_junction() {
    let (root, storage) = fixture();
    let outside = TempDir::new();
    std::fs::write(outside.path().join("sentinel"), "keep").unwrap();
    // cmd 内建命令把正斜杠解释为开关；逐段 join 保持 Windows 原生分隔符。
    let parent = root.path().join(".quailcard").join("agent");
    std::fs::create_dir_all(&parent).unwrap();
    let junction = parent.join(".video");
    let status = std::process::Command::new("cmd")
        .args(["/C", "mklink", "/J"])
        .arg(&junction)
        .arg(outside.path())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::inherit())
        .status()
        .unwrap();
    assert!(status.success(), "创建无特权测试目录联接失败");
    assert!(storage.task_dir("escape").is_err());
    assert!(storage.cleanup_task_media("escape").is_err());
    assert_eq!(std::fs::read_dir(outside.path()).unwrap().count(), 1);
    // 显式只删除联接本身，避免测试夹具清理含糊处理目标目录。
    std::fs::remove_dir(junction).unwrap();
}
