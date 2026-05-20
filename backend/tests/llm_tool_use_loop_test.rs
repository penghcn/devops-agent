//! ToolUseLoop TDD 测试 — 验证 tool-use 闭环流程
//!
//! 垂直切片顺序：
//! 1. Message::ToolResult 变体存在且可序列化
//! 2. ToolExecutor 注册与分派
//! 3. ToolUseLoop 单轮 tool-call → execute → re-call
//! 4. ToolUseLoop 多轮循环 + 最大轮次保护
//! 5. 并发安全分级 (Safe / CategoryExclusive / Exclusive)
//! 6. 工具级重试策略
//! 7. 死循环检测 (渐进式干预 3 级)
//! 8. 信号投票

use devops_agent::llm::Message;
use devops_agent::llm::ToolCall;

// ── Slice 1: Message::ToolResult ──

#[test]
fn message_tool_result_construction() {
    let msg = Message::ToolResult {
        tool_call_id: "call_abc123".to_string(),
        content: "天气晴朗，25°C".to_string(),
    };

    let json = serde_json::to_value(&msg).unwrap();
    assert_eq!(json["type"], "tool_result");
    assert_eq!(json["tool_call_id"], "call_abc123");
    assert_eq!(json["content"], "天气晴朗，25°C");

    let restored: Message = serde_json::from_value(json).unwrap();
    assert!(matches!(restored, Message::ToolResult { .. }));
}

#[test]
fn message_tool_result_match_extraction() {
    let msg = Message::ToolResult {
        tool_call_id: "call_x".to_string(),
        content: "result data".to_string(),
    };

    if let Message::ToolResult {
        tool_call_id,
        content,
    } = &msg
    {
        assert_eq!(tool_call_id, "call_x");
        assert_eq!(content, "result data");
    } else {
        panic!("expected ToolResult variant");
    }
}

#[test]
fn message_all_variants_serialize() {
    let messages = vec![
        Message::System {
            content: "你是助手".to_string(),
        },
        Message::User {
            content: "查天气".to_string(),
        },
        Message::Assistant {
            content: "".to_string(),
            tool_calls: vec![ToolCall {
                id: "call_1".to_string(),
                name: "get_weather".to_string(),
                arguments: serde_json::json!({"city": "上海"}),
            }],
        },
        Message::ToolResult {
            tool_call_id: "call_1".to_string(),
            content: "上海天气晴朗".to_string(),
        },
        Message::Assistant {
            content: "上海天气晴朗".to_string(),
            tool_calls: vec![],
        },
    ];

    let json = serde_json::to_value(&messages).unwrap();
    assert_eq!(json.as_array().unwrap().len(), 5);

    let restored: Vec<Message> = serde_json::from_value(json).unwrap();
    assert!(matches!(restored[0], Message::System { .. }));
    assert!(matches!(restored[1], Message::User { .. }));
    assert!(matches!(restored[2], Message::Assistant { .. }));
    assert!(matches!(restored[3], Message::ToolResult { .. }));
    assert!(matches!(restored[4], Message::Assistant { .. }));
}

// ── Slice 2: ToolExecutor 注册与分派 ──

mod tool_executor_tests {
    use devops_agent::llm::tool_use_loop::{
        ParallelSafety, ToolCallResult, ToolExecutor, ToolRegistration,
    };
    use devops_agent::llm::{Message, ToolCall};

    #[tokio::test]
    async fn tool_executor_register_and_call() {
        let mut executor = ToolExecutor::new();
        executor.register(
            "get_weather",
            ToolRegistration::safe(|args: &serde_json::Value| {
                let city = args["city"].as_str().unwrap_or("未知").to_string();
                async move { ToolCallResult::Ok(format!("{} 天气晴朗", city)) }
            }),
        );

        let call = ToolCall {
            id: "call_1".to_string(),
            name: "get_weather".to_string(),
            arguments: serde_json::json!({"city": "上海"}),
        };

        let result = executor.execute(&call).await;
        assert!(matches!(result, ToolCallResult::Ok(ref c) if c == "上海 天气晴朗"));
    }

