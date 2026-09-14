//! 带时间轴的转录模型：解析、格式化、质量评估与长文分块。

use crate::error::CommandError;

/// 单条带时间轴的转录片段，时间单位为秒。
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub(crate) struct Segment {
    pub start: f64,
    pub end: f64,
    pub text: String,
}

/// 一次取字的完整结果。
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct Transcript {
    pub language: String,
    /// 来源标记：bilibili_cc / bilibili_ai / whisper。
    pub source: String,
    pub segments: Vec<Segment>,
}

impl Transcript {
    /// 正常文本字符数（不含空白），用于质量护栏与长度提示。
    pub(crate) fn characters(&self) -> usize {
        self.segments
            .iter()
            .map(|segment| segment.text.chars().count())
            .sum()
    }
}

/// 转录质量护栏结果。
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct TranscriptQuality {
    pub characters: usize,
    pub speech_seconds: f64,
    pub coverage: f64,
    pub insufficient: bool,
}

/// 评估转录是否值得生成笔记：长时间视频却几乎没有语音时判为无效。
pub(crate) fn quality(transcript: &Transcript, duration: f64) -> TranscriptQuality {
    let characters = transcript.characters();
    let speech_seconds: f64 = transcript
        .segments
        .iter()
        .map(|segment| (segment.end - segment.start).max(0.0))
        .sum();
    let coverage = if duration > 0.0 {
        (speech_seconds / duration).min(1.0)
    } else {
        0.0
    };
    let per_second = if duration > 0.0 {
        characters as f64 / duration
    } else {
        0.0
    };
    let insufficient = duration >= 120.0 && coverage < 0.25 && per_second < 1.0;
    TranscriptQuality {
        characters,
        speech_seconds,
        coverage,
        insufficient,
    }
}

/// 解析 B 站字幕 JSON：body 数组中的 from/to/content。
pub(crate) fn parse_subtitle_json(payload: &str) -> Result<Vec<Segment>, CommandError> {
    let value: serde_json::Value = serde_json::from_str(payload).map_err(|error| {
        eprintln!("VIDEO_SUBTITLE_PARSE(detail): {error}");
        CommandError::new("VIDEO_SUBTITLE_INVALID", "字幕内容无法解析")
    })?;
    let body = value
        .get("body")
        .and_then(|body| body.as_array())
        .ok_or_else(|| CommandError::new("VIDEO_SUBTITLE_INVALID", "字幕内容为空"))?;
    let mut segments = Vec::new();
    for item in body {
        let text = normalize_text(
            item.get("content")
                .and_then(|value| value.as_str())
                .unwrap_or(""),
        );
        if text.is_empty() {
            continue;
        }
        let start = item
            .get("from")
            .and_then(|value| value.as_f64())
            .unwrap_or(0.0)
            .max(0.0);
        let end = item
            .get("to")
            .and_then(|value| value.as_f64())
            .unwrap_or(start)
            .max(start);
        segments.push(Segment { start, end, text });
    }
    Ok(segments)
}

/// 合并多分 P：时间轴按分 P 时长累加，多 P 时在每段首句标注 P 号与标题。
pub(crate) fn merge_pages(pages: &[(u32, String, f64, Vec<Segment>)]) -> Vec<Segment> {
    let multiple = pages.len() > 1;
    let mut merged = Vec::new();
    let mut offset = 0.0;
    for (page, title, duration, segments) in pages {
        for (index, segment) in segments.iter().enumerate() {
            let mut text = segment.text.clone();
            if multiple && index == 0 {
                text = format!("【P{page} {title}】{text}");
            }
            merged.push(Segment {
                start: offset + segment.start,
                end: offset + segment.end,
                text,
            });
        }
        offset += duration.max(0.0);
    }
    merged
}

/// 时间戳：分钟以内 MM:SS，超过一小时 HH:MM:SS。
pub(crate) fn format_timestamp(seconds: f64) -> String {
    let total = seconds.max(0.0) as u64;
    let (hours, minutes, secs) = (total / 3600, (total % 3600) / 60, total % 60);
    if hours > 0 {
        format!("{hours:02}:{minutes:02}:{secs:02}")
    } else {
        format!("{minutes:02}:{secs:02}")
    }
}

/// 生成提示词用的一行行转录，形如 [MM:SS-MM:SS] 正文。
pub(crate) fn segments_to_prompt(segments: &[Segment]) -> String {
    let mut text = String::new();
    for segment in segments {
        text.push_str(&format!(
            "[{}-{}] {}\n",
            format_timestamp(segment.start),
            format_timestamp(segment.end),
            segment.text
        ));
    }
    text
}

