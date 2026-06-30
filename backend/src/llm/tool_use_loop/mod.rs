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
    ChatRequest, ChatResponse, LlmError, LlmProvider, Message, StreamEvent, ToolCall,
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

    /// 执行 tool-use 循环（非流式）。
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

    /// 执行 tool-use 循环（流式）。
    ///
    /// 流式模式下使用 stream 获取 token 流，实时转发事件，并在需要时执行工具。
    pub async fn execute_streaming(
        self,
        event_tx: Arc<tokio::sync::mpsc::Sender<StreamEvent>>,
    ) -> Result<ToolUseResult, LlmError> {
        use futures::StreamExt;

        let mut req = self.request;

        for iteration in 1..=self.max_iterations {
            let mut stream = self.provider.stream(&req).await?;

            // Accumulate response from stream
            let mut content_text = String::new();
            let mut tool_calls: Vec<ToolCall> = Vec::new();
            let mut usage = crate::llm::TokenUsage::default();

            while let Some(event) = stream.next().await {
                let event = event?;
                match event {
                    StreamEvent::TextDelta(token) => {
                        content_text.push_str(&token);
                        // Forward to SSE
                        let _ = event_tx.send(StreamEvent::TextDelta(token)).await;
                    }
                    StreamEvent::ThinkingDelta { thinking, redacted } => {
                        let _ = event_tx
                            .send(StreamEvent::ThinkingDelta { thinking, redacted })
                            .await;
                    }
                    StreamEvent::ToolCallDelta {
                        index,
                        id,
                        name,
                        arguments_delta,
                    } => {
                        // Accumulate tool calls
                        while tool_calls.len() <= index {
                            tool_calls.push(ToolCall {
                                id: String::new(),
                                name: String::new(),
                                arguments: serde_json::json!({}),
                            });
                        }
                        if let Some(id) = id {
                            tool_calls[index].id = id;
                        }
                        if let Some(name) = name {
                            tool_calls[index].name = name;
                        }
                        if let Some(delta) = arguments_delta {
                            // Append to existing arguments string
                            if let serde_json::Value::String(ref mut s) = tool_calls[index].arguments
                            {
                                s.push_str(&delta);
                            } else {
                                tool_calls[index].arguments =
                                    serde_json::Value::String(delta);
                            }
                        }
                        let _ = event_tx
                            .send(StreamEvent::ToolCallDelta {
                                index,
                                id: tool_calls[index].id.clone().into(),
                                name: tool_calls[index].name.clone().into(),
                                arguments_delta: None,
                            })
                            .await;
                    }
                    StreamEvent::Usage(u) => {
                        usage = u;
                    }
                    StreamEvent::Done => break,
                }
            }

            // Parse tool call arguments (they were accumulated as strings)
            for tc in &mut tool_calls {
                if let serde_json::Value::String(ref s) = tc.arguments {
                    if let Ok(parsed) = serde_json::from_str(s) {
                        tc.arguments = parsed;
                    }
                }
            }

            // Build content blocks
            let mut content_blocks = Vec::new();
            if !content_text.is_empty() {
                content_blocks.push(crate::llm::ContentBlock::text(content_text));
            }
            for tc in &tool_calls {
                content_blocks.push(crate::llm::ContentBlock::ToolCall(tc.clone()));
            }

            let response = ChatResponse::new(content_blocks, usage, serde_json::json!({}));

            if tool_calls.is_empty() {
                // No tool calls — return final result
                let _ = event_tx.send(StreamEvent::Done).await;
                return Ok(ToolUseResult {
                    response,
                    messages: req.messages,
                    iterations: iteration,
                });
            }

            // Has tool calls — add assistant message and execute tools
            req.messages
                .push(assistant_with_tools(response.content.clone(), tool_calls.clone()));

            let tool_results = self.executor.execute_batch(&tool_calls).await;
            req.messages.extend(tool_results);

            tracing::debug!(
                iteration,
                tool_calls = tool_calls.len(),
                "tool-use loop streaming iteration"
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
