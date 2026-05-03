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
