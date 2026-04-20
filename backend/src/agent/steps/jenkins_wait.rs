use std::future::Future;
use std::pin::Pin;

use super::super::{Step, StepContext, StepResult};

pub struct JenkinsWaitStep;

impl Step for JenkinsWaitStep {
    fn name(&self) -> &str {
        "jenkins_wait"
    }

    fn execute<'a>(
        &'a self,
        ctx: &'a mut StepContext,
    ) -> Pin<Box<dyn Future<Output = StepResult> + Send + 'a>> {
        Box::pin(async move {
            let build_number = match ctx.build_number {
                Some(n) => n,
                None => return StepResult::Failed {
                    error: "没有可用的 build_number".to_string(),
                },
            };

            let job_name = match &ctx.job_name {
                Some(name) => name.clone(),
                None => return StepResult::Failed {
                    error: "未提供 job_name".to_string(),
                },
            };

            let branch = match &ctx.branch {
                Some(b) => b.clone(),
                None => return StepResult::Failed {
                    error: "未提供 branch".to_string(),
                },
            };

            match crate::tools::jenkins::wait_for_pipeline(
                &job_name, &branch, build_number, &ctx.config, 5, 1800,
            ).await {
                Ok(status) => {
                    ctx.pipeline_status = Some(status.clone());
                    StepResult::Success {
                        message: format!("构建 #{} 状态: {:?}", build_number, status),
                    }
                }
                Err(e) => StepResult::Failed {
                    error: e.to_string(),
                },
            }
        })
    }
}
