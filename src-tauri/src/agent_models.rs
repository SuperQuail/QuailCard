use serde::{Deserialize, Serialize};
use serde_json::Value;

/// 会话按知识库持久化，旧文件缺失字段取默认值。
#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AgentSession {
    pub format_version: u64,
    pub id: String,
    pub title: String,
    pub updated_at: i64,
    pub messages: Vec<AgentMessage>,
    pub summary: String,
    pub selected_paths: Vec<String>,
    /// 子会话的笔记写入授权：父级在派生时授予并持久化；空表示只读。
    ///
    /// 根会话为空表示整库，绝不落盘成"无限制"授予；旧文件缺字段按只读读取。
    pub write_scope: Vec<String>,
    /// 子会话固定父身份，根会话为空。
    pub parent_session_id: Option<String>,
    pub delegation_depth: u32,
    /// Fork 只复制此位置之前已经结束轮次的消息。
    pub completed_message_count: usize,
    pub children: Vec<String>,
    pub goal: Option<crate::agent_autonomy_models::Goal>,
    pub plan: crate::agent_autonomy_models::Plan,
}

/// 消息块由注册组件解释，工具结果仅包含安全内容。
#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AgentMessage {
    pub id: String,
    pub role: String,
    pub content: String,
    pub kind: String,
    pub data: Value,
}

/// 工具历史只公开封闭状态，不把内部错误或供应商状态字符串当作界面契约。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AgentToolState {
    /// 尚无完成收据；不代表重新打开会话时会自动执行。
    Running,
    /// 已保存的中立收据明确报告成功。
    Ok,
    /// 失败或记录无法安全识别时保守降级。
    #[default]
    #[serde(other)]
    Error,
}

/// 单条安全工具摘要；仅允许类型白名单参数和统计，原始正文、协议身份和凭据不可进入。
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AgentToolRow {
    /// 从本地消息身份和索引派生，绝不使用供应商调用 ID。
    pub id: String,
    /// 只允许注册表中的名称，否则固定为 unknown_tool。
    pub name: String,
    /// 仅允许 running、ok、error 三种公开状态。
    pub state: AgentToolState,
    /// 静态文案可拼接已校验计数，不拼接自由文本或错误正文。
    pub summary: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub details: Vec<AgentToolDetail>,
    /// 只允许已知错误码，未知代码不回显。
    #[serde(skip_serializing_if = "String::is_empty")]
    pub error_code: String,
}

/// 标签来自静态白名单，值仅为有界数字、布尔描述或标准 UUID。
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AgentToolDetail {
    pub label: String,
    pub value: String,
}

/// tool_calls 消息的附加数据；旧文件缺少字段时按空工具列表读取。
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AgentToolCalls {
    /// 工具顺序与供应商中立调用记录一致。
    pub rows: Vec<AgentToolRow>,
}

/// 每轮固定供应商与资料范围，重试必须复用请求身份。
#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentInput {
    pub session_id: String,
    pub request_id: String,
    pub content: String,
    pub provider_id: String,
    pub selected_paths: Vec<String>,
    #[serde(default)]
    pub images: Vec<crate::models::GenerationImage>,
}

impl AgentInput {
    /// 在任务登记前限制附件大小并验证编码，禁止无效图片进入历史文件。
    pub(crate) fn validate_images(&self) -> Result<(), crate::error::CommandError> {
        use crate::error::CommandError;
        use base64::Engine as _;
        if self.images.len() > 4 {
            return Err(CommandError::validation("一次最多发送 4 张图片"));
        }
        let mut total = 0;
        for image in &self.images {
            if image.name.trim().is_empty()
                || image.name.chars().count() > 255
                || !["image/png", "image/jpeg", "image/webp"].contains(&image.mime_type.as_str())
                || image.data_base64.len() > 7 * 1024 * 1024
            {
                return Err(CommandError::validation(
                    "图片名称、格式或大小无效，仅支持 PNG、JPG 和 WebP",
                ));
            }
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(&image.data_base64)
                .map_err(|_| CommandError::validation("图片编码无效"))?;
            if bytes.is_empty() || bytes.len() > 5 * 1024 * 1024 {
                return Err(CommandError::validation("单张图片大小必须在 5 MiB 以内"));
            }
            total += bytes.len();
        }
        if total > 15 * 1024 * 1024 {
            return Err(CommandError::validation("图片总大小不能超过 15 MiB"));
        }
        Ok(())
    }
}

/// 状态查询返回完整快照，递增序号用于丢弃迟到响应。
#[derive(Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentRun {
    pub id: String,
    pub session_id: String,
    pub state: String,
    pub sequence: u64,
    pub text: String,
    pub text_message_id: String,
    /// 当前步骤的推理文本；与 reasoning_message_id 一起用于实时展示与中断收尾。
    pub reasoning: String,
    pub reasoning_message_id: String,
    pub phase: String,
    pub pending_write: Option<String>,
    pub pending_write_id: Option<String>,
    pub error: Option<String>,
    pub goal_phase: String,
    pub waiting_reason: Option<String>,
}

/// 写前日志保存恢复内容，不能将正文写入调试日志。
#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AgentChange {
    pub format_version: u64,
    pub id: String,
    pub path: String,
    pub before: Option<String>,
    pub after: String,
    pub before_hash: Option<String>,
    pub after_hash: String,
    pub state: String,
}

/// 长期记忆仅由显式用户动作保存。
#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AgentMemory {
    pub format_version: u64,
    pub content: String,
}
