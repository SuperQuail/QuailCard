//! 分 P 选择及合并时间轴。
use super::*;

/// 视频缓存键：BV 优先，裸 av 输入退化为 av{aid}。
pub(crate) fn video_key(video: &VideoRef, aid: u64) -> String {
    if video.bvid.is_empty() {
        format!("av{aid}")
    } else {
        video.bvid.clone()
    }
}

/// 按 P 号升序选择；空选择采用链接 p 或 P1，非法选择不静默换成其他视频。
pub(crate) fn selected_pages(
    info: &media::VideoInfo,
    input: &PipelineInput,
) -> Vec<media::PageInfo> {
    let mut pages: Vec<media::PageInfo> = info
        .pages
        .iter()
        .filter(|page| {
            (input.pages.is_empty() && page.page == input.video.page.unwrap_or(1))
                || input.pages.contains(&page.page)
        })
        .cloned()
        .collect();
    pages.sort_by_key(|page| page.page);
    pages
}

/// 分 P 在合并时间轴上的窗口。
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PageWindow {
    pub page: media::PageInfo,
    pub start: f64,
    pub duration: f64,
}

/// 按分 P 顺序累加时长得到全局时间轴窗口，与 merge_pages 的偏移规则保持一致。
pub(crate) fn page_windows(info: &media::VideoInfo, input: &PipelineInput) -> Vec<PageWindow> {
    let mut start = 0.0;
    selected_pages(info, input)
        .into_iter()
        .map(|page| {
            let duration = page.duration.max(0.0);
            let window = PageWindow {
                page,
                start,
                duration,
            };
            start += duration;
            window
        })
        .collect()
}

/// 把全局秒定位到所属分 P 与其内部秒数。
pub(super) fn locate(windows: &[PageWindow], seconds: f64) -> Option<(usize, f64)> {
    if !seconds.is_finite() || seconds < 0.0 {
        return None;
    }
    let index = windows
        .iter()
        .rposition(|window| seconds >= window.start)
        .unwrap_or(0);
    let window = windows.get(index)?;
    let local = seconds - window.start;
    (local >= 0.0 && local < window.duration).then_some((index, local))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::video::bilibili::media::{PageInfo, VideoInfo};

    /// 构造三段的测试视频：P1=60s、P2=120s、P3=30s。
    fn info() -> VideoInfo {
        VideoInfo {
            aid: 42,
            bvid: "BV1Qwby6DEu1".to_string(),
            title: "测试视频".to_string(),
            owner: "UP".to_string(),
            duration: 210.0,
            pages: vec![
                PageInfo {
                    page: 1,
                    title: "开场".to_string(),
                    cid: 11,
                    duration: 60.0,
                },
                PageInfo {
                    page: 2,
                    title: "正文".to_string(),
                    cid: 22,
                    duration: 120.0,
                },
                PageInfo {
                    page: 3,
                    title: "结尾".to_string(),
                    cid: 33,
                    duration: 30.0,
                },
            ],
        }
    }

    /// 构造任务输入。
    fn input(pages: Vec<u32>) -> PipelineInput {
        PipelineInput {
            video: VideoRef {
                bvid: "BV1Qwby6DEu1".to_string(),
                aid: None,
                page: None,
                source_url: "https://www.bilibili.com/video/BV1Qwby6DEu1".to_string(),
                cache_key: "bilibili:BV1Qwby6DEu1:p1".to_string(),
            },
            pages,
            quality: None,
            screenshots: true,
            note: true,
            mode: crate::video::models::VideoOutputMode::Note,
            force_transcribe: false,
        }
    }

    #[test]
    /// 空选择回退到 P1，且只覆盖第一段。
    fn empty_selection_falls_back_to_first_page() {
        let windows = page_windows(&info(), &input(Vec::new()));
        assert_eq!(windows.len(), 1);
        assert_eq!(windows[0].page.page, 1);
        assert_eq!(windows[0].start, 0.0);
    }

    #[test]
    /// 多分 P 按选中顺序累加时长，跳过未选中的分 P。
    fn windows_follow_selected_durations() {
        let windows = page_windows(&info(), &input(vec![1, 3]));
        assert_eq!(windows.len(), 2);
        assert_eq!(windows[1].page.page, 3);
        assert_eq!(windows[1].start, 60.0);
        assert_eq!(windows[1].duration, 30.0);
    }

    #[test]
    /// 全局秒映射到所属分 P 与该 P 内秒数，跨 P 边界不串档。
    fn locate_maps_global_seconds() {
        let windows = page_windows(&info(), &input(vec![1, 2]));
        assert_eq!(locate(&windows, 0.0), Some((0, 0.0)));
        assert_eq!(locate(&windows, 59.5), Some((0, 59.5)));
        assert_eq!(locate(&windows, 60.0), Some((1, 0.0)));
        assert_eq!(locate(&windows, 179.0), Some((1, 119.0)));
        // 超出总时长的模型标记必须跳过，不能向抽帧器传递越界时间。
        assert_eq!(locate(&windows, 999.0), None);
    }

    #[test]
    /// URL 指定 P2 时空选择尊重链接，元数据只累计所选时长。
    fn url_page_and_invalid_selection() {
        let mut request = input(Vec::new());
        request.video.page = Some(2);
        let windows = page_windows(&info(), &request);
        assert_eq!(windows[0].page.page, 2);
        assert_eq!(windows.iter().map(|w| w.duration).sum::<f64>(), 120.0);
        request.pages = vec![99];
        assert!(selected_pages(&info(), &request).is_empty());
        let mut empty = info();
        empty.pages.clear();
        assert!(selected_pages(&empty, &request).is_empty());
    }

    #[test]
    /// 裸 av 输入使用 av{aid} 作为缓存键。
    fn video_key_prefers_bvid() {
        let mut video = input(Vec::new()).video;
        assert_eq!(video_key(&video, 42), "BV1Qwby6DEu1");
        video.bvid = String::new();
        assert_eq!(video_key(&video, 42), "av42");
    }
}
