use std::{
    path::{Path, PathBuf},
    sync::Mutex,
    time::{Duration, Instant},
};

use base64::{engine::general_purpose, Engine as _};
use futures_util::{SinkExt, StreamExt};
use reqwest::Url;
use sha2::{Digest, Sha256};
use tokio_tungstenite::{connect_async, tungstenite::Message};

use crate::error::CommandError;

/// Edge TTS WebSocket 端点前缀，使用固定客户端令牌鉴权。
const EDGE_TTS_BASE_URL: &str =
    "wss://speech.platform.bing.com/consumer/speech/synthesize/readaloud/edge/v1";
/// Edge TTS 固定客户端令牌，参与反爬令牌计算。
const TRUSTED_CLIENT_TOKEN: &str = "6A5AA1D4EAFF4E9FB37E23D68491D6F4";
/// Windows 时间纪元相对 Unix 纪元的偏移秒数。
const WIN_EPOCH_SECONDS: u64 = 11_644_473_600;
/// 默认美音语音名称。
const DEFAULT_VOICE: &str = "en-US-AriaNeural";
/// 对齐 edge-tts 使用的 Chromium 版本标识。
const CHROMIUM_FULL_VERSION: &str = "143.0.3650.75";
/// 连接与合成整体超时。
const SYNTHESIS_TIMEOUT: Duration = Duration::from_secs(15);
/// 单词文本的最大字符数。
const MAX_SPEECH_TEXT_CHARS: usize = 200;
/// 有道词典真人录音接口，与 TypeWords 用的是同一个源。
const YOUDAO_TTS_URL: &str = "https://dict.youdao.com/dictvoice";
/// 有道口音参数：1 英音、2 美音；本轮固定美音。
const YOUDAO_ACCENT_TYPE: &str = "2";
/// 有道请求超时：单词音频约 11 KB，4 秒足够，离线时不至于拖慢打字。
const YOUDAO_TIMEOUT: Duration = Duration::from_secs(4);
/// 有道失败后的冷却时长：期间跳过有道，直接走 Edge TTS。
const YOUDAO_COOLDOWN: Duration = Duration::from_secs(300);
/// 交给有道的最长词形：更长的文本按整句处理，交给合成语音。
const YOUDAO_WORD_MAX_CHARS: usize = 64;
/// 判定音频有效的最小字节数：接口失败时可能回 200 + HTML 或极短音频。
const MIN_AUDIO_BYTES: usize = 1024;
/// 音源标识参与缓存键，保证两种音源互不覆盖。
const YOUDAO_SOURCE: &str = "youdao:us";
/// Edge TTS 音源标识。
const EDGE_SOURCE: &str = "edge:en-US-AriaNeural";
/// 有道接口沿用的桌面浏览器标识，避免默认 UA 被判成爬虫。
const SPEECH_USER_AGENT: &str =
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/143.0.0.0 Safari/537.36 Edg/143.0.0.0";

/// 真人录音优先、Edge TTS 兜底的朗读服务，音频按音源分目录缓存。
pub struct SpeechService {
    cache_dir: PathBuf,
    /// 有道接口的冷却截止时间：断网时避免每个词都白等一次超时。
    youdao_retry_after: Mutex<Option<Instant>>,
}

impl SpeechService {
    /// 创建使用指定缓存目录的朗读服务。
    pub fn new(cache_dir: PathBuf) -> Self {
        Self {
            cache_dir,
            youdao_retry_after: Mutex::new(None),
        }
    }

