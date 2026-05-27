//! Anthropic Adapter — implements ProviderAdapter for the Anthropic messages API.

use async_trait::async_trait;

use super::base::{BaseConfig, GenericProvider, ProviderAdapter};
use crate::llm::{
    ChatRequest, ChatResponse, ContentBlock, LlmError, LlmProvider, Message, TokenUsage, ToolCall,
    ToolChoice,
};

/// Anthropic Provider — 便捷封装 `GenericProvider<AnthropicAdapter>`
///
/// `AnthropicProvider::new(config)` 单参数构造，内部自动创建 adapter。
#[derive(Debug)]
pub struct AnthropicProvider(GenericProvider<AnthropicAdapter>);

impl AnthropicProvider {
    pub fn new(config: BaseConfig) -> Result<Self, LlmError> {
        Ok(Self(GenericProvider::<AnthropicAdapter>::new(
            config,
            AnthropicAdapter,
        )?))
    }
}

#[async_trait]
impl LlmProvider for AnthropicProvider {
    async fn llm_call(&self, request: &ChatRequest) -> Result<ChatResponse, LlmError> {
        self.0.llm_call(request).await
    }

    fn provider_id(&self) -> &str {
        self.0.provider_id()
    }
}

#[derive(Debug, Default)]
pub struct AnthropicAdapter;

impl ProviderAdapter for AnthropicAdapter {
    fn id(&self) -> &str {
        "anthropic"
    }

    fn endpoint(&self, base: &str) -> String {
        format!("{base}/v1/messages")
    }

    fn headers(&self, api_key: &str, builder: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        builder
            .header("x-api-key", api_key)
            .header("anthropic-version", "2023-10-01")
    }

    fn build_request(&self, request: &ChatRequest, default_model: &str) -> serde_json::Value {
        let model = if request.model.is_empty() {
            default_model
        } else {
            &request.model
        };

        let system = request.messages.iter().find_map(|msg| match msg {
            Message::System { content } => {
                if content.is_empty() {
                    None
                } else {
                    let blocks: Vec<serde_json::Value> = content
                        .iter()
                        .map(|b| match b {
                            ContentBlock::Text {
                                text,
                                cache_control,
                            } => {
                                let mut block = serde_json::json!({ "type": "text", "text": text });
                                if cache_control.is_some() {
                                    block["cache_control"] =
                                        serde_json::json!({ "type": "ephemeral_buffer" });
                                }
                                block
                            }
                            ContentBlock::Image { source } => serde_json::json!({
                                "type": "image",
                                "source": {
                                    "type": "base64",
                                    "media_type": source.media_type,
                                    "data": source.data,
                                }
                            }),
                        })
                        .collect();
                    Some(serde_json::json!(blocks))
                }
            }
            _ => None,
        });

        let mut messages: Vec<serde_json::Value> = request
            .messages
            .iter()
            .filter_map(|msg| self.message_to_anthropic(msg))
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
            "max_tokens": 8192,
            "messages": messages,
            "temperature": request.temperature.unwrap_or(0.6),
        });

        if let Some(sys) = system {
            body["system"] = serde_json::json!(sys);
        }

        if let Some(ref tools) = request.tools {
            let mut anthropic_tools: Vec<serde_json::Value> = Vec::new();
            for t in tools {
                // 跳过缓存断点伪工具（空 name）
                if t.name.is_empty() {
                    continue;
                }
                anthropic_tools.push(serde_json::json!({
                    "name": t.name,
                    "description": t.description,
                    "input_schema": t.parameters,
                }));
                if t.cache_control.is_some() {
                    anthropic_tools.push(serde_json::json!({
                        "cache_control": { "type": "ephemeral_buffer" }
                    }));
                }
            }
            body["tools"] = serde_json::json!(anthropic_tools);
        }

        // Tool Choice
        if let Some(ref choice) = request.tool_choice {
            let tc = match choice {
                ToolChoice::Tool { name } => {
                    serde_json::json!({ "type": "tool", "tool": { "name": name } })
                }
                ToolChoice::Any => {
                    serde_json::json!({ "type": "any" })
                }
            };
            body["tool_choice"] = tc;
        }

        // Stop Sequences
        if let Some(ref stops) = request.stop_sequences {
            body["stop_sequences"] = serde_json::json!(stops);
        }

        body
    }

    fn parse_response(&self, raw: &serde_json::Value) -> Result<ChatResponse, LlmError> {
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
                        let id = block
                            .get("id")
                            .and_then(|i| i.as_str())
                            .unwrap_or("")
                            .to_string();
                        let name = block
                            .get("name")
                            .and_then(|n| n.as_str())
                            .unwrap_or("")
                            .to_string();
                        let input = block
                            .get("input")
                            .cloned()
                            .unwrap_or_else(|| serde_json::json!({}));
                        tool_calls.push(ToolCall {
                            id,
                            name,
                            arguments: input,
                        });
                    }
                    _ => {}
                }
            }
        }

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
            total_tokens: 0,
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

impl AnthropicAdapter {
    fn join_text(blocks: &[crate::llm::ContentBlock]) -> String {
        blocks
            .iter()
            .filter_map(|b| b.as_text().map(|s| s.to_string()))
            .collect::<Vec<_>>()
            .join("")
    }

    fn message_to_anthropic(&self, msg: &Message) -> Option<serde_json::Value> {
        match msg {
            Message::System { .. } => None,
            Message::User { content } => {
                let text = Self::join_text(content);
                if text.is_empty() {
                    None
                } else {
                    Some(serde_json::json!({
                        "role": "user",
                        "content": text,
                    }))
                }
            }
            Message::Assistant {
                content,
                tool_calls,
            } => {
                let text = Self::join_text(content);
                if text.is_empty() && tool_calls.is_empty() {
                    return None;
                }

                let mut blocks = Vec::new();

                if !text.is_empty() {
                    blocks.push(serde_json::json!({
                        "type": "text",
                        "text": text,
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
            Message::ToolResult {
                tool_call_id,
                content,
            } => {
                let text = Self::join_text(content);
                Some(serde_json::json!({
                    "role": "user",
                    "content": [{
                        "type": "tool_result",
                        "tool_use_id": tool_call_id,
                        "content": text,
                    }]
                }))
            }
        }
    }
}
