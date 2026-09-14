//! 两轮候选调度测试：假截帧器只返回临时字节，假模型按实际编号作答。
use super::*;
use crate::services::{
    agent_ports::{AgentFuture, AgentModelReply},
    video_tasks::VideoTaskRegistry,
};
use serde_json::Value;
use std::sync::Mutex;

struct FakeFrames {
    attempted: Vec<f64>,
    fail_first: bool,
    cancel: Option<VideoControl>,
}
impl ShotFrames for FakeFrames {
    /// 测试锚点属于第二个分 P，搜索不得跨到零秒。
    fn bounds(&self, _: f64) -> Option<(f64, f64)> {
        Some((60.0, 180.0))
    }
    /// 首轮可模拟全部失败；帧字节记录时间，验证选中后没有额外取帧。
    fn candidate(&mut self, at: f64) -> AgentFuture<'_, Option<CandidateFrame>> {
        self.attempted.push(at);
        let fail = self.fail_first && self.attempted.len() <= 5;
        if let Some(control) = &self.cancel {
            control.cancel();
        }
        Box::pin(async move {
            if fail {
                return Err(CommandError::new("FAKE_FRAME", "取帧失败"));
            }
            Ok(Some(CandidateFrame {
                seconds: at,
                stamp: "".into(),
                file_name: "temporary".into(),
                path: Default::default(),
                bytes: at.to_le_bytes().to_vec(),
            }))
        })
    }
}

struct FakeModel {
    replies: Mutex<Vec<&'static str>>,
    seen: Mutex<Vec<Vec<usize>>>,
}
impl AgentModel for FakeModel {
    /// 记录实际图像编号及数量，以最后一张的真实编号返回确认。
    fn call<'a>(
        &'a self,
        _: &'a str,
        messages: &'a [Value],
        _: &'a [crate::ai::ToolDefinition],
        _: &'a (dyn Fn(&str) + Send + Sync),
        _: &'a (dyn Fn(&str) + Send + Sync),
    ) -> AgentFuture<'a, AgentModelReply> {
        Box::pin(async move {
            let content = messages[0]["content"].as_array().unwrap();
            let ids: Vec<usize> = content
                .iter()
                .filter_map(|item| item["text"].as_str())
                .filter_map(|text| text.strip_prefix("候选编号 "))
                .map(|id| id.trim().parse().unwrap())
                .collect();
            assert_eq!(
                content
                    .iter()
                    .filter(|item| item["type"] == "image_url")
                    .count(),
                ids.len()
            );
            assert!(ids.len() <= 5);
            let last = *ids.last().unwrap();
            self.seen.lock().unwrap().push(ids);
            let reply = self.replies.lock().unwrap().remove(0);
            if reply == "error" {
                return Err(CommandError::new("FAKE_MODEL", "请求失败"));
            }
            if reply == "cancel" {
                return Err(cancelled());
            }
            let text = if reply == "select" {
                format!("{{\"candidate\":{last},\"matched\":true,\"clear\":true}}")
            } else {
                reply.to_string()
            };
            Ok(AgentModelReply {
                text,
                ..Default::default()
            })
        })
    }
}

/// 创建互相隔离的控制块与两轮测试依赖。
fn fixture(replies: Vec<&'static str>, fail_first: bool) -> (VideoControl, FakeFrames, FakeModel) {
    let record = VideoTaskRecord::new("candidate-test", "key", "url");
    let control = VideoTaskRegistry::default()
        .register("owner", &record)
        .unwrap();
    (
        control,
        FakeFrames {
            attempted: vec![],
            fail_first,
            cancel: None,
        },
        FakeModel {
            replies: Mutex::new(replies),
            seen: Mutex::new(vec![]),
        },
    )
}

