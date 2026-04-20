use super::super::claude;
use super::super::step::{Step, StepContext, StepResult};

pub struct ClaudeCodeStep {
    pub prompt: String,
    pub allowed_tools: String,
}

#[async_trait::async_trait]
impl Step for ClaudeCodeStep {
    fn name(&self) -> &str {
        "ClaudeCode"
    }

    async fn execute(&self, _ctx: &mut StepContext) -> StepResult {
        match claude::call_claude_code(&self.prompt, &self.allowed_tools).await {
            Ok(result) => StepResult::Success { message: result },
            Err(e) => StepResult::Failed { error: e.to_string() },
        }
    }
}
