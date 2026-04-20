use std::future::Future;
use std::pin::Pin;

use super::super::{Step, StepContext, StepResult};

pub struct JenkinsLogStep;

impl Step for JenkinsLogStep {
    fn name(&self) -> &str {
        "jenkins_log"
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

            let log_url = format!(
                "{}/job/{}/job/{}/{}/consoleText",
                ctx.config.jenkins_url, job_name, branch, build_number
            );

            ctx.build_log = Some(format!("Log URL: {}", log_url));

            StepResult::Success {
                message: format!("构建 #{} 日志已获取", build_number),
            }
        })
    }
}
