//! 严格编号与视觉确认契约测试。
use super::*;

#[test]
/// 编号可以不连续，不能选择本轮没有实际发送的帧。
fn accepts_only_actual_ids_and_explicit_confirmation() {
    assert_eq!(
        parse_selection(r#"{"candidate":7,"matched":true,"clear":true}"#, &[2, 7]).unwrap(),
        Some(7)
    );
    for reply in [
        r#"{"candidate":1,"matched":true,"clear":true}"#,
        r#"{"candidate":0,"matched":true,"clear":true}"#,
        r#"{"candidate":"7","matched":true,"clear":true}"#,
        r#"{"candidate":7}"#,
        r#"{"candidate":7,"matched":false,"clear":true}"#,
        r#"{"candidate":7,"matched":true,"clear":false}"#,
        r#"{"candidate":7,"matched":"true","clear":true}"#,
        r#"{"candidate":7.5,"matched":true,"clear":true}"#,
    ] {
        assert!(parse_selection(reply, &[2, 7]).is_err(), "{reply}");
    }
}

#[test]
/// 无匹配显式返回空，错误回复绝不转为选中。
fn none_and_malformed_never_keep() {
    assert_eq!(
        parse_selection(r#"{"candidate":"none"}"#, &[1]).unwrap(),
        None
    );
    for reply in [
        "",
        "none",
        "keep",
        "我建议第一张",
        "[]",
        "{}",
        r#"{"candidate":null}"#,
        "说明{\"candidate\":1,\"matched\":true,\"clear\":true}",
    ] {
        assert!(parse_selection(reply, &[1]).is_err());
    }
}

#[test]
/// 高相似度界面不能取代具体画面目标的确认。
fn prompt_requires_concrete_visual_evidence() {
    for phrase in [
        "底部字幕",
        "函数定义",
        "调用点不能替代",
        "强类型封装",
        "动态查询",
        "看不到图片",
    ] {
        assert!(SYSTEM.contains(phrase));
    }
}

#[test]
/// 不分配大图就验证字节边界，溢出长度也必须安全拒绝。
fn image_and_group_limits_are_checked_before_encoding() {
    assert!(
        validate_sizes([MAX_IMAGE_BYTES, MAX_GROUP_BYTES - MAX_IMAGE_BYTES].into_iter()).is_ok()
    );
    for sizes in [
        vec![MAX_IMAGE_BYTES + 1],
        vec![MAX_IMAGE_BYTES, MAX_IMAGE_BYTES],
        vec![usize::MAX],
    ] {
        assert_eq!(
            validate_sizes(sizes.into_iter()).unwrap_err().code,
            "VIDEO_SHOT_IMAGE_LIMIT"
        );
    }
}
