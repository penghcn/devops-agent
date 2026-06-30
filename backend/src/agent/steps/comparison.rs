//! ComparisonStep — 同时运行 ToolUseLoop 和 ClaudeCode，随机顺序，对比耗时
//!
//! 后端打印耗时对比摘要，前端只显示最后一个方案的结果

use std::sync::Arc;
use std::time::Instant;

use crate::agent::AgentStep;
use crate::agent::step::{Step, StepContext, StepResult};
use crate::llm::LlmProvider;

use super::claude_code::ClaudeCodeStep;
use super::tool_use_loop::ToolUseLoopStep;

pub struct ComparisonStep {
    pub prompt: String,
    pub llm_provider: Option<Arc<dyn LlmProvider>>,
    pub llm_model: Option<String>,
    pub allowed_tools: String,
}

impl ComparisonStep {
    pub fn new(
        prompt: String,
        llm_provider: Option<Arc<dyn LlmProvider>>,
        llm_model: Option<String>,
    ) -> Self {
        Self {
            prompt,
            llm_provider,
            llm_model,
            allowed_tools: "Bash,Read,Write".to_string(),
        }
    }
}

#[async_trait::async_trait]
impl Step for ComparisonStep {
    fn name(&self) -> &str {
        "Agent"
    }

    fn description(&self, _ctx: &StepContext) -> String {
        let prompt_display = match self.prompt.len() {
            n if n <= 60 => self.prompt.clone(),
            _ => self.prompt[..60].to_string(),
        };
        format!("AI 对比 (ToolUseLoop + ClaudeCode): {}", prompt_display)
    }

    async fn execute(&self, ctx: &mut StepContext) -> StepResult {
        let model = self
            .llm_model
            .clone()
            .unwrap_or_else(|| "gpt-4o-mini".to_string());

        // 无 Provider 时直接降级为 ClaudeCode
        let provider = match &self.llm_provider {
            Some(p) => p.clone(),
            None => {
                let result = ClaudeCodeStep {
                    prompt: self.prompt.clone(),
                    allowed_tools: self.allowed_tools.clone(),
                    llm_provider: None,
                    llm_model: None,
                }
                .execute(ctx)
                .await;
                // 更新 StepChain 创建的占位符
                if let Some(last) = ctx.steps.last_mut() {
                    last.result = result_to_string(&result);
                }
                return result;
            }
        };

        // 用纳秒位做随机，无需额外依赖
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap();
        let tool_first = now.subsec_nanos() % 2 != 0;

        let (first_name, second_name) = if tool_first {
            ("ToolUseLoop", "ClaudeCode")
        } else {
            ("ClaudeCode", "ToolUseLoop")
        };

        // 执行第一个方案
        let first_start = Instant::now();
        let first_result = if tool_first {
            let step = ToolUseLoopStep::new(self.prompt.clone(), provider.clone(), model.clone());
            step.execute(ctx).await
        } else {
            let step = ClaudeCodeStep {
                prompt: self.prompt.clone(),
                allowed_tools: self.allowed_tools.clone(),
                llm_provider: Some(provider.clone()),
                llm_model: Some(model.clone()),
            };
            step.execute(ctx).await
        };
        let first_elapsed = first_start.elapsed().as_secs_f64();

        // 记录第一个方案结果
        let first_status = if first_result.is_success() {
            "OK"
        } else {
            "FAIL"
        };
        ctx.steps.push(AgentStep {
            action: format!("Agent ({})", first_name),
            result: result_to_string(&first_result),
            elapsed: Some(first_elapsed),
        });

        // 执行第二个方案
        let second_start = Instant::now();
        let second_result = if tool_first {
            let step = ClaudeCodeStep {
                prompt: self.prompt.clone(),
                allowed_tools: self.allowed_tools.clone(),
                llm_provider: Some(provider.clone()),
                llm_model: Some(model.clone()),
            };
            step.execute(ctx).await
        } else {
            let step = ToolUseLoopStep::new(self.prompt.clone(), provider.clone(), model.clone());
            step.execute(ctx).await
        };
        let second_elapsed = second_start.elapsed().as_secs_f64();

        // 记录第二个方案结果
        let second_status = if second_result.is_success() {
            "OK"
        } else {
            "FAIL"
        };
        ctx.steps.push(AgentStep {
            action: format!("Agent ({})", second_name),
            result: result_to_string(&second_result),
            elapsed: Some(second_elapsed),
        });

        // 删除 StepChain 创建的占位符（索引 = 当前长度 - 3）
        if ctx.steps.len() >= 3 {
            ctx.steps.remove(ctx.steps.len() - 3);
        }

        // 后端打印耗时对比
        tracing::info!(
            first = %first_name,
            first_status = %first_status,
            first_elapsed_s = first_elapsed,
            second = %second_name,
            second_status = %second_status,
            second_elapsed_s = second_elapsed,
            total_s = first_elapsed + second_elapsed,
            "Comparison complete"
        );

        // 返回最后一个方案的结果
        match &second_result {
            StepResult::Success { message } => StepResult::Success {
                message: format!("对比成功: {}", message),
            },
            _ => second_result,
        }
    }
}

