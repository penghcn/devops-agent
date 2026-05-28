//! PostgreSQL 连接池。
//!
//! 启动时初始化连接池并运行迁移。

use sqlx::PgPool;

use crate::config::PgConfig;

pub type DbPool = PgPool;

/// 创建 PostgreSQL 连接池
pub async fn connect(config: &PgConfig) -> anyhow::Result<DbPool> {
    let url = config.connection_url();
    tracing::info!(
        host = %config.host,
        port = config.port,
        database = %config.database,
        "Connecting to PostgreSQL"
    );

    let pool = PgPool::connect(&url)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to connect PostgreSQL: {}", e))?;

    Ok(pool)
}

/// 健康检查
pub async fn health_check(pool: &DbPool) -> anyhow::Result<()> {
    sqlx::query("SELECT 1")
        .execute(pool)
        .await
        .map(|_| ())
        .map_err(|e| anyhow::anyhow!("PostgreSQL health check failed: {}", e))
}