#[tokio::test]
/// 首轮拒绝后扩大范围，只选第二轮实际看过的字节，第二轮后不重截。
async fn none_then_select_has_no_extra_grab() {
    let (control, mut source, model) = fixture(vec![r#"{"candidate":"none"}"#, "select"], false);
    let (frame, _) = select_target(
        &mut source,
        &model,
        &control,
        100.0,
        "函数定义",
        &HashSet::new(),
    )
    .await
    .unwrap();
    let frame = frame.unwrap();
    assert_eq!(frame.seconds, 130.0);
    assert_eq!(frame.bytes, 130f64.to_le_bytes());
    assert_eq!(source.attempted.len(), 9);
    let seen = model.seen.lock().unwrap();
    assert_eq!(seen.len(), 2);
    assert!(seen[1].iter().all(|id| !seen[0].contains(id)));
}

#[tokio::test]
/// 首轮五张全部失败也能进入扩大搜索，而不是提前判定无图。
async fn failed_initial_candidates_still_expand() {
    let (control, mut source, model) = fixture(vec!["select"], true);
    let (frame, _) = select_target(
        &mut source,
        &model,
        &control,
        100.0,
        "目标",
        &HashSet::new(),
    )
    .await
    .unwrap();
    assert!(frame.is_some());
    assert_eq!(source.attempted.len(), 9);
    assert_eq!(model.seen.lock().unwrap().len(), 1);
}

#[tokio::test]
/// 两轮拒绝、格式错误或请求失败均不产出可保存帧。
async fn no_match_and_errors_never_produce_attachment() {
    for reply in [r#"{"candidate":"none"}"#, "invalid", "error"] {
        let (control, mut source, model) = fixture(vec![reply, reply], false);
        let (frame, reason) = select_target(
            &mut source,
            &model,
            &control,
            100.0,
            "目标",
            &HashSet::new(),
        )
        .await
        .unwrap();
        assert!(frame.is_none());
        assert!(!reason.is_empty());
        assert_eq!(model.seen.lock().unwrap().len(), 2);
    }
}

#[tokio::test]
/// 媒体中取消及模型取消错误都向上传播，不能作为普通无匹配处理。
async fn cancellation_propagates() {
    let (control, mut source, model) = fixture(vec!["select"], false);
    source.cancel = Some(control.clone());
    let result = select_target(
        &mut source,
        &model,
        &control,
        100.0,
        "目标",
        &HashSet::new(),
    )
    .await;
    assert_eq!(result.err().unwrap().code, "VIDEO_CANCELLED");
    assert!(model.seen.lock().unwrap().is_empty());
    let (control, mut source, model) = fixture(vec!["cancel"], false);
    let result = select_target(
        &mut source,
        &model,
        &control,
        100.0,
        "目标",
        &HashSet::new(),
    )
    .await;
    assert_eq!(result.err().unwrap().code, "VIDEO_CANCELLED");
}

#[test]
/// 边界五点仍落在原分 P，短视频、非法值与上轮候选不会越界或重复。
fn candidate_bounds_and_no_duplicates() {
    for at in [60.0, 60.1, 100.0, 179.999] {
        let first = candidate_times(at, (60.0, 180.0), 12.0, &HashSet::new());
        assert_eq!(first.len(), 5);
        assert!(first
            .iter()
            .all(|time| *time >= 60.0 && *time < 180.0 && (*time - at).abs() <= 12.001));
        let used = first.iter().map(|time| time_key(*time)).collect();
        let second = candidate_times(at, (60.0, 180.0), 30.0, &used);
        assert!(second.len() <= 5);
        assert!(second
            .iter()
            .all(|time| !used.contains(&time_key(*time)) && *time >= 60.0 && *time < 180.0));
    }
    for at in [f64::NAN, f64::INFINITY, -1.0, 180.0] {
        assert!(candidate_times(at, (60.0, 180.0), 12.0, &HashSet::new()).is_empty());
    }
    let tiny = candidate_times(0.0, (0.0, 0.0001), 12.0, &HashSet::new());
    assert_eq!(tiny, vec![0.0]);
}

#[tokio::test]
/// 不同目标不能复用已经选中的实际时间点。
async fn already_selected_time_is_excluded() {
    let (control, mut source, model) = fixture(vec!["select"], false);
    let used = HashSet::from([time_key(112.0)]);
    let (frame, _) = select_target(&mut source, &model, &control, 100.0, "目标", &used)
        .await
        .unwrap();
    assert_ne!(frame.unwrap().seconds, 112.0);
    assert!(!source.attempted.contains(&112.0));
}

#[test]
/// 上下文按字符截取，清除标记不会改写正文或增加时间轴。
fn legacy_context_and_marker_cleanup() {
    let body = "汉".repeat(350);
    let markdown = format!("{body}[[shot:01:40]]后文");
    assert_eq!(
        marker_context(&markdown, "[[shot:01:40]]").chars().count(),
        300
    );
    let cleaned = video_note::remove_skipped_shots(&markdown);
    assert!(cleaned.starts_with(&format!("{body}后文")));
    assert!(!cleaned.contains("[[shot:"));
    assert!(cleaned.contains("截图提示"));
}
