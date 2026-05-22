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
    general_ab_ratio: f64,
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
            // A/B 分流：根据 prompt 哈希值分流，保证同一请求稳定走同一条路径
            let use_tool_use_loop = if general_ab_ratio >= 1.0 {
                true
            } else if general_ab_ratio <= 0.0 {
                false
            } else {
                let hash = simple_hash(prompt);
                (hash % 100) < (general_ab_ratio * 100.0) as u64
            };

            if let Some(provider) = llm_provider {
                let model = llm_model.unwrap_or_else(|| "gpt-4o-mini".to_string());
                if use_tool_use_loop {
                    StepChain::new(vec![Box::new(ToolUseLoopStep::new(
                        prompt.to_string(),
                        provider,
                        model,
                    ))])
                } else {
                    StepChain::new(vec![Box::new(ClaudeCodeStep {
                        prompt: prompt.to_string(),
                        allowed_tools: "Bash,Read,Write".to_string(),
                        llm_provider: Some(provider),
                        llm_model: Some(model),
                    })])
                }
            } else {
                // 无 Provider 时降级到 Claude Code CLI
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

/// 简单的字符串哈希，用于 A/B 分流
fn simple_hash(s: &str) -> u64 {
    let mut hash: u64 = 5381;
    for c in s.bytes() {
        hash = hash.wrapping_mul(33).wrapping_add(c as u64);
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::step::StepContext;
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
            1.0,
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
        let chain = to_chain_with_prompt(&Intent::General, "test prompt", None, None, 1.0);

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
        let chain =
            to_chain_with_prompt(&Intent::General, "test prompt", Some(provider), None, 1.0);

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
            1.0,
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
        let chain = to_chain_with_prompt(&intent, "query test", None, None, 1.0);

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
        let chain = to_chain_with_prompt(&intent, "analyze test", None, None, 1.0);

        let names = chain.step_names();
        assert_eq!(
            names,
            vec!["JobValidate", "JenkinsLog", "BuildAnalysis"],
            "AnalyzeBuild 步骤链应保持不变"
        );
    }

    #[test]
    fn general_ab_split_intermediate_ratio() {
        let provider: Arc<dyn LlmProvider> = Arc::new(MockProvider);
        let config = Arc::new(crate::config::Config::test_default());
        let ctx = StepContext::new("test".to_string(), Default::default(), None, None, config);

        // ratio=0.5: hash%100 < 50 → ToolUseLoop, >= 50 → ClaudeCode
        // "test 30" → hash%100=20 < 50 → ToolUseLoop
        let chain_low = to_chain_with_prompt(
            &Intent::General,
            "test 30",
            Some(provider.clone()),
            Some("gpt-4o-mini".to_string()),
            0.5,
        );
        let descs_low = chain_low.step_descriptions(&ctx);
        assert!(
            !descs_low.first().unwrap().contains("(CLI)"),
            "hash%100=20 < 50 时应走 ToolUseLoop"
        );

        // "test 0" → hash%100=81 >= 50 → ClaudeCode
        let chain_high = to_chain_with_prompt(
            &Intent::General,
            "test 0",
            Some(provider.clone()),
            Some("gpt-4o-mini".to_string()),
            0.5,
        );
        let descs_high = chain_high.step_descriptions(&ctx);
        assert!(
            descs_high.first().unwrap().contains("(CLI)"),
            "hash%100=81 >= 50 时应走 ClaudeCode 降级路径"
        );
    }
}
