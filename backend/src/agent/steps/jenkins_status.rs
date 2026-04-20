use std::future::Future;
use std::pin::Pin;

use super::super::{Step, StepContext, StepResult};

pub struct JenkinsStatusStep;

impl Step for JenkinsStatusStep {
    fn name(&self) -> &str {
        "jenkins_status"
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

            match crate::tools::jenkins::get_pipeline_status(
                &job_name, &branch, build_number, &ctx.config,
            ).await {
                Ok(status) => {
                    ctx.pipeline_status = Some(status.clone());
                    let result = status.get("result")
                        .and_then(|r| r.as_str())
                        .unwrap_or("RUNNING");
                    let in_progress = status.get("inProgress")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(true);
                    StepResult::Success {
                        message: format!(
                            "Pipeline #{} [{}]: {} ({})",
                            build_number, branch, result,
                            if in_progress { "构建中" } else { "已完成" }
                        ),
                    }
                }
                Err(e) => StepResult::Failed {
                    error: e.to_string(),
                },
            }
        })
    }
}
