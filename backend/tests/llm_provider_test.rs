//! LLM Provider trait and data type tests
//!
//! Verifies:
//! - LlmProvider trait has llm_call() and provider_id() methods
//! - ChatRequest, ChatResponse, TokenUsage, Message, ToolCall, LlmError types

use devops_agent::llm::*;
use lellm_core::text_block;

// ── Type Structure Tests ──

#[test]
fn test_message_enum_variants() {
    let sys = Message::System {
        content: text_block("You are helpful".to_string()),
    };
    let user = Message::User {
        content: text_block("Hello".to_string()),
    };
    let assistant = Message::assistant(text_block("Hi there".to_string()));

    match sys {
        Message::System { content } => {
            assert_eq!(content.len(), 1);
            assert_eq!(content[0].as_text(), Some("You are helpful"));
        }
        _ => panic!("Wrong variant"),
    }

    match user {
        Message::User { content } => {
            assert_eq!(content.len(), 1);
            assert_eq!(content[0].as_text(), Some("Hello"));
        }
        _ => panic!("Wrong variant"),
    }

    match assistant {
        Message::Assistant { content } => {
            assert_eq!(content.len(), 1);
            assert_eq!(content[0].as_text(), Some("Hi there"));
        }
        _ => panic!("Wrong variant"),
    }
}

#[test]
fn test_chat_request_fields() {
    let req = ChatRequest {
        model: "gpt-4o".to_string(),
        messages: vec![Message::User {
            content: text_block("Hello".to_string()),
        }],
        ..Default::default()
    };

    assert_eq!(req.model, "gpt-4o");
    assert_eq!(req.messages.len(), 1);
    assert!(req.tools.is_none());
}

#[test]
fn test_chat_request_user_prompt() {
    let req = ChatRequest::user_prompt("test prompt".to_string());
    assert_eq!(req.messages.len(), 1);
    assert_eq!(req.messages[0].content()[0].as_text(), Some("test prompt"));
}

#[test]
fn test_chat_request_builder_methods() {
    let req = ChatRequest::user_prompt("test".to_string())
        .with_model("gpt-4o".to_string())
        .with_temperature(0.8)
        .with_max_tokens(1024);

    assert_eq!(req.model, "gpt-4o");
    assert_eq!(req.temperature, Some(0.8));
    assert_eq!(req.max_tokens, Some(1024));
}

#[test]
fn test_tool_definition() {
    let tool = ToolDefinition {
        name: "search".to_string(),
        description: "Search the web".to_string(),
        parameters: serde_json::json!({"type": "object"}),
        cache_control: None,
    };

    assert_eq!(tool.name, "search");
    assert_eq!(tool.description, "Search the web");
}

#[test]
fn test_tool_choice() {
    let tool_choice = ToolChoice::Tool {
        name: "search".to_string(),
    };
    match tool_choice {
        ToolChoice::Tool { name } => assert_eq!(name, "search"),
        _ => panic!("Wrong variant"),
    }

    let any_choice = ToolChoice::Any;
    assert!(matches!(any_choice, ToolChoice::Any));
}

#[test]
fn test_chat_response() {
    let resp = ChatResponse::new(
        vec![ContentBlock::text("Hello!".to_string())],
        TokenUsage::default(),
        serde_json::json!({}),
    );

    assert_eq!(resp.text_content(), "Hello!");
    assert!(!resp.has_tool_calls());
}

#[test]
fn test_chat_response_with_tool_calls() {
    let resp = ChatResponse::new(
        vec![
            ContentBlock::text("Let me search".to_string()),
            ContentBlock::ToolCall(ToolCall {
                id: "call_1".to_string(),
                name: "search".to_string(),
                arguments: serde_json::json!({"query": "test"}),
            }),
        ],
        TokenUsage::default(),
        serde_json::json!({}),
    );

    assert!(resp.has_tool_calls());
    assert_eq!(resp.tool_calls().count(), 1);
    assert_eq!(resp.tool_calls().next().unwrap().name, "search");
}

#[test]
fn test_token_usage() {
    let usage = TokenUsage {
        prompt_tokens: 100,
        completion_tokens: 50,
        total_tokens: 150,
    };
    assert_eq!(usage.prompt_tokens, 100);
    assert_eq!(usage.completion_tokens, 50);
    assert_eq!(usage.total_tokens, 150);
}

#[test]
fn test_content_block_text() {
    let block = ContentBlock::text("hello".to_string());
    assert_eq!(block.as_text(), Some("hello"));
}

#[test]
fn test_content_block_tool_call() {
    let tc = ToolCall {
        id: "1".to_string(),
        name: "test".to_string(),
        arguments: serde_json::json!({}),
    };
    let block = ContentBlock::ToolCall(tc);
    assert!(block.as_text().is_none());
}

#[test]
fn test_message_extract_tool_calls() {
    let msg = Message::Assistant {
        content: vec![
            ContentBlock::text("Let me call a tool".to_string()),
            ContentBlock::ToolCall(ToolCall {
                id: "call_1".to_string(),
                name: "search".to_string(),
                arguments: serde_json::json!({}),
            }),
        ],
    };
    let calls = msg.extract_tool_calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].name, "search");
}

