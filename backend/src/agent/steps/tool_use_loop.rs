//! ToolUseLoopStep — 基于 ToolUseLoop 的 LLM 驱动 Step
//!
//! 取代 ClaudeCodeStep 和 BuildAnalysisStep，使用 LLM 原生工具调用能力。

use std::sync::Arc;

use crate::agent::step::{Step, StepContext, StepResult};
use crate::llm::tool_use_loop::{ToolExecutor, ToolUseLoop, ToolUseResult};
use crate::llm::{ChatRequest, LlmProvider, Message, ToolDefinition};
use crate::tools::builtin::{
    Tool, get_heavy_tool_definitions, register_all_builtin, register_heavy_tools,
};

pub struct ToolUseLoopStep {
    pub prompt: String,
    pub system_message: Option<String>,
    pub llm_provider: Arc<dyn LlmProvider>,
    pub llm_model: String,
    pub max_iterations: usize,
    /// 额外追加的工具定义（如 Jenkins/GitLab 场景工具）
    pub extra_tools: Vec<ToolDefinition>,
}

impl ToolUseLoopStep {
    pub fn new(prompt: String, provider: Arc<dyn LlmProvider>, model: String) -> Self {
        Self {
            prompt,
            system_message: None,
            llm_provider: provider,
            llm_model: model,
            max_iterations: 15,
            extra_tools: Vec::new(),
        }
    }

    pub fn with_system(mut self, system: String) -> Self {
        self.system_message = Some(system);
        self
    }

    pub fn with_max_iterations(mut self, max: usize) -> Self {
        self.max_iterations = max;
        self
    }

    pub fn with_extra_tools(mut self, tools: Vec<ToolDefinition>) -> Self {
        self.extra_tools = tools;
        self
    }

    /// 构建完整的工具定义列表（内置 + 重型 + 额外）
    fn build_tools(&self, config: &crate::config::Config) -> Vec<ToolDefinition> {
        let mut tools = Vec::new();

        // 内置简单工具定义
        tools.push(crate::tools::builtin::GetTimeTool::new().definition());
        tools.push(crate::tools::builtin::GetEnvTool::new().definition());
        tools.push(crate::tools::builtin::GetConfigTool::new(config).definition());

        // 重型工具定义（Read/Write/Bash/Git）
        tools.extend(get_heavy_tool_definitions());

        // 额外场景工具
        tools.extend(self.extra_tools.iter().cloned());

        tools
    }

    /// 构建消息列表
    fn build_messages(&self) -> Vec<Message> {
        let mut messages = Vec::new();

        if let Some(ref system) = self.system_message {
            messages.push(Message::System {
                content: crate::llm::text_block(system.clone()),
            });
        }

        messages.push(Message::User {
            content: crate::llm::text_block(self.prompt.clone()),
        });

        messages
    }
}

#[async_trait::async_trait]
impl Step for ToolUseLoopStep {
    fn name(&self) -> &str {
        "Agent"
    }

    fn description(&self, _ctx: &StepContext) -> String {
        let prompt_display = match self.prompt.len() {
            n if n <= 60 => self.prompt.clone(),
            _ => self.prompt[..60].to_string(),
        };
        format!("AI 处理 ({}): {}", self.llm_model, prompt_display)
    }

    async fn execute(&self, ctx: &mut StepContext) -> StepResult {
        let tools = self.build_tools(&ctx.config);
        let messages = self.build_messages();

        let mut executor = ToolExecutor::new();
        register_all_builtin(&mut executor, &ctx.config);
        register_heavy_tools(&mut executor);

        let request = ChatRequest {
            model: self.llm_model.clone(),
            messages,
            tools: Some(tools),
            temperature: Some(0.6),
            tool_choice: None,
            stop_sequences: None,
            prefill: None,
        };

        let mut loop_builder = ToolUseLoop::new(self.llm_provider.clone(), executor, request);
        loop_builder.set_max_iterations(self.max_iterations);

        match loop_builder.execute().await {
            Ok(result) => self.handle_success(result),
            Err(e) => StepResult::Failed {
                error: format!("ToolUseLoop 执行失败: {}", e),
            },
        }
    }
}

impl ToolUseLoopStep {
    fn handle_success(&self, result: ToolUseResult) -> StepResult {
        if result.response.has_tool_calls() {
            return StepResult::Failed {
                error: format!("循环 {} 轮后 LLM 仍返回工具调用", result.iterations),
            };
        }

        StepResult::Success {
            message: result.response.content,
        }
    }
}
