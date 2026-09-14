//! 转录 → Markdown 笔记：分块压缩、分层归并、配图标记与元信息组装。

#[path = "video_note_material.rs"]
mod condense;
#[path = "video_note_call.rs"]
mod request;

use crate::{
    error::CommandError,
    services::agent_ports::AgentModel,
    video::transcript::{format_timestamp, Transcript},
};

/// 笔记元信息，用于生成标题与落款。
pub(crate) struct NoteMeta<'a> {
    pub title: &'a str,
    pub owner: &'a str,
    pub duration: f64,
    pub source_url: &'a str,
    pub model_label: &'a str,
    pub transcript_source: &'a str,
    pub max_shots: u32,
}

/// 配图请求：保留标记原文，便于在正文中精确替换。
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ShotRequest {
    pub seconds: f64,
    pub marker: String,
    /// 需要在画面中真实可见的内容；旧格式留空，由调用方使用正文上下文。
    pub target: String,
}

/// 生成结果。
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct GeneratedNote {
    pub markdown: String,
    pub shots: Vec<ShotRequest>,
}

/// 生成笔记：短转录直接成稿，长转录先压缩再归并。
pub(crate) async fn generate(
    model: &dyn AgentModel,
    transcript: &Transcript,
    meta: &NoteMeta<'_>,
    progress: &(dyn Fn(u8) + Send + Sync),
    log: &(dyn Fn(&str) + Send + Sync),
) -> Result<GeneratedNote, CommandError> {
    log(&format!(
        "笔记生成：segments={} transcript_chars={} duration_seconds={:.0}",
        transcript.segments.len(),
        transcript
            .segments
            .iter()
            .map(|segment| segment.text.chars().count())
            .sum::<usize>(),
        meta.duration
    ));
    let material = condense::prepare(model, transcript, meta, progress, log).await?;
    progress(84);
    let prompt = note_prompt(meta, &material);
    let text = request::call(model, &prompt, "最终成稿", log).await?;
    let body = strip_fence(&text);
    let (body, shots) = extract_shots(&body, meta.max_shots);
    progress(87);
    Ok(GeneratedNote {
        markdown: assemble(meta, &body),
        shots,
    })
}

/// 组装最终提示词：任务、元信息、配图标记与标题约束。
///
/// 笔记模式不写时间轴：内容是重建后的信息结构，时间点只作为配图锚点存在。
fn note_prompt(meta: &NoteMeta<'_>, material: &str) -> String {
    format!(
        "视频标题：{title}\nUP 主：{owner}\n时长：{duration}\n转录来源：{source}\n\n把视频内容整理成一份结构化的 Markdown 学习笔记：先给出主题与结论，再按内容自身的逻辑重建信息——概念与定义、原理与机制、依据与推导、例子与数据、步骤与操作、易错点与限制；同一主题要跨时间归并到一起，不要按讲述顺序逐段复述，正文里不要写时间点或时间轴。保留材料中的具体事实、数字与例子，不加入外部知识或评价。\n\n只在画面能提供文字之外的具体信息时配图，单独输出一行 [[shot:MM:SS|需要展示的具体画面]]，最多 {max_shots} 处，这是上限而非配额。时间点只用于搜索附近候选帧；目标必须具体，例如“材料中所述函数的定义与回退分支（写明函数名）”，不能写“代码截图”或“相关画面”。只依据材料描述目标，不臆造函数名或内容；抽象观点、总结、重复的编辑器界面不配图，同一个画面只配一次。\n\n第一行必须输出标题：# 视频笔记：《{title}》。不要在正文重复 UP 主、时长等元信息，应用会统一补充。\n\n材料：\n{material}",
        title = meta.title,
        owner = if meta.owner.is_empty() { "未知" } else { meta.owner },
        duration = format_timestamp(meta.duration),
        source = meta.transcript_source,
        max_shots = meta.max_shots,
    )
}

/// 去掉整段代码块围栏，避免写入笔记时格式错乱。
fn strip_fence(text: &str) -> String {
    let trimmed = text.trim();
    let fence = '`';
    let triple = format!("{fence}{fence}{fence}");
    if trimmed.starts_with(&triple) && trimmed.ends_with(&triple) && trimmed.len() > 6 {
        let inner = &trimmed[3..trimmed.len() - 3];
        let inner = inner
            .trim_start_matches("markdown")
            .trim_start_matches("md");
        return inner.trim().to_string();
    }
    trimmed.to_string()
}

