//! Prompt 构建器 — 七层组装，前缀缓存最大化。
//!
//! 分层（稳定性递减）：
//! 1. 静态 System 核心（100% 缓存）
//! 2. 静态工具定义（100% 缓存）
//! 3. 半静态规则（高缓存）
//! 4. 动态记忆（部分缓存）
//! 5. Session 上下文（低缓存）
//! 6. 动态工具（低缓存）
//! 7. 对话 Messages（0% 缓存）

use std::sync::Arc;

use super::{ChatRequest, Message, ToolDefinition};

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
    pub fn new() -> Self {
        Self {
            max_slot_chars: 500,
            ..Default::default()
        }
    }

    /// 添加步骤结果，超出限制时移除最旧
    pub fn add_step(&mut self, step: String) {
        let max = self.recent_steps.len();
        if self.recent_steps.len() >= max.max(5) {
            self.recent_steps.remove(0);
        }
        self.recent_steps
            .push(step.truncate_to(self.max_slot_chars));
    }

    /// 添加活跃错误
    pub fn add_error(&mut self, error: String) {
        if self.active_errors.len() >= 3 {
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
    /// 配置
    config: PromptBuilderConfig,
}

impl PromptBuilder {
    /// 创建构建器
    pub fn new(static_prefix: StaticPrefix, config: PromptBuilderConfig) -> Self {
        Self {
            static_prefix: Arc::new(static_prefix),
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
            PromptBuilderConfig::default(),
        )
    }

    /// 构建完整的 ChatRequest
    pub fn build(
        &self,
        user_prompt: String,
        memory_slots: Vec<MemorySlot>,
        session: &SessionSlots,
        dynamic_tools: Vec<ToolDefinition>,
        conversation: Vec<Message>,
    ) -> ChatRequest {
        // 层 1~3: 静态前缀（预编译）
        let mut system_parts = vec![self.static_prefix.as_str().to_string()];

        // 层 4: 动态记忆（高分过滤 + 截断）
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
            system_parts.push(format!("相关记忆:\n{}", mem_text));
        }

        // 层 5: Session 上下文
        let session_ctx = session.to_context();
        if !session_ctx.is_empty() {
            system_parts.push(format!("会话上下文:\n{}", session_ctx));
        }

        let system_content = system_parts.join("\n\n");

        // 层 6: 动态工具（场景工具追加）
        let tools = if dynamic_tools.is_empty() {
            None
        } else {
            Some(dynamic_tools)
        };

        // 层 7: 对话 Messages
        let mut messages = vec![Message::System {
            content: system_content,
        }];

        // 追加历史对话
        messages.extend(conversation);

        // 追加当前用户输入
        messages.push(Message::User {
            content: user_prompt,
        });

        ChatRequest {
            model: String::new(),
            messages,
            tools,
            temperature: None,
            tool_choice: None,
            stop_sequences: None,
            prefill: None,
        }
    }

    /// 仅构建 System prompt（用于调试）
    pub fn build_system_prompt(
        &self,
        memory_slots: Vec<MemorySlot>,
        session: &SessionSlots,
    ) -> String {
        let mut parts = vec![self.static_prefix.as_str().to_string()];

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
            parts.push(format!("相关记忆:\n{}", mem_text));
        }

        let session_ctx = session.to_context();
        if !session_ctx.is_empty() {
            parts.push(format!("会话上下文:\n{}", session_ctx));
        }

        parts.join("\n\n")
    }

    /// 估算 System prompt 的 Token 数
    pub fn estimate_system_tokens(&self, system_prompt: &str) -> u32 {
        estimate_tokens(system_prompt)
    }
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
        );

        assert!(!req.messages.is_empty());
        assert!(matches!(req.messages.first(), Some(Message::System { .. })));
        assert!(matches!(req.messages.last(), Some(Message::User { .. })));
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
        assert!(estimate_tokens("Hi") > 0); // TODO 当前 >= 0 恒成立，后续优化
    }
}
