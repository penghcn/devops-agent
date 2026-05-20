//! LLM Provider trait and data type tests
//!
//! Verifies:
//! - LlmProvider trait has llm_call() and provider_id() methods
//! - ChatRequest, ChatResponse, TokenUsage, Message, ToolCall, LlmError types
//! - Provider implementations compile correctly

use devops_agent::llm::*;

// ── Type Structure Tests ──

/// Test: Message enum supports System/User/Assistant variants
#[test]
fn test_message_enum_variants() {
    let sys = Message::System {
        content: "You are helpful".to_string(),
    };
    let user = Message::User {
        content: "Hello".to_string(),
    };
    let assistant = Message::Assistant {
        content: "Hi there".to_string(),
        tool_calls: vec![],
    };

    match sys {
        Message::System { content } => assert_eq!(content, "You are helpful"),
        _ => panic!("Wrong variant"),
    }

    match user {
        Message::User { content } => assert_eq!(content, "Hello"),
        _ => panic!("Wrong variant"),
    }

    match assistant {
        Message::Assistant {
            content,
            tool_calls,
        } => {
            assert_eq!(content, "Hi there");
            assert!(tool_calls.is_empty());
        }
        _ => panic!("Wrong variant"),
    }
}

/// Test: ChatRequest contains all required fields
#[test]
fn test_chat_request_fields() {
    let req = ChatRequest {
        model: "gpt-4o".to_string(),
        messages: vec![Message::User {
            content: "test".to_string(),
        }],
        tools: None,
        temperature: Some(0.7),
        tool_choice: None,
        stop_sequences: None,
        prefill: None,
    };

    assert_eq!(req.model, "gpt-4o");
    assert_eq!(req.messages.len(), 1);
    assert!(req.tools.is_none());
    assert_eq!(req.temperature, Some(0.7));
}

/// Test: ChatRequest with tools
#[test]
fn test_chat_request_with_tools() {
    let tools = vec![ToolDefinition {
        name: "read_file".to_string(),
        description: "Read a file".to_string(),
        parameters: serde_json::json!({
            "type": "object",
            "properties": { "path": { "type": "string" } }
        }),
    }];

    let req = ChatRequest {
        model: "gpt-4o".to_string(),
        messages: vec![],
        tools: Some(tools),
        temperature: None,
        tool_choice: None,
        stop_sequences: None,
        prefill: None,
    };

    assert!(req.tools.is_some());
    let t = req.tools.unwrap();
    assert_eq!(t.len(), 1);
    assert_eq!(t[0].name, "read_file");
}

/// Test: ChatResponse contains all required fields
#[test]
fn test_chat_response_fields() {
    let resp = ChatResponse {
        content: "Hello!".to_string(),
        tool_calls: vec![],
        usage: TokenUsage::default(),
        raw: serde_json::json!({}),
    };

    assert_eq!(resp.content, "Hello!");
    assert!(resp.tool_calls.is_empty());
    assert_eq!(resp.usage.total_tokens, 0);
    assert!(resp.raw.is_object());
}

/// Test: ChatResponse with tool calls
#[test]
fn test_chat_response_with_tool_calls() {
    let resp = ChatResponse {
        content: String::new(),
        tool_calls: vec![ToolCall {
            id: "call_1".to_string(),
            name: "read_file".to_string(),
            arguments: serde_json::json!({"path": "/tmp/test.txt"}),
        }],
        usage: TokenUsage {
            prompt_tokens: 10,
            completion_tokens: 20,
            total_tokens: 30,
        },
        raw: serde_json::json!({}),
    };

    assert_eq!(resp.tool_calls.len(), 1);
    assert_eq!(resp.tool_calls[0].name, "read_file");
    assert_eq!(resp.usage.prompt_tokens, 10);
    assert_eq!(resp.usage.completion_tokens, 20);
    assert_eq!(resp.usage.total_tokens, 30);
}

/// Test: TokenUsage defaults to zero
#[test]
fn test_token_usage_default() {
    let usage = TokenUsage::default();
    assert_eq!(usage.prompt_tokens, 0);
    assert_eq!(usage.completion_tokens, 0);
    assert_eq!(usage.total_tokens, 0);
}

