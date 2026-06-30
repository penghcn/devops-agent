//! ToolUseLoopStep — 基于 ToolUseLoop + PromptBuilder 的 LLM 驱动 Step

use std::sync::Arc;

use crate::agent::step::{Step, StepContext, StepResult};
use crate::llm::tool_use_loop::{ToolExecutor, ToolUseLoop, ToolUseResult};
use crate::llm::{ChatResponseExt, PromptBuilder, SessionSlots, ToolDefinition};
use crate::tools::builtin::{
    Tool, get_heavy_tool_definitions, register_all_builtin, register_heavy_tools,
};

pub struct ToolUseLoopStep {
    /// 用户原始提示
    pub prompt: String,
    /// LLM provider
    pub llm_provider: Arc<dyn crate::llm::LlmProvider>,
    /// LLM 模型名称
    pub llm_model: String,
    /// 最大循环轮次
    pub max_iterations: usize,
    /// 额外追加的场景工具（如 Jenkins/GitLab）
    pub extra_tools: Vec<ToolDefinition>,
}

impl ToolUseLoopStep {
    pub fn new(prompt: String, provider: Arc<dyn crate::llm::LlmProvider>, model: String) -> Self {
        Self {
            prompt,
            llm_provider: provider,
            llm_model: model,
            max_iterations: 15,
            extra_tools: Vec::new(),
        }
    }

    pub fn with_max_iterations(mut self, max: usize) -> Self {
        self.max_iterations = max;
        self
    }

    pub fn with_extra_tools(mut self, tools: Vec<ToolDefinition>) -> Self {
        self.extra_tools = tools;
        self
    }

    /// 构建静态工具定义（辅助工具 + 重型工具）
    fn build_static_tools(&self, config: &crate::config::Config) -> Vec<ToolDefinition> {
        let mut tools = Vec::new();
        tools.push(crate::tools::builtin::GetTimeTool::new().definition());
        tools.push(crate::tools::builtin::GetEnvTool::new().definition());
        tools.push(crate::tools::builtin::GetConfigTool::new(config).definition());
        tools.extend(get_heavy_tool_definitions());
        tools
    }

    /// 构建 PromptBuilder
    fn build_prompt_builder(&self, config: &crate::config::Config) -> PromptBuilder {
        let static_tools = self.build_static_tools(config);
        PromptBuilder::with_static_tools(static_tools)
    }

    /// 构建 ChatRequest（通过 PromptBuilder 七层组装）
    fn build_request(&self, config: &crate::config::Config) -> crate::llm::ChatRequest {
        let builder = self.build_prompt_builder(config);
        let session = SessionSlots::new();

        builder.build(
            self.prompt.clone(),
            vec![],                   // memory_slots: 暂未接入记忆系统
            &session,                 // session: 空槽位
            self.extra_tools.clone(), // workflow_tools: 场景工具
            vec![],                   // request_tools: 暂无
            vec![],                   // conversation: 首轮无历史
        )
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
        let mut executor = ToolExecutor::new();
        register_all_builtin(&mut executor, &ctx.config);
        register_heavy_tools(&mut executor);

        let mut request = self.build_request(&ctx.config);
        request.model = self.llm_model.clone();

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
            message: result.response.text_content(),
        }
    }
}
