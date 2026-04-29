pub mod long_term;
pub mod short_term;
pub mod store;

pub use long_term::LongTermMemory;
pub use short_term::ShortTermMemory;
pub use store::MemoryStore;

use chrono::DateTime;
use chrono::Utc;

/// 记忆条目的数据类型
#[derive(Debug, Clone, PartialEq)]
pub enum MemoryType {
    ToolCall,
    ToolResult,
    LlmResponse,
    UserInput,
    Decision,
    Summary,
}

/// 记忆条目
#[derive(Debug, Clone)]
pub struct MemoryEntry {
    pub id: u64,
    pub content: String,
    pub r#type: MemoryType,
    pub timestamp: DateTime<Utc>,
    pub score: f32,
}
