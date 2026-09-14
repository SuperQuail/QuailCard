use std::collections::HashSet;
use std::time::Duration;

use super::{
    generation_ports::{DictionaryLookup, GenerationModel},
    generation_round::{self, process_generation_round},
    turn_loop::{self, TurnCancel, TurnFuture, TurnOutcome, TurnStep},
    GenerationControl,
};
use crate::{
    ai::{
        build_generation_prompt, generation_tools, GenerationSession, MultiToolRequest,
        ToolDefinition, ToolMessage,
    },
    error::CommandError,
    models::{GenerationInput, GenerationResult},
};

/// 单次请求的传输层看门狗；不是整轮或整任务总时限。
const REQUEST_TIMEOUT: Duration = Duration::from_secs(120);

impl TurnCancel for GenerationControl {
    /// 生成任务按原子标记判断停止，步内取消由本执行器自己 select。
    fn cancel_requested(&self) -> bool {
        self.is_cancelled()
    }
}

/// 一轮 = 一次模型请求加它带回的工具批次；状态在轮次之间保留。
struct GenerationTurn<'a> {
    model: &'a dyn GenerationModel,
    dictionary: &'a dyn DictionaryLookup,
    input: &'a GenerationInput,
    session: GenerationSession,
    control: &'a GenerationControl,
    system_prompt: String,
    user_prompt: String,
    max_tokens: u32,
    tools: generation_round::GenerationTools,
    definitions: Vec<ToolDefinition>,
    trace_id: String,
    history: Vec<ToolMessage>,
    lookups: HashSet<String>,
    stalled_phase: Option<String>,
    finish_reason: Option<String>,
}

impl TurnStep for GenerationTurn<'_> {
    /// 模型调用与工具处理都响应取消；结束原因留给外层统一收尾。
    fn step<'a>(&'a mut self, step: u32) -> TurnFuture<'a, TurnOutcome> {
        Box::pin(async move {
            self.control.progress(
                self.stalled_phase.as_deref().unwrap_or("generating"),
                self.session.generated(),
            );
            // 逐字回显由运行时负责，这里只需满足端口的增量回调契约。
            let delta = |_text: &str| {};
            let request = MultiToolRequest {
                trace_id: &self.trace_id,
                turn: step as usize,
                system_prompt: &self.system_prompt,
                user_prompt: &self.user_prompt,
                images: &self.input.images,
                tools: &self.definitions,
                history: &self.history,
                max_tokens: self.max_tokens,
                timeout: REQUEST_TIMEOUT,
            };
            let batch = tokio::select! { biased;
                _ = self.control.cancelled() => return Ok(TurnOutcome::Finish),
                batch = self.model.call_streaming(request, &delta) => batch?,
            };
            let round = tokio::select! { biased;
                _ = self.control.cancelled() => return Ok(TurnOutcome::Finish),
                round = process_generation_round(self.dictionary, self.input, &mut self.session, batch, &self.tools, &mut self.lookups, self.control) => round,
            };
            if self.session.fixed_complete() {
                self.finish_reason = None;
                return Ok(TurnOutcome::Finish);
            }
            if let Some(reason) = round.finish_reason {
                self.finish_reason = Some(reason);
                return Ok(TurnOutcome::Finish);
            }
            if round.progressed {
                self.stalled_phase = None;
            }
            self.history.extend(round.history);
            Ok(if round.progressed {
                TurnOutcome::Progress
            } else {
                TurnOutcome::Stalled
            })
        })
    }

    /// 停滞仅提示用户，不按轮数终止；真实进展会清除旧提示。
    fn stalled(&mut self, _step: u32, rounds: u32) {
        self.stalled_phase = Some(format!("连续 {rounds} 轮无有效进展"));
    }

    /// 生成取消不是错误：外层据停止标记返回已完成的草稿。
    fn cancel(&mut self) -> Result<(), CommandError> {
        Ok(())
    }
}

/// 执行器仅依赖模型、词典端口和已校验会话，取消会丢弃正在等待的请求。
pub(super) async fn execute_generation(
    model: &dyn GenerationModel,
    dictionary: &dyn DictionaryLookup,
    input: &GenerationInput,
    session: GenerationSession,
    control: &GenerationControl,
) -> Result<GenerationResult, CommandError> {
    let (system_prompt, user_prompt, max_tokens) = build_generation_prompt(input)?;
    let tools = generation_round::GenerationTools::build(generation_tools(input)?)?;
    let definitions = tools.definitions();
    let mut turn = GenerationTurn {
        model,
        dictionary,
        input,
        session,
        control,
        system_prompt,
        user_prompt,
        max_tokens,
        tools,
        definitions,
        trace_id: uuid::Uuid::now_v7().to_string(),
        history: Vec::new(),
        lookups: HashSet::new(),
        stalled_phase: None,
        finish_reason: None,
    };
    // 统一骨架负责步进与取消；模型失败时保留已通过校验的草稿。
    if let Err(error) = turn_loop::drive(&mut turn, control).await {
        return partial_or_error(turn.session, error, "后续模型请求失败，已保留有效草稿");
    }
    if control.is_cancelled() {
        return Ok(turn
            .session
            .finish(Some("已停止生成，保留已完成草稿".to_string())));
    }
    let reason = turn.finish_reason.take();
    Ok(turn.session.finish(reason))
}

/// 运行失败且没有草稿时返回安全错误，有草稿则返回部分成功。
fn partial_or_error(
    session: GenerationSession,
    error: CommandError,
    warning: &str,
) -> Result<GenerationResult, CommandError> {
    if session.generated() == 0 {
        Err(error)
    } else {
        Ok(session.finish(Some(warning.to_string())))
    }
}

#[cfg(test)]
mod tests {
    include!("generation_executor_tests.rs");
    mod progress {
        include!("generation_executor_progress_tests.rs");
    }
}
