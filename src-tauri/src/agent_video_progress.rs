//! Agent 视频阶段文案只使用数值与流水线步骤，禁止透传子进程输出。
use crate::video::models::VideoTaskStatus;

/// 音频处理量与平均倍速来自整数百分比和元数据时长，因此明确标为估算。
pub(super) fn summary(status: &VideoTaskStatus) -> String {
    let Some(asr) = &status.asr_progress else {
        return status.step.clone();
    };
    let Some(percent) = asr.percent else {
        return format!("Whisper 本地转写 P{} · 等待组件报告进度", asr.page);
    };
    let mut text = format!("Whisper 本地转写 P{} · {}%", asr.page, percent);
    if asr.attempt > 1 {
        text.push_str(" · CPU 重试");
    }
    if let Some(total) = asr
        .total_audio_seconds
        .filter(|value| value.is_finite() && *value > 0.0)
    {
        let processed = total * f64::from(percent) / 100.0;
        text.push_str(&format!(
            " · 音频约 {} / {}",
            clock(processed),
            clock(total)
        ));
        if percent > 0 && asr.elapsed_seconds.is_finite() && asr.elapsed_seconds >= 1.0 {
            text.push_str(&format!(
                " · 平均约 {:.2}×（音频秒/秒）",
                processed / asr.elapsed_seconds
            ));
        }
    }
    text
}

/// 不把小数精度伪装成准确的识别时间戳。
fn clock(seconds: f64) -> String {
    let seconds = seconds.floor() as u64;
    format!(
        "{:02}:{:02}:{:02}",
        seconds / 3600,
        seconds / 60 % 60,
        seconds % 60
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::video::models::AsrProgress;

    /// 未知回调不能借总体进度制造转录百分比，只有真实回调才显示估算量。
    #[test]
    fn only_observed_percent_produces_metrics() {
        let mut status = VideoTaskStatus {
            progress: 65,
            asr_progress: Some(AsrProgress {
                page: 2,
                attempt: 1,
                percent: None,
                total_audio_seconds: Some(120.0),
                elapsed_seconds: 30.0,
            }),
            ..Default::default()
        };
        assert!(!summary(&status).contains("%"));
        status.asr_progress.as_mut().unwrap().percent = Some(50);
        let text = summary(&status);
        assert!(text.contains("50%"));
        assert!(text.contains("00:01:00 / 00:02:00"));
        assert!(text.contains("平均约 2.00×"));
        assert!(!text.contains("剩余"));
    }
}
