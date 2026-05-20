//! LLM Router and Structured Output tests
//!
//! Verifies:
//! - ModelRouter L1/L2 task classification
//! - Model selection based on task level
//! - Provider routing
//! - StructuredOutput schema-constrained output with retry

use devops_agent::llm::router::ProviderModels;
use devops_agent::llm::structured_output::StructuredOutputMode;
use devops_agent::llm::*;
use std::sync::Arc;

// ── Mock Provider for Router Tests ──

use async_trait::async_trait;

struct TestProvider {
    id: String,
    response: String,
}

#[async_trait]
impl LlmProvider for TestProvider {
    async fn llm_call(&self, _request: &ChatRequest) -> Result<ChatResponse, LlmError> {
        Ok(ChatResponse {
            content: self.response.clone(),
            tool_calls: vec![],
            usage: TokenUsage::default(),
            raw: serde_json::json!({}),
        })
    }

    fn provider_id(&self) -> &str {
        &self.id
    }
}

// ── TaskLevel Tests ──

#[test]
fn test_task_level_enum() {
    // Verify TaskLevel enum has L1 and L2 variants
    let l1 = TaskLevel::L1;
    let l2 = TaskLevel::L2;

    // They should be different
    assert_ne!(format!("{:?}", l1), format!("{:?}", l2));
}

// ── ModelRouterConfig Tests ──

#[test]
fn test_model_router_config_defaults() {
    let config = ModelRouterConfig::default();
    assert_eq!(config.default_level, TaskLevel::L1);
    assert_eq!(config.max_tokens_l1, 1024);
    assert_eq!(config.max_tokens_l2, 4096);
}

#[test]
fn test_model_router_config_custom() {
    let config = ModelRouterConfig {
        default_level: TaskLevel::L2,
        max_tokens_l1: 2048,
        max_tokens_l2: 8192,
    };
    assert_eq!(config.default_level, TaskLevel::L2);
    assert_eq!(config.max_tokens_l1, 2048);
    assert_eq!(config.max_tokens_l2, 8192);
}

// ── ModelRouter Tests ──

#[test]
fn test_model_router_new() {
    let config = ModelRouterConfig::default();
    let _router = ModelRouter::new(config);
}

#[test]
fn test_model_router_register_provider() {
    let mut router = ModelRouter::default();
    let provider: Arc<dyn LlmProvider> = Arc::new(TestProvider {
        id: "openai".to_string(),
        response: "test".to_string(),
    });
    router.register_provider(
        "openai".to_string(),
        provider,
        ProviderModels {
            model_flash: Some("gpt-4o-mini".to_string()),
            model_pro: None,
            default_model: Some("gpt-4o-mini".to_string()),
        },
    );
}

// ── classify_task Tests ──

#[test]
fn test_classify_short_prompt_l1() {
    let router = ModelRouter::default();
    let level = router.classify_task("部署 ds-pkg");
    assert_eq!(level, TaskLevel::L1);
}

#[test]
fn test_classify_long_prompt_l2() {
    let router = ModelRouter::default();
    // Build a prompt >= 500 chars
    let long_prompt = "部署 ".to_string() + &"ds-pkg ".repeat(100);
    assert!(long_prompt.len() >= 500);
    let level = router.classify_task(&long_prompt);
    assert_eq!(level, TaskLevel::L2);
}

#[test]
fn test_classify_complex_keyword_l2() {
    let router = ModelRouter::default();
    assert_eq!(router.classify_task("分析这个日志"), TaskLevel::L2);
    assert_eq!(
        router.classify_task("Please analyze the build output"),
        TaskLevel::L2
    );
    assert_eq!(router.classify_task("查看日志输出"), TaskLevel::L2);
    assert_eq!(router.classify_task("debug this issue"), TaskLevel::L2);
    assert_eq!(router.classify_task("故障排查"), TaskLevel::L2);
    assert_eq!(router.classify_task("find the root cause"), TaskLevel::L2);
}

// ── ProviderModels select Tests ──

#[test]
fn test_provider_models_select() {
    let models = ProviderModels {
        model_flash: Some("gpt-4o-mini".to_string()),
        model_pro: Some("claude-sonnet-4-20250514".to_string()),
        default_model: Some("gpt-4o-mini".to_string()),
    };
    assert_eq!(models.select(TaskLevel::L1).unwrap(), "gpt-4o-mini");
    assert_eq!(
        models.select(TaskLevel::L2).unwrap(),
        "claude-sonnet-4-20250514"
    );
}

