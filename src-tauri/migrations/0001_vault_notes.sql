PRAGMA foreign_keys = ON;

-- ============================================================
-- 全新 schema：笔记是 Vault 中的 .md 文件，SQLite 只保存
-- 拆出的卡片、复习调度、索引缓存与供应商/保险库配置。
-- ============================================================

-- 卡片：绑定来源笔记的 Vault 相对路径
CREATE TABLE IF NOT EXISTS cards (
    id TEXT PRIMARY KEY NOT NULL,
    note_path TEXT NOT NULL,
    source_ref TEXT NOT NULL DEFAULT '',
    kind TEXT NOT NULL CHECK (kind IN ('vocabulary', 'qa', 'ai')),
    front TEXT NOT NULL CHECK (length(trim(front)) BETWEEN 1 AND 2000),
    back TEXT NOT NULL CHECK (length(trim(back)) BETWEEN 1 AND 8000),
    detail TEXT NOT NULL DEFAULT '',
    example TEXT NOT NULL DEFAULT '',
    aliases_json TEXT NOT NULL DEFAULT '[]' CHECK (json_valid(aliases_json)),
    rubric_json TEXT NOT NULL DEFAULT '[]' CHECK (json_valid(rubric_json)),
    position INTEGER NOT NULL,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    UNIQUE (note_path, position)
);

CREATE INDEX IF NOT EXISTS cards_note_idx ON cards(note_path);

-- 调度状态：沿用自适应调度模型
CREATE TABLE IF NOT EXISTS review_states (
    card_id TEXT PRIMARY KEY NOT NULL REFERENCES cards(id) ON DELETE CASCADE,
    due_at INTEGER NOT NULL,
    interval_days INTEGER NOT NULL DEFAULT 0 CHECK (interval_days BETWEEN 0 AND 365),
    repetitions INTEGER NOT NULL DEFAULT 0,
    lapses INTEGER NOT NULL DEFAULT 0,
    total_reviews INTEGER NOT NULL DEFAULT 0,
    last_result TEXT NULL,
    version INTEGER NOT NULL DEFAULT 0,
    updated_at INTEGER NOT NULL,
    stability REAL NULL,
    difficulty REAL NOT NULL DEFAULT 5.0,
    last_review_at INTEGER NULL,
    scheduler_phase TEXT NOT NULL DEFAULT 'new',
    learning_step INTEGER NOT NULL DEFAULT 0
);

CREATE INDEX IF NOT EXISTS review_states_due_idx ON review_states(due_at);

-- 复习历史：幂等键与 AI 判定内容
CREATE TABLE IF NOT EXISTS review_records (
    id TEXT PRIMARY KEY NOT NULL,
    card_id TEXT NOT NULL REFERENCES cards(id) ON DELETE CASCADE,
    result TEXT NOT NULL CHECK (result IN ('again', 'hard', 'good')),
    scheduled_due_at INTEGER NOT NULL,
    reviewed_at INTEGER NOT NULL,
    idempotency_key TEXT NOT NULL UNIQUE,
    ai_evaluation_json TEXT NULL
    CHECK (ai_evaluation_json IS NULL OR json_valid(ai_evaluation_json))
);

CREATE INDEX IF NOT EXISTS review_records_card_time_idx ON review_records(card_id, reviewed_at);

-- 笔记索引缓存：由启动扫描与文件监听维护
CREATE TABLE IF NOT EXISTS note_index (
    path TEXT PRIMARY KEY NOT NULL,
    title TEXT NOT NULL,
    tags_json TEXT NOT NULL DEFAULT '[]' CHECK (json_valid(tags_json)),
    body_fts TEXT NOT NULL DEFAULT '',
    mtime INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);

-- 全文搜索
CREATE VIRTUAL TABLE IF NOT EXISTS note_fts USING fts5(path UNINDEXED, title, body);
CREATE VIRTUAL TABLE IF NOT EXISTS cards_fts USING fts5(card_id UNINDEXED, front, back);

-- 供应商：非敏感配置
CREATE TABLE IF NOT EXISTS providers (
    id TEXT PRIMARY KEY NOT NULL,
    name TEXT NOT NULL,
    short_code TEXT NOT NULL,
    protocol TEXT NOT NULL,
    model TEXT NOT NULL,
    base_url TEXT NOT NULL,
    has_api_key INTEGER NOT NULL DEFAULT 0 CHECK (has_api_key IN (0, 1)),
    supports_vision INTEGER NOT NULL DEFAULT 0 CHECK (supports_vision IN (0, 1)),
    status TEXT NOT NULL DEFAULT 'untested' CHECK (status IN ('connected', 'untested')),
    secret_ref TEXT NULL,
    auth_type TEXT NULL CHECK (auth_type IS NULL OR auth_type IN ('api_key', 'openai_oauth')),
    oauth_account_id TEXT NULL,
    provider_type TEXT NOT NULL DEFAULT 'api' CHECK (provider_type IN ('api', 'openai_subscription')),
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);

CREATE UNIQUE INDEX IF NOT EXISTS providers_secret_ref_idx
ON providers(secret_ref) WHERE secret_ref IS NOT NULL;

-- 加密凭据保险库
CREATE TABLE IF NOT EXISTS credential_vault (
    id INTEGER PRIMARY KEY NOT NULL CHECK (id = 1),
    format_version INTEGER NOT NULL CHECK (format_version = 1),
    protection_mode TEXT NOT NULL CHECK (protection_mode IN ('default', 'password')),
    kdf_salt BLOB NOT NULL CHECK (length(kdf_salt) = 16),
    kdf_iterations INTEGER NULL CHECK (
        (protection_mode = 'default' AND kdf_iterations IS NULL)
        OR
        (protection_mode = 'password' AND kdf_iterations BETWEEN 100000 AND 2000000)
    ),
    nonce BLOB NOT NULL CHECK (length(nonce) = 12),
    ciphertext BLOB NOT NULL CHECK (length(ciphertext) BETWEEN 16 AND 1048576),
    updated_at INTEGER NOT NULL
);

-- 全局设置
CREATE TABLE IF NOT EXISTS app_settings (
    key TEXT PRIMARY KEY NOT NULL,
    value TEXT NOT NULL,
    updated_at INTEGER NOT NULL
);

INSERT OR IGNORE INTO providers (
    id, name, short_code, protocol, model, base_url,
    has_api_key, supports_vision, status, created_at, updated_at,
    secret_ref, auth_type, oauth_account_id, provider_type
) VALUES
    ('openai', 'OpenAI', 'OA', 'OpenAI Compatible', 'gpt-4.1-mini',
     'https://api.openai.com/v1', 0, 1, 'untested', unixepoch(), unixepoch(),
     NULL, NULL, NULL, 'api'),
    ('anthropic', 'Anthropic', 'AN', 'Anthropic Messages', 'claude-sonnet-4-5',
     'https://api.anthropic.com', 0, 1, 'untested', unixepoch(), unixepoch(),
     NULL, NULL, NULL, 'api'),
    ('openai_subscription', 'OpenAI 订阅', 'OS', 'OpenAI Compatible',
     'gpt-5.5', 'https://chatgpt.com/backend-api/codex/responses',
     0, 1, 'untested', unixepoch(), unixepoch(),
     NULL, NULL, NULL, 'openai_subscription');

INSERT OR IGNORE INTO app_settings (key, value, updated_at)
VALUES ('active_provider_id', 'openai', unixepoch());

INSERT OR IGNORE INTO app_settings (key, value, updated_at)
VALUES ('font_size', 'comfortable', unixepoch());