    /// 合成单词发音：本地缓存 → 有道真人录音 → Edge TTS，逐级回退后写入缓存。
    ///
    /// 契约：调用方只拿得到 Data URL 或既有错误码；音源失败只体现为音色退化，
    /// 绝不因为换源失败让朗读整体不可用。
    pub async fn synthesize(&self, text: &str) -> Result<String, CommandError> {
        let text = text.trim();
        if text.is_empty() || text.chars().count() > MAX_SPEECH_TEXT_CHARS {
            return Err(CommandError::validation("朗读文本长度必须为 1-200 个字符"));
        }
        // 真人录音只服务英文词与短语；中文、整句与带数字符号的文本交给合成语音。
        if let Some(word) = speech_text(text) {
            let cache_file = self.cache_file(YOUDAO_SOURCE, &word);
            if cache_file.exists() {
                return read_audio_data_url(&cache_file);
            }
            if self.should_try_youdao() {
                match fetch_youdao_audio(&word).await {
                    Ok(audio) => return self.store_audio(&cache_file, &audio),
                    Err(detail) => {
                        // 原始错误只进后端日志，前端只看到回退后的结果。
                        eprintln!("TTS_SOURCE_ERROR(detail): {detail}");
                        self.cool_down_youdao();
                    }
                }
            }
        }
        let cache_file = self.cache_file(EDGE_SOURCE, text);
        if cache_file.exists() {
            return read_audio_data_url(&cache_file);
        }
        let audio = synthesize_edge_tts(text).await?;
        self.store_audio(&cache_file, &audio)
    }

    /// 写入音频缓存并返回 Data URL；缓存目录不可写时沿用既有错误码。
    fn store_audio(&self, cache_file: &Path, audio: &[u8]) -> Result<String, CommandError> {
        std::fs::create_dir_all(&self.cache_dir).map_err(io_error)?;
        std::fs::write(cache_file, audio).map_err(io_error)?;
        encode_audio_data_url(audio)
    }

    /// 计算音频的缓存文件名：音源与文本共同决定，两种音源不会互相覆盖。
    fn cache_file(&self, source: &str, text: &str) -> PathBuf {
        let mut hasher = Sha256::new();
        hasher.update(source.as_bytes());
        hasher.update(b"\0");
        hasher.update(text.as_bytes());
        let digest = hasher.finalize();
        self.cache_dir.join(format!("{}.mp3", hex_first(&digest)))
    }

    /// 有道是否值得尝试：冷却期内直接跳过，避免断网时拖慢整批预取。
    fn should_try_youdao(&self) -> bool {
        cooldown_elapsed(Instant::now(), *self.lock_cooldown())
    }

    /// 记录一次有道失败，进入冷却期。
    fn cool_down_youdao(&self) {
        *self.lock_cooldown() = Some(Instant::now() + YOUDAO_COOLDOWN);
    }