/// 提取时间与画面目标；有效标记留待选图替换，无效或超额标记直接移除。
fn extract_shots(body: &str, max_shots: u32) -> (String, Vec<ShotRequest>) {
    let mut shots: Vec<ShotRequest> = Vec::new();
    let mut output = String::new();
    let mut rest = body;
    while let Some(start) = rest.find("[[shot:") {
        output.push_str(&rest[..start]);
        let after = &rest[start + 7..];
        let Some(end) = after.find("]]") else {
            output.push_str(&rest[start..]);
            rest = "";
            break;
        };
        let inside = after[..end].trim();
        let marker = format!("[[shot:{inside}]]");
        let (clock, target) = inside.split_once('|').unwrap_or((inside, ""));
        let seconds = parse_clock(clock.trim());
        let target: String = target.trim().chars().take(240).collect();
        match seconds {
            Some(seconds)
                if (shots.len() as u32) < max_shots
                    && !shots
                        .iter()
                        .any(|shot| (shot.seconds - seconds).abs() < 1.0) =>
            {
                shots.push(ShotRequest {
                    seconds,
                    marker: marker.clone(),
                    target,
                });
                output.push_str(&marker);
            }
            _ => {}
        }
        rest = &after[end + 2..];
    }
    output.push_str(rest);
    (output.trim().to_string(), shots)
}

/// 解析 MM:SS 或 HH:MM:SS 时间点。
fn parse_clock(value: &str) -> Option<f64> {
    let parts: Vec<&str> = value.split(':').collect();
    let seconds = match parts.len() {
        3 => {
            parts[0].parse::<f64>().ok()? * 3600.0
                + parts[1].parse::<f64>().ok()? * 60.0
                + parts[2].parse::<f64>().ok()?
        }
        2 => parts[0].parse::<f64>().ok()? * 60.0 + parts[1].parse::<f64>().ok()?,
        _ => return None,
    };
    (seconds.is_finite() && seconds >= 0.0).then_some(seconds)
}

/// 未兑现的截图请求全部移除，并在正文留下对用户可见的说明。
pub(crate) fn remove_skipped_shots(markdown: &str) -> String {
    if !markdown.contains("[[shot:") {
        return markdown.to_string();
    }
    let (mut clean, _) = extract_shots(markdown, 0);
    clean = clean.replace("[[shot:", "");
    clean.push_str("\n\n> 截图提示：部分关键画面未能提取或确认匹配，已保留文字笔记。\n");
    clean
}

/// 字幕模式：按时间轴原样成文，不做模型改写，字幕稿要能逐句对照。
pub(crate) fn transcript_markdown(meta: &NoteMeta<'_>, transcript: &Transcript) -> String {
    let mut content = format!("# 视频字幕：《{}》\n\n", meta.title);
    for segment in &transcript.segments {
        let text = segment.text.trim();
        if text.is_empty() {
            continue;
        }
        content.push_str(&format!(
            "**[{}]** {text}\n\n",
            format_timestamp(segment.start)
        ));
    }
    // 字幕稿没有经过模型改写，落款不写“生成模型”。
    content.push_str(&format!(
        "\n\n> UP主：{} · 时长：{} · 转录：{}\n\n---\n来源：{}\n",
        if meta.owner.is_empty() {
            "未知"
        } else {
            meta.owner
        },
        format_timestamp(meta.duration),
        meta.transcript_source,
        meta.source_url
    ));
    content
}

/// 组装元信息行与来源落款。
fn assemble(meta: &NoteMeta<'_>, body: &str) -> String {
    let mut content = String::new();
    if !body.starts_with("# ") {
        content.push_str(&format!("# 视频笔记：《{}》\n\n", meta.title));
    }
    content.push_str(body);
    content.push_str(&footer(meta));
    content
}

