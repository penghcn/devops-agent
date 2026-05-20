//! Prompt Builder integration tests
//!
//! Verifies seven-layer prompt construction, SessionSlots behavior,
//! memory filtering, and token estimation.

use devops_agent::llm::prompt_builder::{
    MemorySlot, PromptBuilder, PromptBuilderConfig, SessionSlots, StaticPrefix, estimate_tokens,
};
use devops_agent::llm::{Message, ToolDefinition};

// ── StaticPrefix Tests ──

#[test]
fn test_static_prefix_compilation() {
    let prefix = StaticPrefix::new("核心".into(), "工具".into(), "规则".into());
    assert_eq!(prefix.as_str(), "核心\n\n工具\n\n规则");
}

#[test]
fn test_static_prefix_empty_parts() {
    let prefix = StaticPrefix::new("核心".into(), String::new(), String::new());
    let s = prefix.as_str();
    assert!(s.starts_with("核心"));
}

// ── SessionSlots Boundary Tests ──

#[test]
fn test_session_slots_empty_context() {
    let slots = SessionSlots::new();
    let ctx = slots.to_context();
    assert!(ctx.is_empty());
}

#[test]
fn test_session_slots_goal_only() {
    let mut slots = SessionSlots::new();
    slots.goal = Some("部署 ds-pkg".into());
    let ctx = slots.to_context();
    assert!(ctx.contains("当前目标: 部署 ds-pkg"));
    assert!(!ctx.contains("最近步骤"));
    assert!(!ctx.contains("活跃错误"));
}

#[test]
fn test_session_steps_overflow_removes_oldest() {
    let mut slots = SessionSlots::new();
    // Add 7 steps — should keep only the last 5
    for i in 1..=7 {
        slots.add_step(format!("步骤 {}", i));
    }
    assert_eq!(slots.recent_steps.len(), 5);
    assert_eq!(slots.recent_steps[0], "步骤 3");
    assert_eq!(slots.recent_steps[4], "步骤 7");
}

#[test]
fn test_session_errors_capped_at_three() {
    let mut slots = SessionSlots::new();
    for i in 1..=5 {
        slots.add_error(format!("错误 {}", i));
    }
    assert_eq!(slots.active_errors.len(), 3);
    assert_eq!(slots.active_errors[0], "错误 3");
    assert_eq!(slots.active_errors[2], "错误 5");
}

#[test]
fn test_session_slots_truncation() {
    let mut slots = SessionSlots::new();
    slots.max_slot_chars = 8;
    slots.add_step("这是一个很长的步骤结果".into());
    assert!(slots.recent_steps[0].ends_with('…'));
    assert!(slots.recent_steps[0].len() < "这是一个很长的步骤结果".len());
}

#[test]
fn test_session_slots_to_context_formatting() {
    let mut slots = SessionSlots::new();
    slots.goal = Some("构建 nightly".into());
    slots.add_step("验证配置".into());
    slots.add_step("触发构建".into());
    slots.add_error("超时".into());

    let ctx = slots.to_context();
    assert!(ctx.contains("当前目标: 构建 nightly"));
    assert!(ctx.contains("1. 验证配置"));
    assert!(ctx.contains("2. 触发构建"));
    assert!(ctx.contains("活跃错误"));
    assert!(ctx.contains("1. 超时"));
}

// ── PromptBuilderConfig Tests ──

#[test]
fn test_prompt_builder_config_defaults() {
    let config = PromptBuilderConfig::default();
    assert_eq!(config.memory_score_threshold, 0.7);
    assert_eq!(config.max_memory_slots, 5);
    assert_eq!(config.max_memory_chars, 300);
}

#[test]
fn test_prompt_builder_config_custom() {
    let config = PromptBuilderConfig {
        memory_score_threshold: 0.5,
        max_memory_slots: 10,
        max_memory_chars: 100,
    };
    assert_eq!(config.memory_score_threshold, 0.5);
    assert_eq!(config.max_memory_slots, 10);
    assert_eq!(config.max_memory_chars, 100);
}

// ── PromptBuilder Build Tests ──

fn make_builder() -> PromptBuilder {
    PromptBuilder::with_defaults(
        "你是 DevOps Agent。".into(),
        "可用工具: get_time, get_env".into(),
        "项目规则: 使用中文。".into(),
    )
}

#[test]
fn test_build_basic_request() {
    let builder = make_builder();
    let req = builder.build(
        "部署 ds-pkg".into(),
        vec![],
        &SessionSlots::new(),
        vec![],
        vec![],
    );

    assert_eq!(req.messages.len(), 2);
    assert!(matches!(req.messages.first(), Some(Message::System { .. })));
    assert!(matches!(req.messages.last(), Some(Message::User { .. })));

    // System should contain static prefix
    if let Message::System { content } = &req.messages[0] {
        assert!(content.contains("你是 DevOps Agent"));
        assert!(content.contains("可用工具"));
        assert!(content.contains("项目规则"));
    } else {
        panic!("Expected System message");
    }

    // Last message should be user prompt
    if let Message::User { content } = &req.messages[1] {
        assert_eq!(content, "部署 ds-pkg");
    } else {
        panic!("Expected User message");
    }
}

#[test]
fn test_build_with_conversation_history() {
    let builder = make_builder();
    let conversation = vec![
        Message::User {
            content: "之前的问题".into(),
        },
        Message::Assistant {
            content: "之前的回答".into(),
            tool_calls: vec![],
        },
    ];

    let req = builder.build(
        "新问题".into(),
        vec![],
        &SessionSlots::new(),
        vec![],
        conversation,
    );

    // System + 2 history + 1 user = 4 messages
    assert_eq!(req.messages.len(), 4);
    assert!(matches!(req.messages.last(), Some(Message::User { content }) if content == "新问题"));
}

