mod cards;
mod helpers;
mod notes;
mod providers;
mod review;
mod search;
mod settings;
mod stats;
mod vault;

use std::path::Path;

use sqlx::{sqlite::SqliteConnectOptions, sqlite::SqlitePoolOptions, SqlitePool};

use crate::error::CommandError;

/// 封装应用唯一的 SQLite 连接池。
#[derive(Clone)]
pub struct Database {
    pub(crate) pool: SqlitePool,
}

impl Database {
    /// 创建数据库文件、连接池并执行全部迁移。
    ///
    /// 不包含任何静默删除旧库的逻辑；旧版 schema 应由显式迁移或备份流程处理。
    pub async fn connect(path: &Path) -> Result<Self, CommandError> {
        let options = SqliteConnectOptions::new()
            .filename(path)
            .create_if_missing(true)
            .foreign_keys(true);
        let pool = SqlitePoolOptions::new()
            .max_connections(5)
            .connect_with(options)
            .await?;
        sqlx::migrate!("./migrations").run(&pool).await?;
        Ok(Self { pool })
    }

    #[cfg(test)]
    /// 创建使用单连接内存数据库的测试实例。
    pub async fn connect_memory() -> Result<Self, CommandError> {
        let options = SqliteConnectOptions::new()
            .filename(":memory:")
            .foreign_keys(true);
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(options)
            .await?;
        sqlx::migrate!("./migrations").run(&pool).await?;
        Ok(Self { pool })
    }
}

/// 返回当前 UTC Unix 秒数。
pub(crate) fn now_timestamp() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}
