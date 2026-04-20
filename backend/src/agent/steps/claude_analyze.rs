use super::super::claude;
use super::super::step::{Step, StepContext, StepResult};

pub struct ClaudeAnalyzeStep {
    pub prompt_template: String,
}

impl Default for ClaudeAnalyzeStep {
    fn default() -> Self {
        Self {
            prompt_template: "你是一个 DevOps 工程师。请分析以下构建结果，给出:\n1. 构建状态摘要\n2. 如果失败，分析可能的失败原因\n3. 修复建议\n\n构建数据: {}"
                .to_string(),
        }
    }
}

#[async_trait::async_trait]
impl Step for ClaudeAnalyzeStep {
    fn name(&self) -> &str {
        "ClaudeAnalyze"
    }

    async fn execute(&self, ctx: &mut StepContext) -> StepResult {
        let analysis_input = match &ctx.build_log {
            Some(log) => format!("构建日志 ({} 字符):\n{}", log.len(), log),
            None => match &ctx.pipeline_status {
                Some(status) => format!("构建状态 JSON:\n{}", serde_json::to_string_pretty(status).unwrap_or_default()),
                None => return StepResult::Abort { reason: "没有可分析的数据（需要 build_log 或 pipeline_status）".to_string() },
            },
        };

        let prompt = self.prompt_template.replace("{}", &analysis_input);

        match claude::call_claude_code(&prompt, "Bash,Read,Write,Grep,Glob").await {
            Ok(result) => {
                ctx.analysis_result = Some(result.clone());
                StepResult::Success { message: "分析完成".to_string() }
            }
            Err(e) => StepResult::Failed { error: e.to_string() },
        }
    }
}
