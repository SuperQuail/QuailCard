//! 安全工具详情的投影契约，夹具使用真实中立收据外形。
use super::*;
use serde_json::json;

const TASK: &str = "018f19e0-7990-7000-8000-123456789abc";
const SECRET: &str = "SECRET-token https://example.test/a?signature=SECRET";

/// 从中立数据构造历史，不允许投影借用供应商续传对象。
fn rows(calls: Value, results: Value) -> Value {
    let session = AgentSession {
        messages: vec![AgentMessage {
            id: "local".into(),
            kind: "exchange".into(),
            data: json!({"calls":calls,"results":results,
                "assistant":{"arguments":{"at":999},"error":SECRET},"replay":SECRET}),
            ..Default::default()
        }],
        ..Default::default()
    };
    let public = project(session);
    let serialized = serde_json::to_string(&public).unwrap();
    assert!(!serialized.contains("SECRET"));
    assert!(!serialized.contains("signature"));
    public.messages[0].data["rows"].clone()
}

/// 内容字符串与真实工具执行保存的包装保持一致。
fn receipt(id: &str, value: Value) -> Value {
    json!({"tool_call_id":id,"content":value.to_string()})
}

/// 检查白名单标签和值，避免靠全文 contains 掩盖错配。
fn has_detail(row: &Value, label: &str, value: &str) -> bool {
    row["details"].as_array().is_some_and(|items| {
        items
            .iter()
            .any(|item| item["label"] == label && item["value"] == value)
    })
}

/// ffmpeg 实际 VIDEO_FRAME_FAILED 错误可读，但任意原始错误与输入自由文本不可见。
#[test]
fn actual_frame_failure_has_safe_explanation_and_inputs() {
    let error = crate::error::CommandError::new("VIDEO_FRAME_FAILED", SECRET);
    let public = rows(
        json!([{"id":"a","name":"video_shot","arguments":{
        "taskId":TASK,"at":12.5,"reason":SECRET,"url":SECRET,"token":SECRET}}]),
        json!([receipt("a", json!({"ok":false,"error":error}))]),
    );
    let row = &public[0];
    assert_eq!(row["state"], "error");
    assert_eq!(row["errorCode"], "VIDEO_FRAME_FAILED");
    assert_eq!(row["summary"], "视频取帧失败，请换一个时间点或重试");
    assert!(has_detail(row, "输入·时间点（秒）", "12.5"));
    assert!(has_detail(row, "输入·任务 ID", TASK));
    assert_eq!(row["details"].as_array().unwrap().len(), 2);
}

/// 旧截图无参数仍能显示任务错误，不能从 assistant 参数猜测时间点。
#[test]
fn legacy_missing_arguments_keep_known_result_error() {
    for code in [
        "VIDEO_TASK_MISSING",
        "VIDEO_TASK_NOT_FOUND",
        "VIDEO_NO_TRANSCRIPT",
    ] {
        let public = rows(
            json!([{"id":"a","name":"video_shot"}]),
            json!([receipt(
                "a",
                json!({"ok":false,"error":{"code":code,"message":SECRET}})
            )]),
        );
        assert_eq!(public[0]["errorCode"], code);
        assert!(public[0].get("details").is_none());
        assert_ne!(public[0]["summary"], "工具记录不可用，无法确认执行结果");
    }
}

/// 真实视频转录总数与首屏游标可见，字幕、标题和签名链接永不透出。
#[test]
fn video_transcript_counters_are_descriptive() {
    let public = rows(
        json!([{"id":"a","name":"video_transcript"}]),
        json!([receipt(
            "a",
            json!({"ok":true,"result":{
            "taskId":TASK,"segments":2478,"shots":2,"title":SECRET,"notePath":SECRET,
            "transcript":{"text":SECRET,"offset":0,"nextOffset":6000,"totalCharacters":48000},
            "url":SECRET,"unknown":{"count":999}}})
        )]),
    );
    let row = &public[0];
    assert_eq!(row["summary"], "读取字幕 2478 段");
    assert!(has_detail(row, "字幕段数", "2478"));
    assert!(has_detail(row, "画面数", "2"));
    assert!(has_detail(row, "下一偏移", "6000"));
    assert!(has_detail(row, "结果·任务 ID", TASK));
    assert!(!has_detail(row, "结果数量", "999"));
}

/// 泛用计数和输入分页信息必须为数字，结束游标 null 不伪造下一页。
#[test]
fn pagination_and_typed_results_reject_malicious_values() {
    let public = rows(
        json!([{"id":"a","name":"read_note","arguments":{
        "offset":1,"limit":40,"page":2,"at":SECRET,"taskId":SECRET}}]),
        json!([receipt(
            "a",
            json!({"ok":true,"result":{
            "count":4,"offset":1,"nextOffset":null,"truncated":true,
            "segments":SECRET,"shots":-1,"total":1.5,"taskId":SECRET,"text":SECRET}})
        )]),
    );
    let row = &public[0];
    assert_eq!(row["summary"], "工具执行完成，返回 4 项");
    assert!(has_detail(row, "输入·偏移", "1"));
    assert!(has_detail(row, "输入·上限", "40"));
    assert!(has_detail(row, "输入·页码", "2"));
    assert!(has_detail(row, "结果已截断", "是"));
    assert_eq!(row["details"].as_array().unwrap().len(), 6);
}

