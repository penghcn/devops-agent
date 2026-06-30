//! Prompt 构建器 — 七层组装，前缀缓存最大化。
//!
//! 使用 lellm-core 的 Prompt 和 PromptBuilder 实现分层缓存。

use std::sync::{Arc, OnceLock};

use super::{CacheControl, ChatRequest, Message, Prompt, ToolDefinition, ToolDefinitionExt};

const SYSTEM_CORE: &str = r#"你是 DevOps Agent，专注于 CI/CD 流水线管理、构建分析和部署运维。
行为准则：
1. 使用中文回复，保持专业简洁
2. 分析构建日志时给出结构化结果
3. 优先使用工具获取信息，不猜测
安全红线：
1. 不执行破坏性命令（rm -rf、格式化磁盘等）
2. 不泄露敏感信息（API Key、Token、密码）
3. 不访问未授权的资源"#;

const TOOL_GUIDELINES: &str = r#"可用辅助工具：
- get_time: 获取当前时间（ISO 8601 + Unix 时间戳）
- get_env: 读取白名单内的环境变量
- get_config: 读取项目配置项
使用建议：先获取上下文信息再执行操作，避免盲目操作。"#;

const DEFAULT_PROJECT_RULES: &str = r#"项目规则：
1. 构建失败时分析根因并给出修复建议
2. 部署前确认目标环境和分支
3. 所有操作需要记录审计日志"#;

#[derive(Debug, Clone)]
pub struct StaticPrefix {
    compiled: String,
}

impl StaticPrefix {
    pub fn new(system_core: String, tool_guidelines: String, project_rules: String) -> Self {
        let compiled = format!(
            "{}\n\n{}\n\n{}",
            system_core, tool_guidelines, project_rules
        );
        Self { compiled }
    }

    pub fn as_str(&self) -> &str {
        &self.compiled
    }
}

static DEFAULT_STATIC_PREFIX: OnceLock<Arc<StaticPrefix>> = OnceLock::new();

fn default_static_prefix() -> &'static Arc<StaticPrefix> {
    DEFAULT_STATIC_PREFIX.get_or_init(|| {
        Arc::new(StaticPrefix::new(
            SYSTEM_CORE.to_string(),
            TOOL_GUIDELINES.to_string(),
            DEFAULT_PROJECT_RULES.to_string(),
        ))
    })
}

#[derive(Debug, Clone, Default)]
pub struct SessionSlots {
    pub goal: Option<String>,
    pub recent_steps: Vec<String>,
    pub active_errors: Vec<String>,
    pub max_slot_chars: usize,
}

impl SessionSlots {
    const MAX_STEPS: usize = 5;
    const MAX_ERRORS: usize = 3;

    pub fn new() -> Self {
        Self {
            max_slot_chars: 500,
            ..Default::default()
        }
    }

    pub fn add_step(&mut self, step: String) {
        if self.recent_steps.len() >= Self::MAX_STEPS {
            self.recent_steps.remove(0);
        }
        self.recent_steps
            .push(step.truncate_to(self.max_slot_chars));
    }

    pub fn add_error(&mut self, error: String) {
        if self.active_errors.len() >= Self::MAX_ERRORS {
            self.active_errors.remove(0);
        }
        self.active_errors
            .push(error.truncate_to(self.max_slot_chars));
    }

    pub fn to_context(&self) -> String {
        let mut parts = Vec::new();

        if let Some(goal) = &self.goal {
            parts.push(format!("当前目标: {}", goal));
        }

        if !self.recent_steps.is_empty() {
            let steps = self
                .recent_steps
                .iter()
                .enumerate()
                .map(|(i, s)| format!("  {}. {}", i + 1, s))
                .collect::<Vec<_>>()
                .join("\n");
            parts.push(format!("最近步骤:\n{}", steps));
        }

        if !self.active_errors.is_empty() {
            let errors = self
                .active_errors
                .iter()
                .enumerate()
                .map(|(i, e)| format!("  {}. {}", i + 1, e))
                .collect::<Vec<_>>()
                .join("\n");
            parts.push(format!("活跃错误:\n{}", errors));
        }

        parts.join("\n\n")
    }
}

