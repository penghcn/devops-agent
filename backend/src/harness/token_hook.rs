use super::{Hook, HookPoint};
use anyhow;
use async_trait::async_trait;
use std::sync::Arc;

use crate::token::TokenTracker;

/// Token 预算追踪 Hook — 在 LlmResult 钩子点检查 Token 预算状态
pub struct TokenHook {
    tracker: Arc<TokenTracker>,
}

impl TokenHook {
    /// 创建新的 TokenHook，持有 TokenTracker 的 Arc 引用
    pub fn new(tracker: Arc<TokenTracker>) -> Self {
        Self { tracker }
    }
}

#[async_trait]
impl Hook for TokenHook {
    async fn on(&self, point: HookPoint) -> anyhow::Result<()> {
        if point == HookPoint::LlmResult && self.tracker.is_exceeded() {
            tracing::warn!(
                "Token budget exceeded: {} / {}",
                self.tracker.usage().total_tokens,
                self.tracker.usage().total_tokens + self.tracker.remaining()
            );
        }
        Ok(())
    }
}
