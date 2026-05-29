//! 知识写入（用户反馈驱动）。
//!
//! 只有用户点赞后才写入知识库。

use sqlx::PgPool;

use super::embedding;
use super::fingerprint;
use super::store;

/// 知识库学习者
pub struct KnowledgeLearner {
    pool: PgPool,
    embedding_api_key: String,
}

impl KnowledgeLearner {
    pub fn new(pool: PgPool, embedding_api_key: String) -> Self {
        Self {
            pool,
            embedding_api_key,
        }
    }

    /// 用户确认方案有效 → 写入知识库
    pub async fn on_confirm(
        &self,
        build_log: &str,
        solution: &str,
        source_build: Option<&str>,
    ) -> anyhow::Result<()> {
        let fingerprint = fingerprint::extract_fingerprint(build_log);
        let category = fingerprint::classify_error(build_log);

        // 尝试获取 Embedding
        let embedding_result = if !self.embedding_api_key.is_empty() {
            let client = reqwest::Client::new();
            embedding::get_embedding(&client, build_log, &self.embedding_api_key).await
        } else {
            None
        };

        store::insert(
            &self.pool,
            &fingerprint,
            build_log,
            solution,
            embedding_result.as_deref(),
            &category,
            source_build,
        )
        .await?;

        tracing::info!(
            fingerprint = &fingerprint,
            category = &category,
            "Knowledge entry saved"
        );

        Ok(())
    }

    /// 用户反馈：标记已有条目为有效
    pub async fn confirm_entry(&self, entry_id: i32) {
        store::confirm(&self.pool, entry_id).await;
    }

    /// 用户反馈：标记已有条目为无效
    pub async fn deny_entry(&self, entry_id: i32) {
        store::deny(&self.pool, entry_id).await;
    }
}
