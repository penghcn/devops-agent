//! PostgreSQL 连接池。
//!
//! 启动时初始化连接池并运行迁移。

use sqlx::PgPool;

use crate::config::PgConfig;

pub type DbPool = PgPool;

/// 创建 PostgreSQL 连接池
pub async fn connect(config: &PgConfig) -> anyhow::Result<DbPool> {
    let url = config.connection_url();

    if let Some(ref u) = config.url {
        tracing::info!(url = %redact_url(u), "Connecting to PostgreSQL via direct URL");
    } else {
        tracing::info!(
            host = %config.host,
            port = config.port,
            database = %config.database,
            "Connecting to PostgreSQL"
        );
    }

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

/// 脱敏 URL 日志：只保留 scheme + host:port，隐藏 user/password/db
fn redact_url(url: &str) -> String {
    if let Some(idx) = url.find("://") {
        let scheme = &url[..idx];
        let rest = &url[idx + 3..];
        if let Some(at_idx) = rest.find('@') {
            let hostpart = &rest[at_idx + 1..];
            return format!("{}://****@{}", scheme, hostpart);
        }
    }
    "****".to_string()
}
