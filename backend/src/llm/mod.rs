//! LLM Provider abstraction layer.
//!
//! Core types come from `lellm_core`. Project-specific logic
//! (ModelRouter, LlmProvider trait, PromptBuilder) stays here.

use async_trait::async_trait;

pub mod prompt_builder;
pub mod provider;
pub mod router;
pub mod structured_output;
pub mod tool_use_loop;

// Re-export lellm-core types as the canonical types
pub use lellm_core::{
    CacheControl, ChatRequest, ChatResponse, ContentBlock, ImageSource, Message, TextBlock,
    ThinkingBlock, TokenUsage, ToolCall, ToolChoice, ToolDefinition, text_block,
};

// Re-export lellm-core error
pub use lellm_core::LlmError;

pub use prompt_builder::{MemorySlot, PromptBuilder, PromptBuilderConfig, SessionSlots, StaticPrefix, estimate_tokens};
pub use provider::{LlmConfigSnapshot, LlmConfigStore, ProviderConfig};
pub use router::{ModelRouter, ModelRouterConfig, ProviderModels, TaskLevel};
pub use structured_output::{StructuredOutput, StructuredOutputError};

// ── Provider Trait (project-specific) ──

/// 流式事件
#[derive(Debug, Clone)]
pub enum StreamEvent {
    /// 文本增量
    TextDelta(String),
    /// 思考增量
    ThinkingDelta { thinking: String, redacted: Option<String> },
    /// 工具调用增量
    ToolCallDelta {
        index: usize,
        id: Option<String>,
        name: Option<String>,
        arguments_delta: Option<String>,
    },
    /// Token 使用量
    Usage(TokenUsage),
    /// 流结束
    Done,
}

/// 流式响应类型
pub type ProviderStream = std::pin::Pin<
    Box<dyn futures::Stream<Item = Result<StreamEvent, LlmError>> + Send>,
>;

/// Unified interface for all LLM providers.
#[async_trait]
pub trait LlmProvider: Send + Sync {
    /// Send a chat completion request and return the response.
    async fn llm_call(&self, request: &ChatRequest) -> Result<ChatResponse, LlmError>;

    /// Send a streaming request and return a stream of events.
    async fn stream(&self, request: &ChatRequest) -> Result<ProviderStream, LlmError>;

    /// Return a stable identifier for this provider (e.g. "openai", "anthropic").
    fn provider_id(&self) -> &str;
}

/// Extension trait to extract plain text from ChatResponse content blocks.
pub trait ChatResponseExt {
    /// Concatenate all text blocks into a single String.
    fn text_content(&self) -> String;
}

impl ChatResponseExt for ChatResponse {
    fn text_content(&self) -> String {
        self.content
            .iter()
            .filter_map(|b| b.as_text().map(|s| s.to_string()))
            .collect::<Vec<_>>()
            .join("")
    }
}

/// Extension trait to build ChatResponse from text string.
pub trait ChatResponseBuilder {
    /// Create a ChatResponse with a single text content block.
    fn from_text(text: String, usage: TokenUsage, raw: serde_json::Value) -> Self;
}

impl ChatResponseBuilder for ChatResponse {
    fn from_text(text: String, usage: TokenUsage, raw: serde_json::Value) -> Self {
        ChatResponse::new(vec![ContentBlock::text(text)], usage, raw)
    }
}

/// Convenience: create Message::Assistant with text content only.
pub fn assistant_text(s: String) -> Message {
    Message::assistant(text_block(s))
}

/// Convenience: create Message::Assistant with tool calls.
pub fn assistant_with_tools(content: Vec<ContentBlock>, tool_calls: Vec<ToolCall>) -> Message {
    let mut blocks = content;
    for tc in tool_calls {
        blocks.push(ContentBlock::ToolCall(tc));
    }
    Message::assistant(blocks)
}

/// Extension trait for ToolDefinition to add methods not possible as inherent impl.
pub trait ToolDefinitionExt {
    fn clone_with_cache(&self, cache: CacheControl) -> Self;
    fn cache_breakpoint() -> Self;
}

impl ToolDefinitionExt for ToolDefinition {
    fn clone_with_cache(&self, cache: CacheControl) -> Self {
        Self {
            name: self.name.clone(),
            description: self.description.clone(),
            parameters: self.parameters.clone(),
            cache_control: Some(cache),
        }
    }

    fn cache_breakpoint() -> Self {
        Self {
            name: String::new(),
            description: String::new(),
            parameters: serde_json::json!({}),
            cache_control: Some(CacheControl::Breakpoint),
        }
    }
}
