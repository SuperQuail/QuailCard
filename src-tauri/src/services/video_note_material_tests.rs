//! 并发材料准备的回归测试：并发峰值、乱序结果与层间依赖。

use super::*;
use crate::{
    ai::ToolDefinition,
    services::agent_ports::{AgentFuture, AgentModelReply},
    video::transcript::Segment,
};
use serde_json::Value;

/// 假模型记录真实并发峰值，并按提示词中的分块编号回复，便于验证结果顺序。
struct CountingModel {
    current: AtomicUsize,
    peak: AtomicUsize,
    calls: AtomicUsize,
}

impl AgentModel for CountingModel {
    /// 分块越快完成，编号越大；若结果按完成顺序拼接，顺序断言会失败。
    fn call<'a>(
        &'a self,
        _: &'a str,
        messages: &'a [Value],
        _: &'a [ToolDefinition],
        _: &'a (dyn Fn(&str) + Send + Sync),
        _: &'a (dyn Fn(&str) + Send + Sync),
    ) -> AgentFuture<'a, AgentModelReply> {
        Box::pin(async move {
            self.calls.fetch_add(1, Ordering::SeqCst);
            let now = self.current.fetch_add(1, Ordering::SeqCst) + 1;
            self.peak.fetch_max(now, Ordering::SeqCst);
            let prompt = messages[0]["content"].as_str().unwrap_or_default();
            let index: usize = prompt
                .split("这是第 ")
                .nth(1)
                .and_then(|rest| rest.split('/').next())
                .and_then(|text| text.trim().parse().ok())
                .unwrap_or(1);
            // 编号越大等待越少，制造与编号相反的完成顺序。
            for _ in 0..(40usize.saturating_sub(index)) {
                tokio::task::yield_now().await;
            }
            self.current.fetch_sub(1, Ordering::SeqCst);
            Ok(AgentModelReply {
                text: format!("片段{index}"),
                ..Default::default()
            })
        })
    }
}

/// 构造超过分块阈值的转录，保证至少产生 WORK_LIMIT 个分块。
fn transcript() -> Transcript {
    Transcript {
        language: "zh".into(),
        source: "whisper".into(),
        segments: (0..40)
            .map(|index| Segment {
                start: index as f64 * 5.0,
                end: index as f64 * 5.0 + 5.0,
                text: "字".repeat(1000),
            })
            .collect(),
    }
}

fn meta() -> NoteMeta<'static> {
    NoteMeta {
        title: "测试视频",
        owner: "UP",
        duration: 200.0,
        source_url: "https://www.bilibili.com/video/BV1",
        model_label: "demo",
        transcript_source: "whisper",
        max_shots: 0,
    }
}

#[tokio::test]
/// 分块确实并发推进，峰值不超过上限；乱序完成仍按编号拼接材料。
async fn chunk_stage_runs_concurrently_and_keeps_order() {
    let model = CountingModel {
        current: AtomicUsize::new(0),
        peak: AtomicUsize::new(0),
        calls: AtomicUsize::new(0),
    };
    let log = |_: &str| {};
    let progress = |_: u8| {};
    let material = prepare(&model, &transcript(), &meta(), &progress, &log)
        .await
        .unwrap();
    let peak = model.peak.load(Ordering::SeqCst);
    assert!(peak > 1, "分块没有并发推进：{peak}");
    assert!(peak <= video_work::WORK_LIMIT, "并发超过上限：{peak}");
    let calls = model.calls.load(Ordering::SeqCst);
    assert!(calls >= video_work::WORK_LIMIT, "分块数不足：{calls}");
    // 按标题切片还原每个片段的实际内容，最后一个片段没有尾随换行也能检查。
    let order: Vec<String> = material
        .split("### 片段 ")
        .skip(1)
        .map(|section| {
            section
                .lines()
                .nth(1)
                .unwrap_or_default()
                .trim()
                .to_string()
        })
        .collect();
    assert_eq!(
        order,
        (1..=calls)
            .map(|index| format!("片段{index}"))
            .collect::<Vec<_>>(),
        "材料顺序与分块编号不一致"
    );
}

/// 记录阶段先后顺序的假模型：分块输出足够长，强制进入归并层。
struct StageOrderModel {
    events: std::sync::Mutex<Vec<String>>,
}

impl AgentModel for StageOrderModel {
    /// 每次调用登记阶段开始与结束，用事件顺序验证层间依赖。
    fn call<'a>(
        &'a self,
        _: &'a str,
        messages: &'a [Value],
        _: &'a [ToolDefinition],
        _: &'a (dyn Fn(&str) + Send + Sync),
        _: &'a (dyn Fn(&str) + Send + Sync),
    ) -> AgentFuture<'a, AgentModelReply> {
        Box::pin(async move {
            let prompt = messages[0]["content"].as_str().unwrap_or_default();
            let chunk = prompt.contains("个连续片段");
            let order = prompt
                .split("这是第 ")
                .nth(1)
                .and_then(|rest| rest.split_whitespace().next())
                .unwrap_or("?")
                .to_string();
            let label = format!("{}{order}", if chunk { "chunk" } else { "merge" });
            self.events
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .push(format!("start:{label}"));
            for _ in 0..5 {
                tokio::task::yield_now().await;
            }
            self.events
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .push(format!("end:{label}"));
            let text = if chunk {
                "字".repeat(5000)
            } else {
                "摘要".to_string()
            };
            Ok(AgentModelReply {
                text,
                ..Default::default()
            })
        })
    }
}

#[tokio::test]
/// 分块必须全部完成才进入归并层；本层完成前不启动下一层。
async fn merge_layer_waits_for_every_chunk() {
    let model = StageOrderModel {
        events: std::sync::Mutex::new(Vec::new()),
    };
    let log = |_: &str| {};
    let progress = |_: u8| {};
    let material = prepare(&model, &transcript(), &meta(), &progress, &log)
        .await
        .unwrap();
    let events = model
        .events
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .clone();
    let last_chunk_end = events
        .iter()
        .rposition(|event| event.starts_with("end:chunk"))
        .expect("分块阶段必须执行");
    let first_merge_start = events
        .iter()
        .position(|event| event.starts_with("start:merge"))
        .expect("分块超预算后必须进入归并层");
    assert!(
        last_chunk_end < first_merge_start,
        "归并在分块完成前启动：{events:?}"
    );
    // 分块全部结束后才出现归并，且归并结果进入最终材料。
    assert!(events.len() > first_merge_start + 1);
    assert!(material.contains("### 片段 1"));
    assert!(!material.contains("字字字字"));
}

#[test]
/// 进度按完成数量落在阶段区间内，处理超过 255 项也不溢出。
fn stage_progress_stays_inside_range() {
    let mut last = 0;
    for finished in 0..=1024 {
        let value = stage_progress((70, 79), finished, 1024);
        assert!(value >= last && value <= 79, "进度回退或越界：{value}");
        last = value;
    }
    assert_eq!(stage_progress((80, 83), 0, 0), 80);
    assert_eq!(stage_progress((80, 83), 9, 9), 83);
}

#[test]
/// 归并分组仍按预算贪心切分，短笔记不会产生多余组。
fn group_notes_respects_budget() {
    assert_eq!(group_notes(&["短".to_string()]).len(), 1);
    let long = "字".repeat(MERGE_CHARACTERS / 2 + 1);
    let groups = group_notes(&[long.clone(), long.clone()]);
    assert_eq!(groups.len(), 2);
}