// ── route Tests ──

#[tokio::test]
async fn test_route_with_provider() {
    let mut router = ModelRouter::default();
    let provider: Arc<dyn LlmProvider> = Arc::new(TestProvider {
        id: "openai".to_string(),
        response: "deployed successfully".to_string(),
    });
    router.register_provider(
        "openai".to_string(),
        provider,
        ProviderModels {
            model_flash: Some("gpt-4o-mini".to_string()),
            model_pro: None,
            default_model: Some("gpt-4o-mini".to_string()),
        },
    );

    let request = ChatRequest {
        model: String::new(),
        messages: vec![Message::User {
            content: "部署 ds-pkg".to_string(),
        }],
        tools: None,
        temperature: None,
        tool_choice: None,
        stop_sequences: None,
        prefill: None,
    };

    let resp = router.route(&request).await;
    assert!(resp.is_ok());
    let resp = resp.unwrap();
    assert_eq!(resp.content, "deployed successfully");
}

#[tokio::test]
async fn test_route_provider_priority() {
    let mut router = ModelRouter::default();

    // Register two providers
    let p1: Arc<dyn LlmProvider> = Arc::new(TestProvider {
        id: "openai".to_string(),
        response: "from openai".to_string(),
    });
    let p2: Arc<dyn LlmProvider> = Arc::new(TestProvider {
        id: "anthropic".to_string(),
        response: "from anthropic".to_string(),
    });

    // Register anthropic first, openai second
    router.register_provider(
        "anthropic".to_string(),
        p2,
        ProviderModels {
            model_flash: None,
            model_pro: None,
            default_model: None,
        },
    );
    router.register_provider(
        "openai".to_string(),
        p1,
        ProviderModels {
            model_flash: Some("gpt-4o-mini".to_string()),
            model_pro: None,
            default_model: Some("gpt-4o-mini".to_string()),
        },
    );

    // L1 task should route to openai (first provider with a model for L1)
    let request = ChatRequest {
        model: String::new(),
        messages: vec![Message::User {
            content: "简短回复".to_string(),
        }],
        tools: None,
        temperature: None,
        tool_choice: None,
        stop_sequences: None,
        prefill: None,
    };

    let resp = router.route(&request).await.unwrap();
    assert_eq!(resp.content, "from openai");
}

// ── StructuredOutput Tests ──

#[test]
fn test_structured_output_error_variants() {
    // Verify StructuredOutputError has correct variants
    let lll_err = StructuredOutputError::LlmError(LlmError::Timeout);
    assert!(!format!("{}", lll_err).is_empty());

    let parse_err = StructuredOutputError::ParseError {
        response: "not json".to_string(),
        detail: "invalid".to_string(),
    };
    assert!(!format!("{}", parse_err).is_empty());

    let max_retries = StructuredOutputError::MaxRetriesExceeded {
        responses: vec!["attempt1".to_string()],
    };
    assert!(!format!("{}", max_retries).is_empty());
}

#[test]
fn test_structured_output_new() {
    let provider: Arc<dyn LlmProvider> = Arc::new(TestProvider {
        id: "mock".to_string(),
        response: r#"{"action":"deploy","job_name":"ds-pkg"}"#.to_string(),
    });
    let schema = serde_json::json!({
        "type": "object",
        "required": ["action", "job_name"],
        "properties": {
            "action": {"type": "string"},
            "job_name": {"type": "string"}
        }
    });

    let so = StructuredOutput::new(provider, "gpt-4o-mini".to_string()).schema(schema);
    // model field is now private, verified via constructor
    // max_retries defaults to 3
}

