use super::*;
use crate::storage::testutil::TempDir;

#[test]
/// 同秒同长度更新必须换代；未知/旧版本保守返回历史，已知版本不再加载正文。
fn revision_skips_reads_and_changes_without_timestamp_change() {
    let temp = TempDir::new();
    let files = AgentFiles::new(temp.path()).unwrap();
    let mut session = files.create_session().unwrap();
    session.title = "甲".into();
    files.save_session(&session).unwrap();
    let (first, history) = files.observe_history(&session.id, None).unwrap();
    assert!(first.starts_with("v1:"));
    assert!(history.is_some());
    let before = AgentFiles::session_read_count();
    assert!(files
        .observe_history(&session.id, Some(&first))
        .unwrap()
        .1
        .is_none());
    files.session_identity(&session.id).unwrap();
    assert_eq!(AgentFiles::session_read_count(), before);
    session.title = "乙".into();
    files.save_session(&session).unwrap();
    let (second, history) = files.observe_history(&session.id, Some(&first)).unwrap();
    assert_ne!(first, second);
    assert_eq!(history.unwrap().title, "乙");
    assert!(files
        .observe_history(&session.id, Some("v0:unknown"))
        .unwrap()
        .1
        .is_some());
}

#[test]
/// 固定文件时间也不能让应用原子保存复用旧代，失效不依赖时间戳精度。
fn writes_invalidate_even_when_file_times_are_restored() {
    let temp = TempDir::new();
    let files = AgentFiles::new(temp.path()).unwrap();
    let mut session = files.create_session().unwrap();
    let path = files.record_path("sessions", &session.id).unwrap();
    let modified = std::fs::metadata(&path).unwrap().modified().unwrap();
    let (first, _) = files.observe_history(&session.id, None).unwrap();
    session.title = "新内容".into();
    files.save_session(&session).unwrap();
    std::fs::File::options()
        .write(true)
        .open(&path)
        .unwrap()
        .set_modified(modified)
        .unwrap();
    let (next, history) = files.observe_history(&session.id, Some(&first)).unwrap();
    assert_ne!(first, next);
    assert_eq!(history.unwrap().title, "新内容");
}

#[test]
/// 缓存按安全 Vault 路径隔离；删除后旧 token 不能伪装成未变。
fn revisions_are_vault_scoped_and_missing_records_fail() {
    let a = TempDir::new();
    let b = TempDir::new();
    let left = AgentFiles::new(a.path()).unwrap();
    let right = AgentFiles::new(b.path()).unwrap();
    let session = left.create_session().unwrap();
    let path = right.record_path("sessions", &session.id).unwrap();
    envelope::save_json(&path, &session).unwrap();
    let (token, _) = left.observe_history(&session.id, None).unwrap();
    let (other, history) = right.observe_history(&session.id, Some(&token)).unwrap();
    assert_ne!(token, other);
    assert!(history.is_some());
    left.delete_session(&session.id).unwrap();
    assert!(left.observe_history(&session.id, Some(&token)).is_err());
}

#[test]
/// 外部普通修改触发重新解析；损坏和高版本必须熔断，不返回缓存历史。
fn external_change_and_corruption_do_not_hide_behind_revision() {
    let temp = TempDir::new();
    let files = AgentFiles::new(temp.path()).unwrap();
    let mut session = files.create_session().unwrap();
    let path = files.record_path("sessions", &session.id).unwrap();
    let (token, _) = files.observe_history(&session.id, None).unwrap();
    session.title = "外部追加内容确保长度变化".into();
    envelope::save_json(&path, &session).unwrap();
    assert_eq!(
        files
            .observe_history(&session.id, Some(&token))
            .unwrap()
            .1
            .unwrap()
            .title,
        session.title
    );
    envelope::write_atomic(&path, b"{").unwrap();
    assert!(files.observe_history(&session.id, Some(&token)).is_err());
    envelope::write_atomic(&path, br#"{"formatVersion":999}"#).unwrap();
    assert_eq!(
        files.observe_history(&session.id, None).err().unwrap().code,
        "FILE_FORMAT_NEWER"
    );
}

#[test]
/// 缓存丢失只要求重新发送，不把旧 token 当成跨进程的信任凭据。
fn eviction_refreshes_conservatively() {
    let temp = TempDir::new();
    let files = AgentFiles::new(temp.path()).unwrap();
    let session = files.create_session().unwrap();
    let (token, _) = files.observe_history(&session.id, None).unwrap();
    files.invalidate_observation(&session.id).unwrap();
    let (next, history) = files.observe_history(&session.id, Some(&token)).unwrap();
    assert_ne!(token, next);
    assert!(history.is_some());
}
