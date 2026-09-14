//! 供应商文件存储的播种与查询测试。

use super::super::testutil;

use crate::models::{
    ProviderConfig, ProviderInput, ProviderModel, MAX_PROVIDER_MODELS, MIN_CONTEXT_WINDOW_TOKENS,
};

/// 构造测试用模型条目：只关心 id 与两个可选上限。
fn model(id: &str, context_window: Option<u64>, max_output_tokens: Option<u32>) -> ProviderModel {
    ProviderModel {
        id: id.to_string(),
        name: id.to_string(),
        context_window,
        max_output_tokens,
    }
}

/// 构造保存/测试用的供应商输入；model 固定为目录外的合法 id，
/// 保证用例失败原因只可能来自目录本身。
fn catalog_input(models: Vec<ProviderModel>) -> ProviderInput {
    ProviderInput {
        id: Some("openai".to_string()),
        name: "OpenAI".to_string(),
        short_code: "OA".to_string(),
        protocol: "OpenAI Compatible".to_string(),
        model: "gpt-4.1-mini".to_string(),
        base_url: "https://api.openai.com/v1".to_string(),
        supports_vision: true,
        api_key: None,
        models,
    }
}

/// 构造只关心目录与当前模型 id 的请求配置。
fn config_with(models: Vec<ProviderModel>, model_id: &str) -> ProviderConfig {
    ProviderConfig {
        id: "provider".to_string(),
        protocol: "OpenAI Compatible".to_string(),
        model: model_id.to_string(),
        models,
        base_url: "https://example.com/v1".to_string(),
        secret_ref: None,
        auth_type: None,
        oauth_account_id: None,
        provider_type: "api".to_string(),
        supports_vision: false,
    }
}

#[tokio::test]
/// 旧 providers.toml 只有 model 字段时，摘要与请求配置都合成单条同名模型。
async fn legacy_model_field_synthesizes_catalog() {
    let config_dir = testutil::TempDir::new();
    std::fs::write(
        config_dir.path().join("providers.toml"),
        r#"formatVersion = 1
activeProviderId = "legacy"

[[providers]]
id = "legacy"
name = "Legacy"
shortCode = "LG"
protocol = "OpenAI Compatible"
model = "legacy-model"
baseUrl = "https://example.com/v1"
"#,
    )
    .expect("写入旧版 providers.toml 失败");
    let storage = crate::storage::Storage::open(config_dir.path()).expect("打开旧版存储失败");

    let summary = storage
        .get_provider_summary("legacy")
        .await
        .expect("读取旧版供应商失败");
    assert_eq!(summary.models.len(), 1);
    assert_eq!(summary.models[0].id, "legacy-model");
    assert_eq!(summary.models[0].name, "legacy-model");
    assert!(summary.models[0].max_output_tokens.is_none());
    // 摘要始终序列化 models，前端可以无条件读取该字段。
    let wire = serde_json::to_value(&summary).expect("摘要应可序列化");
    assert!(wire.get("models").is_some_and(serde_json::Value::is_array));

    let config = storage
        .get_provider_config("legacy")
        .await
        .expect("读取旧版配置失败")
        .expect("旧版供应商不存在");
    assert_eq!(
        config.active_model().map(|entry| entry.id.as_str()),
        Some("legacy-model")
    );
    // 旧记录没有输出上限，必须回退默认值而不是变成无上限请求。
    assert_eq!(config.max_output_tokens(), 32_768);
}

#[tokio::test]
/// 老前端只传 model 时保存仍然成功，读出时合成单条目录。
async fn save_without_catalog_falls_back_to_model() {
    let (storage, _config, _vault) = testutil::test_storage().await;
    let summary = storage
        .save_provider_config(&catalog_input(Vec::new()), "openai", None, None, None, None)
        .await
        .expect("兼容保存失败");
    assert_eq!(summary.models.len(), 1);
    assert_eq!(summary.models[0].id, "gpt-4.1-mini");
}