#[test]
fn test_build_with_dynamic_tools() {
    let builder = make_builder();
    let tools = vec![ToolDefinition {
        name: "jenkins_build".into(),
        description: "触发 Jenkins 构建".into(),
        parameters: serde_json::json!({"type": "object"}),
    }];

    let req = builder.build(
        "构建项目".into(),
        vec![],
        &SessionSlots::new(),
        tools,
        vec![],
    );

    assert!(req.tools.is_some());
    let t = req.tools.as_ref().unwrap();
    assert_eq!(t.len(), 1);
    assert_eq!(t[0].name, "jenkins_build");
}

#[test]
fn test_build_without_dynamic_tools() {
    let builder = make_builder();
    let req = builder.build(
        "简单问题".into(),
        vec![],
        &SessionSlots::new(),
        vec![],
        vec![],
    );

    assert!(req.tools.is_none());
}

#[test]
fn test_build_with_memory_injection() {
    let builder = make_builder();
    let memory = vec![
        MemorySlot {
            content: "用户偏好使用 dev 环境".into(),
            score: 0.9,
        },
        MemorySlot {
            content: "低分记忆不应出现".into(),
            score: 0.3,
        },
        MemorySlot {
            content: "另一个高分记忆".into(),
            score: 0.85,
        },
    ];

    let req = builder.build(
        "部署应用".into(),
        memory,
        &SessionSlots::new(),
        vec![],
        vec![],
    );

    if let Message::System { content } = &req.messages[0] {
        assert!(content.contains("用户偏好使用 dev 环境"));
        assert!(!content.contains("低分记忆不应出现"));
        assert!(content.contains("另一个高分记忆"));
        assert!(content.contains("相关记忆"));
    } else {
        panic!("Expected System message");
    }
}

#[test]
fn test_build_memory_max_slots_limit() {
    let config = PromptBuilderConfig {
        max_memory_slots: 2,
        ..Default::default()
    };
    let prefix = StaticPrefix::new("核心".into(), "工具".into(), "规则".into());
    let builder = PromptBuilder::new(prefix, config);

    // Provide 5 high-score memories, only 2 should be injected
    let memory: Vec<MemorySlot> = (1..=5)
        .map(|i| MemorySlot {
            content: format!("记忆 {}", i),
            score: 0.9,
        })
        .collect();

    let req = builder.build("测试".into(), memory, &SessionSlots::new(), vec![], vec![]);

    if let Message::System { content } = &req.messages[0] {
        assert!(content.contains("记忆 1"));
        assert!(content.contains("记忆 2"));
        assert!(!content.contains("记忆 3"));
    } else {
        panic!("Expected System message");
    }
}

#[test]
fn test_build_memory_chars_truncation() {
    let config = PromptBuilderConfig {
        max_memory_chars: 10,
        ..Default::default()
    };
    let prefix = StaticPrefix::new("核心".into(), "工具".into(), "规则".into());
    let builder = PromptBuilder::new(prefix, config);

    let memory = vec![MemorySlot {
        content: "这是一条非常长的记忆内容应该被截断".into(),
        score: 0.9,
    }];

    let req = builder.build("测试".into(), memory, &SessionSlots::new(), vec![], vec![]);

    if let Message::System { content } = &req.messages[0] {
        assert!(content.contains("…"));
    } else {
        panic!("Expected System message");
    }
}

#[test]
fn test_build_system_prompt_with_session() {
    let builder = make_builder();
    let mut session = SessionSlots::new();
    session.goal = Some("构建并部署".into());
    session.add_step("验证配置".into());

    let memory = vec![MemorySlot {
        content: "偏好信息".into(),
        score: 0.9,
    }];

    let system = builder.build_system_prompt(memory, &session);
    assert!(system.contains("你是 DevOps Agent"));
    assert!(system.contains("偏好信息"));
    assert!(system.contains("当前目标: 构建并部署"));
    assert!(system.contains("验证配置"));
}

#[test]
fn test_build_system_prompt_no_memory() {
    let builder = make_builder();
    let system = builder.build_system_prompt(vec![], &SessionSlots::new());
    assert!(system.contains("你是 DevOps Agent"));
    assert!(!system.contains("相关记忆"));
    assert!(!system.contains("会话上下文"));
}

// ── Token Estimation Tests ──

#[test]
fn test_estimate_tokens_ascii() {
    // ASCII: ~4 chars per token
    let tokens = estimate_tokens("hello world");
    assert!(tokens >= 2);
    assert!(tokens <= 4);
}

#[test]
fn test_estimate_tokens_cjk() {
    // CJK: ~1.5 tokens per char
    let tokens = estimate_tokens("你好世界");
    assert!(tokens >= 5);
    assert!(tokens <= 7);
}

#[test]
fn test_estimate_tokens_mixed() {
    let tokens = estimate_tokens("hello 你好 world 世界");
    assert!(tokens >= 5);
    assert!(tokens <= 10);
}

#[test]
fn test_estimate_tokens_empty() {
    assert_eq!(estimate_tokens(""), 0);
}

#[test]
fn test_estimate_tokens_consistency() {
    // Same string should always return same result
    let s = "测试 test 123";
    let a = estimate_tokens(s);
    let b = estimate_tokens(s);
    assert_eq!(a, b);
}

// ── PromptBuilder estimate_system_tokens ──

#[test]
fn test_estimate_system_tokens() {
    let builder = make_builder();
    let tokens = builder.estimate_system_tokens("你好 world");
    assert!(tokens > 0);
}
