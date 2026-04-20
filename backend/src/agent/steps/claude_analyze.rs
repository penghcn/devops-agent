use super::super::claude;
use super::super::step::{Step, StepContext, StepResult};

pub struct ClaudeAnalyzeStep;

impl Default for ClaudeAnalyzeStep {
    fn default() -> Self {
        Self
    }
}

#[async_trait::async_trait]
impl Step for ClaudeAnalyzeStep {
    fn name(&self) -> &str {
        "ClaudeAnalyze"
    }

    async fn execute(&self, ctx: &mut StepContext) -> StepResult {
        let log = match &ctx.build_log {
            Some(log) if !log.is_empty() => log,
            _ => return StepResult::Abort { reason: "没有可分析的构建日志".to_string() },
        };

        let result = ctx.pipeline_status
            .as_ref()
            .and_then(|s| s.get("result"))
            .and_then(|r| r.as_str())
            .unwrap_or("");

        let prompt = if result == "SUCCESS" {
            build_deploy_check_prompt(log)
        } else {
            build_failure_analysis_prompt(log, result)
        };

        match claude::call_claude_code(&prompt, "Bash,Read,Write,Grep,Glob").await {
            Ok(result) => {
                ctx.analysis_result = Some(result.clone());
                StepResult::Success { message: "分析完成".to_string() }
            }
            Err(e) => StepResult::Failed { error: e.to_string() },
        }
    }
}

fn build_deploy_check_prompt(log: &str) -> String {
    format!(
        "你是一个 DevOps 工程师。请分析以下 Jenkins 构建日志中的 SSH deploy 阶段，给出:\n1. 部署状态（成功/失败）\n2. 部署到了哪些目标服务器/环境\n3. 部署关键日志摘要\n4. 是否需要进一步操作（如验证服务健康状态等）\n\n构建日志:\n{}",
        log
    )
}

fn build_failure_analysis_prompt(log: &str, result: &str) -> String {
    format!(
        "你是一个 DevOps 工程师。请分析以下构建结果，给出:\n1. 构建状态摘要\n2. 如果失败，分析可能的失败原因\n3. 修复建议\n\n构建状态: {}\n构建日志:\n{}",
        result, log
    )
}
