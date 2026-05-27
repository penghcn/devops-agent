//! Prompt 构建器 — 七层组装，前缀缓存最大化。
//!
//! 分层（稳定性递减）：
//! 1. 静态 System 核心（100% 缓存）
//! 2. 静态工具定义（100% 缓存）
//! 3. 半静态规则（高缓存）
//! 4. 动态记忆（部分缓存）
//! 5. Session 上下文（低缓存）
//! 6. 动态工具（低缓存）
//! 7. 动态 Messages（0% 缓存）

use std::sync::{Arc, OnceLock};

use super::{CacheControl, ChatRequest, ContentBlock, Message, ToolDefinition};

/// 层 1: 静态 System 核心
const SYSTEM_CORE: &str = r#"你是 DevOps Agent，专注于 CI/CD 流水线管理、构建分析和部署运维。
行为准则：
1. 使用中文回复，保持专业简洁
2. 分析构建日志时给出结构化结果
3. 优先使用工具获取信息，不猜测
安全红线：
1. 不执行破坏性命令（rm -rf、格式化磁盘等）
2. 不泄露敏感信息（API Key、Token、密码）
3. 不访问未授权的资源"#;

/// 层 2: 静态工具指南
const TOOL_GUIDELINES: &str = r#"可用辅助工具：
- get_time: 获取当前时间（ISO 8601 + Unix 时间戳）
- get_env: 读取白名单内的环境变量
- get_config: 读取项目配置项
使用建议：先获取上下文信息再执行操作，避免盲目操作。"#;

/// 层 3: 默认项目规则
const DEFAULT_PROJECT_RULES: &str = r#"项目规则：
1. 构建失败时分析根因并给出修复建议
2. 部署前确认目标环境和分支
3. 所有操作需要记录审计日志"#;

/// 层 1~3 预编译的静态前缀
#[derive(Debug, Clone)]
pub struct StaticPrefix {
    /// 拼接后的完整前缀（启动时预编译）
    compiled: String,
}

impl StaticPrefix {
    /// 创建静态前缀并预编译
    pub fn new(system_core: String, tool_guidelines: String, project_rules: String) -> Self {
        let compiled = format!(
            "{}\n\n{}\n\n{}",
            system_core, tool_guidelines, project_rules
        );
        Self { compiled }
    }

    /// 获取预编译的完整前缀
    pub fn as_str(&self) -> &str {
        &self.compiled
    }
}

/// 默认静态前缀的全局单例（内容永不变化，所有请求共享）
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

/// Session 上下文结构化槽位
#[derive(Debug, Clone, Default)]
pub struct SessionSlots {
    /// 当前目标（1 条）
    pub goal: Option<String>,
    /// 最近步骤结果（最多 5 个）
    pub recent_steps: Vec<String>,
    /// 活跃错误（最多 3 个）
    pub active_errors: Vec<String>,
    /// 每个槽位的最大字符数（默认 500）
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

    /// 添加步骤结果，超出限制时移除最旧
    pub fn add_step(&mut self, step: String) {
        if self.recent_steps.len() >= Self::MAX_STEPS {
            self.recent_steps.remove(0);
        }
        self.recent_steps
            .push(step.truncate_to(self.max_slot_chars));
    }

    /// 添加活跃错误
    pub fn add_error(&mut self, error: String) {
        if self.active_errors.len() >= Self::MAX_ERRORS {
            self.active_errors.remove(0);
        }
        self.active_errors
            .push(error.truncate_to(self.max_slot_chars));
    }

    /// 拼接为上下文字符串
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

/// 记忆条目（用于注入 prompt）
#[derive(Debug, Clone)]
pub struct MemorySlot {
    pub content: String,
    pub score: f32,
}

/// Prompt 构建器配置
#[derive(Debug, Clone)]
pub struct PromptBuilderConfig {
    /// 记忆注入的最低分数阈值
    pub memory_score_threshold: f32,
    /// 记忆注入的最大条目数
    pub max_memory_slots: usize,
    /// 记忆每条最大字符数
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

/// 七层 Prompt 构建器
#[derive(Debug, Clone)]
pub struct PromptBuilder {
    /// 预编译的静态前缀（层 1~3）
    static_prefix: Arc<StaticPrefix>,
    /// 静态工具集（层 2，构造时注入）
    static_tools: Vec<ToolDefinition>,
    /// 配置
    config: PromptBuilderConfig,
}

impl PromptBuilder {
    /// 创建构建器
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

