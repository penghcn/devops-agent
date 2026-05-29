//! 两层检索器。
//!
//! 第一层：指纹哈希精确匹配（O(1)）
//! 第二层：模糊文本相似度（LIKE 查询）

use sqlx::PgPool;

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
    /// 模糊文本匹配
    SimilarText,
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
}

impl KnowledgeRetriever {
    pub fn new(pool: PgPool, embedding_api_key: String) -> Self {
        Self {
            pool,
            embedding_api_key,
        }
    }

    /// 搜索知识库
    ///
    /// 1. 提取指纹 → 精确匹配
    /// 2. 未命中 → 模糊文本匹配
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

        // 第二层：模糊文本匹配
        let entries = store::find_similar(&self.pool, build_log, 3).await;
        if let Some(best) = entries.first() {
            store::increment_hit(&self.pool, best.id).await;
            return Some(SearchHit {
                entry_id: best.id,
                solution: best.solution.clone(),
                confidence: best.confidence,
                category: best.category.clone(),
                source: SearchSource::SimilarText,
            });
        }

        None
    }

    /// 获取错误分类（不写入知识库，仅分类）
    pub fn classify(&self, build_log: &str) -> String {
        fingerprint::classify_error(build_log)
    }
}
