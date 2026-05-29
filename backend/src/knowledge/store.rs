//! 知识库 PostgreSQL 存储。

use chrono::Utc;
use sqlx::{PgPool, Row};

#[derive(Debug, Clone)]
pub struct KnowledgeEntry {
    pub id: i32,
    pub fingerprint: String,
    pub error_text: String,
    pub solution: String,
    pub embedding: Option<String>,
    pub category: String,
    pub confidence: f32,
    pub hit_count: i32,
    pub confirm_count: i32,
    pub deny_count: i32,
    pub source_build: Option<String>,
    pub created_at: chrono::DateTime<Utc>,
    pub expires_at: chrono::DateTime<Utc>,
}

/// 写入知识条目
pub async fn insert(
    pool: &PgPool,
    fingerprint: &str,
    error_text: &str,
    solution: &str,
    embedding: Option<&str>,
    category: &str,
    source_build: Option<&str>,
) -> anyhow::Result<i32> {
    let mut tx = pool.begin().await?;

    // 检查是否已存在相同指纹
    let existing: Option<i32> = sqlx::query_scalar(
        "SELECT id FROM knowledge_entries WHERE fingerprint = $1 AND confidence > 0.3",
    )
    .bind(fingerprint)
    .fetch_one(&mut *tx)
    .await
    .ok();

    if let Some(id) = existing {
        // 更新置信度
        sqlx::query(
            "UPDATE knowledge_entries SET solution = $1, confirm_count = confirm_count + 1, \
             confidence = LEAST(confidence + 0.1, 1.0), expires_at = now() + INTERVAL '90 days' \
             WHERE id = $2",
        )
        .bind(solution)
        .bind(id)
        .execute(&mut *tx)
        .await?;

        tx.commit().await?;
        return Ok(id);
    }

    // 新条目
    let result = sqlx::query(
        "INSERT INTO knowledge_entries (fingerprint, error_text, solution, embedding, category, confidence, source_build) \
         VALUES ($1, $2, $3, $4, $5, $6, $7) RETURNING id"
    )
    .bind(fingerprint)
    .bind(error_text)
    .bind(solution)
    .bind(embedding)
    .bind(category)
    .bind(0.5_f32)
    .bind(source_build)
    .fetch_one(&mut *tx)
    .await?;

    tx.commit().await?;
    Ok(result.get::<i32, _>(0))
}

/// 按指纹精确查找
pub async fn find_by_fingerprint(pool: &PgPool, fingerprint: &str) -> Option<KnowledgeEntry> {
    let row = sqlx::query_as::<_, KnowledgeRow>(
        "SELECT id, fingerprint, error_text, solution, embedding, category, confidence, \
         hit_count, confirm_count, deny_count, source_build, created_at, expires_at \
         FROM knowledge_entries \
         WHERE fingerprint = $1 AND confidence > 0.3 AND expires_at > now() \
         ORDER BY confidence DESC LIMIT 1",
    )
    .bind(fingerprint)
    .fetch_one(pool)
    .await
    .ok()?;

    Some(row.into())
}

/// 按指纹模糊查找（用于相似错误）
pub async fn find_similar(pool: &PgPool, error_text: &str, limit: i32) -> Vec<KnowledgeEntry> {
    let search_text = format!("%{}%", &error_text.chars().take(50).collect::<String>());

    let rows = sqlx::query_as::<_, KnowledgeRow>(
        "SELECT id, fingerprint, error_text, solution, embedding, category, confidence, \
         hit_count, confirm_count, deny_count, source_build, created_at, expires_at \
         FROM knowledge_entries \
         WHERE error_text LIKE $1 AND confidence > 0.3 AND expires_at > now() \
         ORDER BY confidence DESC LIMIT $2",
    )
    .bind(&search_text)
    .bind(limit)
    .fetch_all(pool)
    .await
    .unwrap_or_default();

    rows.into_iter().map(|r| r.into()).collect()
}

/// 增加命中计数
pub async fn increment_hit(pool: &PgPool, entry_id: i32) {
    let _ = sqlx::query("UPDATE knowledge_entries SET hit_count = hit_count + 1 WHERE id = $1")
        .bind(entry_id)
        .execute(pool)
        .await;
}

/// 确认有效
pub async fn confirm(pool: &PgPool, entry_id: i32) {
    let _ = sqlx::query(
        "UPDATE knowledge_entries SET confirm_count = confirm_count + 1, \
         confidence = LEAST(confidence + 0.2, 1.0), expires_at = now() + INTERVAL '90 days' \
         WHERE id = $1",
    )
    .bind(entry_id)
    .execute(pool)
    .await;
}

/// 标记无效
pub async fn deny(pool: &PgPool, entry_id: i32) {
    let _ = sqlx::query(
        "UPDATE knowledge_entries SET deny_count = deny_count + 1, \
         confidence = GREATEST(confidence - 0.3, 0.0) WHERE id = $1",
    )
    .bind(entry_id)
    .execute(pool)
    .await;
}

/// 获取热门知识条目
pub async fn top_entries(pool: &PgPool, limit: i32) -> Vec<KnowledgeEntry> {
    let rows = sqlx::query_as::<_, KnowledgeRow>(
        "SELECT id, fingerprint, error_text, solution, embedding, category, confidence, \
         hit_count, confirm_count, deny_count, source_build, created_at, expires_at \
         FROM knowledge_entries \
         WHERE confidence > 0.3 AND expires_at > now() \
         ORDER BY hit_count DESC LIMIT $1",
    )
    .bind(limit)
    .fetch_all(pool)
    .await
    .ok();

    rows.map(|r| r.into_iter().map(|r| r.into()).collect())
        .unwrap_or_default()
}

// 内部类型
#[derive(sqlx::FromRow)]
struct KnowledgeRow {
    id: i32,
    fingerprint: String,
    error_text: String,
    solution: String,
    embedding: Option<String>,
    category: String,
    confidence: f32,
    hit_count: i32,
    confirm_count: i32,
    deny_count: i32,
    source_build: Option<String>,
    created_at: chrono::DateTime<chrono::Utc>,
    expires_at: chrono::DateTime<chrono::Utc>,
}

impl From<KnowledgeRow> for KnowledgeEntry {
    fn from(row: KnowledgeRow) -> Self {
        KnowledgeEntry {
            id: row.id,
            fingerprint: row.fingerprint,
            error_text: row.error_text,
            solution: row.solution,
            embedding: row.embedding,
            category: row.category,
            confidence: row.confidence,
            hit_count: row.hit_count,
            confirm_count: row.confirm_count,
            deny_count: row.deny_count,
            source_build: row.source_build,
            created_at: row.created_at,
            expires_at: row.expires_at,
        }
    }
}