    /// 创建默认构建器（用于测试）
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

    /// 使用默认静态内容创建构建器（生产环境使用）
    /// 静态前缀复用全局单例，所有请求共享同一 Arc 实例
    pub fn with_static_tools(static_tools: Vec<ToolDefinition>) -> Self {
        Self {
            static_prefix: default_static_prefix().clone(),
            static_tools,
            config: PromptBuilderConfig::default(),
        }
    }

    /// 使用自定义项目规则创建构建器
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

    // ── 核心构建方法 ──

    /// 构建完整的 ChatRequest
    ///
    /// - `workflow_tools`: 同工作流内稳定的工具（如 Jenkins 场景工具）
    /// - `request_tools`: 本次请求特有的工具
    pub fn build(
        &self,
        user_prompt: String,
        memory_slots: Vec<MemorySlot>,
        session: &SessionSlots,
        workflow_tools: Vec<ToolDefinition>,
        request_tools: Vec<ToolDefinition>,
        conversation: Vec<Message>,
    ) -> ChatRequest {
        // 层 1~5: 构建 System ContentBlock（带缓存断点）
        let system_blocks = self.build_system_blocks(memory_slots, session);

        // 层 6: 工具备并（static + workflow + request），组间插入缓存标记
        let merged_tools = self.merge_tools(workflow_tools, request_tools);

        // 层 7: 对话 Messages
        let mut messages = vec![Message::System {
            content: system_blocks,
        }];

        // 追加历史对话
        messages.extend(conversation);

        // 追加当前用户输入
        messages.push(Message::User {
            content: text_block(user_prompt),
        });

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
        }
    }

    /// 构建简易 ChatRequest（仅静态前缀 System + 用户消息，无工具/记忆/Session）
    /// 适用于单轮分析任务（如 BuildAnalysis），不注入动态层
    pub fn build_simple(&self, user_prompt: String) -> ChatRequest {
        let system_blocks = vec![ContentBlock::text_with_cache(
            self.static_prefix.as_str().to_string(),
            CacheControl::EphemeralBuffer,
        )];

        ChatRequest {
            model: String::new(),
            messages: vec![
                Message::System {
                    content: system_blocks,
                },
                Message::User {
                    content: text_block(user_prompt),
                },
            ],
            tools: None,
            temperature: None,
            tool_choice: None,
            stop_sequences: None,
            prefill: None,
        }
    }

    /// 构建 System 消息的 ContentBlock 列表（带缓存断点）
    ///
    /// 缓存断点位置：
    /// - 层 3 末尾（静态前缀之后）— 第一个断点
    /// - 层 5 末尾（Session 上下文之后）— 第二个断点
    fn build_system_blocks(
        &self,
        memory_slots: Vec<MemorySlot>,
        session: &SessionSlots,
    ) -> Vec<ContentBlock> {
        let mut blocks = Vec::new();

        // 层 1~3: 静态前缀（带缓存断点）
        blocks.push(ContentBlock::text_with_cache(
            self.static_prefix.as_str().to_string(),
            CacheControl::EphemeralBuffer,
        ));

        // 层 4: 动态记忆（带缓存标记，记忆在单次请求间稳定）
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
            blocks.push(ContentBlock::text_with_cache(
                format!("相关记忆:\n{}", mem_text),
                CacheControl::EphemeralBuffer,
            ));
        }

        // 层 5: Session 上下文（带缓存断点）
        let session_ctx = session.to_context();
        if !session_ctx.is_empty() {
            blocks.push(ContentBlock::text_with_cache(
                format!("会话上下文:\n{}", session_ctx),
                CacheControl::EphemeralBuffer,
            ));
        }

        blocks
    }

    /// 工具备并：static → workflow → request，组间插入缓存标记
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

