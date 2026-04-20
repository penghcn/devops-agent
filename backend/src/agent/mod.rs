mod step;
mod steps;
mod router;

pub use step::{Step, StepContext, StepResult, StepChain};
pub use router::{Intent, IntentRouter};

mod claude;

use serde::{Deserialize, Serialize};
use crate::config::Config;
use claude::{call_with_skill, call_claude_code};

#[derive(Debug, Deserialize)]
pub struct AgentRequest {
    pub prompt: String,
    #[serde(default)]
    pub task_type: TaskType,
    /// Jenkins Pipeline 项目名称（如 ds-pkg）
    #[serde(default)]
    pub job_name: Option<String>,
    /// 分支名称（如 dev）
    #[serde(default)]
    pub branch: Option<String>,
}

#[derive(Debug, Deserialize, Default, PartialEq)]
pub enum TaskType {
    #[default]
    Auto,      // 自动识别
    Deploy,
    Build,
    Query,
}

#[derive(Debug, Serialize)]
pub struct AgentResponse {
    pub success: bool,
    pub output: String,
    pub steps: Vec<AgentStep>,  // 展示思考过程
}

#[derive(Debug, Clone, Serialize)]
pub struct AgentStep {
    pub action: String,
    pub result: String,
}

/// 主 Agent 入口 — 基于步骤链架构
pub async fn process_request(req: AgentRequest, _config: &Config) -> AgentResponse {
    let intent_router = IntentRouter;
    let intent = intent_router.identify(&req.prompt).await;

    let chain = intent_router.to_chain_with_prompt(&intent, &req.prompt);

    let ctx = StepContext::new(
        req.prompt.clone(),
        req.task_type,
        req.job_name.clone(),
        req.branch.clone(),
        std::sync::Arc::new(_config.clone()),
    );

    let (final_ctx, steps) = chain.execute(ctx).await;

    AgentResponse {
        success: final_ctx.steps.iter().any(|s| {
            s.result.contains("成功") || s.result.contains("完成")
        }),
        output: final_ctx.analysis_result.clone().unwrap_or_else(|| {
            final_ctx
                .steps
                .last()
                .map(|s| s.result.clone())
                .unwrap_or_else(|| "处理完成".to_string())
        }),
        steps,
    }
}