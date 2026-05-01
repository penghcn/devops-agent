//! LLM Provider abstraction layer.
//!
//! Defines a unified `LlmProvider` trait and shared data types so that
//! different LLM backends (OpenAI, Anthropic, etc.) can be swapped
//! without changing caller code.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

pub mod anthropic_provider;
pub mod config_store;
pub mod openai_provider;
pub mod router;
pub mod structured_output;

pub use anthropic_provider::{AnthropicConfig, AnthropicProvider};
pub use config_store::{LlmConfigSnapshot, LlmConfigStore, LlmConfigUpdate, ProviderConfig};
pub use openai_provider::{OpenAIConfig, OpenAIProvider};
pub use router::{ModelRouter, ModelRouterConfig, TaskLevel};
pub use structured_output::{StructuredOutput, StructuredOutputError};

// ── Provider Trait ──

/// Unified interface for all LLM providers.
///
/// Implementors handle API-specific request/response translation
/// and HTTP communication. Callers work exclusively with this trait.
#[async_trait]
pub trait LlmProvider: Send + Sync {
    /// Send a chat completion request and return the response.
    async fn chat(&self, request: &ChatRequest) -> Result<ChatResponse, LlmError>;

    /// Return a stable identifier for this provider (e.g. "openai", "anthropic").
    fn provider_id(&self) -> &str;
}

// ── Message Types ──

/// A single message in a conversation turn.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Message {
    System {
        content: String,
    },
    User {
        content: String,
    },
    Assistant {
        content: String,
        tool_calls: Vec<ToolCall>,
    },
}

// ── Request Types ──

/// Unified chat request sent to any provider.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatRequest {
    pub model: String,
    pub messages: Vec<Message>,
    pub tools: Option<Vec<ToolDefinition>>,
    pub temperature: Option<f32>,
}

/// Unified tool definition (input side).  Providers translate this to
/// their own format (OpenAI function calling, Anthropic tools, etc.).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
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
