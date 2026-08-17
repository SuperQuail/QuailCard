-- 为新旧数据库提供可直接填写 API Key 的 OpenCode Go 默认供应商。
INSERT OR IGNORE INTO providers (
    id, name, short_code, protocol, model, base_url,
    has_api_key, supports_vision, status, created_at, updated_at,
    secret_ref, auth_type, oauth_account_id, provider_type
) VALUES (
    'opencode_go', 'OpenCode Go', 'OG', 'OpenAI Compatible',
    'deepseek-v4-flash', 'https://opencode.ai/zen/go/v1',
    0, 0, 'untested', unixepoch(), unixepoch(),
    NULL, NULL, NULL, 'api'
);