#[test]
fn test_message_tool_result_ok() {
    let msg = Message::tool_result_ok("call_1", "result data".to_string());
    assert!(matches!(msg, Message::ToolResult { .. }));
    assert!(!msg.is_tool_error());
    assert_eq!(msg.tool_call_id(), "call_1");
}

#[test]
fn test_message_tool_result_error() {
    let msg = Message::tool_error("call_2", "something failed".to_string());
    assert!(matches!(msg, Message::ToolResult { .. }));
    assert!(msg.is_tool_error());
    assert_eq!(msg.tool_call_id(), "call_2");
}

// ── LlmError Tests ──

#[test]
fn test_llm_error_provider() {
    let err = LlmError::Provider {
        provider: "openai".to_string(),
        status: Some(401),
        code: None,
        message: "unauthorized".to_string(),
    };
    let display = format!("{}", err);
    assert!(display.contains("openai"));
}

#[test]
fn test_llm_error_timeout() {
    let err = LlmError::Timeout {
        detail: "request timed out".to_string(),
    };
    let display = format!("{}", err);
    assert!(display.contains("timed out"));
}

#[test]
fn test_llm_error_parse() {
    let err = LlmError::Parse {
        detail: "invalid JSON".to_string(),
    };
    let display = format!("{}", err);
    assert!(display.contains("invalid JSON"));
}

#[test]
fn test_llm_error_invalid_request() {
    let err = LlmError::InvalidRequest {
        message: "bad request".to_string(),
    };
    let display = format!("{}", err);
    assert!(display.contains("bad request"));
}

#[test]
fn test_llm_error_is_std_error() {
    let err: Box<dyn std::error::Error> = Box::new(LlmError::Timeout {
        detail: "timeout".to_string(),
    });
    assert!(err.to_string().contains("timeout"));
}

// ── LlmProvider Trait Tests ──

use async_trait::async_trait;

struct MockProvider;

#[async_trait]
impl LlmProvider for MockProvider {
    async fn llm_call(&self, _request: &ChatRequest) -> Result<ChatResponse, LlmError> {
        Ok(ChatResponse::new(
            vec![ContentBlock::text("mock response".to_string())],
            TokenUsage::default(),
            serde_json::Value::Null,
        ))
    }

    async fn stream(&self, _request: &ChatRequest) -> Result<devops_agent::llm::ProviderStream, LlmError> {
        Err(LlmError::UnsupportedFeature {
            feature: "streaming not supported in mock".to_string(),
        })
    }

    fn provider_id(&self) -> &str {
        "mock"
    }
}

#[tokio::test]
async fn test_mock_provider() {
    let provider = MockProvider;
    let req = ChatRequest::user_prompt("test".to_string());
    let resp = provider.llm_call(&req).await.unwrap();
    assert_eq!(resp.text_content(), "mock response");
    assert_eq!(provider.provider_id(), "mock");
}

#[tokio::test]
async fn test_mock_provider_with_model() {
    let provider = MockProvider;
    let req = ChatRequest::user_prompt("test".to_string()).with_model("gpt-4o".to_string());
    let resp = provider.llm_call(&req).await.unwrap();
    assert!(resp.text_content().contains("mock"));
}

#[test]
fn test_tool_definition_cache_control() {
    use devops_agent::llm::ToolDefinitionExt;

    let tool = ToolDefinition {
        name: "search".to_string(),
        description: "Search".to_string(),
        parameters: serde_json::json!({"type": "object"}),
        cache_control: None,
    };

    let cached = tool.clone_with_cache(CacheControl::Breakpoint);
    assert!(cached.cache_control.is_some());
    assert_eq!(cached.name, "search");
}

#[test]
fn test_tool_definition_cache_breakpoint() {
    use devops_agent::llm::ToolDefinitionExt;

    let bp = ToolDefinition::cache_breakpoint();
    assert!(bp.name.is_empty());
    assert!(bp.cache_control.is_some());
}

// ── CacheControl Tests ──

#[test]
fn test_cache_control_breakpoint() {
    let cc = CacheControl::Breakpoint;
    assert_eq!(cc, CacheControl::Breakpoint);
}

// ── PromptBuilder Integration Tests ──

#[test]
fn test_prompt_builder_simple() {
    let builder = PromptBuilder::with_static_tools(vec![]);
    let req = builder.build_simple("分析构建日志".to_string());
    assert_eq!(req.messages.len(), 2);
    assert!(matches!(req.messages[0], Message::System { .. }));
    assert!(matches!(req.messages[1], Message::User { .. }));
}

#[test]
fn test_prompt_builder_with_tools() {
    let tool = ToolDefinition {
        name: "search".to_string(),
        description: "Search".to_string(),
        parameters: serde_json::json!({"type": "object"}),
        cache_control: None,
    };
    let builder = PromptBuilder::with_static_tools(vec![]);
    let req = builder.build(
        "test".to_string(),
        vec![],
        &SessionSlots::new(),
        vec![tool],
        vec![],
        vec![],
    );
    assert!(req.tools.is_some());
    let tools = req.tools.unwrap();
    assert!(tools.iter().any(|t| t.name == "search"));
}
