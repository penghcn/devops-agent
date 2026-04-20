use std::future::Future;
use std::pin::Pin;

use super::super::claude;
use super::super::{Step, StepContext, StepResult};

pub struct ClaudeCodeStep;

impl Step for ClaudeCodeStep {
    fn name(&self) -> &str {
        "claude_code"
    }

    fn execute<'a>(
        &'a self,
        ctx: &'a mut StepContext,
    ) -> Pin<Box<dyn Future<Output = StepResult> + Send + 'a>> {
        Box::pin(async move {
            let allowed_tools = "Bash,Read,Write";

            match claude::call_claude_code(&ctx.prompt, allowed_tools).await {
                Ok(result) => StepResult::Success { message: result },
                Err(e) => StepResult::Failed {
                    error: e.to_string(),
                },
            }
        })
    }
}