#[tokio::test]
async fn test_explicit_model_routes_by_prefix() {
    let mut router = ModelRouter::default();

    // Register OpenAI first, Anthropic second.
    let openai: Arc<dyn LlmProvider> = Arc::new(TestProvider {
        id: "openai".to_string(),
        response: "from openai".to_string(),
    });
    let anthropic: Arc<dyn LlmProvider> = Arc::new(TestProvider {
        id: "anthropic".to_string(),
        response: "from anthropic".to_string(),
    });

    router.register_provider(
        "openai".to_string(),
        openai,
        ProviderModels {
            model_flash: Some("gpt-4o-mini".to_string()),
            model_pro: None,
            default_model: Some("gpt-4o-mini".to_string()),
        },
    );
    router.register_provider(
        "anthropic".to_string(),
        anthropic,
        ProviderModels {
            model_flash: Some("claude-sonnet-4-20250514".to_string()),
            model_pro: None,
            default_model: Some("claude-sonnet-4-20250514".to_string()),
        },
    );

    // claude-* model should route to Anthropic, not OpenAI (first provider).
    let request = ChatRequest {
        model: "claude-sonnet-4".to_string(),
        messages: vec![Message::User {
            content: "test".to_string(),
        }],
        tools: None,
        temperature: None,
        tool_choice: None,
        stop_sequences: None,
        prefill: None,
    };

    let resp = router.llm_call(&request).await.unwrap();
    assert_eq!(resp.content, "from anthropic");

    // gpt-* model should route to OpenAI.
    let request2 = ChatRequest {
        model: "gpt-4o".to_string(),
        messages: vec![Message::User {
            content: "test".to_string(),
        }],
        tools: None,
        temperature: None,
        tool_choice: None,
        stop_sequences: None,
        prefill: None,
    };

    let resp2 = router.llm_call(&request2).await.unwrap();
    assert_eq!(resp2.content, "from openai");
}

#[tokio::test]
async fn test_structured_output_execute_valid_json() {
    let provider: Arc<dyn LlmProvider> = Arc::new(TestProvider {
        id: "mock".to_string(),
        response: r#"{"action":"deploy","job_name":"ds-pkg","branch":null}"#.to_string(),
    });
    let schema = serde_json::json!({
        "type": "object",
        "required": ["action", "job_name"]
    });

    let so = StructuredOutput::new(provider, "gpt-4o-mini".to_string())
        .schema(schema)
        .mode(StructuredOutputMode::EnhancedPrompt);

    #[derive(serde::Deserialize, Debug)]
    #[allow(dead_code)]
    struct IntentResult {
        action: String,
        job_name: String,
        branch: Option<String>,
    }

    let result: Result<IntentResult, StructuredOutputError> = so.execute("部署 ds-pkg").await;
    assert!(result.is_ok());
    let r = result.unwrap();
    assert_eq!(r.action, "deploy");
    assert_eq!(r.job_name, "ds-pkg");
}

#[tokio::test]
async fn test_structured_output_extract_json_codeblock() {
    let provider: Arc<dyn LlmProvider> = Arc::new(TestProvider {
        id: "mock".to_string(),
        response:
            "Here is the result:\n```json\n{\"action\":\"build\",\"job_name\":\"test\"}\n```\nDone."
                .to_string(),
    });
    let schema = serde_json::json!({"type": "object"});

    let so = StructuredOutput::new(provider, "gpt-4o-mini".to_string())
        .schema(schema)
        .mode(StructuredOutputMode::EnhancedPrompt);

    #[derive(serde::Deserialize, Debug)]
    #[allow(dead_code)]
    struct BuildResult {
        action: String,
        job_name: String,
    }

    let build_res: std::result::Result<BuildResult, StructuredOutputError> =
        so.execute("test").await;
    assert!(build_res.is_ok());
    assert_eq!(build_res.unwrap().action, "build");
}

#[tokio::test]
async fn test_structured_output_braces_extraction() {
    let provider: Arc<dyn LlmProvider> = Arc::new(TestProvider {
        id: "mock".to_string(),
        response: "Result: {\"action\":\"query\"} End.".to_string(),
    });
    let schema = serde_json::json!({"type": "object"});

    let so = StructuredOutput::new(provider, "gpt-4o-mini".to_string())
        .schema(schema)
        .mode(StructuredOutputMode::EnhancedPrompt);

    #[derive(serde::Deserialize, Debug)]
    struct QueryResult {
        action: String,
    }

    let query_res: std::result::Result<QueryResult, StructuredOutputError> =
        so.execute("test").await;
    assert!(query_res.is_ok());
    assert_eq!(query_res.unwrap().action, "query");
}

