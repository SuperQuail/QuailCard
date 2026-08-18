//! 供应商文件存储的播种与查询测试。

use super::super::testutil;

#[tokio::test]
/// 播种后应存在 4 个内置供应商且活动供应商为 openai。
async fn loads_default_providers() {
    let (storage, _config, _vault) = testutil::test_storage().await;
    let providers = storage.list_providers().await.expect("查询供应商失败");
    assert_eq!(providers.len(), 4);
    assert!(providers
        .iter()
        .all(|provider| provider.status == "untested"));
    assert!(providers.iter().all(|provider| !provider.has_credential));
    let subscription = providers
        .iter()
        .find(|provider| provider.id == "openai_subscription")
        .expect("缺少 OpenAI 订阅供应商");
    assert_eq!(subscription.provider_type, "openai_subscription");
    assert_eq!(subscription.model, "gpt-5.5");
    assert!(subscription.supports_vision);
    let opencode_go = providers
        .iter()
        .find(|provider| provider.id == "opencode_go")
        .expect("缺少 OpenCode Go 供应商");
    assert_eq!(opencode_go.protocol, "OpenAI Compatible");
    assert!(!opencode_go.supports_vision);
    assert_eq!(
        storage
            .get_active_provider_id()
            .await
            .expect("查询活动供应商失败"),
        "openai"
    );
}

#[test]
/// 播种函数生成的记录不携带任何凭据字段。
fn seeded_providers_have_no_credentials() {
    let file = crate::storage::providers_file::seed_providers_file();
    assert!(file
        .providers
        .iter()
        .all(|record| record.secret_ref.is_none() && record.auth_type.is_none()));
}