/// Test: TokenUsage can be set
#[test]
fn test_token_usage_values() {
    let usage = TokenUsage {
        prompt_tokens: 100,
        completion_tokens: 50,
        total_tokens: 150,
    };
    assert_eq!(
        usage.total_tokens,
        usage.prompt_tokens + usage.completion_tokens
    );
}

/// Test: LlmError variants exist and implement Display
#[test]
fn test_llm_error_variants() {
    let api_err = LlmError::ApiError {
        status: 500,
        body: "Internal error".to_string(),
    };
    assert!(!format!("{}", api_err).is_empty());

    let timeout = LlmError::Timeout;
    assert!(!format!("{}", timeout).is_empty());

    let parse_err = LlmError::ParseError {
        detail: "Invalid JSON".to_string(),
    };
    assert!(!format!("{}", parse_err).is_empty());

    let not_found = LlmError::NotFound {
        model: "unknown".to_string(),
    };
    assert!(!format!("{}", not_found).is_empty());

    let missing_key = LlmError::MissingApiKey {
        provider: "openai".to_string(),
    };
    assert!(!format!("{}", missing_key).is_empty());
}

/// Test: LlmError implements std::error::Error
#[test]
fn test_llm_error_is_error_trait() {
    let err: Box<dyn std::error::Error> = Box::new(LlmError::Timeout);
    assert!(!err.to_string().is_empty());
}

/// Test: ToolCall structure
#[test]
fn test_tool_call_fields() {
    let tc = ToolCall {
        id: "call_abc".to_string(),
        name: "git_status".to_string(),
        arguments: serde_json::json!({"repo": "/tmp/repo"}),
    };

    assert_eq!(tc.id, "call_abc");
    assert_eq!(tc.name, "git_status");
    assert!(tc.arguments.is_object());
}

/// Test: ToolDefinition structure
#[test]
fn test_tool_definition_fields() {
    let td = ToolDefinition {
        name: "bash".to_string(),
        description: "Run a shell command".to_string(),
        parameters: serde_json::json!({
            "type": "object",
            "properties": { "command": { "type": "string" } }
        }),
    };

    assert_eq!(td.name, "bash");
    assert!(!td.description.is_empty());
    assert!(td.parameters.is_object());
}

// ── Mock Provider for Trait Integration Tests ──

use async_trait::async_trait;

struct MockProvider {
    id: String,
}

#[async_trait]
impl LlmProvider for MockProvider {
    async fn llm_call(&self, _request: &ChatRequest) -> Result<ChatResponse, LlmError> {
        Ok(ChatResponse {
            content: "mock response".to_string(),
            tool_calls: vec![],
            usage: TokenUsage::default(),
            raw: serde_json::json!({"mock": true}),
        })
    }

    fn provider_id(&self) -> &str {
        &self.id
    }
}

/// Test: Mock provider implements LlmProvider trait correctly
#[tokio::test]
async fn test_mock_provider_chat() {
    let mock = MockProvider {
        id: "mock".to_string(),
    };

    assert_eq!(mock.provider_id(), "mock");

    let req = ChatRequest {
        model: "test".to_string(),
        messages: vec![Message::User {
            content: "hello".to_string(),
        }],
        tools: None,
        temperature: None,
        tool_choice: None,
        stop_sequences: None,
        prefill: None,
    };

    let resp = mock.llm_call(&req).await.unwrap();
    assert_eq!(resp.content, "mock response");
}

/// Test: LlmProvider can be used as trait object
#[tokio::test]
async fn test_provider_trait_object() {
    let provider: Box<dyn LlmProvider> = Box::new(MockProvider {
        id: "trait_obj".to_string(),
    });

    assert_eq!(provider.provider_id(), "trait_obj");

    let req = ChatRequest {
        model: "test".to_string(),
        messages: vec![],
        tools: None,
        temperature: None,
        tool_choice: None,
        stop_sequences: None,
        prefill: None,
    };

    let resp = provider.llm_call(&req).await.unwrap();
    assert!(!resp.content.is_empty());
}

// ── OpenAI Provider Tests ──

/// Test: OpenAIProvider::new() rejects empty api_key
#[test]
fn test_openai_missing_api_key() {
    let config = provider::BaseConfig {
        api_key: String::new(),
        base_url: "https://api.openai.com".to_string(),
        default_model: "gpt-4o".to_string(),
        timeout_secs: 60,
    };
    let result = OpenAIProvider::new(config);
    assert!(result.is_err());
    match result.unwrap_err() {
        LlmError::MissingApiKey { provider } => assert_eq!(provider, "openai"),
        other => panic!("Expected MissingApiKey, got {:?}", other),
    }
}

