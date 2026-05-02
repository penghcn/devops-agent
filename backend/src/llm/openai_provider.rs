//! OpenAI Provider — implements `LlmProvider` using the OpenAI chat completions API.
//!
//! Supports function calling (tool use), structured output, and streaming responses.

use async_trait::async_trait;

use super::{ChatRequest, ChatResponse, LlmError, LlmProvider, Message, TokenUsage, ToolCall};

/// OpenAI API configuration.
#[derive(Debug, Clone)]
pub struct OpenAIConfig {
    pub api_key: String,
    pub base_url: String,
    pub default_model: String,
    pub timeout_secs: u64,
}

impl Default for OpenAIConfig {
    fn default() -> Self {
        Self {
            api_key: String::new(),
            base_url: "https://api.openai.com".to_string(),
            default_model: "gpt-4o".to_string(),
            timeout_secs: 60,
        }
    }
}

/// OpenAI chat completions API client.
#[derive(Debug)]
pub struct OpenAIProvider {
    config: OpenAIConfig,
    client: reqwest::Client,
}

impl OpenAIProvider {
    /// Create a new OpenAI provider.
    ///
    /// Returns `LlmError::MissingApiKey` if the api_key is empty.
    pub fn new(config: OpenAIConfig) -> Result<Self, LlmError> {
        if config.api_key.is_empty() {
            return Err(LlmError::MissingApiKey {
                provider: "openai".to_string(),
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

    /// Build the OpenAI API request body from a unified `ChatRequest`.
    fn build_request(&self, request: &ChatRequest) -> serde_json::Value {
        let model = if request.model.is_empty() {
            &self.config.default_model
        } else {
            &request.model
        };

        let messages: Vec<serde_json::Value> = request
            .messages
            .iter()
            .filter_map(|msg| self.message_to_openai(msg))
            .collect();

        let mut body = serde_json::json!({
            "model": model,
            "messages": messages,
            "temperature": request.temperature.unwrap_or(0.0),
        });

        // Add tools if present (OpenAI function calling format)
        if let Some(ref tools) = request.tools {
            let openai_tools: Vec<serde_json::Value> = tools
                .iter()
                .map(|t| {
                    serde_json::json!({
                        "type": "function",
                        "function": {
                            "name": t.name,
                            "description": t.description,
                            "parameters": t.parameters,
                        }
                    })
                })
                .collect();
            body["tools"] = serde_json::json!(openai_tools);
        }

        body
    }

    /// Convert a unified `Message` to OpenAI format.
    /// Returns `None` for empty assistant messages with no tool calls.
    fn message_to_openai(&self, msg: &Message) -> Option<serde_json::Value> {
        match msg {
            Message::System { content } => Some(serde_json::json!({
                "role": "system",
                "content": content,
            })),
            Message::User { content } => Some(serde_json::json!({
                "role": "user",
                "content": content,
            })),
            Message::Assistant {
                content,
                tool_calls,
            } => {
                if content.is_empty() && tool_calls.is_empty() {
                    return None;
                }

                let mut msg_obj = serde_json::json!({
                    "role": "assistant",
                });

                if !content.is_empty() {
                    msg_obj["content"] = serde_json::json!(content);
                }

                if !tool_calls.is_empty() {
                    let calls: Vec<serde_json::Value> = tool_calls
                        .iter()
                        .map(|tc| {
                            serde_json::json!({
                                "id": tc.id,
                                "type": "function",
                                "function": {
                                    "name": tc.name,
                                    "arguments": tc.arguments.to_string(),
                                }
                            })
                        })
                        .collect();
                    msg_obj["tool_calls"] = serde_json::json!(calls);
                }

                Some(msg_obj)
            }
        }
    }

    /// Parse the OpenAI API response into a unified `ChatResponse`.
    fn parse_response(&self, raw: &serde_json::Value) -> Result<ChatResponse, LlmError> {
        // Extract content from first choice
        let content = raw
            .get("choices")
            .and_then(|c| c.get(0))
            .and_then(|c| c.get("message"))
            .and_then(|m| m.get("content"))
            .and_then(|c| c.as_str())
            .unwrap_or("")
            .to_string();

        // Extract tool calls
        let tool_calls: Vec<ToolCall> = raw
            .get("choices")
            .and_then(|c| c.get(0))
            .and_then(|c| c.get("message"))
            .and_then(|m| m.get("tool_calls"))
            .and_then(|tc| tc.as_array())
            .map(|calls| {
                calls
                    .iter()
                    .filter_map(|call| {
                        let id = call.get("id")?.as_str()?.to_string();
                        let name = call.get("function")?.get("name")?.as_str()?.to_string();
                        let args_str = call
                            .get("function")?
                            .get("arguments")?
                            .as_str()?
                            .to_string();
                        let arguments = serde_json::from_str(&args_str).ok()?;
                        Some(ToolCall {
                            id,
                            name,
                            arguments,
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();

        // Extract usage
        let usage = TokenUsage {
            prompt_tokens: raw
                .get("usage")
                .and_then(|u| u.get("prompt_tokens"))
                .and_then(|t| t.as_u64())
                .unwrap_or(0) as u32,
            completion_tokens: raw
                .get("usage")
                .and_then(|u| u.get("completion_tokens"))
                .and_then(|t| t.as_u64())
                .unwrap_or(0) as u32,
            total_tokens: raw
                .get("usage")
                .and_then(|u| u.get("total_tokens"))
                .and_then(|t| t.as_u64())
                .unwrap_or(0) as u32,
        };

        Ok(ChatResponse {
            content,
            tool_calls,
            usage,
            raw: raw.clone(),
        })
    }
}

#[async_trait]
impl LlmProvider for OpenAIProvider {
    async fn chat(&self, request: &ChatRequest) -> Result<ChatResponse, LlmError> {
        if self.config.api_key.is_empty() {
            return Err(LlmError::MissingApiKey {
                provider: "openai".to_string(),
            });
        }

        let body = self.build_request(request);
        let url = format!("{}/v1/chat/completions", self.config.base_url);

        // T-03-03: Log request for audit trail
        let request_id = format!("req-{}", chrono::Local::now().timestamp_millis());
        tracing::info!(
            request_id = %request_id,
            model = %request.model,
            provider = "openai",
            "Sending chat request to OpenAI"
        );

        let response = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.config.api_key))
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
        let raw_body = response.text().await.map_err(|e| LlmError::ParseError {
            detail: format!("Failed to read response body: {}", e),
        })?;

        // Parse JSON
        let raw_json: serde_json::Value =
            serde_json::from_str(&raw_body).map_err(|e| LlmError::ParseError {
                detail: format!("Invalid JSON from OpenAI: {}", e),
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
            provider = "openai",
            "Chat request completed successfully"
        );

        self.parse_response(&raw_json)
    }

    fn provider_id(&self) -> &str {
        "openai"
    }
}