    /// 冷却时间的互斥访问：锁中毒时沿用内部值，语音不应因一次 panic 永久失效。
    fn lock_cooldown(&self) -> std::sync::MutexGuard<'_, Option<Instant>> {
        self.youdao_retry_after
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

/// 去掉首尾标点：句末的问号、句号不该影响词形，也不该被带进请求。
fn strip_surrounding_punctuation(text: &str) -> &str {
    text.trim()
        .trim_matches(|character: char| {
            !(character.is_ascii_alphanumeric()
                || character == '\''
                || character == '-'
                || character == ' ')
        })
        .trim()
}

/// 交给有道真人录音的文本：去掉首尾标点后仍是纯英文词或短语时才返回。
///
/// 中文、整句与含数字符号的文本返回 None——有道对这类输入会回中文腔或无关音频，
/// 交给合成语音更稳。
fn speech_text(text: &str) -> Option<String> {
    let cleaned = strip_surrounding_punctuation(text);
    let length = cleaned.chars().count();
    if length == 0 || length > YOUDAO_WORD_MAX_CHARS {
        return None;
    }
    let mut has_letter = false;
    for character in cleaned.chars() {
        if character.is_ascii_alphabetic() {
            has_letter = true;
            continue;
        }
        if character == ' ' || character == '\'' || character == '-' {
            continue;
        }
        return None;
    }
    // 全是连字符或撇号时没有可朗读的词形，交给合成语音处理。
    if !has_letter
        || cleaned.starts_with('-')
        || cleaned.ends_with('-')
        || cleaned.starts_with('\'')
        || cleaned.ends_with('\'')
    {
        return None;
    }
    Some(cleaned.to_string())
}

/// 冷却是否已经结束：没有记录过失败时总是值得再试一次。
fn cooldown_elapsed(now: Instant, deadline: Option<Instant>) -> bool {
    match deadline {
        None => true,
        Some(value) => now >= value,
    }
}

/// 响应的类型与长度是否真的是一段音频。
///
/// 有道失败时会回 200 + HTML 或极短音频，只看状态码会把脏数据写进缓存。
fn is_audio_payload(content_type: &str, bytes: usize) -> bool {
    content_type.to_ascii_lowercase().starts_with("audio/") && bytes >= MIN_AUDIO_BYTES
}

/// 拉取有道词典的真人发音；失败返回只用于后端日志的明细。
async fn fetch_youdao_audio(word: &str) -> Result<Vec<u8>, String> {
    let mut url = Url::parse(YOUDAO_TTS_URL).map_err(|error| format!("端点地址无效：{error}"))?;
    // 查询串交给 Url 编码，空格与撇号不会破坏请求。
    url.query_pairs_mut()
        .append_pair("audio", word)
        .append_pair("type", YOUDAO_ACCENT_TYPE);
    let client = reqwest::Client::builder()
        .timeout(YOUDAO_TIMEOUT)
        .build()
        .map_err(|error| format!("客户端构建失败：{error}"))?;
    let response = client
        .get(url)
        .header(reqwest::header::USER_AGENT, SPEECH_USER_AGENT)
        .send()
        .await
        .map_err(|error| format!("请求失败：{error}"))?;
    if !response.status().is_success() {
        return Err(format!("状态码 {}", response.status()));
    }
    let content_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .to_string();
    let audio = response
        .bytes()
        .await
        .map_err(|error| format!("读取响应失败：{error}"))?;
    if !is_audio_payload(&content_type, audio.len()) {
        return Err(format!(
            "响应不是音频：{content_type} / {} 字节",
            audio.len()
        ));
    }
    Ok(audio.to_vec())
}

/// 通过 Edge TTS WebSocket 合成音频并拼接全部音频分片。
async fn synthesize_edge_tts(text: &str) -> Result<Vec<u8>, CommandError> {
    let request = http_connect_request();
    let (mut socket, _) = tokio::time::timeout(SYNTHESIS_TIMEOUT, connect_async(request))
        .await
        .map_err(|_| tts_error("TTS_TIMEOUT", "语音合成连接超时，请检查网络"))?
        .map_err(|error| {
            tts_error(
                "TTS_CONNECT_FAILED",
                &format!("无法连接语音合成服务：{error:?}"),
            )
        })?;

    socket
        .send(Message::Text(synthesis_context_config()))
        .await
        .map_err(|_| tts_error("TTS_SEND_FAILED", "语音合成请求发送失败"))?;
    socket
        .send(Message::Text(ssml_message(text)))
        .await
        .map_err(|_| tts_error("TTS_SEND_FAILED", "语音合成请求发送失败"))?;
    let mut audio = Vec::new();
    let result = tokio::time::timeout(SYNTHESIS_TIMEOUT, async {
        while let Some(message) = socket.next().await {
            let message =
                message.map_err(|_| tts_error("TTS_STREAM_FAILED", "语音合成数据流中断"))?;
            match &message {
                Message::Text(text) => {
                    if text.contains("Path:turn.end") {
                        break;
                    }
                }
                Message::Binary(payload) => {
                    if let Some(chunk) = extract_audio_chunk(payload) {
                        audio.extend_from_slice(chunk);
                    }
                }
                Message::Close(_) => break,
                Message::Ping(_) | Message::Pong(_) | Message::Frame(_) => {}
            }
        }
        Ok::<(), CommandError>(())
    })
    .await;
    match result {
        Ok(Ok(())) if !audio.is_empty() => Ok(audio),
        Ok(Ok(())) => Err(tts_error("TTS_EMPTY", "语音合成没有返回音频数据")),
        Ok(Err(error)) => Err(error),
        Err(_) => Err(tts_error("TTS_TIMEOUT", "语音合成超时，请稍后重试")),
    }
}

/// 构造携带必要请求头的 WebSocket 握手请求（对齐 edge-tts 最新实现）。
fn http_connect_request() -> tokio_tungstenite::tungstenite::client::ClientRequestBuilder {
    use tokio_tungstenite::tungstenite::client::ClientRequestBuilder;
    let endpoint = format!(
        "{EDGE_TTS_BASE_URL}?TrustedClientToken={TRUSTED_CLIENT_TOKEN}&ConnectionId={}&Sec-MS-GEC={}&Sec-MS-GEC-Version=1-{CHROMIUM_FULL_VERSION}",
        connection_id(),
        sec_ms_gec(),
    );
    ClientRequestBuilder::new(endpoint.parse().expect("Edge TTS 端点地址无效"))
        .with_header("Pragma", "no-cache")
        .with_header("Cache-Control", "no-cache")
        .with_header("Origin", "chrome-extension://jdiccldimpdaibmpdkjnbmckianbfold")
        .with_header(
            "User-Agent",
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/143.0.0.0 Safari/537.36 Edg/143.0.0.0",
        )
        .with_header("Accept-Encoding", "gzip, deflate, br, zstd")
        .with_header("Accept-Language", "en-US,en;q=0.9")
        .with_header("Cookie", format!("muid={};", generate_muid()))
}

/// 生成无横线的随机连接标识。
fn connection_id() -> String {
    use rand::Rng;
    let mut rng = rand::thread_rng();
    (0..32).map(|_| rng.gen_range('0'..='9')).collect()
}

/// 生成随机的 MUID Cookie 值（32 位大写十六进制）。
fn generate_muid() -> String {
    use rand::Rng;
    const HEX_CHARS: &[u8] = b"0123456789abcdef";
    let mut rng = rand::thread_rng();
    (0..32)
        .map(|_| HEX_CHARS[rng.gen_range(0..HEX_CHARS.len())] as char)
        .collect::<String>()
        .to_uppercase()
}

/// 生成 Edge TTS 反爬验证令牌（5 分钟取整的 Windows FILETIME 与客户端令牌的 SHA256）。
fn sec_ms_gec() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let windowed = (now + WIN_EPOCH_SECONDS) / 300 * 300;
    let ticks = windowed * 10_000_000;
    let mut hasher = Sha256::new();
    hasher.update(format!("{ticks}{TRUSTED_CLIENT_TOKEN}").as_bytes());
    let digest = hasher.finalize();
    digest.iter().map(|byte| format!("{byte:02X}")).collect()
}

