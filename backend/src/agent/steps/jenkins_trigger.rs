use std::future::Future;
use std::pin::Pin;

use crate::tools::jenkins;

use super::super::{Step, StepContext, StepResult};

pub struct JenkinsTriggerStep;

impl Step for JenkinsTriggerStep {
    fn name(&self) -> &str {
        "jenkins_trigger"
    }

    fn execute<'a>(
        &'a self,
        ctx: &'a mut StepContext,
    ) -> Pin<Box<dyn Future<Output = StepResult> + Send + 'a>> {
        Box::pin(async move {
            let job_name = match &ctx.job_name {
                Some(name) => name.clone(),
                None => return StepResult::Abort { reason: "未提供 job_name".to_string() },
            };

            let branch = ctx.branch.as_deref();

            match jenkins::trigger_pipeline(&job_name, branch, &ctx.config).await {
                Ok(message) => StepResult::Success { message },
                Err(e) => StepResult::Failed {
                    error: e.to_string(),
                },
            }
        })
    }
}