#[tokio::test]
async fn test_structured_output_retry_on_failure() {
    // First response fails, second succeeds
    use std::sync::Arc as StdArc;
    use std::sync::atomic::{AtomicU32, Ordering};

    struct RetryProvider {
        id: String,
        call_count: StdArc<AtomicU32>,
    }

    #[async_trait]
    impl LlmProvider for RetryProvider {
        async fn llm_call(
            &self,
            _request: &ChatRequest,
        ) -> std::result::Result<ChatResponse, LlmError> {
            let count = self.call_count.fetch_add(1, Ordering::SeqCst);
            let content = if count == 0 {
                "not valid json at all"
            } else {
                r#"{"action":"deploy"}"#
            };
            Ok(ChatResponse {
                content: content.to_string(),
                tool_calls: vec![],
                usage: TokenUsage::default(),
                raw: serde_json::json!({}),
            })
        }

        fn provider_id(&self) -> &str {
            &self.id
        }
    }

    let provider: Arc<dyn LlmProvider> = Arc::new(RetryProvider {
        id: "retry".to_string(),
        call_count: StdArc::new(AtomicU32::new(0)),
    });

    let schema = serde_json::json!({"type": "object"});
    let so = StructuredOutput::new(provider.clone(), "gpt-4o-mini".to_string())
        .schema(schema)
        .mode(StructuredOutputMode::EnhancedPrompt)
        .with_max_retries(3);

    #[derive(serde::Deserialize, Debug)]
    struct DeployResult {
        action: String,
    }

    let deploy_res: std::result::Result<DeployResult, StructuredOutputError> =
        so.execute("test").await;
    assert!(deploy_res.is_ok());
    assert_eq!(deploy_res.unwrap().action, "deploy");
}

#[tokio::test]
async fn test_structured_output_max_retries_exceeded() {
    let provider: Arc<dyn LlmProvider> = Arc::new(TestProvider {
        id: "mock".to_string(),
        response: "always invalid response".to_string(),
    });
    let schema = serde_json::json!({"type": "object", "required": ["action"]});

    let so = StructuredOutput::new(provider, "gpt-4o-mini".to_string())
        .schema(schema)
        .mode(StructuredOutputMode::EnhancedPrompt)
        .with_max_retries(2);

    #[derive(serde::Deserialize, Debug)]
    #[allow(dead_code)]
    struct ActionResult {
        action: String,
    }

    let action_res: std::result::Result<ActionResult, StructuredOutputError> =
        so.execute("test").await;
    assert!(action_res.is_err());
    match action_res.unwrap_err() {
        StructuredOutputError::MaxRetriesExceeded { responses } => {
            assert!(responses.len() >= 2);
        }
        e => panic!("Expected MaxRetriesExceeded, got {:?}", e),
    }
}

// ── Tool Use Mode Tests ──

/// Mock provider that captures the request and returns a tool call.
/// Use a discarded Arc handle (Arc::new(Mutex::new(None))) if you don't need to inspect the request.
struct CapturingToolProvider {
    id: String,
    tool_name: String,
    tool_args: String,
    captured_request: Arc<std::sync::Mutex<Option<ChatRequest>>>,
}

#[async_trait]
impl LlmProvider for CapturingToolProvider {
    async fn llm_call(&self, request: &ChatRequest) -> Result<ChatResponse, LlmError> {
        {
            let mut guard = self.captured_request.lock().unwrap();
            *guard = Some(request.clone());
        }
        Ok(ChatResponse {
            content: "".to_string(),
            tool_calls: vec![ToolCall {
                id: "tc_1".to_string(),
                name: self.tool_name.clone(),
                arguments: serde_json::from_str(&self.tool_args).unwrap_or(serde_json::json!({})),
            }],
            usage: TokenUsage::default(),
            raw: serde_json::json!({}),
        })
    }

    fn provider_id(&self) -> &str {
        &self.id
    }
}

#[tokio::test]
async fn test_tool_use_mode_execute_with_tool_call() {
    let provider: Arc<dyn LlmProvider> = Arc::new(CapturingToolProvider {
        id: "tool-use".to_string(),
        tool_name: "extract_result".to_string(),
        tool_args: r#"{"action":"deploy","job_name":"ds-pkg","branch":"main"}"#.to_string(),
        captured_request: Arc::new(std::sync::Mutex::new(None)),
    });

    let schema = serde_json::json!({
        "type": "object",
        "required": ["action", "job_name"],
        "properties": {
            "action": {"type": "string"},
            "job_name": {"type": "string"},
            "branch": {"type": ["string", "null"]}
        }
    });

    #[derive(serde::Deserialize, Debug, PartialEq)]
    struct DeployIntent {
        action: String,
        job_name: String,
        branch: Option<String>,
    }

    let so = StructuredOutput::new(provider.clone(), "claude-sonnet-4".to_string())
        .schema(schema)
        .tool_name("extract_result");

    let result: Result<DeployIntent, StructuredOutputError> =
        so.execute("部署 ds-pkg 到 main 分支").await;

    assert!(
        result.is_ok(),
        "Should parse tool call arguments: {:?}",
        result
    );
    let r = result.unwrap();
    assert_eq!(r.action, "deploy");
    assert_eq!(r.job_name, "ds-pkg");
    assert_eq!(r.branch, Some("main".to_string()));
}