#[derive(Debug, Clone)]
pub struct MemorySlot {
    pub content: String,
    pub score: f32,
}

#[derive(Debug, Clone)]
pub struct PromptBuilderConfig {
    pub memory_score_threshold: f32,
    pub max_memory_slots: usize,
    pub max_memory_chars: usize,
}

impl Default for PromptBuilderConfig {
    fn default() -> Self {
        Self {
            memory_score_threshold: 0.7,
            max_memory_slots: 5,
            max_memory_chars: 300,
        }
    }
}

/// 项目 Prompt 构建器 — 基于 lellm-core 的 PromptBuilder
#[derive(Debug, Clone)]
pub struct PromptBuilder {
    static_prefix: Arc<StaticPrefix>,
    static_tools: Vec<ToolDefinition>,
    config: PromptBuilderConfig,
}

impl PromptBuilder {
    pub fn new(
        static_prefix: StaticPrefix,
        static_tools: Vec<ToolDefinition>,
        config: PromptBuilderConfig,
    ) -> Self {
        Self {
            static_prefix: Arc::new(static_prefix),
            static_tools,
            config,
        }
    }

    pub fn with_defaults(
        system_core: String,
        tool_guidelines: String,
        project_rules: String,
    ) -> Self {
        Self::new(
            StaticPrefix::new(system_core, tool_guidelines, project_rules),
            vec![],
            PromptBuilderConfig::default(),
        )
    }

    pub fn with_static_tools(static_tools: Vec<ToolDefinition>) -> Self {
        Self {
            static_prefix: default_static_prefix().clone(),
            static_tools,
            config: PromptBuilderConfig::default(),
        }
    }

    pub fn with_project_rules(project_rules: String, static_tools: Vec<ToolDefinition>) -> Self {
        Self::new(
            StaticPrefix::new(
                SYSTEM_CORE.to_string(),
                TOOL_GUIDELINES.to_string(),
                project_rules,
            ),
            static_tools,
            PromptBuilderConfig::default(),
        )
    }

    /// 构建完整的 ChatRequest
    pub fn build(
        &self,
        user_prompt: String,
        memory_slots: Vec<MemorySlot>,
        session: &SessionSlots,
        workflow_tools: Vec<ToolDefinition>,
        request_tools: Vec<ToolDefinition>,
        conversation: Vec<Message>,
    ) -> ChatRequest {
        let system_prompt = self.build_system_prompt_inner(memory_slots, session);
        let merged_tools = self.merge_tools(workflow_tools, request_tools);

        let mut messages = vec![Message::system(system_prompt.to_content_blocks())];
        messages.extend(conversation);
        messages.push(Message::user_text(&user_prompt));

        ChatRequest {
            model: String::new(),
            messages,
            tools: if merged_tools.is_empty() {
                None
            } else {
                Some(merged_tools)
            },
            temperature: None,
            tool_choice: None,
            stop_sequences: None,
            prefill: None,
            ..Default::default()
        }
    }

    /// 构建简易 ChatRequest（仅静态前缀 System + 用户消息）
    pub fn build_simple(&self, user_prompt: String) -> ChatRequest {
        let system_prompt = lellm_core::Prompt::builder()
            .layer_cached(self.static_prefix.as_str().to_string())
            .build();

        ChatRequest {
            model: String::new(),
            messages: vec![
                Message::system(system_prompt.to_content_blocks()),
                Message::user_text(&user_prompt),
            ],
            tools: None,
            temperature: None,
            tool_choice: None,
            stop_sequences: None,
            prefill: None,
            ..Default::default()
        }
    }

