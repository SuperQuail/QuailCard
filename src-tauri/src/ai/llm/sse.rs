//! SSE 分帧：把字节流切成完整事件的 data 负载。

use crate::error::CommandError;

/// 累积字节并按空行分帧；与 ai/stream.rs 的临时实现等价，P6 后唯一保留。
#[derive(Default)]
pub(crate) struct SseParser {
    pending: Vec<u8>,
}

impl SseParser {
    /// 追加一段字节并返回本次完成的全部 data 负载。
    pub(crate) fn push(&mut self, bytes: &[u8]) -> Result<Vec<String>, CommandError> {
        self.pending.extend_from_slice(bytes);
        let mut events = Vec::new();
        while let Some((index, len)) = find_delimiter(&self.pending) {
            let block = self.pending[..index].to_vec();
            self.pending.drain(..index + len);
            if let Some(data) = extract_data(&block)? {
                events.push(data);
            }
        }
        Ok(events)
    }

    /// 流结束时处理最后一个没有空行结尾的事件。
    pub(crate) fn finish(&mut self) -> Result<Option<String>, CommandError> {
        if self.pending.is_empty() {
            return Ok(None);
        }
        let block = std::mem::take(&mut self.pending);
        extract_data(&block)
    }
}

/// 提取一个事件块中所有 data: 行并拼接。
fn extract_data(block: &[u8]) -> Result<Option<String>, CommandError> {
    let text = std::str::from_utf8(block).map_err(|_| invalid_stream())?;
    let data = text
        .lines()
        .filter_map(|line| line.strip_prefix("data:").map(str::trim_start))
        .collect::<Vec<_>>()
        .join("\n");
    Ok((!data.is_empty()).then_some(data))
}

/// 查找最近的 LF 或 CRLF 空行分隔符。
fn find_delimiter(bytes: &[u8]) -> Option<(usize, usize)> {
    let lf = bytes
        .windows(2)
        .position(|window| window == b"\n\n")
        .map(|index| (index, 2));
    let crlf = bytes
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .map(|index| (index, 4));
    match (lf, crlf) {
        (Some(left), Some(right)) => Some(if left.0 <= right.0 { left } else { right }),
        (Some(value), None) | (None, Some(value)) => Some(value),
        (None, None) => None,
    }
}

/// 只依据 content-type 判定事件流。
pub(crate) fn is_event_stream(response: &reqwest::Response) -> bool {
    response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.contains("text/event-stream"))
}

/// 创建无法识别流式响应的安全错误。
fn invalid_stream() -> CommandError {
    CommandError::provider(
        "PROVIDER_RESPONSE_INVALID",
        "供应商返回了无法识别的流式响应",
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    /// LF、CRLF 与跨块累积都能分帧，data 多行被拼接。
    fn frames_lf_and_crlf_events() {
        let mut parser = SseParser::default();
        assert!(parser.push(b"data: one\n").unwrap().is_empty());
        let events = parser.push(b"\ndata: two\r\n\r\n").unwrap();
        assert_eq!(events, vec!["one".to_string(), "two".to_string()]);
        let multi = parser.push(b"data: a\ndata: b\n\n").unwrap();
        assert_eq!(multi, vec!["a\nb".to_string()]);
    }

    #[test]
    /// 无空行结尾的尾块在 finish 时解析一次。
    fn finish_parses_tail() {
        let mut parser = SseParser::default();
        parser.push(b"data: tail").unwrap();
        assert_eq!(parser.finish().unwrap(), Some("tail".to_string()));
        assert_eq!(parser.finish().unwrap(), None);
    }
}