#[tokio::test]
/// 保存多模型目录后重新打开文件，目录、上限与当前模型完全一致。
async fn saved_catalog_round_trips_through_file() {
    let config_dir = testutil::TempDir::new();
    let storage = crate::storage::Storage::open(config_dir.path()).expect("打开测试存储失败");
    let models = vec![
        model("gpt-4.1-mini", Some(128_000), Some(16_384)),
        model("gpt-4.1", Some(1_000_000), None),
    ];
    storage
        .save_provider_config(&catalog_input(models), "openai", None, None, None, None)
        .await
        .expect("保存模型目录失败");

    let raw = std::fs::read_to_string(config_dir.path().join("providers.toml"))
        .expect("读取 providers.toml 失败");
    assert!(
        raw.contains("contextWindow"),
        "目录必须按 camelCase 落盘：{raw}"
    );
    assert!(
        raw.contains("maxOutputTokens"),
        "目录必须按 camelCase 落盘：{raw}"
    );

    let reopened = crate::storage::Storage::open(config_dir.path()).expect("重新打开存储失败");
    let summary = reopened
        .get_provider_summary("openai")
        .await
        .expect("读取摘要失败");
    assert_eq!(summary.models.len(), 2);
    assert_eq!(summary.models[0].context_window, Some(128_000));
    assert_eq!(summary.models[1].id, "gpt-4.1");
    assert_eq!(summary.model, "gpt-4.1-mini");

    let config = reopened
        .get_provider_config("openai")
        .await
        .expect("读取配置失败")
        .expect("供应商不存在");
    assert_eq!(
        config.active_model().map(|entry| entry.id.as_str()),
        Some("gpt-4.1-mini")
    );
    assert_eq!(config.max_output_tokens(), 16_384);
}

#[tokio::test]
/// 非法模型目录必须被拒：空 id、重复 id、越界上限、条目过多。
async fn rejects_invalid_model_catalog() {
    let (storage, _config, _vault) = testutil::test_storage().await;
    let too_many: Vec<ProviderModel> = (0..MAX_PROVIDER_MODELS + 1)
        .map(|index| model(&format!("m{index}"), None, None))
        .collect();
    let cases: Vec<(&str, Vec<ProviderModel>)> = vec![
        ("空模型 ID", vec![model("  ", None, None)]),
        (
            "重复模型 ID",
            vec![model("dup", None, None), model(" dup ", None, None)],
        ),
        (
            "上下文窗口过小",
            vec![model("small", Some(MIN_CONTEXT_WINDOW_TOKENS - 1), None)],
        ),
        ("输出上限为 0", vec![model("zero", None, Some(0))]),
        ("输出上限过大", vec![model("huge", None, Some(200_001))]),
        ("条目过多", too_many),
    ];
    for (label, models) in cases {
        let error = storage
            .save_provider_config(&catalog_input(models), "openai", None, None, None, None)
            .await
            .expect_err(label);
        assert_eq!(error.code, "VALIDATION_ERROR", "用例 {label} 未被拒绝");
    }
}

#[test]
/// active_model 先按 model 字段匹配目录 id，匹配不到时退回目录首项。
fn active_model_prefers_matching_id() {
    let catalog = || vec![model("first", None, None), model("second", None, None)];
    assert_eq!(
        config_with(catalog(), "second")
            .active_model()
            .map(|entry| entry.id.as_str()),
        Some("second")
    );
    assert_eq!(
        config_with(catalog(), "missing")
            .active_model()
            .map(|entry| entry.id.as_str()),
        Some("first")
    );
    assert!(config_with(Vec::new(), "first").active_model().is_none());
}

#[test]
/// 输出上限取选中条目；缺省、条目缺失或填 0 时统一回退 32_768。
fn max_output_tokens_falls_back_to_default() {
    assert_eq!(config_with(Vec::new(), "any").max_output_tokens(), 32_768);
    assert_eq!(
        config_with(vec![model("any", None, None)], "any").max_output_tokens(),
        32_768
    );
    assert_eq!(
        config_with(vec![model("any", None, Some(0))], "any").max_output_tokens(),
        32_768
    );
    assert_eq!(
        config_with(vec![model("any", None, Some(32_768))], "any").max_output_tokens(),
        32_768
    );
}

#[test]
/// 老前端不传 models 时按空数组反序列化，保证只增字段不破坏旧载荷。
fn provider_input_defaults_models_to_empty() {
    let input: ProviderInput = serde_json::from_value(serde_json::json!({
        "id": "openai",
        "name": "OpenAI",
        "shortCode": "OA",
        "protocol": "OpenAI Compatible",
        "model": "gpt-4.1-mini",
        "baseUrl": "https://api.openai.com/v1",
        "supportsVision": true,
        "apiKey": null
    }))
    .expect("老前端载荷应可反序列化");
    assert!(input.models.is_empty());
}