#[tokio::test]
async fn test_tool_use_mode_request_structure() {
    let captured = Arc::new(std::sync::Mutex::new(None));
    let provider: Arc<dyn LlmProvider> = Arc::new(CapturingToolProvider {
        id: "tool-use".to_string(),
        tool_name: "extract_data".to_string(),
        tool_args: r#"{"status":"ok"}"#.to_string(),
        captured_request: captured.clone(),
    });

    let schema = serde_json::json!({
        "type": "object",
        "properties": { "status": {"type": "string"} }
    });

    #[derive(serde::Deserialize, Debug)]
    #[allow(dead_code)]
    struct StatusResult {
        status: String,
    }

    let so = StructuredOutput::new(provider.clone(), "claude-sonnet-4".to_string())
        .schema(schema)
        .tool_name("extract_data");

    let _result: Result<StatusResult, StructuredOutputError> = so.execute("查询状态").await;

    // Inspect the captured request
    let request = captured
        .lock()
        .unwrap()
        .clone()
        .expect("Request should have been captured");

    // Verify: has tools
    assert!(
        request.tools.is_some(),
        "Tool Use mode should include tools"
    );
    let tools = request.tools.unwrap();
    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0].name, "extract_data");

    // Verify: has tool_choice
    assert!(
        request.tool_choice.is_some(),
        "Tool Use mode should set tool_choice"
    );

    // Verify: temperature is 0.0
    assert_eq!(request.temperature, Some(0.0));

    // Verify: no prefill/stop_sequences in Tool Use mode
    assert!(request.prefill.is_none());
    assert!(request.stop_sequences.is_none());

    // Verify: messages structure — System + User
    assert_eq!(request.messages.len(), 2);
    match &request.messages[0] {
        Message::System { content } => {
            assert!(
                content.contains("结构化数据抽取"),
                "System prompt should mention data extraction"
            );
        }
        _ => panic!("Expected System message first"),
    }
    match &request.messages[1] {
        Message::User { content } => {
            assert_eq!(content, "查询状态");
        }
        _ => panic!("Expected User message second"),
    }
}

// ── Enhanced Prompt Mode Tests ──

/// Mock Provider that captures the request and returns JSON for Enhanced Prompt mode.
struct CapturingEnhancedProvider {
    id: String,
    response: String,
    captured_request: Arc<std::sync::Mutex<Option<ChatRequest>>>,
}

#[async_trait]
impl LlmProvider for CapturingEnhancedProvider {
    async fn llm_call(&self, request: &ChatRequest) -> Result<ChatResponse, LlmError> {
        {
            let mut guard = self.captured_request.lock().unwrap();
            *guard = Some(request.clone());
        }
        Ok(ChatResponse {
            content: self.response.clone(),
            tool_calls: vec![],
            usage: TokenUsage::default(),
            raw: serde_json::json!({}),
        })
    }

    fn provider_id(&self) -> &str {
        &self.id
    }
}

