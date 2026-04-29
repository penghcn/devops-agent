use super::{Hook, HookPoint};
use anyhow;
use async_trait::async_trait;
use std::sync::{Arc, Mutex};

use crate::memory::{MemoryType, ShortTermMemory};

/// 记忆保存 Hook — 在 SessionStart、StepStart、StepEnd 等钩子点自动保存记忆
pub struct MemoryHook {
    memory: Arc<Mutex<ShortTermMemory>>,
}

impl MemoryHook {
    /// 创建新的 MemoryHook，将 ShortTermMemory 包装为 Arc<Mutex<>>
    pub fn new(memory: ShortTermMemory) -> Self {
        Self {
            memory: Arc::new(Mutex::new(memory)),
        }
    }

    /// 返回内部记忆的共享引用，供外部验证
    pub fn memory(&self) -> &Arc<Mutex<ShortTermMemory>> {
        &self.memory
    }
}

#[async_trait]
impl Hook for MemoryHook {
    async fn on(&self, point: HookPoint) -> anyhow::Result<()> {
        let mut memory = self.memory.lock().unwrap();

        match point {
            HookPoint::SessionStart => {
                memory.add("session started".to_string(), MemoryType::Decision);
            }
            HookPoint::StepStart => {
                memory.add("step started".to_string(), MemoryType::ToolCall);
            }
            HookPoint::StepEnd => {
                memory.add("step ended".to_string(), MemoryType::ToolResult);
            }
            _ => {}
        }

        Ok(())
    }
}
