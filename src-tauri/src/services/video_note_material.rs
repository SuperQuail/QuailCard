//! 长转录的并发材料准备：分块压缩与同层归并都受统一并发上限约束。
//!
//! 并发只发生在同一层的独立请求之间；本层全部成功后才会形成下一层输入，
//! 因此结果始终按分块编号拼接，完成先后不影响成稿材料顺序。
use std::sync::atomic::{AtomicUsize, Ordering};

use crate::{
    error::CommandError,
    services::{agent_ports::AgentModel, video_work},
    video::transcript::{chunk_segments, segments_to_prompt, Transcript},
};

use super::{request, NoteMeta};

/// 单次输入上限；超过则先分块压缩。
const CHUNK_CHARACTERS: usize = 9000;
/// 归并阶段允许的总字符数。
const MERGE_CHARACTERS: usize = 14000;
/// 单块压缩的最大输出长度提示。
const CONDENSE_HINT: &str =
    "请忠实压缩这个片段，保留重要事实、例子、论证关系与原有时间点；不做评价，不补充外部知识。";

/// 准备最终输入材料；长转录先并发压缩分块，再按层并发归并。
pub(super) async fn prepare(
    model: &dyn AgentModel,
    transcript: &Transcript,
    meta: &NoteMeta<'_>,
    progress: &(dyn Fn(u8) + Send + Sync),
    log: &(dyn Fn(&str) + Send + Sync),
) -> Result<String, CommandError> {
    let plain = segments_to_prompt(&transcript.segments);
    if plain.chars().count() <= CHUNK_CHARACTERS {
        progress(78);
        return Ok(plain);
    }
    let chunks = chunk_segments(&transcript.segments, CHUNK_CHARACTERS);
    let total = chunks.len().max(1);
    let prompts: Vec<String> = chunks
        .iter()
        .enumerate()
        .map(|(index, chunk)| {
            format!(
                "视频标题：{}\n这是第 {}/{} 个连续片段。\n\n{CONDENSE_HINT}\n\n转录：\n{}",
                meta.title,
                index + 1,
                total,
                segments_to_prompt(chunk)
            )
        })
        .collect();
    let phases: Vec<String> = (0..prompts.len())
        .map(|index| format!("分块压缩 {}/{}", index + 1, total))
        .collect();
    let stage = Stage {
        name: "整理分块",
        total: prompts.len(),
        progress,
        log,
        range: (70, 79),
    };
    let mut notes = run(
        &stage,
        prompts
            .iter()
            .zip(phases.iter())
            .map(|(prompt, phase)| request::call(model, prompt, phase, log)),
    )
    .await?;

    let mut level = 1;
    while notes.iter().map(|note| note.chars().count()).sum::<usize>() > MERGE_CHARACTERS
        && notes.len() > 1
    {
        let groups = group_notes(&notes);
        let count = groups.len().max(1);
        let prompts: Vec<String> = groups
            .iter()
            .enumerate()
            .map(|(index, group)| {
                format!(
                    "视频标题：{}\n这是第 {level} 层归并，第 {}/{} 组。\n\n请合并以下连续片段笔记，去除重复但保留观点、依据、例子、时间点与前后关系。只输出供最终成稿使用的连续材料。\n\n片段笔记：\n{}",
                    meta.title,
                    index + 1,
                    count,
                    group.join("\n\n")
                )
            })
            .collect();
        let phases: Vec<String> = (0..prompts.len())
            .map(|index| format!("归并 L{level} {}/{}", index + 1, count))
            .collect();
        let stage = Stage {
            name: "分层归并",
            total: prompts.len(),
            progress,
            log,
            range: (80, 83),
        };
        let merged = run(
            &stage,
            prompts
                .iter()
                .zip(phases.iter())
                .map(|(prompt, phase)| request::call(model, prompt, phase, log)),
        )
        .await?;
        // 归并没有变少说明已经到达收敛边界，避免并发后仍陷入无限归并。
        if merged.len() >= notes.len() {
            break;
        }
        notes = merged;
        level += 1;
    }
    Ok(notes
        .iter()
        .enumerate()
        .map(|(index, note)| format!("### 片段 {}\n{}", index + 1, note))
        .collect::<Vec<_>>()
        .join("\n\n"))
}

/// 并发阶段：工作项、进度区间与唯一日志入口，避免各分块互相覆盖状态。
struct Stage<'a> {
    name: &'a str,
    total: usize,
    progress: &'a (dyn Fn(u8) + Send + Sync),
    log: &'a (dyn Fn(&str) + Send + Sync),
    range: (u8, u8),
}

/// 按统一并发上限推进阶段内工作，结果按分块编号返回。
async fn run<T, F>(
    stage: &Stage<'_>,
    works: impl IntoIterator<Item = F>,
) -> Result<Vec<T>, CommandError>
where
    F: std::future::Future<Output = Result<T, CommandError>>,
{
    let done = AtomicUsize::new(0);
    let on_done = || {
        let finished = done.fetch_add(1, Ordering::SeqCst) + 1;
        (stage.progress)(stage_progress(stage.range, finished, stage.total));
        let running = video_work::WORK_LIMIT.min(stage.total.saturating_sub(finished));
        let waiting = stage.total.saturating_sub(finished + running);
        (stage.log)(&format!(
            "{}：完成 {}/{}，执行中 {}，等待 {}",
            stage.name, finished, stage.total, running, waiting
        ));
    };
    let works: Vec<F> = works.into_iter().collect();
    video_work::ordered(video_work::WORK_LIMIT, works, &on_done).await
}

/// 贪心分组：每组不超过归并预算。
fn group_notes(notes: &[String]) -> Vec<Vec<String>> {
    let mut groups: Vec<Vec<String>> = Vec::new();
    let mut current: Vec<String> = Vec::new();
    let mut used = 0;
    for note in notes {
        let size = note.chars().count();
        if !current.is_empty() && used + size > MERGE_CHARACTERS {
            groups.push(std::mem::take(&mut current));
            used = 0;
        }
        used += size;
        current.push(note.clone());
    }
    if !current.is_empty() {
        groups.push(current);
    }
    groups
}

/// 阶段进度按完成数量映射到固定区间，超过 255 项也不溢出或越界。
fn stage_progress(range: (u8, u8), finished: usize, total: usize) -> u8 {
    let span = range.1.saturating_sub(range.0) as usize;
    (range.0 as usize + finished.saturating_mul(span) / total.max(1)).min(range.1 as usize) as u8
}

#[cfg(test)]
#[path = "video_note_material_tests.rs"]
mod tests;
