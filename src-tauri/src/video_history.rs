//! 视频任务历史：用户视图只包含手动发起的任务，Agent 任务按保留窗口自行清理。
use crate::{
    error::CommandError,
    services::{video_tasks::terminal_retention_secs, AppServices},
    storage::{
        now_timestamp,
        video::{TaskOrigin, VideoStorage, VideoTaskRecord},
    },
    video::models::VideoTaskHistory,
    video_bridge,
};
use tauri::Manager;

/// 用户视图：先清理不再可用的 Agent 任务，再返回手动任务历史。
///
/// Agent 任务不进用户历史，因此这里的清理不会动到界面正在展示的记录。
pub(crate) fn for_user(
    app: &tauri::AppHandle,
    owner: &str,
) -> Result<Vec<VideoTaskHistory>, CommandError> {
    purge_finished(app);
    list(app, owner)
}

/// 清理已经无法再被工具或状态查询访问的 Agent 任务；失败只记日志，不阻断调用方。
pub(crate) fn purge_finished(app: &tauri::AppHandle) {
    if let Err(error) = purge(app) {
        eprintln!("VIDEO_AGENT_TASK_PURGE(detail): {error}");
    }
}

/// 单条记录清理失败只记录日志，避免一条坏记录卡住整个清理。
fn purge(app: &tauri::AppHandle) -> Result<(), CommandError> {
    let storage = VideoStorage::new(&video_bridge::vault_root(app)?);
    let services = app.state::<AppServices>();
    let now = now_timestamp();
    for record in storage.list_tasks()? {
        let live = services
            .video_tasks
            .live_state_for_history(&record.task_id)
            .is_some();
        if !purgeable(&record, live, now) {
            continue;
        }
        if let Err(error) = storage.remove_task(&record.task_id) {
            eprintln!("VIDEO_AGENT_TASK_PURGE(detail): {error}");
        }
    }
    Ok(())
}

/// 清理判定：Agent 任务、已离开内存登记表（工具与界面都读不到它了）、且超出终态保留窗口。
fn purgeable(record: &VideoTaskRecord, live: bool, now: i64) -> bool {
    record.origin == TaskOrigin::Agent
        && !live
        && now.saturating_sub(record.updated_at) >= terminal_retention_secs()
}

/// 用户可见历史：只列出手动任务，运行态由内存登记表补齐。
pub(crate) fn list(
    app: &tauri::AppHandle,
    _owner: &str,
) -> Result<Vec<VideoTaskHistory>, CommandError> {
    let root = video_bridge::vault_root(app)?;
    let storage = VideoStorage::new(&root);
    let services = app.state::<AppServices>();
    Ok(storage
        .list_tasks()?
        .into_iter()
        .filter(visible_to_user)
        .take(100)
        .map(|record| {
            let live = services.video_tasks.live_state_for_history(&record.task_id);
            let state = restored_state(&record.state, live.as_deref());
            VideoTaskHistory {
                task_id: record.task_id,
                url: record.source_url,
                title: record.title,
                state,
                updated_at: record.updated_at,
                note_path: record.note_path,
                error: record.error,
                pages: record.pages,
                quality: record.quality,
            }
        })
        .collect())
}

/// 用户历史只承认手动任务：Agent 任务由工具自己持有任务 id，不进入界面列表。
fn visible_to_user(record: &VideoTaskRecord) -> bool {
    record.origin == TaskOrigin::User
}

/// 保留明确终态，防止历史运行态在重启后误导用户认为仍有后台工作。
fn restored_state(recorded: &str, live: Option<&str>) -> String {
    live.unwrap_or({
        if matches!(recorded, "running" | "queued") {
            "interrupted"
        } else {
            recorded
        }
    })
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 构造指定来源与更新时间的记录，避免用例依赖存储层时间戳。
    fn record(origin: TaskOrigin, updated_at: i64) -> VideoTaskRecord {
        let mut record = VideoTaskRecord::new("t1", "key", "url");
        record.origin = origin;
        record.updated_at = updated_at;
        record
    }

    #[test]
    /// 历史运行态只读恢复为中断，当前运行任务和已完成结果不变。
    fn distinguishes_interrupted_history() {
        assert_eq!(restored_state("running", None), "interrupted");
        assert_eq!(restored_state("queued", None), "interrupted");
        assert_eq!(restored_state("completed", None), "completed");
        assert_eq!(restored_state("running", Some("running")), "running");
    }

    #[test]
    /// 用户视图只认手动任务，Agent 任务一律不出现在列表里。
    fn user_view_keeps_manual_tasks_only() {
        assert!(visible_to_user(&record(TaskOrigin::User, 0)));
        assert!(!visible_to_user(&record(TaskOrigin::Agent, 0)));
    }

    #[test]
    /// 只有离开登记表且超出保留窗口的 Agent 任务才会被清理。
    fn purge_requires_expired_unreferenced_agent_task() {
        let now = 1_000_000;
        let expired = now - terminal_retention_secs();
        let fresh = expired + 1;
        assert!(purgeable(&record(TaskOrigin::Agent, expired), false, now));
        assert!(!purgeable(&record(TaskOrigin::Agent, expired), true, now));
        assert!(!purgeable(&record(TaskOrigin::Agent, fresh), false, now));
        assert!(!purgeable(&record(TaskOrigin::User, expired), false, now));
    }
}
