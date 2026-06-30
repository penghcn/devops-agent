//! ToolUseLoop — LLM ↔ 工具调用闭环

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

use crate::llm::{
    ChatRequest, ChatResponse, LlmError, LlmProvider, Message, ToolCall,
    assistant_with_tools,
};

/// 工具执行结果
#[derive(Debug, Clone)]
pub enum ToolCallResult {
    Ok(String),
    Err(String),
}

/// ToolUseLoop 执行结果
#[derive(Debug)]
pub struct ToolUseResult {
    pub response: ChatResponse,
    pub messages: Vec<Message>,
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
        let mut req = self.request;

        for iteration in 1..=self.max_iterations {
            let response = self.provider.llm_call(&req).await?;

            if !response.has_tool_calls() {
                return Ok(ToolUseResult {
                    response,
                    messages: req.messages,
                    iterations: iteration,
                });
            }

            // Extract tool calls from response content blocks
            let tool_calls: Vec<ToolCall> = response.tool_calls().cloned().collect();

            // Build Assistant message with content + tool calls
            req.messages
                .push(assistant_with_tools(response.content.clone(), tool_calls.clone()));

            let tool_results = self.executor.execute_batch(&tool_calls).await;
            req.messages.extend(tool_results);

            tracing::debug!(
                iteration,
                tool_calls = tool_calls.len(),
                "tool-use loop iteration"
            );
        }

        Err(LlmError::Provider {
            provider: "tool_use_loop".to_string(),
            status: None,
            code: None,
            message: format!("tool-use 循环超过最大轮次限制 ({})", self.max_iterations),
        })
    }
}