/// 构造 Edge TTS 的合成上下文配置消息。
fn synthesis_context_config() -> String {
    format!(
        "X-Timestamp:{}\r\nContent-Type:application/json; charset=utf-8\r\nPath:speech.config\r\n\r\n{{\"context\":{{\"synthesis\":{{\"audio\":{{\"metadataoptions\":{{\"sentenceBoundaryEnabled\":\"false\",\"wordBoundaryEnabled\":\"false\"}},\"outputFormat\":\"audio-24khz-48kbitrate-mono-mp3\"}}}}}}}}",
        edge_timestamp()
    )
}

/// 构造包含目标语音与文本的 SSML 消息。
fn ssml_message(text: &str) -> String {
    let escaped = escape_xml(text);
    let ssml = format!(
        r#"<speak version="1.0" xmlns="http://www.w3.org/2001/10/synthesis" xmlns:mstts="https://www.w3.org/2001/mstts" xml:lang="en-US"><voice name="{DEFAULT_VOICE}">{escaped}</voice></speak>"#
    );
    format!(
        "X-RequestId:{}\r\nContent-Type:application/ssml+xml\r\nX-Timestamp:{}Z\r\nPath:ssml\r\n\r\n{}",
        connection_id(),
        edge_timestamp(),
        ssml
    )
}

