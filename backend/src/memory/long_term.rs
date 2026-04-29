use super::MemoryType;
use super::store::MemoryStore;
use anyhow::Result;

/// 长期记忆，封装 SQLite 持久化
#[derive(Debug)]
pub struct LongTermMemory {
    store: MemoryStore,
}

impl LongTermMemory {
    /// 创建长期记忆实例，连接到指定路径的 SQLite 数据库
    pub fn new(path: &str) -> Result<Self> {
        let store = MemoryStore::new(path)?;
        Ok(Self { store })
    }

    /// 保存记忆条目到长期存储
    pub fn save(
        &mut self,
        content: &str,
        _type: MemoryType,
        keywords: &[&str],
        score: f64,
    ) -> Result<()> {
        let type_str = match _type {
            MemoryType::ToolCall => "ToolCall",
            MemoryType::ToolResult => "ToolResult",
            MemoryType::LlmResponse => "LlmResponse",
            MemoryType::UserInput => "UserInput",
            MemoryType::Decision => "Decision",
            MemoryType::Summary => "Summary",
        };

        self.store.insert(content, type_str, keywords, score)?;
        Ok(())
    }

    /// 按关键词检索长期记忆
    pub fn retrieve(&self, keyword: &str) -> Result<Vec<String>> {
        Ok(self.store.search(keyword)?)
    }
}
