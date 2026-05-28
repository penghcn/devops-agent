//! 数据库迁移。
//!
//! 启动时自动运行 CREATE TABLE IF NOT EXISTS。

use sqlx::PgPool;

use crate::db::DbPool;

/// 运行所有迁移
pub async fn run_migrations(pool: &DbPool) -> anyhow::Result<()> {
    create_users_table(pool).await?;
    create_project_access_table(pool).await?;
    create_knowledge_table(pool).await?;
    create_stats_tables(pool).await?;

    tracing::info!("Database migrations completed");
    Ok(())
}

async fn create_users_table(pool: &PgPool) -> anyhow::Result<()> {
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS users ( \
         id SERIAL PRIMARY KEY, \
         username TEXT UNIQUE NOT NULL, \
         gitlab_id TEXT UNIQUE NOT NULL, \
         avatar_url TEXT, \
         role TEXT NOT NULL DEFAULT 'user', \
         refresh_token TEXT UNIQUE, \
         token_expires_at TIMESTAMP, \
         created_at TIMESTAMP NOT NULL DEFAULT now() \
        )",
    )
    .execute(pool)
    .await
    .map_err(|e| anyhow::anyhow!("users table migration failed: {}", e))?;
    Ok(())
}

async fn create_project_access_table(pool: &PgPool) -> anyhow::Result<()> {
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS user_project_access ( \
         user_id INTEGER NOT NULL REFERENCES users(id), \
         project TEXT NOT NULL, \
         PRIMARY KEY (user_id, project) \
        )",
    )
    .execute(pool)
    .await
    .map_err(|e| anyhow::anyhow!("user_project_access table migration failed: {}", e))?;
    Ok(())
}

async fn create_knowledge_table(pool: &PgPool) -> anyhow::Result<()> {
    // 注意：pg_vector 需要用户在数据库中手动安装：CREATE EXTENSION IF NOT EXISTS vector;
    // 这里先用 TEXT 存储 embedding JSON，后续接入 pg-vec 再改
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS knowledge_entries ( \
         id SERIAL PRIMARY KEY, \
         fingerprint TEXT NOT NULL, \
         error_text TEXT NOT NULL, \
         solution TEXT NOT NULL, \
         embedding TEXT, \
         category TEXT NOT NULL DEFAULT 'other', \
         confidence REAL NOT NULL DEFAULT 0.5, \
         hit_count INTEGER NOT NULL DEFAULT 0, \
         confirm_count INTEGER NOT NULL DEFAULT 0, \
         deny_count INTEGER NOT NULL DEFAULT 0, \
         source_build TEXT, \
         created_at TIMESTAMP NOT NULL DEFAULT now(), \
         expires_at TIMESTAMP NOT NULL DEFAULT (now() + INTERVAL '30 days') \
        )",
    )
    .execute(pool)
    .await
    .map_err(|e| anyhow::anyhow!("knowledge_entries table migration failed: {}", e))?;

    sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_knowledge_fingerprint ON knowledge_entries(fingerprint)",
    )
    .execute(pool)
    .await
    .ok();

    Ok(())
}

async fn create_stats_tables(pool: &PgPool) -> anyhow::Result<()> {
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS stats_hourly ( \
         hour TIMESTAMP NOT NULL, \
         project_name TEXT NOT NULL, \
         total_builds INTEGER NOT NULL DEFAULT 0, \
         failed_builds INTEGER NOT NULL DEFAULT 0, \
         error_category TEXT NOT NULL DEFAULT 'other', \
         avg_duration REAL, \
         PRIMARY KEY (hour, project_name, error_category) \
        )",
    )
    .execute(pool)
    .await
    .map_err(|e| anyhow::anyhow!("stats_hourly table migration failed: {}", e))?;

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS stats_daily ( \
         day DATE NOT NULL, \
         project_name TEXT NOT NULL, \
         total_builds INTEGER NOT NULL DEFAULT 0, \
         failed_builds INTEGER NOT NULL DEFAULT 0, \
         success_rate REAL, \
         PRIMARY KEY (day, project_name) \
        )",
    )
    .execute(pool)
    .await
    .map_err(|e| anyhow::anyhow!("stats_daily table migration failed: {}", e))?;

    Ok(())
}