/// 生成 Edge TTS 时间戳头使用的日期字符串。
fn edge_timestamp() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;
    let days = seconds.div_euclid(86_400);
    let day_of_week =
        ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"][(days + 4).rem_euclid(7) as usize];
    let (year, month, day) = civil_from_days(days);
    let months = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    let hour = seconds.rem_euclid(86_400) / 3_600;
    let minute = seconds.rem_euclid(3_600) / 60;
    let second = seconds.rem_euclid(60);
    format!(
        "{day_of_week} {} {day:>2} {year:04} {hour:02}:{minute:02}:{second:02} GMT+0000 (Coordinated Universal Time)",
        months[(month - 1) as usize]
    )
}

/// 将自 1970-01-01 起的天数转换为公历日期（Howard Hinnant 算法）。
fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if month <= 2 { year + 1 } else { year };
    (year, month, day)
}

/// 从 Edge TTS 二进制帧中提取音频分片（前 2 字节为大端头部长度）。
fn extract_audio_chunk(payload: &[u8]) -> Option<&[u8]> {
    if payload.len() < 2 {
        return None;
    }
    let header_length = u16::from_be_bytes([payload[0], payload[1]]) as usize;
    if header_length == 0 || payload.len() < header_length + 2 {
        return None;
    }
    let header = std::str::from_utf8(&payload[2..2 + header_length]).ok()?;
    if !header
        .lines()
        .any(|line| line.trim_start().starts_with("Path:audio"))
    {
        return None;
    }
    let chunk = &payload[2 + header_length..];
    if chunk.is_empty() {
        return None;
    }
    Some(chunk)
}