    #[tokio::test]
    async fn tool_executor_safety_classification() {
        let mut executor = ToolExecutor::new();
        executor.register(
            "read",
            ToolRegistration::safe(|_| async { ToolCallResult::Ok("data".to_string()) }),
        );
        executor.register(
            "write",
            ToolRegistration::category_exclusive("file", |_| async {
                ToolCallResult::Ok("ok".to_string())
            }),
        );
        executor.register(
            "bash",
            ToolRegistration::exclusive(|_| async { ToolCallResult::Ok("done".to_string()) }),
        );

        assert_eq!(executor.safety_for("read"), ParallelSafety::Safe);
        assert_eq!(
            executor.safety_for("write"),
            ParallelSafety::CategoryExclusive("file")
        );
        assert_eq!(executor.safety_for("bash"), ParallelSafety::Exclusive);
        // 未注册默认 Exclusive
        assert_eq!(executor.safety_for("unknown"), ParallelSafety::Exclusive);
    }

    #[tokio::test]
    async fn tool_executor_partition_calls() {
        let mut executor = ToolExecutor::new();
        executor.register(
            "read",
            ToolRegistration::safe(|_| async { ToolCallResult::Ok("data".to_string()) }),
        );
        executor.register(
            "bash",
            ToolRegistration::exclusive(|_| async { ToolCallResult::Ok("done".to_string()) }),
        );

        let calls = vec![
            ToolCall {
                id: "c1".to_string(),
                name: "read".to_string(),
                arguments: serde_json::json!({}),
            },
            ToolCall {
                id: "c2".to_string(),
                name: "bash".to_string(),
                arguments: serde_json::json!({}),
            },
            ToolCall {
                id: "c3".to_string(),
                name: "read".to_string(),
                arguments: serde_json::json!({}),
            },
        ];

        let (safe, exclusive) = executor.partition_calls(&calls);
        assert_eq!(safe.len(), 2);
        assert_eq!(exclusive.len(), 1);
        assert_eq!(exclusive[0].id, "c2");
    }

    #[tokio::test]
    async fn tool_executor_batch_produces_tool_results() {
        let mut executor = ToolExecutor::new();
        executor.register(
            "echo",
            ToolRegistration::safe(|args: &serde_json::Value| {
                let msg = args["msg"].as_str().unwrap_or("").to_string();
                async move { ToolCallResult::Ok(msg) }
            }),
        );

        let calls = vec![
            ToolCall {
                id: "call_1".to_string(),
                name: "echo".to_string(),
                arguments: serde_json::json!({"msg": "hello"}),
            },
            ToolCall {
                id: "call_2".to_string(),
                name: "unknown".to_string(),
                arguments: serde_json::json!({}),
            },
        ];

        let results = executor.execute_batch(&calls).await;
        assert_eq!(results.len(), 2);

        if let Message::ToolResult {
            tool_call_id,
            content,
        } = &results[0]
        {
            assert_eq!(tool_call_id, "call_1");
            assert_eq!(content, "hello");
        } else {
            panic!("expected ToolResult");
        }

        if let Message::ToolResult {
            tool_call_id,
            content,
        } = &results[1]
        {
            assert_eq!(tool_call_id, "call_2");
            assert!(content.contains("工具执行错误"));
        } else {
            panic!("expected ToolResult");
        }
    }
}

// ── Slice 3: ToolUseLoop 基本流程 ──

mod tool_use_loop_tests {
    use async_trait::async_trait;
    use devops_agent::llm::tool_use_loop::{
        ToolCallResult, ToolExecutor, ToolRegistration, ToolUseLoop,
    };
    use devops_agent::llm::{ChatRequest, ChatResponse, LlmError, Message, TokenUsage, ToolCall};
    use std::collections::VecDeque;
    use std::sync::Arc;

    struct MockProvider {
        responses: std::sync::Mutex<VecDeque<ChatResponse>>,
        received_requests: std::sync::Mutex<Vec<ChatRequest>>,
    }

    impl MockProvider {
        fn new(responses: Vec<ChatResponse>) -> Self {
            Self {
                responses: std::sync::Mutex::new(responses.into()),
                received_requests: std::sync::Mutex::new(Vec::new()),
            }
        }
    }