    /// 使用 lellm PromptBuilder 构建 System Prompt
    fn build_system_prompt_inner(
        &self,
        memory_slots: Vec<MemorySlot>,
        session: &SessionSlots,
    ) -> Prompt {
        let mut builder = lellm_core::Prompt::builder();

        // L1~L3: 静态前缀（带缓存断点）
        builder = builder.layer_cached(self.static_prefix.as_str().to_string());

        // L4: 动态记忆（带缓存标记）
        let injected_memory: Vec<String> = memory_slots
            .into_iter()
            .filter(|m| m.score >= self.config.memory_score_threshold)
            .take(self.config.max_memory_slots)
            .map(|m| m.content.truncate_to(self.config.max_memory_chars))
            .collect();
        if !injected_memory.is_empty() {
            let mem_text = injected_memory
                .iter()
                .enumerate()
                .map(|(i, m)| format!("  {}. {}", i + 1, m))
                .collect::<Vec<_>>()
                .join("\n");
            builder = builder.layer_cached(format!("相关记忆:\n{}", mem_text));
        }

        // L5: Session 上下文（带缓存断点）
        let session_ctx = session.to_context();
        if !session_ctx.is_empty() {
            builder = builder.layer_cached(format!("会话上下文:\n{}", session_ctx));
        }

        builder.build()
    }

    fn merge_tools(
        &self,
        workflow_tools: Vec<ToolDefinition>,
        request_tools: Vec<ToolDefinition>,
    ) -> Vec<ToolDefinition> {
        let total = self.static_tools.len() + workflow_tools.len() + request_tools.len();
        if total == 0 {
            return vec![];
        }

        let mut tools = Vec::with_capacity(total + 2);

        if !self.static_tools.is_empty() {
            tools.extend(
                self.static_tools
                    .iter()
                    .map(|t| t.clone_with_cache(CacheControl::Breakpoint)),
            );
        }

        if !workflow_tools.is_empty() {
            tools.push(ToolDefinition::cache_breakpoint());
            tools.extend(workflow_tools.into_iter().map(|mut t| {
                t.cache_control = Some(CacheControl::Breakpoint);
                t
            }));
        }

        if !request_tools.is_empty() {
            tools.push(ToolDefinition::cache_breakpoint());
            tools.extend(request_tools);
        }

        tools
    }

    pub fn build_system_prompt(
        &self,
        memory_slots: Vec<MemorySlot>,
        session: &SessionSlots,
    ) -> String {
        self.build_system_prompt_inner(memory_slots, session)
            .build_text()
    }

    pub fn estimate_system_tokens(&self, system_prompt: &str) -> u32 {
        estimate_tokens(system_prompt)
    }
}

pub fn estimate_tokens(s: &str) -> u32 {
    let mut cjk: f32 = 0.0;
    let mut ascii: f32 = 0.0;
    for ch in s.chars() {
        if ch.is_ascii() {
            ascii += 1.0;
        } else if ch.is_alphabetic() || ch.is_numeric() {
            cjk += 1.5;
        } else {
            cjk += 1.0;
        }
    }
    ((ascii / 4.0) + cjk) as u32
}

trait TruncateExt {
    fn truncate_to(self, max_chars: usize) -> String;
}