/// 转义 SSML 中需要特殊处理的 XML 字符。
fn escape_xml(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

/// 读取缓存音频并编码为浏览器可直接播放的 Data URL。
fn read_audio_data_url(path: &Path) -> Result<String, CommandError> {
    let audio = std::fs::read(path).map_err(io_error)?;
    encode_audio_data_url(&audio)
}

/// 将音频字节编码为 Data URL。
fn encode_audio_data_url(audio: &[u8]) -> Result<String, CommandError> {
    Ok(format!(
        "data:audio/mpeg;base64,{}",
        general_purpose::STANDARD.encode(audio)
    ))
}

/// 将文件系统错误转换为用户可读错误。
fn io_error(error: std::io::Error) -> CommandError {
    CommandError::provider("TTS_CACHE_FAILED", format!("语音缓存读写失败：{error}"))
}

/// 构造固定错误码的语音合成错误。
fn tts_error(code: &'static str, message: &str) -> CommandError {
    CommandError::provider(code, message)
}

/// 将摘要字节转换为十六进制字符串。
fn hex_first(digest: &[u8]) -> String {
    digest
        .iter()
        .take(16)
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    /// SSML 消息会转义 XML 特殊字符。
    fn escapes_xml_special_characters() {
        assert_eq!(
            escape_xml("a & b <c> \"d\" 'e'"),
            "a &amp; b &lt;c&gt; &quot;d&quot; &apos;e&apos;"
        );
        assert!(ssml_message("hello & goodbye").contains("hello &amp; goodbye"));
    }

    #[test]
    /// 二进制帧按 2 字节头部长度提取音频分片。
    fn extracts_audio_chunk_from_binary_frame() {
        let header = b"Path:audio\r\nContent-Type:audio/mpeg\r\n\r\n";
        let header_length = (header.len() as u16).to_be_bytes();
        let mut frame = Vec::new();
        frame.extend_from_slice(&header_length);
        frame.extend_from_slice(header);
        frame.extend_from_slice(&[0x00, 0x00, 0xff, 0xff]);
        assert_eq!(
            extract_audio_chunk(&frame),
            Some(&frame[2 + header.len()..])
        );
    }

    #[test]
    /// 非音频帧、空分片或畸形帧不参与拼接。
    fn ignores_non_audio_frames() {
        let header = b"Path:turn.start\r\n\r\n";
        let mut frame = Vec::new();
        frame.extend_from_slice(&(header.len() as u16).to_be_bytes());
        frame.extend_from_slice(header);
        frame.extend_from_slice(b"payload");
        assert_eq!(extract_audio_chunk(&frame), None);
        assert_eq!(extract_audio_chunk(b"\x00\x01"), None);
        assert_eq!(extract_audio_chunk(b""), None);
    }

    #[test]
    /// 时间戳包含标准的日期头格式。
    fn builds_edge_timestamp_header() {
        let timestamp = edge_timestamp();
        assert!(timestamp.ends_with("GMT+0000 (Coordinated Universal Time)"));
        assert!(
            timestamp.starts_with("Mon")
                || timestamp.starts_with("Tue")
                || timestamp.starts_with("Wed")
                || timestamp.starts_with("Thu")
                || timestamp.starts_with("Fri")
                || timestamp.starts_with("Sat")
                || timestamp.starts_with("Sun")
        );
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(11_061), (2000, 4, 14));
    }

    #[test]
    /// 缓存文件名由音源与文本共同决定：同词同源稳定，跨源互不覆盖。
    fn builds_stable_cache_file_name() {
        let service = SpeechService::new(PathBuf::from("cache"));
        let first = service.cache_file(YOUDAO_SOURCE, "hello");
        let second = service.cache_file(YOUDAO_SOURCE, "hello");
        let other_word = service.cache_file(YOUDAO_SOURCE, "world");
        let other_source = service.cache_file(EDGE_SOURCE, "hello");
        assert_eq!(first, second);
        assert_ne!(first, other_word);
        assert_ne!(first, other_source);
        assert!(first.extension().is_some_and(|ext| ext == "mp3"));
    }

    #[test]
    /// 英文词与短语走真人录音，中文、整句与含数字的文本交给合成语音。
    fn selects_youdao_for_english_words() {
        assert_eq!(speech_text("ephemeral"), Some("ephemeral".to_string()));
        assert_eq!(speech_text("in terms of"), Some("in terms of".to_string()));
        assert_eq!(speech_text("don't"), Some("don't".to_string()));
        assert_eq!(speech_text("ephemeral."), Some("ephemeral".to_string()));
        assert_eq!(speech_text("  hello  "), Some("hello".to_string()));
        for text in [
            "你好",
            "B2",
            "hello world 123",
            "a".repeat(65).as_str(),
            "-",
            "''",
        ] {
            assert_eq!(speech_text(text), None, "不应交给有道：{text}");
        }
    }

    #[test]
    /// 首尾标点被剥掉，词形本身保持原样。
    fn strips_surrounding_punctuation() {
        assert_eq!(strip_surrounding_punctuation("  ephemeral?  "), "ephemeral");
        assert_eq!(strip_surrounding_punctuation("\"frugal\""), "frugal");
        assert_eq!(strip_surrounding_punctuation("don't"), "don't");
        assert_eq!(strip_surrounding_punctuation("well-known"), "well-known");
    }

    #[test]
    /// 有道请求把空格与撇号交给 URL 编码，解析回来仍是原词形。
    fn builds_youdao_url_with_encoding() {
        let mut url = Url::parse(YOUDAO_TTS_URL).expect("端点地址应可解析");
        url.query_pairs_mut()
            .append_pair("audio", "in terms of")
            .append_pair("type", YOUDAO_ACCENT_TYPE);
        let pairs: Vec<(String, String)> = url
            .query_pairs()
            .map(|(key, value)| (key.into_owned(), value.into_owned()))
            .collect();
        assert_eq!(
            pairs,
            vec![
                ("audio".to_string(), "in terms of".to_string()),
                ("type".to_string(), "2".to_string()),
            ]
        );
        assert!(url.as_str().starts_with(YOUDAO_TTS_URL));
    }

    #[test]
    /// 只有真正的音频响应才允许写缓存：HTML 或过短内容一律拒绝。
    fn accepts_only_audio_payloads() {
        assert!(is_audio_payload("audio/mpeg", MIN_AUDIO_BYTES));
        assert!(is_audio_payload("Audio/MP3", MIN_AUDIO_BYTES + 1));
        assert!(!is_audio_payload(
            "text/html; charset=utf-8",
            MIN_AUDIO_BYTES * 4
        ));
        assert!(!is_audio_payload("audio/mpeg", MIN_AUDIO_BYTES - 1));
        assert!(!is_audio_payload("", 0));
    }

    #[test]
    /// 冷却期内跳过有道，冷却结束后重新尝试。
    fn cooldown_blocks_youdao() {
        let now = Instant::now();
        assert!(cooldown_elapsed(now, None));
        assert!(!cooldown_elapsed(now, Some(now + YOUDAO_COOLDOWN)));
        assert!(cooldown_elapsed(
            now + YOUDAO_COOLDOWN,
            Some(now + YOUDAO_COOLDOWN)
        ));
    }

    #[tokio::test]
    /// 冷却期内的词不会再触发有道请求：断网时整批预取不会逐词等待超时。
    async fn skips_youdao_during_cooldown() {
        let service = SpeechService::new(PathBuf::from("cache"));
        assert!(service.should_try_youdao());
        service.cool_down_youdao();
        assert!(!service.should_try_youdao());
    }

    #[test]
    /// 超长文本被拒绝，避免滥用合成接口。
    fn rejects_overlong_text() {
        let service = SpeechService::new(PathBuf::from("cache"));
        let long = "x".repeat(201);
        let error = tokio::runtime::Runtime::new()
            .expect("创建运行时失败")
            .block_on(service.synthesize(&long))
            .expect_err("超长文本不应合成");
        assert_eq!(error.code, "VALIDATION_ERROR");
    }

    #[test]
    /// Sec-MS-GEC 令牌长度固定为 64 位大写十六进制。
    fn generates_sec_ms_gec_token() {
        let token = sec_ms_gec();
        assert_eq!(token.len(), 64);
        assert!(token.chars().all(|character| character.is_ascii_hexdigit()));
    }
    #[tokio::test]
    #[ignore = "依赖有道在线服务，仅在开发环境手动运行"]
    /// 真实拉取一个单词的真人录音，确认接口与校验规则可用。
    async fn fetches_youdao_audio_over_network() {
        let audio = fetch_youdao_audio("ephemeral")
            .await
            .expect("有道发音拉取失败");
        assert!(audio.len() >= MIN_AUDIO_BYTES);
    }

    #[tokio::test]
    #[ignore = "依赖 Edge TTS 在线服务，仅在开发环境手动运行"]
    /// 真实合成一个单词并返回可播放的音频 Data URL。
    async fn synthesizes_word_over_network() {
        let service =
            SpeechService::new(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/tts-cache"));
        let result = service.synthesize("ephemeral").await;
        eprintln!(
            "合成结果: {:?}",
            result.as_ref().map(|value| value.chars().count())
        );
        let result = result.expect("在线合成失败");
        assert!(result.starts_with("data:audio/mpeg;base64,"));
        assert!(result.len() > 200);
        let cached = service.synthesize("ephemeral").await.expect("缓存读取失败");
        assert_eq!(result, cached);
    }
}
