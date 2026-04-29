use anyhow;
use async_trait::async_trait;

/// 钩子点枚举 — 定义 Harness 生命周期中的所有扩展点
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum HookPoint {
    SessionStart,
    SessionEnd,
    StepStart,
    StepEnd,
    ToolCalled,
    ToolResult,
    LlmCalled,
    LlmResult,
    TokenBudgetExceeded,
    MemorySave,
    DecisionMade, // 为下轮 DecisionStep 预留
}

/// Hook trait — 任何结构体实现此 trait 即可接入 Harness 生命周期
#[async_trait]
pub trait Hook: Send + Sync {
    async fn on(&self, point: HookPoint) -> anyhow::Result<()>;
}
