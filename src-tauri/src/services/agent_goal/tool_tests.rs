use super::*;

/// 模型工具声明不再承诺或接受可配置的目标续轮额度。
#[test]
fn goal_schemas_do_not_advertise_round_limits() {
    for tool in registry() {
        assert!(tool.spec.schema["properties"]
            .get("maxGoalRounds")
            .is_none());
        assert!(!tool.spec.description.contains("轮次上限"));
    }
}

/// 新建使用零惰性值，编辑保留旧数据；解析不依赖执行设置，也不接受旧参数重新设限。
#[test]
fn goal_spec_ignores_model_round_limit_and_preserves_legacy_value() {
    let args = json!({
        "objective": "核对来源", "acceptanceCriteria": ["已核对"],
        "maxGoalRounds": u64::MAX
    });
    for legacy_max in [0, 1, 16, u32::MAX] {
        let parsed = spec(&args, legacy_max).unwrap();
        assert_eq!(parsed.max_goal_rounds, legacy_max);
        assert_eq!(parsed.objective, "核对来源");
    }
}
