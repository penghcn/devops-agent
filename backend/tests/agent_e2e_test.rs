use devops_agent::agent::{AgentRequest, TaskType};
use devops_agent::config::Config;
use devops_agent::tools::jenkins_cache::JenkinsCacheManager;
use std::sync::Arc;

#[ctor::ctor]
fn init_env() {
    dotenv::dotenv().ok();
}

fn make_request(prompt: &str) -> AgentRequest {
    AgentRequest {
        prompt: prompt.to_string(),
        task_type: TaskType::Auto,
        job_name: None,
        branch: None,
    }
}

/// 无法识别的意图返回 General (ClaudeCodeStep)
/// 不触发缓存访问，因为关键词不匹配 parse_simple
#[tokio::test]
async fn test_process_request_general_intent() {
    let config = Config::test_default();
    let cache = Arc::new(JenkinsCacheManager::new(config.clone()));
    // "今天的天气怎么样" 不包含任何 CI-CD 关键词（部署/构建/查询/分析/状态等）
    let req = make_request("今天的天气怎么样");

    let response = devops_agent::agent::process_request(req, &config, cache).await;

    let step_names: Vec<&str> = response.steps.iter().map(|s| s.action.as_str()).collect();
    assert!(
        step_names.contains(&"ClaudeCode"),
        "General intent should use ClaudeCode, got: {:?}",
        step_names
    );
}

/// 部署意图 — 需要 Jenkins 缓存命中才能完整验证 StepChain
#[tokio::test]
#[ignore]
async fn test_process_request_deploy_intent() {
    let config = Config::from_env();
    let cache = Arc::new(JenkinsCacheManager::new(config.clone()));
    cache.refresh().await.ok();
    let req = make_request("部署 ds-pkg/dev 到 staging");

    let response = devops_agent::agent::process_request(req, &config, cache).await;

    let step_names: Vec<&str> = response.steps.iter().map(|s| s.action.as_str()).collect();
    assert!(
        step_names.contains(&"JobValidate"),
        "Deploy chain should start with JobValidate, got: {:?}",
        step_names
    );
}

/// 查询意图 — 需要 Jenkins 缓存
#[tokio::test]
#[ignore]
async fn test_process_request_query_intent() {
    let config = Config::from_env();
    let cache = Arc::new(JenkinsCacheManager::new(config.clone()));
    cache.refresh().await.ok();
    let req = make_request("查询 ds-pkg dev 的构建状态");

    let response = devops_agent::agent::process_request(req, &config, cache).await;

    let step_names: Vec<&str> = response.steps.iter().map(|s| s.action.as_str()).collect();
    assert!(
        step_names.contains(&"JobValidate"),
        "Query chain should have JobValidate, got: {:?}",
        step_names
    );
}

/// 分析意图 — 需要 Jenkins 缓存
#[tokio::test]
#[ignore]
async fn test_process_request_analyze_intent() {
    let config = Config::from_env();
    let cache = Arc::new(JenkinsCacheManager::new(config.clone()));
    cache.refresh().await.ok();
    let req = make_request("分析 ds-pkg dev 的构建日志");

    let response = devops_agent::agent::process_request(req, &config, cache).await;

    let step_names: Vec<&str> = response.steps.iter().map(|s| s.action.as_str()).collect();
    assert!(
        step_names.contains(&"JobValidate"),
        "Analyze chain should have JobValidate, got: {:?}",
        step_names
    );
}

/// 完整链路：触发 → 等待 → 日志 → 分析（需要真实 Jenkins 环境）
#[tokio::test]
#[ignore]
async fn test_e2e_multi_branch_pipeline() {
    let config = Config::from_env();
    let cache = Arc::new(JenkinsCacheManager::new(config.clone()));
    let req = make_request("部署 ds-pkg dev 到 staging");

    let response = devops_agent::agent::process_request(req, &config, cache).await;

    assert!(
        response.success,
        "E2E deploy should succeed: {}",
        response.output
    );
    assert!(
        response.steps.len() >= 4,
        "Should have trigger, wait, log, analyze steps"
    );
}

/// 验证 process_request 返回的 AgentResponse 结构正确
#[tokio::test]
async fn test_process_request_response_structure() {
    let config = Config::test_default();
    let cache = Arc::new(JenkinsCacheManager::new(config.clone()));
    let req = make_request("帮我看看项目状态");

    let response = devops_agent::agent::process_request(req, &config, cache).await;

    // 验证响应结构
    assert!(!response.steps.is_empty(), "should have at least one step");
    for step in &response.steps {
        assert!(!step.action.is_empty(), "step action should not be empty");
        assert!(!step.result.is_empty(), "step result should not be empty");
    }
}
