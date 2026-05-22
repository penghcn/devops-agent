use std::sync::Arc;

use serde_json::Value;

use crate::config::Config;
use crate::llm::LlmProvider;
use crate::tools::jenkins_cache::JenkinsCacheManager;

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
    pub cache: Option<Arc<JenkinsCacheManager>>,
    pub build_number: Option<u32>,
    pub pipeline_status: Option<Value>,
    pub build_log: Option<String>,
    pub analysis_result: Option<String>,
    pub structured_analysis: Option<serde_json::Value>,
    /// 记录每个 Step 的执行结果（用于展示思考过程）
    pub steps: Vec<super::AgentStep>,
    /// 每个 Step 的耗时（秒）
    pub step_elapsed: Vec<f64>,
    /// identify() 中 Claude 调用的耗时（秒），StepChain 执行时加到第一步
    pub identify_elapsed: Option<f64>,
    /// 模糊修正列表（job/branch）
    pub corrections: Vec<super::Correction>,
    /// LLM provider passed from IntentRouter for step use
    pub llm_provider: Option<Arc<dyn LlmProvider>>,
    /// LLM model name passed from IntentRouter for step use
    pub llm_model: Option<String>,
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
            cache: None,
            build_number: None,
            pipeline_status: None,
            build_log: None,
            analysis_result: None,
            structured_analysis: None,
            steps: Vec::new(),
            step_elapsed: Vec::new(),
            identify_elapsed: None,
            corrections: Vec::new(),
            llm_provider: None,
            llm_model: None,
        }
    }

    pub fn with_llm_provider(mut self, provider: Arc<dyn LlmProvider>) -> Self {
        self.llm_provider = Some(provider);
        self
    }

    pub fn with_llm_model(mut self, model: String) -> Self {
        self.llm_model = Some(model);
        self
    }

    pub fn with_cache(mut self, cache: Arc<JenkinsCacheManager>) -> Self {
        self.cache = Some(cache);
        self
    }

    pub fn with_identify_elapsed(mut self, elapsed: f64) -> Self {
        self.identify_elapsed = Some(elapsed);
        self
    }

    pub fn add_correction(mut self, kind: String, original: String, corrected: String) -> Self {
        self.corrections.push(super::Correction {
            kind,
            original,
            corrected,
        });
        self
    }
}

/// Step trait — 所有工具调用必须实现此 trait
#[async_trait::async_trait]
pub trait Step: Send + Sync {
    fn name(&self) -> &str;
    /// 面向用户的友好描述（默认返回 name）
    fn description(&self, _ctx: &StepContext) -> String {
        self.name().to_string()
    }
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

    /// 返回链中所有步骤的名称（用于测试观测）
    pub fn step_names(&self) -> Vec<&str> {
        self.steps.iter().map(|s| s.name()).collect()
    }

    /// 返回链中所有步骤的描述（用于测试区分 ClaudeCode vs ToolUseLoop）
    #[cfg(test)]
    pub(crate) fn step_descriptions(&self, ctx: &StepContext) -> Vec<String> {
        self.steps.iter().map(|s| s.description(ctx)).collect()
    }

    pub async fn execute(&self, ctx: StepContext) -> (StepContext, Vec<super::AgentStep>) {
        let mut ctx = ctx;
        let mut final_steps = Vec::new();

        for (i, step) in self.steps.iter().enumerate() {
            let step_name = step.name().to_string();
            let start = std::time::Instant::now();

            ctx.steps.push(super::AgentStep {
                action: step_name.clone(),
                result: "执行中...".to_string(),
                elapsed: None,
            });

            let result = step.execute(&mut ctx).await;
            let elapsed = start.elapsed().as_millis() as f64 / 1000.0;

            // 第一步累加 identify() 的耗时
            let total_elapsed = if i == 0 {
                ctx.identify_elapsed.map(|e| e + elapsed).unwrap_or(elapsed)
            } else {
                elapsed
            };

            // 更新 result 并回填 elapsed
            let step_result = match &result {
                StepResult::Success { message } => message.clone(),
                StepResult::Failed { error } => format!("失败: {}", error),
                StepResult::Abort { reason } => format!("中止: {}", reason),
            };
            let last = ctx.steps.last_mut().unwrap();
            last.result = step_result;
            last.elapsed = Some(total_elapsed);

            final_steps = ctx.steps.clone();

            if result.is_abort() || !result.is_success() {
                break;
            }
        }

        (ctx, final_steps)
    }

    /// 流式执行 — 每个 Step 完成后通过 channel 推送事件
    pub async fn execute_stream(
        &self,
        ctx: StepContext,
        sender: tokio::sync::mpsc::Sender<super::StreamEvent>,
    ) -> (StepContext, Vec<super::AgentStep>) {
        let mut ctx = ctx;
        let mut final_steps = Vec::new();

        for (i, step) in self.steps.iter().enumerate() {
            let step_name = step.name().to_string();
            let start = std::time::Instant::now();

            ctx.steps.push(super::AgentStep {
                action: step_name.clone(),
                result: "执行中...".to_string(),
                elapsed: None,
            });

            // 推送 StepStart
            let desc = step.description(&ctx);
            tracing::info!(step = %step_name, description = %desc, "StepStart");
            let _ = sender
                .send(super::StreamEvent::StepStart {
                    step_index: i,
                    action: step_name.clone(),
                    description: desc,
                })
                .await;

            let result = step.execute(&mut ctx).await;
            let elapsed = start.elapsed().as_millis() as f64 / 1000.0;

            // 第一步累加 identify() 的耗时
            let total_elapsed = if i == 0 {
                ctx.identify_elapsed.map(|e| e + elapsed).unwrap_or(elapsed)
            } else {
                elapsed
            };

            // 更新 result 并回填 elapsed
            let step_result = match &result {
                StepResult::Success { message } => message.clone(),
                StepResult::Failed { error } => format!("失败: {}", error),
                StepResult::Abort { reason } => format!("中止: {}", reason),
            };
            let last = ctx.steps.last_mut().unwrap();
            last.result = step_result.clone();
            last.elapsed = Some(total_elapsed);

            final_steps = ctx.steps.clone();

            // 推送 StepDone
            tracing::info!(step = %step_name, result = %step_result, elapsed_s = total_elapsed, "StepDone");
            let _ = sender
                .send(super::StreamEvent::StepDone {
                    step_index: i,
                    action: step_name,
                    result: step_result,
                    elapsed: total_elapsed,
                })
                .await;

            if result.is_abort() || !result.is_success() {
                break;
            }
        }

        (ctx, final_steps)
    }
}
