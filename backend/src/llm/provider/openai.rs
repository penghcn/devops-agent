//! OpenAI Adapter — implements ProviderAdapter for the OpenAI chat completions API.

use async_trait::async_trait;

use super::base::{BaseConfig, GenericProvider, ProviderAdapter};
use crate::llm::{ChatRequest, ChatResponse, LlmError, LlmProvider, Message, TokenUsage, ToolCall, ToolChoice};

/// OpenAI Provider — 便捷封装 `GenericProvider<OpenAIAdapter>`
///
/// `OpenAIProvider::new(config)` 单参数构造，内部自动创建 adapter。
#[derive(Debug)]
pub struct OpenAIProvider(GenericProvider<OpenAIAdapter>);

impl OpenAIProvider {
    pub fn new(config: BaseConfig) -> Result<Self, LlmError> {
        Ok(Self(GenericProvider::<OpenAIAdapter>::new(
            config,
            OpenAIAdapter,
        )?))
    }
}

#[async_trait]
impl LlmProvider for OpenAIProvider {
    async fn llm_call(&self, request: &ChatRequest) -> Result<ChatResponse, LlmError> {
        self.0.llm_call(request).await
    }

    fn provider_id(&self) -> &str {
        self.0.provider_id()
    }
}

#[derive(Debug, Default)]
pub struct OpenAIAdapter;

impl ProviderAdapter for OpenAIAdapter {
    fn id(&self) -> &str {
        "openai"
    }

    fn endpoint(&self, base: &str) -> String {
        format!("{base}/v1/chat/completions")
    }

    fn headers(&self, api_key: &str, builder: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        builder.header("Authorization", format!("Bearer {api_key}"))
    }

    fn build_request(&self, request: &ChatRequest, default_model: &str) -> serde_json::Value {
        let model = if request.model.is_empty() {
            default_model
        } else {
            &request.model
        };

        let mut messages: Vec<serde_json::Value> = request
            .messages
            .iter()
            .filter_map(|msg| self.message_to_openai(msg))
            .collect();

        // Prefill: append assistant message as the last message
        if let Some(ref prefill) = request.prefill {
            messages.push(serde_json::json!({
                "role": "assistant",
                "content": prefill,
            }));
        }

        let mut body = serde_json::json!({
            "model": model,
            "messages": messages,
            "temperature": request.temperature.unwrap_or(0.0),
        });

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

        // Tool Choice
        if let Some(ref choice) = request.tool_choice {
            let tc = match choice {
                ToolChoice::Tool { name } => {
                    serde_json::json!({
                        "type": "function",
                        "function": { "name": name }
                    })
                }
                ToolChoice::Any => {
                    serde_json::json!("required")
                }
            };
            body["tool_choice"] = tc;
        }

        // Stop Sequences
        if let Some(ref stops) = request.stop_sequences {
            body["stop"] = serde_json::json!(stops);
        }

        body
    }

    fn parse_response(&self, raw: &serde_json::Value) -> Result<ChatResponse, LlmError> {
        let content = raw
            .get("choices")
            .and_then(|c| c.get(0))
            .and_then(|c| c.get("message"))
            .and_then(|m| m.get("content"))
            .and_then(|c| c.as_str())
            .unwrap_or("")
            .to_string();

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

impl OpenAIAdapter {
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

                let mut msg_obj = serde_json::json!({ "role": "assistant" });

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
}
