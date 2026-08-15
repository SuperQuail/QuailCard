use super::{now_timestamp, Database};
use crate::error::CommandError;

/// 界面字号支持的四种合法档位。
const SUPPORTED_FONT_SIZES: [&str; 4] = ["compact", "standard", "comfortable", "large"];

/// 界面字号缺失或损坏时的默认档位。
const DEFAULT_FONT_SIZE: &str = "comfortable";

impl Database {
    /// 读取持久化的界面字号，缺失或非法时返回默认舒适档。
    pub async fn get_font_size(&self) -> String {
        let stored = sqlx::query_scalar::<_, String>(
            "SELECT value FROM app_settings WHERE key = 'font_size'",
        )
        .fetch_optional(&self.pool)
        .await
        .ok()
        .flatten();
        match stored.as_deref() {
            Some(value) if SUPPORTED_FONT_SIZES.contains(&value) => value.to_string(),
            _ => DEFAULT_FONT_SIZE.to_string(),
        }
    }

    /// 校验并持久化界面字号档位。
    pub async fn set_font_size(&self, font_size: &str) -> Result<(), CommandError> {
        if !SUPPORTED_FONT_SIZES.contains(&font_size) {
            return Err(CommandError::validation("不支持的界面字号档位"));
        }
        sqlx::query(
            "INSERT INTO app_settings (key, value, updated_at) VALUES ('font_size', ?, ?)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at",
        )
        .bind(font_size)
        .bind(now_timestamp())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// 读取"使用问答时启用AI评分"设置，默认关闭。
    pub async fn get_ai_grading_enabled(&self) -> Result<bool, CommandError> {
        let stored = sqlx::query_scalar::<_, String>(
            "SELECT value FROM app_settings WHERE key = 'ai_grading_enabled'",
        )
        .fetch_optional(&self.pool)
        .await?;
        Ok(stored.as_deref() == Some("true"))
    }

    /// 持久化"使用问答时启用AI评分"设置。
    pub async fn set_ai_grading_enabled(&self, enabled: bool) -> Result<(), CommandError> {
        sqlx::query(
            "INSERT INTO app_settings (key, value, updated_at) VALUES ('ai_grading_enabled', ?, ?)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at",
        )
        .bind(if enabled { "true" } else { "false" })
        .bind(now_timestamp())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// 读取历史打开的 Vault 路径列表（最新在前，最多 10 个）。
    pub async fn get_recent_vaults(&self) -> Result<Vec<String>, CommandError> {
        let stored = sqlx::query_scalar::<_, String>(
            "SELECT value FROM app_settings WHERE key = 'recent_vaults'",
        )
        .fetch_optional(&self.pool)
        .await?;
        let list: Vec<String> = stored
            .as_deref()
            .map(|value| serde_json::from_str(value).unwrap_or_default())
            .unwrap_or_default();
        Ok(list)
    }

    /// 记录一次打开的 Vault：去重后移到最前并限制数量。
    pub async fn record_recent_vault(&self, path: &str) -> Result<(), CommandError> {
        let mut list = self.get_recent_vaults().await?;
        list.retain(|item| item != path);
        list.insert(0, path.to_string());
        list.truncate(10);
        let value = serde_json::to_string(&list)
            .map_err(|error| CommandError::new("SERIALIZATION_ERROR", error.to_string()))?;
        sqlx::query(
            "INSERT INTO app_settings (key, value, updated_at) VALUES ('recent_vaults', ?, ?)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at",
        )
        .bind(value)
        .bind(now_timestamp())
        .execute(&self.pool)
        .await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 新数据库应通过迁移获得舒适字号默认值。
    #[tokio::test]
    async fn defaults_to_comfortable_font_size() {
        let database = Database::connect_memory().await.unwrap();

        assert_eq!(database.get_font_size().await, "comfortable");
    }

    /// 四种合法字号均应可写入并重新读取。
    #[tokio::test]
    async fn persists_all_supported_font_sizes() {
        let database = Database::connect_memory().await.unwrap();

        for value in SUPPORTED_FONT_SIZES {
            database.set_font_size(value).await.unwrap();
            assert_eq!(database.get_font_size().await, value);
        }
    }

    /// 非法字号应被拒绝且不改变已保存的值。
    #[tokio::test]
    async fn rejects_invalid_font_size() {
        let database = Database::connect_memory().await.unwrap();
        database.set_font_size("comfortable").await.unwrap();

        let error = database.set_font_size("oversized").await.unwrap_err();

        assert_eq!(error.code, "VALIDATION_ERROR");
        assert_eq!(database.get_font_size().await, "comfortable");
    }
}
