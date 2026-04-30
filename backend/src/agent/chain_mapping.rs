use crate::agent::intent::Intent;
use crate::agent::step::StepChain;
use crate::agent::steps::{
    claude_analyze::ClaudeAnalyzeStep, claude_code::ClaudeCodeStep, jenkins_log::JenkinsLogStep,
    jenkins_status::JenkinsStatusStep, jenkins_trigger::JenkinsTriggerStep,
    jenkins_wait::JenkinsWaitStep, job_validate::JobValidateStep,
};

/// Map Intent to StepChain
pub fn to_chain_with_prompt(intent: &Intent, prompt: &str) -> StepChain {
    match intent {
        Intent::DeployPipeline { .. } | Intent::BuildPipeline { .. } => {
            StepChain::new(vec![
                Box::new(JobValidateStep),
                Box::new(JenkinsTriggerStep),
                Box::new(JenkinsWaitStep::default()),
                Box::new(JenkinsLogStep),
                Box::new(ClaudeAnalyzeStep),
            ])
        }
        Intent::QueryPipeline { .. } => {
            StepChain::new(vec![Box::new(JobValidateStep), Box::new(JenkinsStatusStep)])
        }
        Intent::AnalyzeBuild { .. } => StepChain::new(vec![
            Box::new(JobValidateStep),
            Box::new(JenkinsLogStep),
            Box::new(ClaudeAnalyzeStep),
        ]),
        Intent::General => StepChain::new(vec![Box::new(ClaudeCodeStep {
            prompt: prompt.to_string(),
            allowed_tools: "Bash,Read,Write".to_string(),
        })]),
    }
}
