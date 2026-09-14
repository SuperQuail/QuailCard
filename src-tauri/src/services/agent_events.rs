//! Agent 循环对外的进度事件；状态快照、日志与前端流式都从同一事件派生。

/// 一次 Agent 执行中可观察的进度事件。
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum AgentEvent {
    /// 新的一步开始，并绑定将要产出的推理与文本消息身份。
    StepStart {
        step: u32,
        message_id: String,
        reasoning_message_id: String,
    },
    /// 可见文本增量。
    TextDelta { text: String },
    /// 推理增量；只用于实时展示，不进入会话存储。
    ReasoningDelta { text: String },
    /// 文本已写入会话历史，流式缓冲可以清空。
    TextCommitted { message_id: String },
    /// 工具开始执行；label 是安全描述，不含参数与正文。
    ToolStart { name: String, label: &'static str },
    /// 连续多轮没有有效进展；只作为可见提示，不终止本轮。
    Stalled { step: u32, rounds: u32 },
}

/// 循环只依赖这个窄接口；实现方决定如何投影状态。
pub(crate) trait AgentEventSink: Send + Sync {
    fn emit(&self, event: AgentEvent);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// 记录型 sink，证明事件按发出顺序可被消费。
    #[derive(Default)]
    struct Recorder(Mutex<Vec<AgentEvent>>);

    impl AgentEventSink for Recorder {
        fn emit(&self, event: AgentEvent) {
            self.0.lock().unwrap().push(event);
        }
    }

    #[test]
    fn records_events_in_order() {
        let recorder = Recorder::default();
        recorder.emit(AgentEvent::StepStart {
            step: 1,
            message_id: "m1".into(),
            reasoning_message_id: "r1".into(),
        });
        recorder.emit(AgentEvent::TextDelta {
            text: "你好".into(),
        });
        recorder.emit(AgentEvent::TextCommitted {
            message_id: "m1".into(),
        });
        let events = recorder.0.lock().unwrap();
        assert_eq!(events.len(), 3);
        assert!(matches!(events[0], AgentEvent::StepStart { step: 1, .. }));
        assert!(matches!(events[2], AgentEvent::TextCommitted { .. }));
    }
}