        // Static tools（带缓存标记）
        if !self.static_tools.is_empty() {
            tools.extend(
                self.static_tools
                    .iter()
                    .map(|t| t.clone_with_cache(CacheControl::EphemeralBuffer)),
            );
        }

        // Workflow tools（带缓存标记）
        if !workflow_tools.is_empty() {
            tools.push(ToolDefinition::cache_breakpoint());
            tools.extend(workflow_tools.into_iter().map(|mut t| {
                t.cache_control = Some(CacheControl::EphemeralBuffer);
                t
            }));
        }

        // Request tools（无缓存标记）
        if !request_tools.is_empty() {
            tools.push(ToolDefinition::cache_breakpoint());
            tools.extend(request_tools);
        }

        tools
    }

    /// 仅构建 System prompt 字符串（用于调试）
    pub fn build_system_prompt(
        &self,
        memory_slots: Vec<MemorySlot>,
        session: &SessionSlots,
    ) -> String {
        self.build_system_blocks(memory_slots, session)
            .iter()
            .filter_map(|b| b.as_text().map(|s| s.to_string()))
            .collect::<Vec<_>>()
            .join("\n\n")
    }

    /// 估算 System prompt 的 Token 数
    pub fn estimate_system_tokens(&self, system_prompt: &str) -> u32 {
        estimate_tokens(system_prompt)
    }
}

impl ToolDefinition {
    /// 克隆并设置缓存标记
    pub fn clone_with_cache(&self, cache: CacheControl) -> Self {
        Self {
            name: self.name.clone(),
            description: self.description.clone(),
            parameters: self.parameters.clone(),
            cache_control: Some(cache),
        }
    }

    /// 创建纯缓存断点工具（仅用于工具列表中的缓存标记）
    pub fn cache_breakpoint() -> Self {
        Self {
            name: String::new(),
            description: String::new(),
            parameters: serde_json::json!({}),
            cache_control: Some(CacheControl::EphemeralBuffer),
        }
    }
}

/// 便捷地将 String 转为单元素 ContentBlock 数组
pub fn text_block(s: String) -> Vec<ContentBlock> {
    vec![ContentBlock::text(s)]
}

/// 混合 Token 估算（与 ContextWindow 一致）
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

/// 字符串截断工具
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

// ── 用于 TokenUsage 估算（不暴露完整类型）─────────

#[cfg(test)]
mod tests {
    use super::*;

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
        // Should have at least the workflow tool
        assert!(tools.iter().any(|t| t.name == "jenkins_build"));
        // Workflow tools should have cache_control set
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
            // First block should have cache_control (static prefix)
            assert!(matches!(
                content.first(),
                Some(ContentBlock::Text {
                    cache_control: Some(_),
                    ..
                })
            ));
            // Last block should have cache_control (session context)
            assert!(matches!(
                content.last(),
                Some(ContentBlock::Text {
                    cache_control: Some(_),
                    ..
                })
            ));
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
        // ASCII "Hi" = 2 chars / 4 = 0 tokens (floor division, expected)
    }

    #[test]
    fn test_build_simple() {
        let builder = make_builder();
        let req = builder.build_simple("分析构建日志".into());

        // Should have exactly 2 messages: System + User
        assert_eq!(req.messages.len(), 2);
        assert!(matches!(req.messages[0], Message::System { .. }));
        assert!(matches!(req.messages[1], Message::User { .. }));
        // No tools
        assert!(req.tools.is_none());
        // System block should have cache control
        if let Message::System { content } = &req.messages[0] {
            assert!(matches!(
                content.first(),
                Some(ContentBlock::Text {
                    cache_control: Some(_),
                    ..
                })
            ));
        }
    }

    #[test]
    fn test_default_static_prefix_sharing() {
        // 两次调用 with_static_tools 应共享同一 Arc 实例
        let builder1 = PromptBuilder::with_static_tools(vec![]);
        let builder2 = PromptBuilder::with_static_tools(vec![]);
        // Arc::ptr_eq 检查两个 Arc 是否指向同一分配
        assert!(Arc::ptr_eq(
            &builder1.static_prefix,
            &builder2.static_prefix
        ));
    }
}
