//! Anthropic Provider — implements `LlmProvider` using the Anthropic messages API.
//!
//! Supports tool use (native Anthropic format), system messages, and
//! multi-turn conversations with tool results.

use async_trait::async_trait;

use super::{ChatRequest, ChatResponse, LlmError, LlmProvider, Message, ToolCall, TokenUsage};

/// Anthropic API configuration.
#[derive(Debug, Clone)]
pub struct AnthropicConfig {
    pub api_key: String,
    pub base_url: String,
    pub default_model: String,
    pub timeout_secs: u64,
}

impl Default for AnthropicConfig {
    fn default() -> Self {
        Self {
            api_key: String::new(),
            base_url: "https://api.anthropic.com".to_string(),
            default_model: "claude-sonnet-4-20250514".to_string(),
            timeout_secs: 60,
        }
    }
}

/// Anthropic messages API client.
#[derive(Debug)]
pub struct AnthropicProvider {
    config: AnthropicConfig,
    client: reqwest::Client,
}

impl AnthropicProvider {
    /// Create a new Anthropic provider.
    ///
    /// Returns `LlmError::MissingApiKey` if the api_key is empty.
    pub fn new(config: AnthropicConfig) -> Result<Self, LlmError> {
        if config.api_key.is_empty() {
            return Err(LlmError::MissingApiKey {
                provider: "anthropic".to_string(),
            });
        }

        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(config.timeout_secs))
            .build()
            .map_err(|e| LlmError::ParseError {
                detail: format!("Failed to build HTTP client: {}", e),
            })?;

        Ok(Self { config, client })
    }

    /// Build the Anthropic API request body from a unified `ChatRequest`.
    fn build_request(&self, request: &ChatRequest) -> serde_json::Value {
        let model = if request.model.is_empty() {
            &self.config.default_model
        } else {
            &request.model
        };

        // Extract system message (Anthropic puts it at the top level, not in messages)
        let system = request
            .messages
            .iter()
            .find_map(|msg| match msg {
                Message::System { content } => Some(content.clone()),
                _ => None,
            });

        // Convert non-system messages to Anthropic format
        let messages: Vec<serde_json::Value> = request
            .messages
            .iter()
            .filter_map(|msg| self.message_to_anthropic(msg))
            .collect();

        let mut body = serde_json::json!({
            "model": model,
            "max_tokens": 4096,
            "messages": messages,
            "temperature": request.temperature.unwrap_or(0.0),
        });

        // Add system if present
        if let Some(sys) = system {
            body["system"] = serde_json::json!(sys);
        }

        // Add tools if present (Anthropic tool format uses input_schema)
        if let Some(ref tools) = request.tools {
            let anthropic_tools: Vec<serde_json::Value> = tools
                .iter()
                .map(|t| {
                    serde_json::json!({
                        "name": t.name,
                        "description": t.description,
                        "input_schema": t.parameters,
                    })
                })
                .collect();
            body["tools"] = serde_json::json!(anthropic_tools);
        }

        body
    }

    /// Convert a unified `Message` to Anthropic format.
    /// System messages are handled separately (top-level `system` field).
    fn message_to_anthropic(&self, msg: &Message) -> Option<serde_json::Value> {
        match msg {
            Message::System { .. } => None, // Handled at top level
            Message::User { content } => Some(serde_json::json!({
                "role": "user",
                "content": content,
            })),
            Message::Assistant { content, tool_calls } => {
                if content.is_empty() && tool_calls.is_empty() {
                    return None;
                }

                // Anthropic expects content as an array of blocks
                let mut blocks = Vec::new();

                if !content.is_empty() {
                    blocks.push(serde_json::json!({
                        "type": "text",
                        "text": content,
                    }));
                }

                for tc in tool_calls {
                    blocks.push(serde_json::json!({
                        "type": "tool_use",
                        "id": tc.id,
                        "name": tc.name,
                        "input": tc.arguments,
                    }));
                }

                Some(serde_json::json!({
                    "role": "assistant",
                    "content": blocks,
                }))
            }
        }
    }

    /// Parse the Anthropic API response into a unified `ChatResponse`.
    fn parse_response(&self, raw: &serde_json::Value) -> Result<ChatResponse, LlmError> {
        // Extract content: iterate over content blocks, collect text
        let mut content_parts = Vec::new();
        let mut tool_calls = Vec::new();

        if let Some(content_array) = raw.get("content").and_then(|c| c.as_array()) {
            for block in content_array {
                let block_type = block.get("type").and_then(|t| t.as_str());
                match block_type {
                    Some("text") => {
                        if let Some(text) = block.get("text").and_then(|t| t.as_str()) {
                            content_parts.push(text.to_string());
                        }
                    }
                    Some("tool_use") => {
                        let id = block.get("id").and_then(|i| i.as_str()).unwrap_or("").to_string();
                        let name =
                            block.get("name").and_then(|n| n.as_str()).unwrap_or("").to_string();
                        let input = block
                            .get("input")
                            .cloned()
                            .unwrap_or_else(|| serde_json::json!({}));
                        tool_calls.push(ToolCall { id, name, arguments: input });
                    }
                    _ => {}
                }
            }
        }

        // Extract usage
        let usage = TokenUsage {
            prompt_tokens: raw
                .get("usage")
                .and_then(|u| u.get("input_tokens"))
                .and_then(|t| t.as_u64())
                .unwrap_or(0) as u32,
            completion_tokens: raw
                .get("usage")
                .and_then(|u| u.get("output_tokens"))
                .and_then(|t| t.as_u64())
                .unwrap_or(0) as u32,
            total_tokens: 0, // Will be computed below
        };

        let total_tokens = usage.prompt_tokens + usage.completion_tokens;

        Ok(ChatResponse {
            content: content_parts.join("\n"),
            tool_calls,
            usage: TokenUsage {
                total_tokens,
                ..usage
            },
            raw: raw.clone(),
        })
    }
}

