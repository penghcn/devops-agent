use super::{Hook, HookPoint};
use anyhow;
use async_trait::async_trait;
use std::sync::{Arc, Mutex};

use crate::token::TokenTracker;

/// Token 预算追踪 Hook — 在 LlmResult 钩子点检查 Token 预算状态
pub struct TokenHook {
    tracker: Arc<TokenTracker>,
    budget: u32,
    warned: Arc<Mutex<bool>>,
}

impl TokenHook {
    /// 创建新的 TokenHook，持有 TokenTracker 的 Arc 引用
    pub fn new(tracker: Arc<TokenTracker>) -> Self {
        let budget = tracker.budget();
        Self {
            tracker,
            budget,
            warned: Arc::new(Mutex::new(false)),
        }
    }
}

#[async_trait]
impl Hook for TokenHook {
    async fn on(&self, point: HookPoint) -> anyhow::Result<()> {
        if point == HookPoint::LlmResult && self.tracker.is_exceeded() {
            let mut warned = self.warned.lock().unwrap();
            if !*warned {
                tracing::warn!(
                    "Token budget exceeded: {} / {}",
                    self.tracker.usage().total_tokens,
                    self.budget
                );
                *warned = true;
            }
        }
        Ok(())
    }
}