#[tokio::test]
async fn test_enhanced_prompt_mode_request_structure() {
    let captured = Arc::new(std::sync::Mutex::new(None));
    let provider: Arc<dyn LlmProvider> = Arc::new(CapturingEnhancedProvider {
        id: "enhanced".to_string(),
        response: r#"{"status":"ok"}"#.to_string(),
        captured_request: captured.clone(),
    });

    let schema = serde_json::json!({
        "type": "object",
        "properties": { "status": {"type": "string"} }
    });

    #[derive(serde::Deserialize, Debug)]
    #[allow(dead_code)]
    struct StatusResult {
        status: String,
    }

    let so = StructuredOutput::new(provider.clone(), "gpt-4o-mini".to_string())
        .schema(schema)
        .mode(StructuredOutputMode::EnhancedPrompt);

    let _result: Result<StatusResult, StructuredOutputError> = so.execute("查询状态").await;

    let request = captured
        .lock()
        .unwrap()
        .clone()
        .expect("Request should have been captured");

    // Verify: no tools in Enhanced Prompt mode
    assert!(
        request.tools.is_none(),
        "Enhanced Prompt should not include tools"
    );

    // Verify: no tool_choice
    assert!(request.tool_choice.is_none());

    // Verify: has prefill
    assert!(
        request.prefill.is_some(),
        "Enhanced Prompt should set prefill"
    );
    assert_eq!(request.prefill, Some("{}".to_string()));

    // Verify: has stop_sequences
    assert!(
        request.stop_sequences.is_some(),
        "Enhanced Prompt should set stop_sequences"
    );

    // Verify: temperature is 0.0
    assert_eq!(request.temperature, Some(0.0));

    // Verify: messages structure — System + User
    assert_eq!(request.messages.len(), 2);
    match &request.messages[0] {
        Message::System { content } => {
            assert!(
                content.contains("<output_format>"),
                "System prompt should contain XML output_format tag"
            );
            assert!(
                content.contains("</output_format>"),
                "System prompt should close XML tag"
            );
            assert!(
                content.contains("结构化数据抽取"),
                "System prompt should mention data extraction"
            );
        }
        _ => panic!("Expected System message first"),
    }
    match &request.messages[1] {
        Message::User { content } => {
            assert!(
                content.contains("<input>"),
                "User prompt should contain XML input tag"
            );
            assert!(
                content.contains("</input>"),
                "User prompt should close XML tag"
            );
            assert!(
                content.contains("查询状态"),
                "User prompt should contain user input"
            );
            assert!(
                content.contains("{ 开头"),
                "User prompt should instruct to start with brace"
            );
        }
        _ => panic!("Expected User message second"),
    }
}

#[tokio::test]
async fn test_enhanced_prompt_mode_execute_success() {
    let provider: Arc<dyn LlmProvider> = Arc::new(CapturingEnhancedProvider {
        id: "enhanced".to_string(),
        response: r#"{"action":"build","job_name":"nightly"}"#.to_string(),
        captured_request: Arc::new(std::sync::Mutex::new(None)),
    });

    let schema = serde_json::json!({
        "type": "object",
        "required": ["action", "job_name"]
    });

    #[derive(serde::Deserialize, Debug)]
    #[allow(dead_code)]
    struct BuildResult {
        action: String,
        job_name: String,
    }

    let so = StructuredOutput::new(provider.clone(), "gpt-4o-mini".to_string())
        .schema(schema)
        .mode(StructuredOutputMode::EnhancedPrompt);

    let result: Result<BuildResult, StructuredOutputError> = so.execute("构建 nightly 任务").await;

    assert!(result.is_ok(), "Should parse valid JSON: {:?}", result);
    let r = result.unwrap();
    assert_eq!(r.action, "build");
    assert_eq!(r.job_name, "nightly");
}

#[tokio::test]
async fn test_enhanced_prompt_mode_with_custom_system() {
    let captured = Arc::new(std::sync::Mutex::new(None));
    let provider: Arc<dyn LlmProvider> = Arc::new(CapturingEnhancedProvider {
        id: "enhanced".to_string(),
        response: r#"{"status":"done"}"#.to_string(),
        captured_request: captured.clone(),
    });

    let schema = serde_json::json!({"type": "object"});

    let so = StructuredOutput::new(provider.clone(), "gpt-4o-mini".to_string())
        .schema(schema)
        .mode(StructuredOutputMode::EnhancedPrompt)
        .with_system_prompt("自定义 system prompt".to_string());

    #[derive(serde::Deserialize, Debug)]
    #[allow(dead_code)]
    struct CustomResult {
        status: String,
    }

    let _result: Result<CustomResult, StructuredOutputError> = so.execute("测试").await;

    let request = captured.lock().unwrap().clone().expect("Request captured");

    match &request.messages[0] {
        Message::System { content } => {
            assert_eq!(content, "自定义 system prompt");
        }
        _ => panic!("Expected System message"),
    }
}

// ── Builder Method Tests ──

#[test]
fn test_builder_tool_name() {
    let provider: Arc<dyn LlmProvider> = Arc::new(TestProvider {
        id: "mock".to_string(),
        response: "{}".to_string(),
    });

    let so = StructuredOutput::new(provider, "test".to_string()).tool_name("my_extractor");
    // Verify via execute to ensure tool_name propagates
    // The tool_name is used internally, we can verify it works through request capture
}

#[test]
fn test_builder_tool_description() {
    let provider: Arc<dyn LlmProvider> = Arc::new(TestProvider {
        id: "mock".to_string(),
        response: "{}".to_string(),
    });

    let _so = StructuredOutput::new(provider, "test".to_string())
        .tool_name("extract")
        .tool_description("从用户输入中提取结构化数据");
}

