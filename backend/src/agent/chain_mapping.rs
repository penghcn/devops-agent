use std::sync::Arc;

use crate::agent::intent::Intent;
use crate::agent::step::StepChain;
use crate::agent::steps::{
    build_analysis::BuildAnalysisStep, claude_code::ClaudeCodeStep, jenkins_log::JenkinsLogStep,
    jenkins_status::JenkinsStatusStep, jenkins_trigger::JenkinsTriggerStep,
    jenkins_wait::JenkinsWaitStep, job_validate::JobValidateStep, tool_use_loop::ToolUseLoopStep,
};
use crate::llm::LlmProvider;

/// Map Intent to StepChain
pub fn to_chain_with_prompt(
    intent: &Intent,
    prompt: &str,
    llm_provider: Option<Arc<dyn LlmProvider>>,
    llm_model: Option<String>,
) -> StepChain {
    match intent {
        Intent::DeployPipeline { .. } | Intent::BuildPipeline { .. } => StepChain::new(vec![
            Box::new(JobValidateStep),
            Box::new(JenkinsTriggerStep),
            Box::new(JenkinsWaitStep::default()),
            Box::new(JenkinsLogStep),
            Box::new(BuildAnalysisStep::with_provider(
                llm_provider.clone(),
                llm_model.clone(),
            )),
        ]),
        Intent::QueryPipeline { .. } => {
            StepChain::new(vec![Box::new(JobValidateStep), Box::new(JenkinsStatusStep)])
        }
        Intent::AnalyzeBuild { .. } => StepChain::new(vec![
            Box::new(JobValidateStep),
            Box::new(JenkinsLogStep),
            Box::new(BuildAnalysisStep::with_provider(llm_provider, llm_model)),
        ]),
        Intent::General => {
            // 优先使用 ToolUseLoop（LLM 原生工具调用），降级到 ClaudeCode
            if let Some(provider) = llm_provider {
                let model = llm_model.unwrap_or_else(|| "gpt-4o-mini".to_string());
                StepChain::new(vec![Box::new(ToolUseLoopStep::new(
                    prompt.to_string(),
                    provider,
                    model,
                ))])
            } else {
                StepChain::new(vec![Box::new(ClaudeCodeStep {
                    prompt: prompt.to_string(),
                    allowed_tools: "Bash,Read,Write".to_string(),
                    llm_provider: None,
                    llm_model: None,
                })])
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::{ChatRequest, ChatResponse, LlmError};
    use async_trait::async_trait;

    /// 仅用于测试的 Mock LlmProvider（不执行真实调用）
    struct MockProvider;

    #[async_trait]
    impl LlmProvider for MockProvider {
        async fn llm_call(&self, _request: &ChatRequest) -> Result<ChatResponse, LlmError> {
            Err(LlmError::ApiError {
                status: 500,
                body: "mock provider - not for real calls".to_string(),
            })
        }

        fn provider_id(&self) -> &str {
            "mock"
        }
    }

    #[test]
    fn general_with_provider_uses_tool_use_loop() {
        let provider: Arc<dyn LlmProvider> = Arc::new(MockProvider);
        let chain = to_chain_with_prompt(
            &Intent::General,
            "test prompt",
            Some(provider),
            Some("gpt-4o-mini".to_string()),
        );

        let names = chain.step_names();
        assert_eq!(
            names,
            vec!["Agent"],
            "General 意图有 Provider 时应使用 ToolUseLoopStep"
        );
    }

    #[test]
    fn general_without_provider_falls_back_to_claude_code() {
        let chain = to_chain_with_prompt(&Intent::General, "test prompt", None, None);

        let names = chain.step_names();
        assert_eq!(
            names,
            vec!["Agent"],
            "General 意图无 Provider 时应降级到 ClaudeCodeStep"
        );
    }

    #[test]
    fn general_without_model_uses_default() {
        let provider: Arc<dyn LlmProvider> = Arc::new(MockProvider);
        let chain = to_chain_with_prompt(&Intent::General, "test prompt", Some(provider), None);

        let names = chain.step_names();
        assert_eq!(
            names,
            vec!["Agent"],
            "General 意图有 Provider 无 model 时应使用默认 model"
        );
    }

    #[test]
    fn deploy_pipeline_chain_composition() {
        let provider: Arc<dyn LlmProvider> = Arc::new(MockProvider);
        let intent = Intent::DeployPipeline {
            job_name: "test".to_string(),
            branch: Some("main".to_string()),
            job_type: crate::agent::intent::JobType::Standard,
        };
        let chain = to_chain_with_prompt(
            &intent,
            "deploy test",
            Some(provider),
            Some("gpt-4o-mini".to_string()),
        );

        let names = chain.step_names();
        assert_eq!(
            names,
            vec![
                "JobValidate",
                "JenkinsTrigger",
                "JenkinsWait",
                "JenkinsLog",
                "BuildAnalysis"
            ],
            "DeployPipeline 步骤链应保持不变"
        );
    }

    #[test]
    fn query_pipeline_chain_composition() {
        let intent = Intent::QueryPipeline {
            job_name: "test".to_string(),
            branch: Some("main".to_string()),
            job_type: crate::agent::intent::JobType::Standard,
        };
        let chain = to_chain_with_prompt(&intent, "query test", None, None);

        let names = chain.step_names();
        assert_eq!(
            names,
            vec!["JobValidate", "JenkinsStatus"],
            "QueryPipeline 步骤链应保持不变"
        );
    }

    #[test]
    fn analyze_build_chain_composition() {
        let intent = Intent::AnalyzeBuild {
            job_name: "test".to_string(),
            branch: Some("main".to_string()),
            job_type: crate::agent::intent::JobType::Standard,
        };
        let chain = to_chain_with_prompt(&intent, "analyze test", None, None);

        let names = chain.step_names();
        assert_eq!(
            names,
            vec!["JobValidate", "JenkinsLog", "BuildAnalysis"],
            "AnalyzeBuild 步骤链应保持不变"
        );
    }
}
