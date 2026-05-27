//! OpenAI 兼容 Provider 宏。
//!
//! 为 NVIDIA / DeepSeek / LLaMA / vLLM 生成 Adapter + Provider 包装。
//! 所有实现与 OpenAIAdapter 一致，仅 id 和 endpoint 路径可能不同。

use super::base::{BaseConfig, GenericProvider, ProviderAdapter};
use crate::llm::{
    ChatRequest, ChatResponse, LlmError, LlmProvider, Message, TokenUsage, ToolCall, ToolChoice,
};

/// 生成 OpenAI 兼容的 Adapter + Provider 包装。
///
/// # 参数
/// - `$adapter:ident` — Adapter 结构体名
/// - `$provider:ident` — Provider 包装结构体名
/// - `$id:expr` — provider id 字符串
/// - `$endpoint_path:expr` — endpoint 路径后缀（如 `"/v1/chat/completions"`）
macro_rules! define_openai_compat_adapter {
    ($adapter:ident, $provider:ident, $id:expr, $endpoint_path:expr) => {
        #[derive(Debug, Default)]
        pub struct $adapter;

        impl ProviderAdapter for $adapter {
            fn id(&self) -> &str {
                $id
            }

            fn endpoint(&self, base: &str) -> String {
                format!("{base}{}", $endpoint_path)
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
                    .filter_map(|msg| $adapter::message_to_openai(msg))
                    .collect();

                if let Some(ref prefill) = request.prefill {
                    messages.push(serde_json::json!({
                        "role": "assistant",
                        "content": prefill,
                    }));
                }

                let mut body = serde_json::json!({
                    "model": model,
                    "messages": messages,
                    "temperature": request.temperature.unwrap_or(0.6),
                });

                if let Some(ref tools) = request.tools {
                    let compat_tools: Vec<serde_json::Value> = tools
                        .iter()
                        .filter(|t| !t.name.is_empty())
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
                    body["tools"] = serde_json::json!(compat_tools);
                }

                if let Some(ref choice) = request.tool_choice {
                    let tc = match choice {
                        ToolChoice::Tool { name } => {
                            serde_json::json!({
                                "type": "function",
                                "function": { "name": name }
                            })
                        }
                        ToolChoice::Any => serde_json::json!("required"),
                    };
                    body["tool_choice"] = tc;
                }

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

        impl $adapter {
            fn join_text(blocks: &[crate::llm::ContentBlock]) -> String {
                blocks
                    .iter()
                    .filter_map(|b| b.as_text().map(|s| s.to_string()))
                    .collect::<Vec<_>>()
                    .join("")
            }

            fn message_to_openai(msg: &Message) -> Option<serde_json::Value> {
                match msg {
                    Message::System { content } => {
                        let text = Self::join_text(content);
                        if text.is_empty() {
                            None
                        } else {
                            Some(serde_json::json!({
                                "role": "system",
                                "content": text,
                            }))
                        }
                    }
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

                        let mut msg_obj = serde_json::json!({ "role": "assistant" });

                        if !text.is_empty() {
                            msg_obj["content"] = serde_json::json!(text);
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
                    Message::ToolResult {
                        tool_call_id,
                        content,
                    } => {
                        let text = Self::join_text(content);
                        Some(serde_json::json!({
                            "role": "tool",
                            "tool_call_id": tool_call_id,
                            "content": text,
                        }))
                    }
                }
            }
        }

        /// Provider 包装 — 便捷封装 `GenericProvider<$adapter>`
        #[derive(Debug)]
        pub struct $provider(GenericProvider<$adapter>);

        impl $provider {
            pub fn new(config: BaseConfig) -> Result<Self, LlmError> {
                Ok(Self(GenericProvider::<$adapter>::new(
                    config,
                    $adapter,
                )?))
            }
        }

        #[async_trait::async_trait]
        impl LlmProvider for $provider {
            async fn llm_call(&self, request: &ChatRequest) -> Result<ChatResponse, LlmError> {
                self.0.llm_call(request).await
            }

            fn provider_id(&self) -> &str {
                self.0.provider_id()
            }
        }
    };
}

// NVIDIA AI Foundation Inference API
define_openai_compat_adapter!(
    NVIDIAAdapter,
    NVIDIAProvider,
    "nvidia",
    "/v1/chat/completions"
);

// DeepSeek API (endpoint 不带 /v1)
define_openai_compat_adapter!(
    DeepSeekAdapter,
    DeepSeekProvider,
    "deepseek",
    "/chat/completions"
);

// LLaMA (llama.cpp OpenAI 兼容服务)
define_openai_compat_adapter!(LLaMAAdapter, LLaMAProvider, "llama", "/v1/chat/completions");

// VLLM OpenAI 兼容服务
define_openai_compat_adapter!(VLLMAdapter, VLLMProvider, "vllm", "/v1/chat/completions");