#[async_trait]
impl LlmProvider for AnthropicProvider {
    async fn chat(&self, request: &ChatRequest) -> Result<ChatResponse, LlmError> {
        if self.config.api_key.is_empty() {
            return Err(LlmError::MissingApiKey {
                provider: "anthropic".to_string(),
            });
        }

        let body = self.build_request(request);
        let url = format!("{}/v1/messages", self.config.base_url);

        // T-03-03: Log request for audit trail
        let request_id = format!("req-{}", chrono::Local::now().timestamp_millis());
        tracing::info!(
            request_id = %request_id,
            model = %request.model,
            provider = "anthropic",
            "Sending chat request to Anthropic"
        );

        let response = self
            .client
            .post(&url)
            .header("x-api-key", &self.config.api_key)
            .header("anthropic-version", "2023-10-01")
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| {
                if e.is_timeout() {
                    LlmError::Timeout
                } else {
                    LlmError::ApiError {
                        status: 0,
                        body: e.to_string(),
                    }
                }
            })?;

        let status = response.status().as_u16();

        // Read raw body for error handling
        let raw_body = response
            .text()
            .await
            .map_err(|e| LlmError::ParseError {
                detail: format!("Failed to read response body: {}", e),
            })?;

        // Parse JSON
        let raw_json: serde_json::Value = serde_json::from_str(&raw_body).map_err(|e| {
            LlmError::ParseError {
                detail: format!("Invalid JSON from Anthropic: {}", e),
            }
        })?;

        // Handle error responses
        if status >= 400 {
            return Err(LlmError::ApiError {
                status,
                body: raw_body,
            });
        }

        // T-03-03: Log successful response
        tracing::info!(
            request_id = %request_id,
            model = %request.model,
            provider = "anthropic",
            "Chat request completed successfully"
        );

        self.parse_response(&raw_json)
    }

    fn provider_id(&self) -> &str {
        "anthropic"
    }
}