/// Test: OpenAIProvider::new() accepts valid api_key
#[test]
fn test_openai_valid_creation() {
    let config = provider::BaseConfig {
        api_key: "sk-test".to_string(),
        base_url: "https://api.openai.com".to_string(),
        default_model: "gpt-4o".to_string(),
        timeout_secs: 60,
    };
    let provider = OpenAIProvider::new(config).unwrap();
    assert_eq!(provider.provider_id(), "openai");
}

/// Test: OpenAIProvider with custom base_url
#[test]
fn test_openai_custom_base_url() {
    let config = provider::BaseConfig {
        api_key: "sk-test".to_string(),
        base_url: "https://custom.api.com/v1".to_string(),
        default_model: "gpt-4o".to_string(),
        timeout_secs: 60,
    };
    let provider = OpenAIProvider::new(config).unwrap();
    assert_eq!(provider.provider_id(), "openai");
}

// ── Anthropic Provider Tests ──

/// Test: AnthropicProvider::new() rejects empty api_key
#[test]
fn test_anthropic_missing_api_key() {
    let config = provider::BaseConfig {
        api_key: String::new(),
        base_url: "https://api.anthropic.com".to_string(),
        default_model: "claude-sonnet-4-20250514".to_string(),
        timeout_secs: 60,
    };
    let result = AnthropicProvider::new(config);
    assert!(result.is_err());
    match result.unwrap_err() {
        LlmError::MissingApiKey { provider } => assert_eq!(provider, "anthropic"),
        other => panic!("Expected MissingApiKey, got {:?}", other),
    }
}

/// Test: AnthropicProvider::new() accepts valid api_key
#[test]
fn test_anthropic_valid_creation() {
    let config = provider::BaseConfig {
        api_key: "sk-ant-test".to_string(),
        base_url: "https://api.anthropic.com".to_string(),
        default_model: "claude-sonnet-4-20250514".to_string(),
        timeout_secs: 60,
    };
    let provider = AnthropicProvider::new(config).unwrap();
    assert_eq!(provider.provider_id(), "anthropic");
}

// ── Adapter build_request / parse_response Tests ──

use devops_agent::llm::provider::{AnthropicAdapter, OpenAIAdapter, ProviderAdapter};

/// Test: OpenAIAdapter build_request produces correct body
#[test]
fn test_openai_build_request() {
    let adapter = OpenAIAdapter;
    let req = ChatRequest {
        model: "gpt-4o".to_string(),
        messages: vec![
            Message::System {
                content: "You are helpful".to_string(),
            },
            Message::User {
                content: "Hello".to_string(),
            },
        ],
        tools: None,
        temperature: Some(0.5),
        tool_choice: None,
        stop_sequences: None,
        prefill: None,
    };

    let body = adapter.build_request(&req, "default-model");

    assert_eq!(body["model"], "gpt-4o");
    assert_eq!(body["temperature"], 0.5);
    assert_eq!(body["messages"].as_array().unwrap().len(), 2);
    assert_eq!(body["messages"][0]["role"], "system");
    assert_eq!(body["messages"][1]["role"], "user");
    assert!(body.get("tools").is_none());
}

/// Test: OpenAIAdapter build_request with tools
#[test]
fn test_openai_build_request_with_tools() {
    let adapter = OpenAIAdapter;
    let tools = vec![ToolDefinition {
        name: "read_file".to_string(),
        description: "Read a file".to_string(),
        parameters: serde_json::json!({
            "type": "object",
            "properties": { "path": { "type": "string" } }
        }),
    }];

    let req = ChatRequest {
        model: String::new(),
        messages: vec![Message::User {
            content: "read /tmp/test.txt".to_string(),
        }],
        tools: Some(tools),
        temperature: None,
        tool_choice: None,
        stop_sequences: None,
        prefill: None,
    };

    let body = adapter.build_request(&req, "gpt-4o-mini");

    assert_eq!(body["model"], "gpt-4o-mini"); // uses default_model when model is empty
    let tools_arr = body["tools"].as_array().unwrap();
    assert_eq!(tools_arr.len(), 1);
    assert_eq!(tools_arr[0]["type"], "function");
    assert_eq!(tools_arr[0]["function"]["name"], "read_file");
}

