use crate::tools::jenkins;

use super::super::step::{Step, StepContext, StepResult};

pub struct JenkinsTriggerStep;

#[async_trait::async_trait]
impl Step for JenkinsTriggerStep {
    fn name(&self) -> &str {
        "JenkinsTrigger"
    }

    async fn execute(&self, ctx: &mut StepContext) -> StepResult {
        let job_name = match &ctx.job_name {
            Some(name) => name.clone(),
            None => return StepResult::Abort { reason: "未提供 job_name".to_string() },
        };

        let branch = ctx.branch.as_deref();

        match jenkins::trigger_pipeline(&job_name, branch, &ctx.config).await {
            Ok(message) => {
                // 从消息中提取构建号
                if let Some(build_num) = extract_build_number(&message) {
                    ctx.build_number = Some(build_num);
                    StepResult::Success {
                        message: format!("触发成功, 构建号: {}", build_num),
                    }
                } else {
                    StepResult::Success { message }
                }
            }
            Err(e) => StepResult::Failed { error: e.to_string() },
        }
    }
}

/// 从 Jenkins 返回消息中提取构建号
fn extract_build_number(msg: &str) -> Option<u32> {
    msg.split('/')
        .filter(|s| s.parse::<u32>().is_ok())
        .next()
        .and_then(|s| s.parse::<u32>().ok())
}
