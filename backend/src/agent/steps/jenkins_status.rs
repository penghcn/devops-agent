use super::super::step::{Step, StepContext, StepResult};
use crate::tools::jenkins;

pub struct JenkinsStatusStep;

#[async_trait::async_trait]
impl Step for JenkinsStatusStep {
    fn name(&self) -> &str {
        "JenkinsStatus"
    }

    async fn execute(&self, ctx: &mut StepContext) -> StepResult {
        let job_name = match &ctx.job_name {
            Some(j) => j.clone(),
            None => {
                return StepResult::Abort {
                    reason: "缺少 job_name".to_string(),
                };
            }
        };

        let branch = match &ctx.branch {
            Some(b) => b.clone(),
            None => {
                return StepResult::Abort {
                    reason: "缺少 branch".to_string(),
                };
            }
        };

        match jenkins::get_job_status(&job_name, &ctx.config).await {
            Ok(job_status) => {
                if let Some(last_build) = job_status
                    .get("lastBuild")
                    .and_then(|b| b.get("number"))
                    .and_then(|n| n.as_u64())
                {
                    let build_num = last_build as u32;
                    match jenkins::get_pipeline_status(&job_name, &branch, build_num, &ctx.config)
                        .await
                    {
                        Ok(status) => {
                            ctx.pipeline_status = Some(status.clone());
                            let result = status
                                .get("result")
                                .and_then(|r| r.as_str())
                                .unwrap_or("RUNNING");
                            let in_progress = status
                                .get("inProgress")
                                .and_then(|v| v.as_bool())
                                .unwrap_or(true);
                            StepResult::Success {
                                message: format!(
                                    "Pipeline #{} [{}]: {} ({}s)",
                                    build_num,
                                    branch,
                                    result,
                                    if in_progress {
                                        "构建中"
                                    } else {
                                        "已完成"
                                    }
                                ),
                            }
                        }
                        Err(e) => StepResult::Failed {
                            error: e.to_string(),
                        },
                    }
                } else {
                    StepResult::Success {
                        message: format!("No builds found for {}/{}", job_name, branch),
                    }
                }
            }
            Err(e) => StepResult::Failed {
                error: e.to_string(),
            },
        }
    }
}
