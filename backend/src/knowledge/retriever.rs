//! 两层检索器。
//!
//! 第一层：指纹哈希精确匹配（O(1)）
//! 第二层：远程 Embedding + pg-vec 向量余弦相似度检索

use std::sync::Arc;

use sqlx::PgPool;

use super::embedding;
use super::fingerprint;
use super::store;

/// 知识库检索结果
#[derive(Debug, Clone)]
pub struct SearchHit {
    pub entry_id: i32,
    pub solution: String,
    pub confidence: f32,
    pub category: String,
    pub source: SearchSource,
}

#[derive(Debug, Clone, PartialEq)]
pub enum SearchSource {
    /// 指纹精确匹配
    ExactFingerprint,
    /// 向量语义匹配
    EmbeddingSimilar,
}

impl SearchHit {
    /// 是否已验证（置信度 >= 0.7）
    pub fn is_verified(&self) -> bool {
        self.confidence >= 0.7
    }
}

/// 知识库检索器
pub struct KnowledgeRetriever {
    pool: PgPool,
    embedding_api_key: String,
    http_client: Arc<reqwest::Client>,
}

impl KnowledgeRetriever {
    pub fn new(pool: PgPool, embedding_api_key: String) -> Self {
        Self {
            pool,
            embedding_api_key,
            http_client: Arc::new(reqwest::Client::new()),
        }
    }

    /// 搜索知识库
    ///
    /// 1. 提取指纹 → 精确匹配
    /// 2. 未命中 → Embedding 向量检索（300ms 超时）
    /// 3. 返回最佳结果
    pub async fn search(&self, build_log: &str) -> Option<SearchHit> {
        // 第一层：指纹精确匹配
        let fingerprint = fingerprint::extract_fingerprint(build_log);
        if let Some(entry) = store::find_by_fingerprint(&self.pool, &fingerprint).await {
            store::increment_hit(&self.pool, entry.id).await;
            return Some(SearchHit {
                entry_id: entry.id,
                solution: entry.solution,
                confidence: entry.confidence,
                category: entry.category,
                source: SearchSource::ExactFingerprint,
            });
        }

        // 第二层：Embedding 向量检索（300ms 超时）
        let embedding = match tokio::time::timeout(
            std::time::Duration::from_millis(300),
            embedding::get_embedding(
                &self.http_client,
                build_log,
                &self.embedding_api_key,
            ),
        )
        .await
        {
            Ok(Some(emb)) => Some(emb),
            Ok(None) => {
                // Embedding API 返回 None（网络错误或解析失败）
                None
            }
            Err(_) => {
                tracing::debug!("Embedding API timeout (300ms)");
                None
            }
        };

        if let Some(emb) = embedding {
            let entries = store::find_similar(&self.pool, &emb, 3).await;
            if let Some(best) = entries.first() {
                store::increment_hit(&self.pool, best.id).await;
                return Some(SearchHit {
                    entry_id: best.id,
                    solution: best.solution.clone(),
                    confidence: best.confidence,
                    category: best.category.clone(),
                    source: SearchSource::EmbeddingSimilar,
                });
            }
        }

        None
    }

    /// 获取错误分类（不写入知识库，仅分类）
    pub fn classify(&self, build_log: &str) -> String {
        fingerprint::classify_error(build_log)
    }
}