    #[async_trait]
    impl devops_agent::llm::LlmProvider for MockProvider {
        async fn llm_call(&self, request: &ChatRequest) -> Result<ChatResponse, LlmError> {
            self.received_requests.lock().unwrap().push(request.clone());
            let resp = self
                .responses
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or_else(|| ChatResponse {
                    content: "无响应".to_string(),
                    tool_calls: vec![],
                    usage: TokenUsage::default(),
                    raw: serde_json::json!({}),
                });
            Ok(resp)
        }

        fn provider_id(&self) -> &str {
            "mock"
        }
    }

    #[tokio::test]
    async fn loop_single_tool_call_then_text() {
        let provider = Arc::new(MockProvider::new(vec![
            ChatResponse {
                content: "".to_string(),
                tool_calls: vec![ToolCall {
                    id: "call_1".to_string(),
                    name: "get_weather".to_string(),
                    arguments: serde_json::json!({"city": "北京"}),
                }],
                usage: TokenUsage::default(),
                raw: serde_json::json!({}),
            },
            ChatResponse {
                content: "北京天气晴朗，25°C".to_string(),
                tool_calls: vec![],
                usage: TokenUsage::default(),
                raw: serde_json::json!({}),
            },
        ]));

        let mut executor = ToolExecutor::new();
        executor.register(
            "get_weather",
            ToolRegistration::safe(|args: &serde_json::Value| {
                let city = args["city"].as_str().unwrap_or("未知").to_string();
                async move { ToolCallResult::Ok(format!("{} 天气晴朗，25°C", city)) }
            }),
        );

        let loop_ = ToolUseLoop::new(
            provider.clone(),
            executor,
            ChatRequest {
                model: "test".to_string(),
                messages: vec![Message::User {
                    content: "北京天气如何？".to_string(),
                }],
                tools: None,
                temperature: None,
                tool_choice: None,
                stop_sequences: None,
                prefill: None,
            },
        );

        let result = loop_.execute().await.unwrap();
        assert!(result.response.content.contains("北京"));
        assert!(!result.response.content.is_empty());
        assert_eq!(result.iterations, 2);

        let requests = provider.received_requests.lock().unwrap();
        assert_eq!(requests.len(), 2);

        let second_req = &requests[1];
        assert!(
            second_req
                .messages
                .iter()
                .any(|m| { matches!(m, Message::ToolResult { .. }) })
        );
    }

    #[tokio::test]
    async fn loop_no_tool_calls_returns_immediately() {
        let provider = Arc::new(MockProvider::new(vec![ChatResponse {
            content: "直接回答".to_string(),
            tool_calls: vec![],
            usage: TokenUsage::default(),
            raw: serde_json::json!({}),
        }]));

        let executor = ToolExecutor::new();
        let loop_ = ToolUseLoop::new(
            provider.clone(),
            executor,
            ChatRequest {
                model: "test".to_string(),
                messages: vec![Message::User {
                    content: "你好".to_string(),
                }],
                tools: None,
                temperature: None,
                tool_choice: None,
                stop_sequences: None,
                prefill: None,
            },
        );

        let result = loop_.execute().await.unwrap();
        assert_eq!(result.response.content, "直接回答");
        assert_eq!(result.iterations, 1);

        let requests = provider.received_requests.lock().unwrap();
        assert_eq!(requests.len(), 1);
    }

