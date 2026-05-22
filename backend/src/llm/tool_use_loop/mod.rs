//! ToolUseLoop — LLM ↔ 工具调用闭环
//!
//! 负责 LLM 返回 tool_calls → 执行工具 → 结果注入 → 再次调用 LLM 的循环，
//! 直到 LLM 返回纯文本或达到最大轮次。

mod dag;
mod executor;
mod fallback;
mod loop_detector;
mod retry;
mod signal_voter;
mod tool_registry;

pub use dag::{DagNode, DagOrchestrator, DagResult};
pub use executor::{ParallelSafety, ToolExecutor, ToolRegistration};
pub use fallback::{FallbackHandler, FallbackReason, FallbackResult};
pub use loop_detector::{LoopDetector, LoopIntervention};
pub use retry::{RetryPolicy, ToolErrorKind};
pub use signal_voter::{NegativeSignal, SignalVoter};
pub use tool_registry::{ToolRegistry, ToolSearchResult, ToolSource};

use std::sync::Arc;

use crate::llm::{ChatRequest, ChatResponse, LlmError, LlmProvider, Message};

/// 工具执行结果
#[derive(Debug, Clone)]
pub enum ToolCallResult {
    Ok(String),
    Err(String),
}

/// ToolUseLoop 执行结果
#[derive(Debug)]
pub struct ToolUseResult {
    /// 最终 LLM 响应（纯文本）
    pub response: ChatResponse,
    /// 完整消息历史（包含所有中间 tool_calls 和 tool_results）
    pub messages: Vec<Message>,
    /// 实际循环轮次
    pub iterations: usize,
}

/// 管理 LLM ↔ 工具调用闭环
pub struct ToolUseLoop {
    provider: Arc<dyn LlmProvider>,
    executor: ToolExecutor,
    request: ChatRequest,
    max_iterations: usize,
}

impl ToolUseLoop {
    pub fn new(
        provider: Arc<dyn LlmProvider>,
        executor: ToolExecutor,
        request: ChatRequest,
    ) -> Self {
        Self {
            provider,
            executor,
            request,
            max_iterations: 15,
        }
    }

    pub fn set_max_iterations(&mut self, max: usize) -> &mut Self {
        self.max_iterations = max;
        self
    }

    /// 执行 tool-use 循环。
    pub async fn execute(self) -> Result<ToolUseResult, LlmError> {
        let mut messages = self.request.messages.clone();

        for iteration in 1..=self.max_iterations {
            let req = ChatRequest {
                model: self.request.model.clone(),
                messages: messages.clone(),
                tools: self.request.tools.clone(),
                temperature: self.request.temperature,
                tool_choice: self.request.tool_choice.clone(),
                stop_sequences: self.request.stop_sequences.clone(),
                prefill: self.request.prefill.clone(),
            };

            let response = self.provider.llm_call(&req).await?;

            if !response.has_tool_calls() {
                return Ok(ToolUseResult {
                    response,
                    messages,
                    iterations: iteration,
                });
            }

            messages.push(Message::Assistant {
                content: response.content.clone(),
                tool_calls: response.tool_calls.clone(),
            });

            let tool_results = self.executor.execute_batch(&response.tool_calls).await;
            messages.extend(tool_results);

            tracing::debug!(
                iteration,
                tool_calls = response.tool_calls.len(),
                "tool-use loop iteration"
            );
        }

        Err(LlmError::ApiError {
            status: 0,
            body: format!("tool-use 循环超过最大轮次限制 ({})", self.max_iterations),
        })
    }
}