/// Test: OpenAIAdapter parse_response
#[test]
fn test_openai_parse_response() {
    let adapter = OpenAIAdapter;
    let raw = serde_json::json!({
        "choices": [{
            "message": {
                "content": "Hello there!",
                "tool_calls": [{
                    "id": "call_1",
                    "type": "function",
                    "function": {
                        "name": "read_file",
                        "arguments": "{\"path\": \"/tmp/test.txt\"}"
                    }
                }]
            }
        }],
        "usage": {
            "prompt_tokens": 10,
            "completion_tokens": 20,
            "total_tokens": 30
        }
    });

    let resp = adapter.parse_response(&raw).unwrap();

    assert_eq!(resp.content, "Hello there!");
    assert_eq!(resp.tool_calls.len(), 1);
    assert_eq!(resp.tool_calls[0].id, "call_1");
    assert_eq!(resp.tool_calls[0].name, "read_file");
    assert_eq!(resp.usage.prompt_tokens, 10);
    assert_eq!(resp.usage.completion_tokens, 20);
    assert_eq!(resp.usage.total_tokens, 30);
}

/// Test: OpenAIAdapter parse_response with no tool calls
#[test]
fn test_openai_parse_response_text_only() {
    let adapter = OpenAIAdapter;
    let raw = serde_json::json!({
        "choices": [{
            "message": { "content": "Just text" }
        }],
        "usage": {
            "prompt_tokens": 5,
            "completion_tokens": 10,
            "total_tokens": 15
        }
    });

    let resp = adapter.parse_response(&raw).unwrap();

    assert_eq!(resp.content, "Just text");
    assert!(resp.tool_calls.is_empty());
}

/// Test: AnthropicAdapter build_request produces correct body
#[test]
fn test_anthropic_build_request() {
    let adapter = AnthropicAdapter;
    let req = ChatRequest {
        model: "claude-sonnet-4".to_string(),
        messages: vec![
            Message::System {
                content: "Be concise".to_string(),
            },
            Message::User {
                content: "What is Rust?".to_string(),
            },
        ],
        tools: None,
        temperature: Some(0.0),
        tool_choice: None,
        stop_sequences: None,
        prefill: None,
    };

    let body = adapter.build_request(&req, "default-model");

    assert_eq!(body["model"], "claude-sonnet-4");
    assert_eq!(body["max_tokens"], 8192);
    assert_eq!(body["system"], "Be concise");
    let msgs = body["messages"].as_array().unwrap();
    assert_eq!(msgs.len(), 1); // system is extracted separately
    assert_eq!(msgs[0]["role"], "user");
}

/// Test: AnthropicAdapter build_request with tools
#[test]
fn test_anthropic_build_request_with_tools() {
    let adapter = AnthropicAdapter;
    let tools = vec![ToolDefinition {
        name: "bash".to_string(),
        description: "Run shell".to_string(),
        parameters: serde_json::json!({
            "type": "object",
            "properties": { "command": { "type": "string" } }
        }),
    }];

    let req = ChatRequest {
        model: String::new(),
        messages: vec![],
        tools: Some(tools),
        temperature: None,
        tool_choice: None,
        stop_sequences: None,
        prefill: None,
    };

    let body = adapter.build_request(&req, "claude-sonnet-4");

    assert_eq!(body["model"], "claude-sonnet-4");
    let tools_arr = body["tools"].as_array().unwrap();
    assert_eq!(tools_arr.len(), 1);
    assert_eq!(tools_arr[0]["name"], "bash");
    assert!(tools_arr[0].get("input_schema").is_some());
}

/// Test: AnthropicAdapter parse_response
#[test]
fn test_anthropic_parse_response() {
    let adapter = AnthropicAdapter;
    let raw = serde_json::json!({
        "content": [
            {"type": "text", "text": "Rust is great"},
            {
                "type": "tool_use",
                "id": "tool_1",
                "name": "bash",
                "input": {"command": "ls"}
            }
        ],
        "usage": {
            "input_tokens": 20,
            "output_tokens": 30
        }
    });

    let resp = adapter.parse_response(&raw).unwrap();

    assert_eq!(resp.content, "Rust is great");
    assert_eq!(resp.tool_calls.len(), 1);
    assert_eq!(resp.tool_calls[0].id, "tool_1");
    assert_eq!(resp.tool_calls[0].name, "bash");
    assert_eq!(resp.usage.prompt_tokens, 20);
    assert_eq!(resp.usage.completion_tokens, 30);
    assert_eq!(resp.usage.total_tokens, 50);
}