impl TruncateExt for String {
    fn truncate_to(self, max_chars: usize) -> String {
        if self.chars().count() <= max_chars {
            self
        } else {
            let truncated: String = self.chars().take(max_chars).collect();
            format!("{}…", truncated)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::ContentBlock;

    fn make_builder() -> PromptBuilder {
        PromptBuilder::with_defaults(
            "你是 DevOps Agent。".into(),
            "可用工具: get_time, get_env, get_config".into(),
            "项目规则: 使用中文回复。".into(),
        )
    }

    #[test]
    fn test_static_prefix_compilation() {
        let prefix = StaticPrefix::new("核心".into(), "工具".into(), "规则".into());
        assert_eq!(prefix.as_str(), "核心\n\n工具\n\n规则");
    }

    #[test]
    fn test_session_slots() {
        let mut slots = SessionSlots::new();
        slots.goal = Some("部署 ds-pkg".into());
        slots.add_step("步骤 1: 验证配置".into());
        slots.add_step("步骤 2: 触发构建".into());
        slots.add_error("错误: 超时".into());

        let ctx = slots.to_context();
        assert!(ctx.contains("当前目标: 部署 ds-pkg"));
        assert!(ctx.contains("步骤 1: 验证配置"));
        assert!(ctx.contains("错误: 超时"));
    }

    #[test]
    fn test_memory_filtering() {
        let builder = make_builder();
        let memory = vec![
            MemorySlot {
                content: "高分记忆".into(),
                score: 0.9,
            },
            MemorySlot {
                content: "低分记忆".into(),
                score: 0.3,
            },
        ];

        let system = builder.build_system_prompt(memory, &SessionSlots::new());
        assert!(system.contains("高分记忆"));
        assert!(!system.contains("低分记忆"));
    }

    #[test]
    fn test_build_request() {
        let builder = make_builder();
        let req = builder.build(
            "部署 ds-pkg/dev".into(),
            vec![],
            &SessionSlots::new(),
            vec![],
            vec![],
            vec![],
        );

        assert!(!req.messages.is_empty());
        assert!(matches!(req.messages.first(), Some(Message::System { .. })));
        assert!(matches!(req.messages.last(), Some(Message::User { .. })));
    }

    #[test]
    fn test_build_with_tools() {
        let builder = make_builder();
        let workflow_tools = vec![ToolDefinition {
            name: "jenkins_build".into(),
            description: "触发构建".into(),
            parameters: serde_json::json!({"type": "object"}),
            cache_control: None,
        }];

        let req = builder.build(
            "构建".into(),
            vec![],
            &SessionSlots::new(),
            workflow_tools,
            vec![],
            vec![],
        );

        assert!(req.tools.is_some());
        let tools = req.tools.as_ref().unwrap();
        assert!(tools.iter().any(|t| t.name == "jenkins_build"));
        let jenkins_tool = tools.iter().find(|t| t.name == "jenkins_build").unwrap();
        assert!(jenkins_tool.cache_control.is_some());
    }

    #[test]
    fn test_build_cache_breakpoints() {
        let builder = make_builder();
        let mut session = SessionSlots::new();
        session.goal = Some("测试".into());

        let req = builder.build("测试提示".into(), vec![], &session, vec![], vec![], vec![]);

        if let Message::System { content } = &req.messages[0] {
            assert!(matches!(content.first(), Some(ContentBlock::Text(_))));
            assert!(matches!(content.last(), Some(ContentBlock::Text(_))));
        } else {
            panic!("Expected System message");
        }
    }

    #[test]
    fn test_session_slot_truncation() {
        let mut slots = SessionSlots::new();
        slots.max_slot_chars = 10;
        slots.add_step("这是一个很长的步骤结果应该会截断".into());
        assert!(slots.recent_steps[0].ends_with('…'));
    }

    #[test]
    fn test_estimate_tokens() {
        assert!((estimate_tokens("你好") as i32 - 3).abs() <= 1);
    }

    #[test]
    fn test_build_simple() {
        let builder = make_builder();
        let req = builder.build_simple("分析构建日志".into());

        assert_eq!(req.messages.len(), 2);
        assert!(matches!(req.messages[0], Message::System { .. }));
        assert!(matches!(req.messages[1], Message::User { .. }));
        assert!(req.tools.is_none());
        if let Message::System { content } = &req.messages[0] {
            assert!(matches!(content.first(), Some(ContentBlock::Text(_))));
        }
    }

    #[test]
    fn test_default_static_prefix_sharing() {
        let builder1 = PromptBuilder::with_static_tools(vec![]);
        let builder2 = PromptBuilder::with_static_tools(vec![]);
        assert!(Arc::ptr_eq(
            &builder1.static_prefix,
            &builder2.static_prefix
        ));
    }
}