/// 笔记与字幕稿共用同一份元信息落款。
fn footer(meta: &NoteMeta<'_>) -> String {
    format!(
        "\n\n> UP主：{} · 时长：{} · 转录：{}\n\n---\n来源：{}\n生成模型：{}\n",
        if meta.owner.is_empty() {
            "未知"
        } else {
            meta.owner
        },
        format_timestamp(meta.duration),
        meta.transcript_source,
        meta.source_url,
        meta.model_label
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    /// 配图标记被抽离，正文不再包含原始标记。
    fn extracts_shot_markers() {
        let body = "开头 [[shot:01:05]] 中间 [[shot:65|同一位置]] 结尾";
        let (text, shots) = extract_shots(body, 12);
        assert_eq!(shots.len(), 1);
        assert!(text.contains("[[shot:01:05]]"));
        assert!(!text.contains("65|"));
    }

    #[test]
    /// 新标记保留具体目标，旧标记兼容空目标，超长目标按字符截断。
    fn extracts_visual_targets_without_leaking_markers() {
        let body = "正文 [[shot:10:43| 接口定义与回退分支 ]] 旧格式 [[shot:00:16]]";
        let (text, shots) = extract_shots(body, 12);
        assert_eq!(shots.len(), 2);
        assert_eq!(shots[0].target, "接口定义与回退分支");
        assert_eq!(shots[0].seconds, 643.0);
        assert!(shots[1].target.is_empty());
        assert!(text.contains(&shots[0].marker));
        let clean = remove_skipped_shots(&text);
        assert!(!clean.contains("shot:"));
        assert!(!clean.contains("接口定义"));
        let (_, bounded) = extract_shots(&format!("[[shot:00:16|{}]]", "字".repeat(300)), 1);
        assert_eq!(bounded[0].target.chars().count(), 240);
    }

    #[test]
    /// 非法时间点直接删除，不留在笔记里。
    fn drops_invalid_markers() {
        let (text, shots) = extract_shots("前 [[shot:abc]] 后", 12);
        assert!(shots.is_empty());
        assert!(!text.contains("shot"));
    }

    #[test]
    /// 未兑现的截图标记被清除，同时保留正文与可读提示。
    fn missing_shots_are_removed_with_notice() {
        let clean = remove_skipped_shots("正文 [[shot:00:20]] 结尾");
        assert!(!clean.contains("[[shot:"));
        assert!(clean.contains("截图提示"));
        assert!(clean.contains("结尾"));
        assert!(parse_clock("NaN:00").is_none());
    }

    #[test]
    /// 字幕稿保留时间轴，笔记模式提示词明确不要时间轴但保留配图锚点。
    fn transcript_keeps_timeline_and_note_prompt_drops_it() {
        let meta = NoteMeta {
            title: "测试视频",
            owner: "UP",
            duration: 125.0,
            source_url: "https://www.bilibili.com/video/BV1",
            model_label: "demo",
            transcript_source: "whisper",
            max_shots: 3,
        };
        let transcript = Transcript {
            language: "zh".into(),
            source: "whisper".into(),
            segments: vec![
                crate::video::transcript::Segment {
                    start: 0.0,
                    end: 2.0,
                    text: " 第一句 ".into(),
                },
                crate::video::transcript::Segment {
                    start: 65.0,
                    end: 68.0,
                    text: "第二句".into(),
                },
            ],
        };
        let markdown = transcript_markdown(&meta, &transcript);
        assert!(markdown.starts_with("# 视频字幕：《测试视频》"));
        assert!(markdown.contains("**[00:00]** 第一句"));
        assert!(markdown.contains("**[01:05]** 第二句"));
        assert!(markdown.contains("转录：whisper"));
        assert!(!markdown.contains("生成模型"));
        let prompt = note_prompt(&meta, "材料");
        assert!(prompt.contains("不要写时间点或时间轴"));
        assert!(prompt.contains("[[shot:MM:SS|需要展示的具体画面]]"));
        assert!(prompt.contains("这是上限而非配额"));
        assert!(!prompt.contains("[起点-终点]"));
    }

    #[test]
    /// 元信息与落款始终追加，标题缺失时补标题。
    fn assembles_metadata() {
        let meta = NoteMeta {
            title: "测试视频",
            owner: "UP",
            duration: 125.0,
            source_url: "https://www.bilibili.com/video/BV1",
            model_label: "demo",
            transcript_source: "bilibili_ai",
            max_shots: 12,
        };
        let content = assemble(&meta, "正文");
        assert!(content.starts_with("# 视频笔记：《测试视频》"));
        assert!(content.contains("> UP主：UP · 时长：02:05 · 转录：bilibili_ai"));
        assert!(content.contains("来源：https://www.bilibili.com/video/BV1"));
    }
}