/// Test: AnthropicAdapter parse_response text only
#[test]
fn test_anthropic_parse_response_text_only() {
    let adapter = AnthropicAdapter;
    let raw = serde_json::json!({
        "content": [{"type": "text", "text": "Hello"}],
        "usage": {"input_tokens": 5, "output_tokens": 3}
    });

    let resp = adapter.parse_response(&raw).unwrap();

    assert_eq!(resp.content, "Hello");
    assert!(resp.tool_calls.is_empty());
    assert_eq!(resp.usage.total_tokens, 8);
}

// ── Tool Choice Translation Tests ──

#[test]
fn test_openai_tool_choice_specific_tool() {
    let adapter = OpenAIAdapter;
    let req = ChatRequest {
        model: "gpt-4o".to_string(),
        messages: vec![Message::User {
            content: "测试".to_string(),
        }],
        tools: Some(vec![ToolDefinition {
            name: "extract".to_string(),
            description: "提取数据".to_string(),
            parameters: serde_json::json!({"type": "object"}),
        }]),
        temperature: None,
        tool_choice: Some(ToolChoice::Tool {
            name: "extract".to_string(),
        }),
        stop_sequences: None,
        prefill: None,
    };

    let body = adapter.build_request(&req, "gpt-4o");

    assert_eq!(body["tool_choice"]["type"], "function");
    assert_eq!(body["tool_choice"]["function"]["name"], "extract");
}

#[test]
fn test_openai_tool_choice_any() {
    let adapter = OpenAIAdapter;
    let req = ChatRequest {
        model: "gpt-4o".to_string(),
        messages: vec![Message::User {
            content: "测试".to_string(),
        }],
        tools: Some(vec![ToolDefinition {
            name: "read".to_string(),
            description: "读取文件".to_string(),
            parameters: serde_json::json!({"type": "object"}),
        }]),
        temperature: None,
        tool_choice: Some(ToolChoice::Any),
        stop_sequences: None,
        prefill: None,
    };

    let body = adapter.build_request(&req, "gpt-4o");

    assert_eq!(body["tool_choice"], "required");
}

#[test]
fn test_anthropic_tool_choice_specific_tool() {
    let adapter = AnthropicAdapter;
    let req = ChatRequest {
        model: "claude-sonnet-4".to_string(),
        messages: vec![
            Message::System {
                content: "你是助手".to_string(),
            },
            Message::User {
                content: "测试".to_string(),
            },
        ],
        tools: Some(vec![ToolDefinition {
            name: "extract".to_string(),
            description: "提取数据".to_string(),
            parameters: serde_json::json!({"type": "object"}),
        }]),
        temperature: None,
        tool_choice: Some(ToolChoice::Tool {
            name: "extract".to_string(),
        }),
        stop_sequences: None,
        prefill: None,
    };

    let body = adapter.build_request(&req, "claude-sonnet-4");

    assert_eq!(body["tool_choice"]["type"], "tool");
    assert_eq!(body["tool_choice"]["tool"]["name"], "extract");
}

#[test]
fn test_anthropic_tool_choice_any() {
    let adapter = AnthropicAdapter;
    let req = ChatRequest {
        model: "claude-sonnet-4".to_string(),
        messages: vec![
            Message::System {
                content: "你是助手".to_string(),
            },
            Message::User {
                content: "测试".to_string(),
            },
        ],
        tools: Some(vec![ToolDefinition {
            name: "read".to_string(),
            description: "读取文件".to_string(),
            parameters: serde_json::json!({"type": "object"}),
        }]),
        temperature: None,
        tool_choice: Some(ToolChoice::Any),
        stop_sequences: None,
        prefill: None,
    };

    let body = adapter.build_request(&req, "claude-sonnet-4");

    assert_eq!(body["tool_choice"]["type"], "any");
}

