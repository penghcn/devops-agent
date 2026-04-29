use super::{MemoryEntry, MemoryType};

/// 短期记忆环形缓冲区，默认 200 条容量
#[derive(Debug)]
pub struct ShortTermMemory {
    buffer: Vec<MemoryEntry>,
    capacity: usize,
    next_id: u64,
}

impl ShortTermMemory {
    /// 创建指定容量的短期记忆缓冲区
    pub fn new(capacity: usize) -> Self {
        Self {
            buffer: Vec::new(),
            capacity,
            next_id: 1,
        }
    }

    /// 添加记忆条目，满时淘汰最旧条目（FIFO）
    pub fn add(&mut self, content: String, r#type: MemoryType) {
        if self.buffer.len() >= self.capacity {
            self.buffer.remove(0);
        }

        let entry = MemoryEntry {
            id: self.next_id,
            content,
            r#type,
            timestamp: chrono::Utc::now(),
            score: 1.0,
        };

        self.next_id += 1;
        self.buffer.push(entry);
    }

    /// 返回最近 n 条记忆
    pub fn recent(&self, n: usize) -> &[MemoryEntry] {
        if n >= self.buffer.len() {
            &self.buffer
        } else {
            &self.buffer[self.buffer.len() - n..]
        }
    }

    /// 返回所有记忆条目
    pub fn entries(&self) -> &[MemoryEntry] {
        &self.buffer
    }

    /// 返回当前条目数量
    pub fn len(&self) -> usize {
        self.buffer.len()
    }

    /// 判断是否为空
    pub fn is_empty(&self) -> bool {
        self.buffer.is_empty()
    }

    /// 清空所有记忆
    pub fn clear(&mut self) {
        self.buffer.clear();
    }
}

impl Default for ShortTermMemory {
    fn default() -> Self {
        Self::new(200)
    }
}