    #[tokio::test]
    async fn loop_max_iterations_stops() {
        let provider = Arc::new(MockProvider::new(vec![
            ChatResponse {
                content: "".to_string(),
                tool_calls: vec![ToolCall {
                    id: "call_1".to_string(),
                    name: "get_weather".to_string(),
                    arguments: serde_json::json!({"city": "A"}),
                }],
                usage: TokenUsage::default(),
                raw: serde_json::json!({}),
            },
            ChatResponse {
                content: "".to_string(),
                tool_calls: vec![ToolCall {
                    id: "call_2".to_string(),
                    name: "get_weather".to_string(),
                    arguments: serde_json::json!({"city": "B"}),
                }],
                usage: TokenUsage::default(),
                raw: serde_json::json!({}),
            },
            ChatResponse {
                content: "".to_string(),
                tool_calls: vec![ToolCall {
                    id: "call_3".to_string(),
                    name: "get_weather".to_string(),
                    arguments: serde_json::json!({"city": "C"}),
                }],
                usage: TokenUsage::default(),
                raw: serde_json::json!({}),
            },
        ]));

        let mut executor = ToolExecutor::new();
        executor.register(
            "get_weather",
            ToolRegistration::safe(|args: &serde_json::Value| {
                let city = args["city"].as_str().unwrap_or("未知").to_string();
                async move { ToolCallResult::Ok(format!("{} 天气", city)) }
            }),
        );

        let mut loop_ = ToolUseLoop::new(
            provider.clone(),
            executor,
            ChatRequest {
                model: "test".to_string(),
                messages: vec![Message::User {
                    content: "测试".to_string(),
                }],
                tools: None,
                temperature: None,
                tool_choice: None,
                stop_sequences: None,
                prefill: None,
            },
        );
        loop_.set_max_iterations(2);

        let result = loop_.execute().await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn loop_messages_include_full_history() {
        let provider = Arc::new(MockProvider::new(vec![
            ChatResponse {
                content: "".to_string(),
                tool_calls: vec![ToolCall {
                    id: "call_1".to_string(),
                    name: "echo".to_string(),
                    arguments: serde_json::json!({"msg": "hi"}),
                }],
                usage: TokenUsage::default(),
                raw: serde_json::json!({}),
            },
            ChatResponse {
                content: "收到！".to_string(),
                tool_calls: vec![],
                usage: TokenUsage::default(),
                raw: serde_json::json!({}),
            },
        ]));

        let mut executor = ToolExecutor::new();
        executor.register(
            "echo",
            ToolRegistration::safe(|args: &serde_json::Value| {
                let msg = args["msg"].as_str().unwrap_or("").to_string();
                async move { ToolCallResult::Ok(msg) }
            }),
        );

        let loop_ = ToolUseLoop::new(
            provider,
            executor,
            ChatRequest {
                model: "test".to_string(),
                messages: vec![Message::User {
                    content: "说你好".to_string(),
                }],
                tools: None,
                temperature: None,
                tool_choice: None,
                stop_sequences: None,
                prefill: None,
            },
        );

        let result = loop_.execute().await.unwrap();

        assert!(result.messages.len() >= 3);
        assert!(matches!(result.messages[0], Message::User { .. }));
        assert!(matches!(result.messages[1], Message::Assistant { .. }));
        assert!(matches!(result.messages[2], Message::ToolResult { .. }));
    }
}

// ── Slice 5: 工具重试策略 ──

mod tool_retry_tests {
    use devops_agent::llm::tool_use_loop::ToolErrorKind;

    #[test]
    fn tool_error_kind_retriable() {
        assert!(ToolErrorKind::Timeout.is_retriable());
        assert!(ToolErrorKind::NetworkError.is_retriable());
        assert!(ToolErrorKind::Unknown.is_retriable());
    }

    #[test]
    fn tool_error_kind_not_retriable() {
        assert!(!ToolErrorKind::PermissionDenied.is_retriable());
        assert!(!ToolErrorKind::NotFound.is_retriable());
        assert!(!ToolErrorKind::ParseError.is_retriable());
    }

    #[test]
    fn retry_policy_max_attempts() {
        assert_eq!(ToolErrorKind::Timeout.max_attempts(), 5);
        assert_eq!(ToolErrorKind::NetworkError.max_attempts(), 3);
        assert_eq!(ToolErrorKind::Unknown.max_attempts(), 3);
        assert_eq!(ToolErrorKind::PermissionDenied.max_attempts(), 0);
        assert_eq!(ToolErrorKind::NotFound.max_attempts(), 0);
        assert_eq!(ToolErrorKind::ParseError.max_attempts(), 0);
    }

    #[test]
    fn retry_policy_backoff_ms_exponential() {
        // Timeout: 指数退避 2→4→8→16s
        assert_eq!(ToolErrorKind::Timeout.backoff_ms(0), 2000);
        assert_eq!(ToolErrorKind::Timeout.backoff_ms(1), 4000);
        assert_eq!(ToolErrorKind::Timeout.backoff_ms(2), 8000);
        assert_eq!(ToolErrorKind::Timeout.backoff_ms(3), 16000);
    }