#[test]
fn test_openai_no_tool_choice() {
    let adapter = OpenAIAdapter;
    let req = ChatRequest {
        model: "gpt-4o".to_string(),
        messages: vec![Message::User {
            content: "测试".to_string(),
        }],
        tools: Some(vec![ToolDefinition {
            name: "read".to_string(),
            description: "读取文件".to_string(),
            parameters: serde_json::json!({"type": "object"}),
        }]),
        temperature: None,
        tool_choice: None,
        stop_sequences: None,
        prefill: None,
    };

    let body = adapter.build_request(&req, "gpt-4o");

    // OpenAI: without tool_choice, the key should be absent or "auto"
    assert!(body.get("tool_choice").is_none());
}

#[test]
fn test_anthropic_no_tool_choice() {
    let adapter = AnthropicAdapter;
    let req = ChatRequest {
        model: "claude-sonnet-4".to_string(),
        messages: vec![
            Message::System {
                content: "你是助手".to_string(),
            },
            Message::User {
                content: "测试".to_string(),
            },
        ],
        tools: Some(vec![ToolDefinition {
            name: "read".to_string(),
            description: "读取文件".to_string(),
            parameters: serde_json::json!({"type": "object"}),
        }]),
        temperature: None,
        tool_choice: None,
        stop_sequences: None,
        prefill: None,
    };

    let body = adapter.build_request(&req, "claude-sonnet-4");

    assert!(body.get("tool_choice").is_none());
}

// ── Prefill Translation Tests ──

#[test]
fn test_openai_prefill_appends_assistant_message() {
    let adapter = OpenAIAdapter;
    let req = ChatRequest {
        model: "gpt-4o".to_string(),
        messages: vec![Message::User {
            content: "查询状态".to_string(),
        }],
        tools: None,
        temperature: None,
        tool_choice: None,
        stop_sequences: None,
        prefill: Some("{".to_string()),
    };

    let body = adapter.build_request(&req, "gpt-4o");

    let messages = body["messages"].as_array().unwrap();
    // Should have 2 messages: user + assistant prefill
    assert_eq!(messages.len(), 2);
    assert_eq!(messages[1]["role"], "assistant");
    assert_eq!(messages[1]["content"], "{");
}

#[test]
fn test_anthropic_prefill_appends_assistant_message() {
    let adapter = AnthropicAdapter;
    let req = ChatRequest {
        model: "claude-sonnet-4".to_string(),
        messages: vec![
            Message::System {
                content: "你是助手".to_string(),
            },
            Message::User {
                content: "查询状态".to_string(),
            },
        ],
        tools: None,
        temperature: None,
        tool_choice: None,
        stop_sequences: None,
        prefill: Some("{".to_string()),
    };

    let body = adapter.build_request(&req, "claude-sonnet-4");

    let messages = body["messages"].as_array().unwrap();
    // Anthropic: system goes to top-level, messages = user + assistant prefill
    assert_eq!(messages.len(), 2);
    assert_eq!(messages[0]["role"], "user");
    assert_eq!(messages[1]["role"], "assistant");
    assert_eq!(messages[1]["content"], "{");
}

#[test]
fn test_openai_no_prefill() {
    let adapter = OpenAIAdapter;
    let req = ChatRequest {
        model: "gpt-4o".to_string(),
        messages: vec![Message::User {
            content: "测试".to_string(),
        }],
        tools: None,
        temperature: None,
        tool_choice: None,
        stop_sequences: None,
        prefill: None,
    };

    let body = adapter.build_request(&req, "gpt-4o");

    let messages = body["messages"].as_array().unwrap();
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0]["role"], "user");
}

#[test]
fn test_anthropic_no_prefill() {
    let adapter = AnthropicAdapter;
    let req = ChatRequest {
        model: "claude-sonnet-4".to_string(),
        messages: vec![
            Message::System {
                content: "你是助手".to_string(),
            },
            Message::User {
                content: "测试".to_string(),
            },
        ],
        tools: None,
        temperature: None,
        tool_choice: None,
        stop_sequences: None,
        prefill: None,
    };

    let body = adapter.build_request(&req, "claude-sonnet-4");

    let messages = body["messages"].as_array().unwrap();
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0]["role"], "user");
}

// ── Stop Sequences Translation Tests ──

#[test]
fn test_openai_stop_sequences() {
    let adapter = OpenAIAdapter;
    let req = ChatRequest {
        model: "gpt-4o".to_string(),
        messages: vec![Message::User {
            content: "测试".to_string(),
        }],
        tools: None,
        temperature: None,
        tool_choice: None,
        stop_sequences: Some(vec!["\n\n\n".to_string(), "```\n".to_string()]),
        prefill: None,
    };

    let body = adapter.build_request(&req, "gpt-4o");

    assert_eq!(body["stop"][0], "\n\n\n");
    assert_eq!(body["stop"][1], "```\n");
}

