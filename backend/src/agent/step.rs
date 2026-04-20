use std::sync::Arc;

use serde_json::Value;

use crate::config::Config;

use super::TaskType;

/// Step 执行结果
#[derive(Debug)]
pub enum StepResult {
    Success { message: String },
    Failed { error: String },
    Abort { reason: String },
}

impl StepResult {
    pub fn is_success(&self) -> bool {
        matches!(self, StepResult::Success { .. })
    }

    pub fn is_abort(&self) -> bool {
        matches!(self, StepResult::Abort { .. })
    }
}

/// Step 间共享上下文
pub struct StepContext {
    pub prompt: String,
    pub task_type: TaskType,
    pub job_name: Option<String>,
    pub branch: Option<String>,
    pub config: Arc<Config>,
    pub build_number: Option<u32>,
    pub pipeline_status: Option<Value>,
    pub build_log: Option<String>,
    pub analysis_result: Option<String>,
    /// 记录每个 Step 的执行结果（用于展示思考过程）
    pub steps: Vec<super::AgentStep>,
}

impl StepContext {
    pub fn new(
        prompt: String,
        task_type: TaskType,
        job_name: Option<String>,
        branch: Option<String>,
        config: Arc<Config>,
    ) -> Self {
        Self {
            prompt,
            task_type,
            job_name,
            branch,
            config,
            build_number: None,
            pipeline_status: None,
            build_log: None,
            analysis_result: None,
            steps: Vec::new(),
        }
    }
}

/// Step trait — 所有工具调用必须实现此 trait
#[async_trait::async_trait]
pub trait Step: Send + Sync {
    fn name(&self) -> &str;
    async fn execute(&self, ctx: &mut StepContext) -> StepResult;
}

/// StepChain — 编排器，顺序执行 Step
pub struct StepChain {
    steps: Vec<Box<dyn Step>>,
}

impl StepChain {
    pub fn new(steps: Vec<Box<dyn Step>>) -> Self {
        Self { steps }
    }

    pub async fn execute(&self, ctx: StepContext) -> (StepContext, Vec<super::AgentStep>) {
        let mut ctx = ctx;
        let mut final_steps = Vec::new();

        for step in &self.steps {
            let step_name = step.name().to_string();
            ctx.steps.push(super::AgentStep {
                action: step_name.clone(),
                result: "执行中...".to_string(),
            });

            let result = step.execute(&mut ctx).await;

            match &result {
                StepResult::Success { message } => {
                    ctx.steps.last_mut().unwrap().result = message.clone();
                }
                StepResult::Failed { error } => {
                    ctx.steps.last_mut().unwrap().result = format!("失败: {}", error);
                    final_steps = ctx.steps.clone();
                    break;
                }
                StepResult::Abort { reason } => {
                    ctx.steps.last_mut().unwrap().result = format!("中止: {}", reason);
                    final_steps = ctx.steps.clone();
                    break;
                }
            }

            final_steps = ctx.steps.clone();

            if result.is_abort() {
                break;
            }
        }

        (ctx, final_steps)
    }
}
