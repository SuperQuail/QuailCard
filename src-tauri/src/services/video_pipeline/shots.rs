//! 每个配图目标独立生成两轮候选，仅保存模型实际确认的原始帧。
use super::*;
use frames::{CandidateFrame, FrameDeps, FrameGrabber};
use std::collections::HashSet;
#[path = "shots_work.rs"]
mod work;

/// 候选端口让两轮调度可独立验证，不依赖网络和实际 ffmpeg。
trait ShotFrames: Send {
    /// 返回锚点所在分 P 的半开时间窗口。
    fn bounds(&self, at: f64) -> Option<(f64, f64)>;
    /// 生成临时候选，任何失败都不能产生永久附件。
    fn candidate(
        &mut self,
        at: f64,
    ) -> crate::services::agent_ports::AgentFuture<'_, Option<CandidateFrame>>;
    /// 真实媒体在读取之前按本组剩余额度拒绝超限文件。
    fn candidate_limited(
        &mut self,
        at: f64,
        _remaining: usize,
    ) -> crate::services::agent_ports::AgentFuture<'_, Option<CandidateFrame>> {
        self.candidate(at)
    }
}

impl ShotFrames for FrameGrabber<'_> {
    /// 复用已有分 P 定位规则。
    fn bounds(&self, at: f64) -> Option<(f64, f64)> {
        self.search_bounds(at)
    }
    /// 复用同一任务的来源缓存与取消句柄。
    fn candidate(
        &mut self,
        at: f64,
    ) -> crate::services::agent_ports::AgentFuture<'_, Option<CandidateFrame>> {
        Box::pin(self.grab_candidate(at))
    }
    /// 让受限读取发生在分配候选字节之前。
    fn candidate_limited(
        &mut self,
        at: f64,
        remaining: usize,
    ) -> crate::services::agent_ports::AgentFuture<'_, Option<CandidateFrame>> {
        Box::pin(self.grab_candidate_limited(at, remaining))
    }
}

/// 逐目标审查；无视觉直接保留正文，标记必须消失。
pub(super) async fn extract_shots(
    deps: &PipelineDeps<'_>,
    control: &VideoControl,
    info: &media::VideoInfo,
    input: &PipelineInput,
    markdown: &str,
    shots: Vec<video_note::ShotRequest>,
) -> Result<(String, u32), CommandError> {
    if control.is_cancelled() {
        return Err(cancelled());
    }
    let total = shots.len();
    if !deps.supports_vision {
        report(
            control,
            0,
            total,
            "当前供应商未启用视觉，未保存未经确认的配图",
        );
        return Ok((video_note::remove_skipped_shots(markdown), 0));
    }
    resolve_component(
        deps,
        Component::Ffmpeg,
        &deps.settings.ffmpeg_path,
        "媒体组件（ffmpeg）",
    )?;
    let task_id = control.snapshot().task_id;
    let handle = control.clone();
    let cancel: CancelHandle = std::sync::Arc::new(move || handle.is_cancelled());
    let referer = input.video.page_url();
    let frame_deps = FrameDeps {
        client: &deps.client,
        frames: deps.tools.frames,
        notes: deps.notes,
        settings: &deps.settings,
        vault_root: &deps.vault_root,
        task_id: &task_id,
        referer: &referer,
        stream_only: false,
        budget: Some(deps.budget),
        control: Some(control.clone()),
    };
    let grabber = tokio::select! { biased;
        _ = control.cancelled() => return Err(cancelled()),
        result = FrameGrabber::start(frame_deps, info, input, cancel) => result?,
    };
    // 每个 future 自持独立抽帧器，共享来源缓存；图组额度覆盖原图和模型编码。
    let jobs = shots.into_iter().map(|shot| {
        let mut source = grabber.clone();
        async move {
            let _images = deps.budget.images(control).await?;
            let target = if shot.target.trim().is_empty() {
                marker_context(markdown, &shot.marker)
            } else {
                shot.target.clone()
            };
            let (selected, reason) = select_target(
                &mut source,
                deps.model,
                control,
                shot.seconds,
                &target,
                &HashSet::new(),
            )
            .await?;
            let selected = selected.map(FrameGrabber::selected);
            Ok((shot, selected, reason))
        }
    });
    let results =
        work::collect_targets(jobs, total, control, |(_, selected, _)| selected.is_some()).await?;
    let mut output = markdown.to_string();
    let mut count = 0;
    let mut used = HashSet::new();
    let mut reasons = Vec::new();
    for (shot, selected, reason) in results {
        if control.is_cancelled() {
            return Err(cancelled());
        }
        let mut replacement = String::new();
        if let Some(frame) = selected {
            // 原目标顺序决定冲突归属；即使较早目标保存失败也不追加模型调用。
            if !claim_frame(&mut used, frame.seconds) {
                reasons.push("与较早目标重复");
            } else {
                let _images = deps.budget.images(control).await?;
                match grabber.persist_selected(frame) {
                    Ok(saved) => {
                        replacement = format!("![配图]({})", saved.markdown_path);
                        count += 1;
                    }
                    Err(error) if error.code == "VIDEO_CANCELLED" => return Err(error),
                    Err(error) => {
                        eprintln!("VIDEO_SHOT_SAVE_SKIPPED(detail): {error}");
                        reasons.push("附件保存失败或候选变化");
                    }
                }
            }
        } else {
            reasons.push(reason);
        }
        output = output.replace(&shot.marker, &replacement);
    }
    if control.is_cancelled() {
        return Err(cancelled());
    }
    reasons.sort_unstable();
    reasons.dedup();
    report(control, count, total - count as usize, &reasons.join("、"));
    Ok((video_note::remove_skipped_shots(&output), count))
}