#[test]
fn test_anthropic_stop_sequences() {
    let adapter = AnthropicAdapter;
    let req = ChatRequest {
        model: "claude-sonnet-4".to_string(),
        messages: vec![
            Message::System {
                content: "你是助手".to_string(),
            },
            Message::User {
                content: "测试".to_string(),
            },
        ],
        tools: None,
        temperature: None,
        tool_choice: None,
        stop_sequences: Some(vec!["\n\n\n".to_string(), "```\n".to_string()]),
        prefill: None,
    };

    let body = adapter.build_request(&req, "claude-sonnet-4");

    assert_eq!(body["stop_sequences"][0], "\n\n\n");
    assert_eq!(body["stop_sequences"][1], "```\n");
}

// ── Assistant Message with Tool Calls Translation Tests ──

#[test]
fn test_openai_assistant_with_tool_calls() {
    let adapter = OpenAIAdapter;
    let req = ChatRequest {
        model: "gpt-4o".to_string(),
        messages: vec![
            Message::User {
                content: "部署 ds-pkg".to_string(),
            },
            Message::Assistant {
                content: "我来帮你部署".to_string(),
                tool_calls: vec![ToolCall {
                    id: "call_1".to_string(),
                    name: "deploy".to_string(),
                    arguments: serde_json::json!({"job": "ds-pkg"}),
                }],
            },
        ],
        tools: None,
        temperature: None,
        tool_choice: None,
        stop_sequences: None,
        prefill: None,
    };

    let body = adapter.build_request(&req, "gpt-4o");

    let messages = body["messages"].as_array().unwrap();
    assert_eq!(messages.len(), 2);
    // Check the assistant message has tool_calls
    let assistant = &messages[1];
    assert_eq!(assistant["role"], "assistant");
    assert_eq!(assistant["content"], "我来帮你部署");
    let calls = assistant["tool_calls"].as_array().unwrap();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0]["id"], "call_1");
    assert_eq!(calls[0]["type"], "function");
    assert_eq!(calls[0]["function"]["name"], "deploy");
}

#[test]
fn test_openai_assistant_tool_calls_only() {
    let adapter = OpenAIAdapter;
    let req = ChatRequest {
        model: "gpt-4o".to_string(),
        messages: vec![
            Message::User {
                content: "部署".to_string(),
            },
            Message::Assistant {
                content: String::new(),
                tool_calls: vec![ToolCall {
                    id: "call_1".to_string(),
                    name: "deploy".to_string(),
                    arguments: serde_json::json!({"job": "ds-pkg"}),
                }],
            },
        ],
        tools: None,
        temperature: None,
        tool_choice: None,
        stop_sequences: None,
        prefill: None,
    };

    let body = adapter.build_request(&req, "gpt-4o");

    let messages = body["messages"].as_array().unwrap();
    let assistant = &messages[1];
    // No content field when empty
    assert!(assistant.get("content").is_none());
    assert!(assistant["tool_calls"].as_array().is_some());
}

#[test]
fn test_anthropic_assistant_with_tool_calls() {
    let adapter = AnthropicAdapter;
    let req = ChatRequest {
        model: "claude-sonnet-4".to_string(),
        messages: vec![
            Message::System {
                content: "你是助手".to_string(),
            },
            Message::User {
                content: "部署 ds-pkg".to_string(),
            },
            Message::Assistant {
                content: "我来帮你部署".to_string(),
                tool_calls: vec![ToolCall {
                    id: "toolu_1".to_string(),
                    name: "deploy".to_string(),
                    arguments: serde_json::json!({"job": "ds-pkg"}),
                }],
            },
        ],
        tools: None,
        temperature: None,
        tool_choice: None,
        stop_sequences: None,
        prefill: None,
    };

    let body = adapter.build_request(&req, "claude-sonnet-4");

    let messages = body["messages"].as_array().unwrap();
    assert_eq!(messages.len(), 2);
    // Check the assistant message has tool_use blocks
    let assistant = &messages[1];
    assert_eq!(assistant["role"], "assistant");
    let content = assistant["content"].as_array().unwrap();
    // Should have 2 blocks: text + tool_use
    assert_eq!(content.len(), 2);
    assert_eq!(content[0]["type"], "text");
    assert_eq!(content[0]["text"], "我来帮你部署");
    assert_eq!(content[1]["type"], "tool_use");
    assert_eq!(content[1]["id"], "toolu_1");
    assert_eq!(content[1]["name"], "deploy");
}