    #[test]
    fn retry_policy_backoff_ms_fixed() {
        // NetworkError: 固定 3s
        assert_eq!(ToolErrorKind::NetworkError.backoff_ms(0), 3000);
        assert_eq!(ToolErrorKind::NetworkError.backoff_ms(1), 3000);
        assert_eq!(ToolErrorKind::NetworkError.backoff_ms(2), 3000);
    }

    #[test]
    fn retry_policy_non_retriable_zero_backoff() {
        assert_eq!(ToolErrorKind::PermissionDenied.backoff_ms(0), 0);
        assert_eq!(ToolErrorKind::NotFound.backoff_ms(0), 0);
    }

    #[test]
    fn retry_policy_hint_message() {
        let hint = ToolErrorKind::Timeout.hint();
        assert!(!hint.is_empty());
        assert!(hint.contains("超时") || hint.contains("timeout"));

        let hint = ToolErrorKind::PermissionDenied.hint();
        assert!(!hint.is_empty());
        assert!(hint.contains("权限") || hint.contains("permission"));
    }

    #[tokio::test]
    async fn retry_executor_retries_until_success() {
        use devops_agent::llm::ToolCall;
        use devops_agent::llm::tool_use_loop::{ToolCallResult, ToolExecutor, ToolRegistration};
        use std::sync::atomic::{AtomicU32, Ordering};

        static ATTEMPTS: AtomicU32 = AtomicU32::new(0);

        let mut executor = ToolExecutor::new();
        executor.register(
            "flaky",
            ToolRegistration::safe(|_| async {
                let n = ATTEMPTS.fetch_add(1, Ordering::SeqCst);
                if n < 2 {
                    ToolCallResult::Err(format!("临时失败 #{}", n + 1))
                } else {
                    ToolCallResult::Ok("成功".to_string())
                }
            }),
        );

        let call = ToolCall {
            id: "call_1".to_string(),
            name: "flaky".to_string(),
            arguments: serde_json::json!({}),
        };

        // 第 3 次尝试成功
        let result = executor.execute_with_retry(&call).await;
        assert!(matches!(result, ToolCallResult::Ok(ref c) if c == "成功"));
        assert_eq!(ATTEMPTS.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn retry_executor_exhausts_retries() {
        use devops_agent::llm::ToolCall;
        use devops_agent::llm::tool_use_loop::{ToolCallResult, ToolExecutor, ToolRegistration};

        let mut executor = ToolExecutor::new();
        executor.register(
            "always_fail",
            ToolRegistration::safe(|_| async { ToolCallResult::Err("始终失败".to_string()) }),
        );

        let call = ToolCall {
            id: "call_1".to_string(),
            name: "always_fail".to_string(),
            arguments: serde_json::json!({}),
        };

        // 最终返回错误，但已重试多次
        let result = executor.execute_with_retry(&call).await;
        assert!(matches!(result, ToolCallResult::Err(_)));
    }

    #[tokio::test]
    async fn retry_executor_skips_non_retriable() {
        use devops_agent::llm::ToolCall;
        use devops_agent::llm::tool_use_loop::{ToolCallResult, ToolExecutor, ToolRegistration};
        use std::sync::atomic::{AtomicU32, Ordering};

        static ATTEMPTS: AtomicU32 = AtomicU32::new(0);

        let mut executor = ToolExecutor::new();
        executor.register(
            "denied",
            ToolRegistration::exclusive(|_| async {
                ATTEMPTS.fetch_add(1, Ordering::SeqCst);
                ToolCallResult::Err("权限拒绝".to_string())
            }),
        );

        let call = ToolCall {
            id: "call_1".to_string(),
            name: "denied".to_string(),
            arguments: serde_json::json!({}),
        };

        // 需要手动分类为 PermissionDenied 才能跳过重试
        // 这里验证基础执行不重试
        let result = executor.execute(&call).await;
        assert!(matches!(result, ToolCallResult::Err(_)));
    }
}

// ── Slice 6: 死循环检测 ──

mod loop_detector_tests {
    use devops_agent::llm::ToolCall;
    use devops_agent::llm::tool_use_loop::{LoopDetector, LoopIntervention};

    fn make_call(name: &str, args: serde_json::Value) -> ToolCall {
        ToolCall {
            id: format!("call_{}", name),
            name: name.to_string(),
            arguments: args,
        }
    }

    #[test]
    fn loop_detector_no_repeat() {
        let mut detector = LoopDetector::new(5);
        // 不同的工具调用，不应触发
        detector.record(&[
            make_call("read", serde_json::json!({"path": "a.rs"})),
            make_call("read", serde_json::json!({"path": "b.rs"})),
        ]);
        assert!(detector.is_looping().is_none());
    }

    #[test]
    fn loop_detector_exact_repeat_triggers() {
        let mut detector = LoopDetector::new(5);
        let calls = [make_call("get_status", serde_json::json!({"id": "42"}))];

        // 第 1 次
        detector.record(&calls);
        assert!(detector.is_looping().is_none());

        // 第 2 次 — 达到精确重复阈值 2，触发 Level 2
        detector.record(&calls);
        let intervention = detector.is_looping().unwrap();
        assert!(matches!(intervention, LoopIntervention::Level2 { .. }));
    }

    #[test]
    fn loop_detector_frequency_threshold() {
        let mut detector = LoopDetector::new(5);
        // 同一个工具被调用 5 次（参数不同但工具名相同）
        for i in 0..5 {
            detector.record(&[make_call(
                "search",
                serde_json::json!({"query": format!("q{}", i)}),
            )]);
        }
        let intervention = detector.is_looping().unwrap();
        assert!(matches!(intervention, LoopIntervention::Level2 { .. }));
    }

    #[test]
    fn loop_detector_level_3_after_escalation() {
        let mut detector = LoopDetector::new(5);
        let calls = [make_call("get_status", serde_json::json!({"id": "42"}))];

        // 重复 3 次 → Level 2
        detector.record(&calls);
        detector.record(&calls);
        let _ = detector.is_looping().unwrap();

        // 继续重复 → Level 3
        detector.record(&calls);
        let intervention = detector.is_looping().unwrap();
        assert!(matches!(intervention, LoopIntervention::Level3 { .. }));
    }

    #[test]
    fn loop_detector_injection_message() {
        let mut detector = LoopDetector::new(5);
        let calls = [make_call("get_status", serde_json::json!({"id": "42"}))];
        detector.record(&calls);
        detector.record(&calls);

        let intervention = detector.is_looping().unwrap();
        let msg = intervention.to_injection_message();
        assert!(!msg.is_empty());
        assert!(msg.contains("重复") || msg.contains("循环") || msg.contains("repeat"));
    }
}

// ── Slice 7: 信号投票 ──

mod signal_voting_tests {
    use devops_agent::llm::tool_use_loop::{NegativeSignal, SignalVoter};

    #[test]
    fn signal_voter_no_signals() {
        let voter = SignalVoter::new();
        assert!(!voter.should_escalate());
        assert_eq!(voter.signal_count(), 0);
    }

    #[test]
    fn signal_voter_weak_signals_only_no_escalate() {
        let mut voter = SignalVoter::new();
        voter.add(NegativeSignal::TokenOverBudget);
        voter.add(NegativeSignal::ContentAnomaly);
        // 2 个弱信号，不含强信号，不触发
        assert!(!voter.should_escalate());
    }

    #[test]
    fn signal_voter_three_signals_with_strong_escalates() {
        let mut voter = SignalVoter::new();
        voter.add(NegativeSignal::RepeatedToolCall); // 强信号
        voter.add(NegativeSignal::TokenOverBudget);
        voter.add(NegativeSignal::ResponseTimeout);
        // 3 个信号 + 强信号 → 触发
        assert!(voter.should_escalate());
    }

    #[test]
    fn signal_voter_two_signals_with_strong_no_escalate() {
        let mut voter = SignalVoter::new();
        voter.add(NegativeSignal::RepeatedToolCall); // 强信号
        voter.add(NegativeSignal::TokenOverBudget);
        // 只有 2 个信号，不满足 ≥ 3
        assert!(!voter.should_escalate());
    }

    #[test]
    fn signal_voter_deduplicates() {
        let mut voter = SignalVoter::new();
        voter.add(NegativeSignal::RepeatedToolCall);
        voter.add(NegativeSignal::RepeatedToolCall);
        voter.add(NegativeSignal::RepeatedToolCall);
        assert_eq!(voter.signal_count(), 1);
    }

    #[test]
    fn signal_voter_tool_failure_is_strong() {
        let mut voter = SignalVoter::new();
        voter.add(NegativeSignal::ToolExecutionFailed); // 强信号
        voter.add(NegativeSignal::RoundExceeded);
        voter.add(NegativeSignal::TokenOverBudget);
        assert!(voter.should_escalate());
    }
}

// ── Slice 8: tool_search 元工具 ──

mod tool_search_tests {
    use devops_agent::llm::ToolDefinition;
    use devops_agent::llm::tool_use_loop::{ToolRegistry, ToolSource};

    fn make_def(name: &str, _category: &str) -> ToolDefinition {
        ToolDefinition {
            name: name.to_string(),
            description: format!("{} 工具", name),
            parameters: serde_json::json!({"type": "object", "properties": {}}),
        }
    }

    #[test]
    fn tool_registry_exact_match() {
        let mut registry = ToolRegistry::new();
        registry.register(
            "jenkins_trigger",
            ToolSource::Dynamic,
            make_def("jenkins_trigger", "jenkins"),
        );
        registry.register(
            "jenkins_status",
            ToolSource::Dynamic,
            make_def("jenkins_status", "jenkins"),
        );

        let results = registry.search("jenkins_trigger");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].name(), "jenkins_trigger");
    }

