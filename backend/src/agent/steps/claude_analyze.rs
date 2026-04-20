use std::future::Future;
use std::pin::Pin;

use super::super::claude;
use super::super::{Step, StepContext, StepResult};

pub struct ClaudeAnalyzeStep;

impl Step for ClaudeAnalyzeStep {
    fn name(&self) -> &str {
        "claude_analyze"
    }

    fn execute<'a>(
        &'a self,
        ctx: &'a mut StepContext,
    ) -> Pin<Box<dyn Future<Output = StepResult> + Send + 'a>> {
        Box::pin(async move {
            let analysis_prompt = format!(
                "请分析以下 Jenkins Pipeline 状态信息，给出简洁的中文总结：\n\n\
                 Pipeline: {}/{}\n\
                 Status: {:?}\n\
                 User prompt: {}",
                ctx.job_name.as_deref().unwrap_or("unknown"),
                ctx.branch.as_deref().unwrap_or("unknown"),
                ctx.pipeline_status,
                ctx.prompt
            );

            match claude::call_claude_code(&analysis_prompt, "").await {
                Ok(result) => {
                    ctx.analysis_result = Some(result.clone());
                    StepResult::Success { message: result }
                }
                Err(e) => StepResult::Failed {
                    error: e.to_string(),
                },
            }
        })
    }
}
