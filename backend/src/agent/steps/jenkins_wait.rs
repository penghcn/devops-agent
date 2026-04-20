use super::super::step::{Step, StepContext, StepResult};
use crate::tools::jenkins;

pub struct JenkinsWaitStep {
    pub poll_interval_secs: u64,
    pub max_wait_secs: u64,
}

impl Default for JenkinsWaitStep {
    fn default() -> Self {
        Self {
            poll_interval_secs: 10,
            max_wait_secs: 1800,
        }
    }
}

#[async_trait::async_trait]
impl Step for JenkinsWaitStep {
    fn name(&self) -> &str {
        "JenkinsWait"
    }

    async fn execute(&self, ctx: &mut StepContext) -> StepResult {
        let build_number = match ctx.build_number {
            Some(n) => n,
            None => return StepResult::Abort { reason: "缺少 build_number，需先触发构建".to_string() },
        };

        let job_name = match &ctx.job_name {
            Some(j) => j.clone(),
            None => return StepResult::Abort { reason: "缺少 job_name".to_string() },
        };

        let branch = match &ctx.branch {
            Some(b) => b.clone(),
            None => return StepResult::Abort { reason: "缺少 branch".to_string() },
        };

        match jenkins::wait_for_pipeline(&job_name, &branch, build_number, &ctx.config, self.poll_interval_secs, self.max_wait_secs).await {
            Ok(status) => {
                ctx.pipeline_status = Some(status.clone());
                let result = status.get("result").and_then(|r| r.as_str()).unwrap_or("UNKNOWN");
                let in_progress = status.get("inProgress").and_then(|v| v.as_bool()).unwrap_or(false);
                StepResult::Success {
                    message: format!("构建 #{} 完成: {} (inProgress: {})", build_number, result, in_progress),
                }
            }
            Err(e) => StepResult::Failed { error: e.to_string() },
        }
    }
}