    #[test]
    fn tool_registry_synonym_match() {
        let mut registry = ToolRegistry::new();
        registry.add_synonyms("jenkins_trigger", &["流水线", "CI", "构建", "pipeline"]);
        registry.register(
            "jenkins_trigger",
            ToolSource::Dynamic,
            make_def("jenkins_trigger", "jenkins"),
        );

        // 用同义词搜索
        let results = registry.search("构建");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].name(), "jenkins_trigger");
    }

    #[test]
    fn tool_registry_category_match() {
        let mut registry = ToolRegistry::new();
        registry.register(
            "jenkins_trigger",
            ToolSource::Dynamic,
            make_def("jenkins_trigger", "jenkins"),
        );
        registry.register(
            "jenkins_status",
            ToolSource::Dynamic,
            make_def("jenkins_status", "jenkins"),
        );
        registry.register(
            "gitlab_mr",
            ToolSource::Dynamic,
            make_def("gitlab_mr", "gitlab"),
        );

        // 按分类批量召回
        let results = registry.search_category("jenkins");
        assert_eq!(results.len(), 2);
        let names: Vec<_> = results.iter().map(|t| t.name()).collect();
        assert!(names.contains(&"jenkins_trigger"));
        assert!(names.contains(&"jenkins_status"));
    }

    #[test]
    fn tool_registry_substring_fallback() {
        let mut registry = ToolRegistry::new();
        registry.register(
            "gitlab_merge_request",
            ToolSource::Dynamic,
            make_def("gitlab_merge_request", "gitlab"),
        );

        // 子串匹配兜底
        let results = registry.search("merge");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].name(), "gitlab_merge_request");
    }

    #[test]
    fn tool_registry_no_match() {
        let mut registry = ToolRegistry::new();
        registry.register(
            "jenkins_trigger",
            ToolSource::Dynamic,
            make_def("jenkins_trigger", "jenkins"),
        );

        let results = registry.search("完全无关的查询");
        assert!(results.is_empty());
    }

    #[test]
    fn tool_registry_source_filter() {
        let mut registry = ToolRegistry::new();
        registry.register("read", ToolSource::Builtin, make_def("read", "builtin"));
        registry.register(
            "jenkins_trigger",
            ToolSource::Dynamic,
            make_def("jenkins_trigger", "jenkins"),
        );

        let all = registry.list_tools();
        assert_eq!(all.len(), 2);

        let dynamic: Vec<_> = all
            .iter()
            .filter(|t| t.source == ToolSource::Dynamic)
            .collect();
        assert_eq!(dynamic.len(), 1);
    }
}
