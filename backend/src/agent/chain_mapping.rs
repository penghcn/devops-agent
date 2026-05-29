use std::sync::Arc;

use crate::agent::intent::Intent;
use crate::agent::step::StepChain;
use crate::agent::steps::{
    build_analysis::BuildAnalysisStep, comparison::ComparisonStep, jenkins_log::JenkinsLogStep,
    jenkins_status::JenkinsStatusStep, jenkins_trigger::JenkinsTriggerStep,
    jenkins_wait::JenkinsWaitStep, job_validate::JobValidateStep,
};
use crate::knowledge::{KnowledgeLearner, KnowledgeRetriever};
use crate::llm::LlmProvider;

/// Map Intent to StepChain
pub fn to_chain_with_prompt(
    intent: &Intent,
    prompt: &str,
    llm_provider: Arc<dyn LlmProvider>,
    llm_model: String,
    knowledge_retriever: Option<Arc<KnowledgeRetriever>>,
    knowledge_learner: Option<Arc<KnowledgeLearner>>,
) -> StepChain {
    match intent {
        Intent::DeployPipeline { .. } | Intent::BuildPipeline { .. } => StepChain::new(vec![
            Box::new(JobValidateStep),
            Box::new(JenkinsTriggerStep),
            Box::new(JenkinsWaitStep::default()),
            Box::new(JenkinsLogStep),
            Box::new(BuildAnalysisStep::new(
                llm_provider.clone(),
                llm_model.clone(),
                knowledge_retriever.clone(),
                knowledge_learner.clone(),
            )),
        ]),
        Intent::QueryPipeline { .. } => {
            StepChain::new(vec![Box::new(JobValidateStep), Box::new(JenkinsStatusStep)])
        }
        Intent::AnalyzeBuild { .. } => StepChain::new(vec![
            Box::new(JobValidateStep),
            Box::new(JenkinsLogStep),
            Box::new(BuildAnalysisStep::new(
                llm_provider,
                llm_model,
                knowledge_retriever,
                knowledge_learner,
            )),
        ]),
        Intent::General => {
            // 两个方案都跑，随机顺序，对比耗时
            StepChain::new(vec![Box::new(ComparisonStep::new(
                prompt.to_string(),
                Some(llm_provider),
                Some(llm_model),
            ))])
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::{ChatRequest, ChatResponse, LlmError};
    use async_trait::async_trait;

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
    fn general_returns_comparison_step() {
        let provider: Arc<dyn LlmProvider> = Arc::new(MockProvider);
        let chain = to_chain_with_prompt(
            &Intent::General,
            "test prompt",
            provider,
            "gpt-4o-mini".to_string(),
            None,
            None,
        );

        let names = chain.step_names();
        assert_eq!(
            names,
            vec!["Agent"],
            "General 意图应返回 ComparisonStep（前端展示为 Agent）"
        );
    }

    #[test]
    fn general_without_provider_returns_comparison() {
        let provider: Arc<dyn LlmProvider> = Arc::new(MockProvider);
        let chain = to_chain_with_prompt(
            &Intent::General,
            "test prompt",
            provider,
            "gpt-4o-mini".to_string(),
            None,
            None,
        );

        let names = chain.step_names();
        assert_eq!(names, vec!["Agent"], "General 意图返回 ComparisonStep");
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
            provider,
            "gpt-4o-mini".to_string(),
            None,
            None,
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
        let provider: Arc<dyn LlmProvider> = Arc::new(MockProvider);
        let chain = to_chain_with_prompt(
            &intent,
            "query test",
            provider,
            "gpt-4o-mini".to_string(),
            None,
            None,
        );

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
        let provider: Arc<dyn LlmProvider> = Arc::new(MockProvider);
        let chain = to_chain_with_prompt(
            &intent,
            "analyze test",
            provider,
            "gpt-4o-mini".to_string(),
            None,
            None,
        );

        let names = chain.step_names();
        assert_eq!(
            names,
            vec!["JobValidate", "JenkinsLog", "BuildAnalysis"],
            "AnalyzeBuild 步骤链应保持不变"
        );
    }
}
