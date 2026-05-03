use std::sync::Arc;

use super::super::claude;
use super::super::step::{Step, StepContext, StepResult};
use crate::llm::{ChatRequest, LlmProvider, Message};

pub struct ClaudeCodeStep {
    pub prompt: String,
    pub allowed_tools: String,
    pub(crate) llm_provider: Option<Arc<dyn LlmProvider>>,
    pub(crate) llm_model: Option<String>,
}

impl ClaudeCodeStep {
    pub fn with_provider(
        prompt: String,
        allowed_tools: String,
        provider: Arc<dyn LlmProvider>,
        model: Option<String>,
    ) -> Self {
        Self {
            llm_provider: Some(provider),
            llm_model: model,
            prompt,
            allowed_tools,
        }
    }
}

#[async_trait::async_trait]
impl Step for ClaudeCodeStep {
    fn name(&self) -> &str {
        "ClaudeCode"
    }

    async fn execute(&self, _ctx: &mut StepContext) -> StepResult {
        let result = if let Some(provider) = &self.llm_provider {
            let model = self
                .llm_model
                .as_deref()
                .unwrap_or("gpt-4o-mini")
                .to_string();
            match provider
                .llm_call(&ChatRequest {
                    model,
                    messages: vec![Message::User {
                        content: self.prompt.clone(),
                    }],
                    tools: None,
                    temperature: Some(0.0),
                })
                .await
            {
                Ok(response) => response.content,
                Err(e) => {
                    tracing::warn!(error = %e, "LlmProvider failed, falling back to Claude Code CLI");
                    match claude::call_claude_code(&self.prompt, &self.allowed_tools).await {
                        Ok(r) => r,
                        Err(e) => {
                            return StepResult::Failed {
                                error: e.to_string(),
                            };
                        }
                    }
                }
            }
        } else {
            match claude::call_claude_code(&self.prompt, &self.allowed_tools).await {
                Ok(r) => r,
                Err(e) => {
                    return StepResult::Failed {
                        error: e.to_string(),
                    };
                }
            }
        };

        StepResult::Success { message: result }
    }
}