/// 首轮即使全部取帧失败也继续扩展；返回的帧就是模型看到的字节，不再重截。
async fn select_target(
    source: &mut dyn ShotFrames,
    model: &dyn AgentModel,
    control: &VideoControl,
    at: f64,
    target: &str,
    used: &HashSet<u64>,
) -> Result<(Option<CandidateFrame>, &'static str), CommandError> {
    let Some(bounds) = source.bounds(at) else {
        return Ok((None, "时间点越界"));
    };
    let mut attempted = used.clone();
    let mut reason = "没有匹配且清晰的候选";
    let mut next_id = 1;
    for radius in [12.0, 30.0] {
        if control.is_cancelled() {
            return Err(cancelled());
        }
        let times = candidate_times(at, bounds, radius, &attempted);
        let mut candidates = Vec::new();
        let mut loaded = 0;
        for seconds in times {
            attempted.insert(time_key(seconds));
            let id = next_id;
            next_id += 1;
            match source
                .candidate_limited(seconds, review::MAX_GROUP_BYTES - loaded)
                .await
            {
                Ok(Some(frame))
                    if !frame.bytes.is_empty()
                        && frame.bytes.len() <= review::MAX_IMAGE_BYTES
                        && frame.bytes.len() <= review::MAX_GROUP_BYTES - loaded =>
                {
                    loaded += frame.bytes.len();
                    candidates.push((id, frame));
                }
                Ok(_) => {
                    reason = "候选取帧失败或没有新候选";
                }
                Err(error) if error.code == "VIDEO_CANCELLED" => return Err(error),
                Err(error) => {
                    eprintln!("VIDEO_SHOT_CANDIDATE_SKIPPED(detail): {error}");
                    reason = "候选取帧失败";
                }
            }
            if control.is_cancelled() {
                return Err(cancelled());
            }
        }
        if candidates.is_empty() {
            continue;
        }
        let input: Vec<_> = candidates
            .iter()
            .map(|(id, frame)| review::Candidate {
                id: *id,
                bytes: &frame.bytes,
            })
            .collect();
        let selection = tokio::select! { biased;
            _ = control.cancelled() => return Err(cancelled()),
            result = review::judge(model, target, &input) => result,
        };
        match selection {
            Ok(Some(id)) => {
                let frame = candidates
                    .into_iter()
                    .find(|(candidate, _)| *candidate == id)
                    .map(|(_, frame)| frame);
                return Ok((frame, "已确认匹配且清晰"));
            }
            Ok(None) => {
                reason = "没有匹配且清晰的候选";
            }
            Err(error) if error.code == "VIDEO_CANCELLED" => return Err(error),
            Err(error) => {
                eprintln!("VIDEO_SHOT_REVIEW_SKIPPED(detail): {error}");
                reason = if error.code == "VIDEO_SHOT_REVIEW_INVALID" {
                    "复查解析无效"
                } else {
                    "复查请求失败"
                };
            }
        }
    }
    Ok((None, reason))
}

/// 裁剪至锚点所属分 P 后均匀取五点；毫秒去重覆盖边界和上一轮失败的尝试。
fn candidate_times(at: f64, bounds: (f64, f64), radius: f64, attempted: &HashSet<u64>) -> Vec<f64> {
    let (start, end) = bounds;
    if !at.is_finite()
        || !start.is_finite()
        || !end.is_finite()
        || at < start
        || at >= end
        || end <= start
    {
        return Vec::new();
    }
    let lo = (at - radius).max(start);
    let hi = (at + radius).min((end - 0.001).max(start));
    let mut seen = attempted.clone();
    (0..5)
        .map(|index| lo + (hi - lo) * index as f64 / 4.0)
        .filter(|seconds| seen.insert(time_key(*seconds)))
        .collect()
}

/// 顺序提交调用此函数，较早目标永久占有冲突身份，不触发补充筛选。
fn claim_frame(used: &mut HashSet<u64>, seconds: f64) -> bool {
    used.insert(time_key(seconds))
}

/// 毫秒身份同时用于候选去重和最终不同目标之间的精确时间去重。
fn time_key(seconds: f64) -> u64 {
    (seconds * 1000.0).round() as u64
}

/// 状态与控制台使用同一明确计数，不把跳过配图误报为笔记失败。
fn report(control: &VideoControl, selected: u32, skipped: usize, reason: &str) {
    let message = format!(
        "配图已选择 {selected} 张，跳过 {skipped} 个目标；正文保留{}{}",
        if reason.is_empty() { "" } else { "；" },
        reason
    );
    eprintln!("VIDEO_SHOT_SELECTION: {message}");
    control.update(|status| {
        status.shots = selected;
        status.message = message;
    });
}

/// 旧标记没有目标时取前方最多三百个 Unicode 字符，不改变正文内容。
fn marker_context(markdown: &str, marker: &str) -> String {
    let Some(position) = markdown.find(marker) else {
        return String::new();
    };
    let tail: String = markdown[..position].chars().rev().take(300).collect();
    tail.chars()
        .rev()
        .collect::<String>()
        .trim()
        .replace('\n', " ")
}

#[cfg(test)]
#[path = "shots_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "no_vision_tests.rs"]
mod no_vision_tests;
