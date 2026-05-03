//! LLM Router and Structured Output tests
//!
//! Verifies:
//! - ModelRouter L1/L2 task classification
//! - Model selection based on task level
//! - Provider routing
//! - StructuredOutput schema-constrained output with retry

use devops_agent::llm::router::ProviderModels;
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

    let so = StructuredOutput::new(provider, "gpt-4o-mini".to_string(), schema);
    assert_eq!(so.model, "gpt-4o-mini");
    assert_eq!(so.max_retries, 3);
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

    let so = StructuredOutput::new(provider, "gpt-4o-mini".to_string(), schema);

    #[derive(serde::Deserialize, Debug)]
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

    let so = StructuredOutput::new(provider, "gpt-4o-mini".to_string(), schema);

    #[derive(serde::Deserialize, Debug)]
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

    let so = StructuredOutput::new(provider, "gpt-4o-mini".to_string(), schema);

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
    let so = StructuredOutput::new(provider.clone(), "gpt-4o-mini".to_string(), schema)
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

    let so = StructuredOutput::new(provider, "gpt-4o-mini".to_string(), schema).with_max_retries(2);

    #[derive(serde::Deserialize, Debug)]
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