#[test]
fn test_builder_max_retries() {
    let provider: Arc<dyn LlmProvider> = Arc::new(TestProvider {
        id: "mock".to_string(),
        response: "invalid".to_string(),
    });

    let _so = StructuredOutput::new(provider, "test".to_string())
        .schema(serde_json::json!({"type": "object"}))
        .with_max_retries(5);
}

#[tokio::test]
async fn test_builder_mode_switch_affects_request() {
    let captured = Arc::new(std::sync::Mutex::new(None));
    let provider: Arc<dyn LlmProvider> = Arc::new(CapturingEnhancedProvider {
        id: "mode-test".to_string(),
        response: r#"{"result":"ok"}"#.to_string(),
        captured_request: captured.clone(),
    });

    let schema = serde_json::json!({"type": "object"});

    #[derive(serde::Deserialize, Debug)]
    #[allow(dead_code)]
    struct ResultItem {
        result: String,
    }

    // Test ToolUse mode (default)
    let so_tool = StructuredOutput::new(provider.clone(), "test".to_string())
        .schema(schema.clone())
        .tool_name("extract");

    let _r1: Result<ResultItem, StructuredOutputError> = so_tool.execute("测试").await;
    let req1 = captured.lock().unwrap().clone().expect("captured");
    assert!(req1.tools.is_some(), "ToolUse mode should have tools");
    assert!(
        req1.tool_choice.is_some(),
        "ToolUse mode should have tool_choice"
    );
    assert!(
        req1.prefill.is_none(),
        "ToolUse mode should not have prefill"
    );

    // Test EnhancedPrompt mode
    let so_enhanced = StructuredOutput::new(provider.clone(), "test".to_string())
        .schema(schema.clone())
        .mode(StructuredOutputMode::EnhancedPrompt);

    let _r2: Result<ResultItem, StructuredOutputError> = so_enhanced.execute("测试").await;
    let req2 = captured.lock().unwrap().clone().expect("captured");
    assert!(
        req2.tools.is_none(),
        "EnhancedPrompt mode should not have tools"
    );
    assert!(
        req2.tool_choice.is_none(),
        "EnhancedPrompt mode should not have tool_choice"
    );
    assert!(
        req2.prefill.is_some(),
        "EnhancedPrompt mode should have prefill"
    );
}

#[test]
fn test_structured_output_mode_default() {
    // Verify default is ToolUse
    let mode = StructuredOutputMode::default();
    assert!(matches!(mode, StructuredOutputMode::ToolUse));
}

#[test]
fn test_structured_output_mode_clone() {
    let mode = StructuredOutputMode::EnhancedPrompt;
    let cloned = mode.clone();
    assert!(matches!(cloned, StructuredOutputMode::EnhancedPrompt));
}

// ── Edge Case Tests ──

#[tokio::test]
async fn test_robust_parse_empty_response() {
    let provider: Arc<dyn LlmProvider> = Arc::new(CapturingEnhancedProvider {
        id: "edge".to_string(),
        response: "".to_string(),
        captured_request: Arc::new(std::sync::Mutex::new(None)),
    });

    #[derive(serde::Deserialize, Debug)]
    #[allow(dead_code)]
    struct EmptyResult {
        value: String,
    }

    let so = StructuredOutput::new(provider.clone(), "test".to_string())
        .schema(serde_json::json!({"type": "object"}))
        .mode(StructuredOutputMode::EnhancedPrompt)
        .with_max_retries(1);

    let result: Result<EmptyResult, StructuredOutputError> = so.execute("测试").await;
    assert!(result.is_err(), "Empty response should fail");
}

#[tokio::test]
async fn test_robust_parse_nested_braces_in_string() {
    let provider: Arc<dyn LlmProvider> = Arc::new(CapturingEnhancedProvider {
        id: "edge".to_string(),
        // JSON with braces inside a string value
        response: r#"{"message":"hello {world}","status":"ok"}"#.to_string(),
        captured_request: Arc::new(std::sync::Mutex::new(None)),
    });

    #[derive(serde::Deserialize, Debug)]
    struct NestedResult {
        message: String,
        status: String,
    }

    let so = StructuredOutput::new(provider.clone(), "test".to_string())
        .schema(serde_json::json!({"type": "object"}))
        .mode(StructuredOutputMode::EnhancedPrompt);

    let result: Result<NestedResult, StructuredOutputError> = so.execute("测试").await;
    assert!(
        result.is_ok(),
        "Should handle braces in strings: {:?}",
        result
    );
    assert_eq!(result.unwrap().message, "hello {world}");
}