/// 长转录分块：按字符预算贪心切分，保证每块不超过上限。
pub(crate) fn chunk_segments(segments: &[Segment], max_characters: usize) -> Vec<Vec<Segment>> {
    let budget = max_characters.max(500);
    let mut chunks: Vec<Vec<Segment>> = Vec::new();
    let mut current: Vec<Segment> = Vec::new();
    let mut used = 0;
    for segment in segments {
        let cost = segment.text.chars().count() + 32;
        if !current.is_empty() && used + cost > budget {
            chunks.push(std::mem::take(&mut current));
            used = 0;
        }
        used += cost;
        current.push(segment.clone());
    }
    if !current.is_empty() {
        chunks.push(current);
    }
    chunks
}

/// 规范化字幕文本：去掉 HTML 标签、还原常见实体并压缩空白。
fn normalize_text(text: &str) -> String {
    let mut stripped = String::new();
    let mut in_tag = false;
    for character in text.chars() {
        match character {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => stripped.push(character),
            _ => {}
        }
    }
    let decoded = stripped
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&nbsp;", " ");
    decoded.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 构造一条测试片段。
    fn segment(start: f64, end: f64, text: &str) -> Segment {
        Segment {
            start,
            end,
            text: text.to_string(),
        }
    }

    #[test]
    /// 时间戳在一小时前后分别使用两种格式。
    fn formats_timestamps() {
        assert_eq!(format_timestamp(65.0), "01:05");
        assert_eq!(format_timestamp(3725.0), "01:02:05");
        assert_eq!(format_timestamp(-3.0), "00:00");
    }

    #[test]
    /// 字幕 JSON 解析去标签、压缩空白并沿用起止时间。
    fn parses_subtitle_json() {
        let payload = r#"{"body":[{"from":0,"to":2.5,"content":" 你好  <b>世界</b> "},{"from":3,"content":""}]}"#;
        let segments = parse_subtitle_json(payload).unwrap();
        assert_eq!(segments.len(), 1);
        assert_eq!(segments[0].text, "你好 世界");
        assert_eq!(segments[0].end, 2.5);
        assert!(parse_subtitle_json("{}").is_err());
    }

    #[test]
    /// 多 P 合并按分 P 时长平移并在首句标注 P 号。
    fn merges_pages_with_offset() {
        let pages = vec![
            (
                1,
                "开场".to_string(),
                60.0,
                vec![segment(0.0, 5.0, "第一段")],
            ),
            (
                2,
                "正文".to_string(),
                120.0,
                vec![segment(0.0, 4.0, "第二段")],
            ),
        ];
        let merged = merge_pages(&pages);
        assert_eq!(merged[0].start, 0.0);
        assert_eq!(merged[1].start, 60.0);
        assert_eq!(merged[0].text, "【P1 开场】第一段");
        assert_eq!(merged[1].text, "【P2 正文】第二段");
    }

    #[test]
    /// 单 P 不加标记，避免污染正文。
    fn single_page_has_no_marker() {
        let pages = vec![(
            1,
            "开场".to_string(),
            60.0,
            vec![segment(0.0, 5.0, "第一段")],
        )];
        assert_eq!(merge_pages(&pages)[0].text, "第一段");
    }

    #[test]
    /// 分块不超预算且不丢片段。
    fn chunks_within_budget() {
        let segments: Vec<Segment> = (0..60)
            .map(|index| segment(index as f64, index as f64 + 1.0, &"字".repeat(50)))
            .collect();
        let chunks = chunk_segments(&segments, 600);
        assert!(chunks.len() > 1);
        for chunk in &chunks {
            let cost: usize = chunk
                .iter()
                .map(|segment| segment.text.chars().count() + 32)
                .sum();
            assert!(cost <= 600);
        }
        assert_eq!(chunks.iter().map(Vec::len).sum::<usize>(), segments.len());
    }

    #[test]
    /// 长时间视频几乎没有语音时判为无效。
    fn flags_insufficient_quality() {
        let transcript = Transcript {
            language: "zh".into(),
            source: "whisper".into(),
            segments: vec![segment(0.0, 2.0, "嗯")],
        };
        assert!(quality(&transcript, 600.0).insufficient);
        let rich = Transcript {
            language: "zh".into(),
            source: "whisper".into(),
            segments: vec![segment(0.0, 400.0, &"内容".repeat(600))],
        };
        assert!(!quality(&rich, 600.0).insufficient);
    }
}
