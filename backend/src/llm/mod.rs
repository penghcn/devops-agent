//! LLM Provider abstraction layer.
//!
//! Defines a unified `LlmProvider` trait and shared data types so that
//! different LLM backends (OpenAI, Anthropic, etc.) can be swapped
//! without changing caller code.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

pub mod prompt_builder;
pub mod provider;
pub mod router;
pub mod structured_output;
pub mod tool_use_loop;

pub use prompt_builder::{
    MemorySlot, PromptBuilder, PromptBuilderConfig, SessionSlots, StaticPrefix, estimate_tokens,
    text_block,
};
pub use provider::{
    AnthropicAdapter, AnthropicProvider, LlmConfigSnapshot, LlmConfigStore, OpenAIAdapter,
    OpenAIProvider, ProviderConfig,
};
pub use router::{ModelRouter, ModelRouterConfig, ProviderModels, TaskLevel};
pub use structured_output::{StructuredOutput, StructuredOutputError};

// ── Provider Trait ──

/// Unified interface for all LLM providers.
///
/// Implementors handle API-specific request/response translation
/// and HTTP communication. Callers work exclusively with this trait.
#[async_trait]
pub trait LlmProvider: Send + Sync {
    /// Send a chat completion request and return the response.
    async fn llm_call(&self, request: &ChatRequest) -> Result<ChatResponse, LlmError>;

    /// Return a stable identifier for this provider (e.g. "openai", "anthropic").
    fn provider_id(&self) -> &str;
}

// ── Content Block Types ──

/// 缓存控制标记（Anthropic prompt caching）
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum CacheControl {
    #[serde(rename = "ephemeral_buffer")]
    EphemeralBuffer,
}

/// 图片资源
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ImageSource {
    /// base64 编码的图片数据
    pub data: String,
    /// MIME 类型，如 "image/png"
    pub media_type: String,
}

/// 内容块——Message 的基本组成单元
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentBlock {
    /// 文本块，可带缓存控制标记
    Text {
        text: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        cache_control: Option<CacheControl>,
    },
    /// 图片块
    Image { source: ImageSource },
}

impl ContentBlock {
    /// 创建纯文本块（无缓存标记）
    pub fn text(s: String) -> Self {
        ContentBlock::Text {
            text: s,
            cache_control: None,
        }
    }

    /// 创建带缓存标记的文本块
    pub fn text_with_cache(s: String, cache: CacheControl) -> Self {
        ContentBlock::Text {
            text: s,
            cache_control: Some(cache),
        }
    }

    /// 获取文本内容（仅 Text 变体）
    pub fn as_text(&self) -> Option<&str> {
        match self {
            ContentBlock::Text { text, .. } => Some(text),
            _ => None,
        }
    }
}

// ── Message Types ──

/// A single message in a conversation turn.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Message {
    System {
        content: Vec<ContentBlock>,
    },
    User {
        content: Vec<ContentBlock>,
    },
    Assistant {
        content: Vec<ContentBlock>,
        tool_calls: Vec<ToolCall>,
    },
    /// Result of a tool execution, fed back to the LLM.
    ToolResult {
        tool_call_id: String,
        content: Vec<ContentBlock>,
    },
}

impl Message {
    /// 提取所有文本块拼接为字符串（provider 序列化用）
    pub fn extract_text(&self) -> String {
        match self {
            Message::System { content } => Self::join_text(content),
            Message::User { content } => Self::join_text(content),
            Message::Assistant { content, .. } => Self::join_text(content),
            Message::ToolResult { content, .. } => Self::join_text(content),
        }
    }

    fn join_text(blocks: &[ContentBlock]) -> String {
        blocks
            .iter()
            .filter_map(|b| b.as_text().map(|s| s.to_string()))
            .collect::<Vec<_>>()
            .join("")
    }
}

// ── Request Types ──

/// Unified chat request sent to any provider.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatRequest {
    pub model: String,
    pub messages: Vec<Message>,
    pub tools: Option<Vec<ToolDefinition>>,
    pub temperature: Option<f64>,
    /// 强制选择特定工具（Tool Use 结构化输出模式）
    pub tool_choice: Option<ToolChoice>,
    /// 停止序列（Anthropic stop_sequences / OpenAI stop）
    pub stop_sequences: Option<Vec<String>>,
    /// Prefill 预填起始内容（如 "{" 用于 JSON 输出）
    pub prefill: Option<String>,
}

impl Default for ChatRequest {
    fn default() -> Self {
        Self {
            model: String::new(),
            messages: Vec::new(),
            tools: None,
            temperature: Some(0.6),
            tool_choice: None,
            stop_sequences: None,
            prefill: None,
        }
    }
}

impl ChatRequest {
    /// 便捷构造：单条用户消息
    pub fn user_prompt(prompt: String) -> Self {
        Self {
            messages: vec![Message::User {
                content: text_block(prompt),
            }],
            ..Default::default()
        }
    }

    /// 设置温度
    pub fn with_temperature(mut self, temp: f64) -> Self {
        self.temperature = Some(temp);
        self
    }

    /// 设置模型
    pub fn with_model(mut self, model: String) -> Self {
        self.model = model;
        self
    }
}

/// 工具选择策略
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ToolChoice {
    /// 强制调用指定名称的工具
    Tool { name: String },
    /// 必须调用任意一个工具
    Any,
}

/// Unified tool definition (input side).  Providers translate this to
/// their own format (OpenAI function calling, Anthropic tools, etc.).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
    /// 缓存控制标记。当设为 `Some` 时，该工具之后插入缓存断点。
    /// 由 PromptBuilder 在工具分组合并时自动设置。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_control: Option<CacheControl>,
}

// ── Response Types ──

/// Unified chat response received from any provider.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatResponse {
    pub content: String,
    pub tool_calls: Vec<ToolCall>,
    pub usage: TokenUsage,
    /// Raw JSON response from the provider (for debugging / introspection).
    pub raw: serde_json::Value,
}

impl ChatResponse {
    /// Returns true if the response contains tool calls.
    pub fn has_tool_calls(&self) -> bool {
        !self.tool_calls.is_empty()
    }
}

/// A tool invocation requested by the model.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: serde_json::Value,
}

/// Token consumption breakdown.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub struct TokenUsage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
}

// ── Error Types ──

/// Errors that can occur during LLM API calls.
#[derive(Debug)]
pub enum LlmError {
    /// HTTP API returned an error status.
    ApiError { status: u16, body: String },
    /// Request timed out.
    Timeout,
    /// Failed to parse the provider response.
    ParseError { detail: String },
    /// Requested model not found on the provider.
    NotFound { model: String },
    /// API key was not configured.
    MissingApiKey { provider: String },
}

impl std::fmt::Display for LlmError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LlmError::ApiError { status, body } => {
                // T-03-01: Truncate body to 200 chars to avoid leaking API internals
                let limit = body
                    .char_indices()
                    .nth(200)
                    .map(|(i, _)| i)
                    .unwrap_or(body.len());
                let truncated = &body[..limit];
                write!(f, "API error {}: {}", status, truncated)
            }
            LlmError::Timeout => write!(f, "LLM API request timed out"),
            LlmError::ParseError { detail } => {
                write!(f, "Failed to parse LLM response: {}", detail)
            }
            LlmError::NotFound { model } => write!(f, "Model not found: {}", model),
            LlmError::MissingApiKey { provider } => {
                write!(f, "Missing API key for provider: {}", provider)
            }
        }
    }
}

impl std::error::Error for LlmError {}