/// 未知错误码和任意错误正文不可公开，包括把已知码藏进 message 的伪造。
#[test]
fn unknown_errors_and_hidden_nested_fields_are_redacted() {
    for error in [
        json!({"code":SECRET,"message":"VIDEO_FRAME_FAILED"}),
        json!({"message":SECRET}),
        json!(SECRET),
    ] {
        let public = rows(
            json!([{"id":"a","name":"video_shot"}]),
            json!([receipt(
                "a",
                json!({"ok":false,"error":error,
                "result":{"count":10},"token":SECRET})
            )]),
        );
        assert_eq!(public[0]["summary"], "工具执行失败，未提供可安全显示的原因");
        assert!(public[0].get("errorCode").is_none());
        assert!(public[0].get("details").is_none());
    }
}

/// 唯一精确配对是所有结果详情的前提，重复或错配的结果不得借尸还魂。
#[test]
fn wrong_and_duplicate_results_never_contribute_details() {
    let success = receipt(
        "a",
        json!({"ok":true,"result":{"segments":2478,"taskId":TASK}}),
    );
    for (calls, results, state) in [
        (
            json!([{"id":"b","name":"video_transcript"}]),
            json!([success.clone()]),
            "running",
        ),
        (
            json!([{"id":"a","name":"video_transcript"}]),
            json!([success.clone(), success.clone()]),
            "error",
        ),
        (
            json!([{"id":"a","name":"video_transcript"},{"id":"a","name":"video_transcript"}]),
            json!([success]),
            "error",
        ),
    ] {
        let public = rows(calls, results);
        for row in public.as_array().unwrap() {
            assert_eq!(row["state"], state);
            assert!(row.get("details").is_none());
            assert!(row.get("errorCode").is_none());
        }
    }
}

/// 持久化投影只保存数字和标准 UUID，无参数旧记录保持空对象且不制造默认值。
#[test]
fn safe_argument_projection_is_bounded_and_idempotent() {
    let input = json!({"at":12.5,"offset":0,"limit":12000,"page":1,"taskId":TASK,
        "url":SECRET,"reason":SECRET,"arguments":SECRET,"pages":[1,2],"token":SECRET});
    let safe = safe_arguments(&input);
    assert_eq!(
        safe,
        json!({"at":12.5,"offset":0,"limit":12000,"page":1,"taskId":TASK})
    );
    assert_eq!(safe_arguments(&safe), safe);
    for input in [
        Value::Null,
        json!(SECRET),
        json!({"taskId":SECRET,"at":86401,
        "limit":-1,"offset":1.5,"page":9007199254740992u64}),
    ] {
        assert_eq!(safe_arguments(&input), json!({}));
    }
}

/// 中立图片收据只解读唯一文本块，图片 URL 与供应商字段完全忽略。
#[test]
fn neutral_image_success_is_ok_without_image_disclosure() {
    let public = rows(
        json!([{"id":"a","name":"video_shot"}]),
        json!([{
        "tool_call_id":"a","imageCount":1,"content":[
            {"type":"text","text":json!({"ok":true,"result":{"at":12.5,"path":SECRET}}).to_string()},
            {"type":"image_url","image_url":{"url":SECRET}},
            {"type":"future","data":SECRET}
        ]}]),
    );
    assert_eq!(public[0]["state"], "ok");
    assert_eq!(public[0]["summary"], "视频画面读取完成");
    assert!(has_detail(&public[0], "画面时间（秒）", "12.5"));
    assert!(has_detail(&public[0], "返回图片数", "1"));
}

/// 多文本或无文本的内容块无法确定唯一收据，保守拒绝而不挑选成功片段。
#[test]
fn ambiguous_image_receipts_fail_closed() {
    let text = json!({"type":"text","text":"{\"ok\":true}"});
    for content in [
        json!([text.clone(), text]),
        json!([{"type":"image_url","image_url":SECRET}]),
    ] {
        let public = rows(
            json!([{"id":"a","name":"video_shot"}]),
            json!([{"tool_call_id":"a","content":content}]),
        );
        assert_eq!(public[0]["state"], "error");
        assert!(public[0].get("details").is_none());
    }
}

/// 增量 DTO 缺字段时可读，空详情不增加旧 wire 对象字段。
#[test]
fn additive_fields_default_and_skip_empty() {
    let row: AgentToolRow = serde_json::from_value(json!({"name":"read_note"})).unwrap();
    assert!(row.details.is_empty());
    assert!(row.error_code.is_empty());
    let value = serde_json::to_value(row).unwrap();
    assert!(value.get("details").is_none());
    assert!(value.get("errorCode").is_none());
}
