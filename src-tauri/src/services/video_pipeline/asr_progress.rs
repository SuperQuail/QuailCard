//! 每次 Whisper 尝试独立计时；GPU 回退不沿用失败尝试的完成量或速度。
use std::{sync::Arc, time::Instant};

use crate::{
    services::video_tasks::VideoControl,
    video::{media::ProgressHandle, models::AsrProgress},
};

/// 回调只消费真实组件百分比；未知总时长保持空，不伪造音频完成量。
pub(super) fn observe(
    control: &VideoControl,
    page: u32,
    duration: f64,
    attempt: u32,
    base: f64,
    span: f64,
) -> ProgressHandle {
    let initial = AsrProgress {
        page,
        attempt,
        percent: None,
        total_audio_seconds: (duration.is_finite() && duration > 0.0).then_some(duration),
        elapsed_seconds: 0.0,
    };
    control.update(|status| {
        status.asr_progress = Some(initial);
        status.backend.clear();
        status.step = format!("本地转写 P{page}");
        status.message = if attempt > 1 {
            "GPU 失败，正在使用 CPU 重新转写"
        } else {
            "正在加载语音模型，等待 Whisper 进度"
        }
        .to_string();
    });
    let started = Instant::now();
    let control = control.clone();
    Arc::new(move |percent| {
        if percent > 100 {
            return;
        }
        control.update(|status| {
            if let Some(observed) = status.asr_progress.as_mut() {
                // 丢弃迟到尝试和回退百分比，防止完成量与计时错配。
                if observed.page != page
                    || observed.attempt != attempt
                    || observed.percent.is_some_and(|previous| percent < previous)
                {
                    return;
                }
                observed.percent = Some(percent);
                observed.elapsed_seconds = started.elapsed().as_secs_f64();
                status.progress = (base + span * f64::from(percent) / 100.0) as u8;
            }
        });
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{services::video_tasks::VideoTaskRegistry, storage::video::VideoTaskRecord};

    /// CPU 重试和分 P 切换必须清空旧完成量；未知时长不产生速度依据。
    #[test]
    fn resets_attempt_and_rejects_stale_progress() {
        let registry = VideoTaskRegistry::default();
        let control = registry
            .register("owner", &VideoTaskRecord::new("task", "key", "url"))
            .unwrap();
        let first = observe(&control, 1, 120.0, 1, 25.0, 40.0);
        first(42);
        first(12);
        assert_eq!(control.snapshot().asr_progress.unwrap().percent, Some(42));
        let second = observe(&control, 1, 120.0, 2, 25.0, 40.0);
        first(90);
        assert_eq!(control.snapshot().asr_progress.unwrap().percent, None);
        second(10);
        assert_eq!(control.snapshot().asr_progress.unwrap().percent, Some(10));
        let next = observe(&control, 2, f64::NAN, 1, 65.0, 0.0);
        next(200);
        let state = control.snapshot().asr_progress.unwrap();
        assert_eq!(state.percent, None);
        assert_eq!(state.total_audio_seconds, None);
    }
}