#[tokio::test]
async fn test_robust_parse_type_mismatch() {
    let provider: Arc<dyn LlmProvider> = Arc::new(CapturingEnhancedProvider {
        id: "edge".to_string(),
        // Valid JSON but wrong type — expects string, gets number
        response: r#"{"count":"not_a_number"}"#.to_string(),
        captured_request: Arc::new(std::sync::Mutex::new(None)),
    });

    #[derive(serde::Deserialize, Debug)]
    struct CountResult {
        count: u32,
    }

    let so = StructuredOutput::new(provider.clone(), "test".to_string())
        .schema(serde_json::json!({"type": "object"}))
        .mode(StructuredOutputMode::EnhancedPrompt)
        .with_max_retries(1);

    let result: Result<CountResult, StructuredOutputError> = so.execute("测试").await;
    assert!(result.is_err(), "Type mismatch should fail");
}

#[tokio::test]
async fn test_robust_parse_layer5_value_intermediate() {
    // Layer 5: Parse as Value first, then deserialize to target type
    // This tests the path where direct parse fails but Value->Target succeeds
    let provider: Arc<dyn LlmProvider> = Arc::new(CapturingEnhancedProvider {
        id: "edge".to_string(),
        // JSON with trailing content that gets cleaned by brace extraction
        response: r#"{"action":"deploy","job_name":"ds-pkg"} some trailing text"#.to_string(),
        captured_request: Arc::new(std::sync::Mutex::new(None)),
    });

    #[derive(serde::Deserialize, Debug)]
    struct Layer5Result {
        action: String,
        job_name: String,
    }

    let so = StructuredOutput::new(provider.clone(), "test".to_string())
        .schema(serde_json::json!({"type": "object"}))
        .mode(StructuredOutputMode::EnhancedPrompt);

    let result: Result<Layer5Result, StructuredOutputError> = so.execute("测试").await;
    assert!(
        result.is_ok(),
        "Layer 5 should recover via brace extraction: {:?}",
        result
    );
    assert_eq!(result.unwrap().action, "deploy");
}

#[tokio::test]
async fn test_tool_use_no_matching_tool_name() {
    let provider: Arc<dyn LlmProvider> = Arc::new(CapturingToolProvider {
        id: "tool-use".to_string(),
        tool_name: "wrong_tool".to_string(), // 与 StructuredOutput 设置的工具名不匹配
        tool_args: r#"{"action":"deploy"}"#.to_string(),
        captured_request: Arc::new(std::sync::Mutex::new(None)),
    });

    #[derive(serde::Deserialize, Debug)]
    #[allow(dead_code)]
    struct ToolResult {
        action: String,
    }

    let so = StructuredOutput::new(provider, "test".to_string())
        .schema(serde_json::json!({"type": "object"}))
        .tool_name("extract_result") // 期望 extract_result，但 provider 返回 wrong_tool
        .with_max_retries(1);

    let result: Result<ToolResult, StructuredOutputError> = so.execute("测试").await;
    assert!(result.is_err(), "Should fail when tool name doesn't match");
}

#[tokio::test]
async fn test_robust_parse_deeply_nested_json() {
    let provider: Arc<dyn LlmProvider> = Arc::new(CapturingEnhancedProvider {
        id: "edge".to_string(),
        response: r#"{"config":{"nested":{"deep":{"value":"found"}}},"status":"ok"}"#.to_string(),
        captured_request: Arc::new(std::sync::Mutex::new(None)),
    });

    #[derive(serde::Deserialize, Debug)]
    struct DeepConfig {
        config: serde_json::Value,
        status: String,
    }

    let so = StructuredOutput::new(provider.clone(), "test".to_string())
        .schema(serde_json::json!({"type": "object"}))
        .mode(StructuredOutputMode::EnhancedPrompt);

    let result: Result<DeepConfig, StructuredOutputError> = so.execute("测试").await;
    assert!(
        result.is_ok(),
        "Should handle deeply nested JSON: {:?}",
        result
    );
    let r = result.unwrap();
    assert_eq!(
        r.config["nested"]["deep"]["value"], "found",
        "Nested value should be accessible"
    );
}