#[test]
fn test_anthropic_assistant_tool_calls_only() {
    let adapter = AnthropicAdapter;
    let req = ChatRequest {
        model: "claude-sonnet-4".to_string(),
        messages: vec![
            Message::System {
                content: "你是助手".to_string(),
            },
            Message::User {
                content: "部署".to_string(),
            },
            Message::Assistant {
                content: String::new(),
                tool_calls: vec![ToolCall {
                    id: "toolu_1".to_string(),
                    name: "deploy".to_string(),
                    arguments: serde_json::json!({"job": "ds-pkg"}),
                }],
            },
        ],
        tools: None,
        temperature: None,
        tool_choice: None,
        stop_sequences: None,
        prefill: None,
    };

    let body = adapter.build_request(&req, "claude-sonnet-4");

    let messages = body["messages"].as_array().unwrap();
    let assistant = &messages[1];
    let content = assistant["content"].as_array().unwrap();
    // Only tool_use block, no text
    assert_eq!(content.len(), 1);
    assert_eq!(content[0]["type"], "tool_use");
}

#[test]
fn test_anthropic_system_extracted_to_top_level() {
    let adapter = AnthropicAdapter;
    let req = ChatRequest {
        model: "claude-sonnet-4".to_string(),
        messages: vec![
            Message::System {
                content: "你是 DevOps 助手".to_string(),
            },
            Message::User {
                content: "测试".to_string(),
            },
        ],
        tools: None,
        temperature: None,
        tool_choice: None,
        stop_sequences: None,
        prefill: None,
    };

    let body = adapter.build_request(&req, "claude-sonnet-4");

    // System should be at top-level, not in messages
    assert_eq!(body["system"], "你是 DevOps 助手");
    let messages = body["messages"].as_array().unwrap();
    // Only user message in messages array
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0]["role"], "user");
}

// ── Tool Definition Format Differences ──

#[test]
fn test_openai_tool_definition_format() {
    let adapter = OpenAIAdapter;
    let tools = vec![ToolDefinition {
        name: "read_file".to_string(),
        description: "读取文件内容".to_string(),
        parameters: serde_json::json!({
            "type": "object",
            "properties": {"path": {"type": "string"}}
        }),
    }];

    let req = ChatRequest {
        model: "gpt-4o".to_string(),
        messages: vec![Message::User {
            content: "测试".to_string(),
        }],
        tools: Some(tools),
        temperature: None,
        tool_choice: None,
        stop_sequences: None,
        prefill: None,
    };

    let body = adapter.build_request(&req, "gpt-4o");

    let tools_arr = body["tools"].as_array().unwrap();
    assert_eq!(tools_arr[0]["type"], "function");
    assert_eq!(tools_arr[0]["function"]["name"], "read_file");
    // OpenAI uses "parameters" directly
    assert!(tools_arr[0]["function"].get("parameters").is_some());
}

#[test]
fn test_anthropic_tool_definition_format() {
    let adapter = AnthropicAdapter;
    let tools = vec![ToolDefinition {
        name: "read_file".to_string(),
        description: "读取文件内容".to_string(),
        parameters: serde_json::json!({
            "type": "object",
            "properties": {"path": {"type": "string"}}
        }),
    }];

    let req = ChatRequest {
        model: "claude-sonnet-4".to_string(),
        messages: vec![
            Message::System {
                content: "你是助手".to_string(),
            },
            Message::User {
                content: "测试".to_string(),
            },
        ],
        tools: Some(tools),
        temperature: None,
        tool_choice: None,
        stop_sequences: None,
        prefill: None,
    };

    let body = adapter.build_request(&req, "claude-sonnet-4");

    let tools_arr = body["tools"].as_array().unwrap();
    // Anthropic uses "input_schema" instead of "parameters"
    assert_eq!(tools_arr[0]["name"], "read_file");
    assert!(tools_arr[0].get("input_schema").is_some());
    // Anthropic does NOT have "type": "function" wrapper
    assert!(tools_arr[0].get("type").is_none());
}
