use super::super::step::{Step, StepContext, StepResult};
use crate::tools::jenkins;

pub struct JenkinsLogStep;

#[async_trait::async_trait]
impl Step for JenkinsLogStep {
    fn name(&self) -> &str {
        "JenkinsLog"
    }

    async fn execute(&self, ctx: &mut StepContext) -> StepResult {
        let build_number = match ctx.build_number {
            Some(n) => n,
            None => return StepResult::Abort { reason: "缺少 build_number".to_string() },
        };

        let job_name = match &ctx.job_name {
            Some(j) => j.clone(),
            None => return StepResult::Abort { reason: "缺少 job_name".to_string() },
        };

        let branch = match &ctx.branch {
            Some(b) => b.clone(),
            None => return StepResult::Abort { reason: "缺少 branch".to_string() },
        };

        match jenkins::get_build_log(&job_name, &branch, build_number, &ctx.config).await {
            Ok(log) => {
                let truncated = if log.len() > 5000 {
                    format!("{}...[日志已截断，共 {} 字符]", &log[..5000], log.len())
                } else {
                    log
                };
                ctx.build_log = Some(truncated.clone());
                StepResult::Success {
                    message: format!("获取日志成功 ({} 字符)", truncated.len()),
                }
            }
            Err(e) => StepResult::Failed { error: e.to_string() },
        }
    }
}
