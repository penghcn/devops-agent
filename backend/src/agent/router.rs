use crate::agent::step::{StepChain, StepResult};
use crate::agent::steps::{
    jenkins_trigger::JenkinsTriggerStep,
    jenkins_wait::JenkinsWaitStep,
    jenkins_log::JenkinsLogStep,
    jenkins_status::JenkinsStatusStep,
    claude_analyze::ClaudeAnalyzeStep,
    claude_code::ClaudeCodeStep,
};
use crate::agent::{claude, AgentResponse, StepContext, TaskType};
use std::sync::Arc;

#[derive(Debug, PartialEq)]
pub enum Intent {
    DeployPipeline { job_name: String, branch: Option<String> },
    BuildPipeline { job_name: String, branch: Option<String> },
    QueryPipeline { job_name: String, branch: Option<String> },
    AnalyzeBuild { job_name: String, branch: Option<String> },
    General,
}

pub struct IntentRouter;

impl IntentRouter {
    /// 调用 Claude 识别用户意图
    pub async fn identify(&self, prompt: &str) -> Intent {
        let intent_prompt = format!(
            "判断以下用户意图，只输出一个词（deploy/build/query/analyze），不要输出其他内容：\n{}",
            prompt
        );

        match claude::call_claude_code(&intent_prompt, "").await {
            Ok(response) => {
                let r = response.trim().to_lowercase();
                if r.contains("deploy") {
                    Intent::DeployPipeline {
                        job_name: extract_job_name(prompt),
                        branch: extract_branch(prompt),
                    }
                } else if r.contains("build") {
                    Intent::BuildPipeline {
                        job_name: extract_job_name(prompt),
                        branch: extract_branch(prompt),
                    }
                } else if r.contains("query") {
                    Intent::QueryPipeline {
                        job_name: extract_job_name(prompt),
                        branch: extract_branch(prompt),
                    }
                } else if r.contains("analyze") {
                    Intent::AnalyzeBuild {
                        job_name: extract_job_name(prompt),
                        branch: extract_branch(prompt),
                    }
                } else {
                    Intent::General
                }
            }
            Err(_) => Intent::General,
        }
    }

    /// 根据 Intent 返回对应的 StepChain
    pub fn to_chain_with_prompt(&self, intent: &Intent, prompt: &str) -> StepChain {
        match intent {
            Intent::DeployPipeline { .. } | Intent::BuildPipeline { .. } => {
                StepChain::new(vec![
                    Box::new(JenkinsTriggerStep),
                    Box::new(JenkinsWaitStep::default()),
                    Box::new(JenkinsLogStep),
                    Box::new(ClaudeAnalyzeStep::default()),
                ])
            }
            Intent::QueryPipeline { .. } => {
                StepChain::new(vec![
                    Box::new(JenkinsStatusStep),
                ])
            }
            Intent::AnalyzeBuild { .. } => {
                StepChain::new(vec![
                    Box::new(JenkinsLogStep),
                    Box::new(ClaudeAnalyzeStep::default()),
                ])
            }
            Intent::General => {
                StepChain::new(vec![
                    Box::new(ClaudeCodeStep {
                        prompt: prompt.to_string(),
                        allowed_tools: "Bash,Read,Write".to_string(),
                    }),
                ])
            }
        }
    }

    /// 完整流程：识别意图 + 执行 StepChain
    pub async fn execute(
        &self,
        prompt: &str,
        task_type: TaskType,
        job_name: Option<String>,
        branch: Option<String>,
    ) -> AgentResponse {
        let intent = self.identify(prompt).await;
        let chain = self.to_chain_with_prompt(&intent, prompt);

        let ctx = StepContext::new(
            prompt.to_string(),
            task_type,
            job_name,
            branch,
            Arc::new(crate::config::Config::from_env()),
        );

        let (final_ctx, steps) = chain.execute(ctx).await;

        AgentResponse {
            success: final_ctx.steps.iter().any(|s| {
                s.result.contains("成功") || s.result.contains("完成")
            }),
            output: final_ctx.analysis_result.clone().unwrap_or_else(|| {
                final_ctx
                    .steps
                    .last()
                    .map(|s| s.result.clone())
                    .unwrap_or_else(|| "处理完成".to_string())
            }),
            steps,
        }
    }
}

/// 从 prompt 中提取 job_name（简单启发式）
fn extract_job_name(prompt: &str) -> String {
    prompt
        .split_whitespace()
        .find(|w| w.chars().all(|c| c.is_alphanumeric() || c == '-' || c == '_' || c == '/'))
        .unwrap_or("ds-pkg")
        .to_string()
}

/// 从 prompt 中提取 branch（简单启发式）
fn extract_branch(prompt: &str) -> Option<String> {
    let lower = prompt.to_lowercase();
    if lower.contains("dev") {
        Some("dev".to_string())
    } else if lower.contains("staging") {
        Some("staging".to_string())
    } else if lower.contains("prod") || lower.contains("production") {
        Some("main".to_string())
    } else {
        None
    }
}
