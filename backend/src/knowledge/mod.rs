//! 知识库模块。
//!
//! 两层检索：指纹哈希精确匹配 + 远程 Embedding 语义检索。

pub mod embedding;
pub mod fingerprint;
pub mod learner;
pub mod retriever;
pub mod store;

pub use learner::KnowledgeLearner;
pub use retriever::{KnowledgeRetriever, SearchHit, SearchSource};
pub use store::KnowledgeEntry;