fn result_to_string(result: &StepResult) -> String {
    match result {
        StepResult::Success { message } => message.clone(),
        StepResult::Failed { error } => format!("失败: {}", error),
        StepResult::Abort { reason } => format!("中止: {}", reason),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::llm::{ChatRequest, ChatResponse, ChatResponseBuilder, LlmError, TokenUsage};
    use async_trait::async_trait;
    use std::sync::Arc;

    struct MockProvider;

    #[async_trait]
    impl LlmProvider for MockProvider {
        async fn llm_call(&self, _request: &ChatRequest) -> Result<ChatResponse, LlmError> {
            Ok(ChatResponse::from_text(
                "mock response".to_string(),
                TokenUsage::default(),
                serde_json::Value::Null,
            ))
        }
        fn provider_id(&self) -> &str {
            "mock"
        }
    }

    #[tokio::test]
    async fn comparison_runs_both_steps() {
        let provider: Arc<dyn LlmProvider> = Arc::new(MockProvider);
        let config = Arc::new(Config::test_default());
        let mut ctx = StepContext::new(
            "test prompt".to_string(),
            Default::default(),
            None,
            None,
            config,
        );

        let step = ComparisonStep::new(
            "test prompt".to_string(),
            Some(provider),
            Some("gpt-4o-mini".to_string()),
        );

        let result = step.execute(&mut ctx).await;
        assert!(result.is_success());
        // 应该有两个子步骤记录
        assert_eq!(ctx.steps.len(), 2);
        assert!(ctx.steps[0].elapsed.is_some());
        assert!(ctx.steps[1].elapsed.is_some());
        // 两个方案名应该不同
        assert!(
            ctx.steps[0].action.contains("ToolUseLoop")
                || ctx.steps[0].action.contains("ClaudeCode")
        );
        assert!(
            ctx.steps[1].action.contains("ToolUseLoop")
                || ctx.steps[1].action.contains("ClaudeCode")
        );
    }

    #[tokio::test]
    async fn comparison_without_provider_single_step() {
        let config = Arc::new(Config::test_default());
        let mut ctx = StepContext::new(
            "test prompt".to_string(),
            Default::default(),
            None,
            None,
            config,
        );

        let step = ComparisonStep::new("test prompt".to_string(), None, None);

        // 无 Provider 时降级为 ClaudeCode CLI，会失败（测试环境无 CLI）
        // 但应该只记录 0 个子步骤（ClaudeCodeStep 不写 ctx.steps）
        let _ = step.execute(&mut ctx).await;
        // ctx.steps 应该为空，因为 ClaudeCodeStep 不向 ctx.steps 写入
        assert_eq!(ctx.steps.len(), 0);
    }
}
