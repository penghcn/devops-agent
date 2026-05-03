use devops_agent::agent::chain_mapping::to_chain_with_prompt;
use devops_agent::agent::{Intent, IntentRouter, Step, StepContext, StepResult, TaskType};
use devops_agent::app_config::Config;
use devops_agent::tools::jenkins;
use devops_agent::tools::jenkins_cache::JenkinsCacheManager;
use std::sync::Arc;

#[ctor::ctor]
fn init_env() {
    dotenv::dotenv().ok();
}

/// 测试 StepContext 创建
#[tokio::test]
async fn test_step_context_creation() {
    let config = Config::from_file();
    let ctx = StepContext::new(
        "deploy order-service".to_string(),
        TaskType::default(),
        Some("ds-pkg".to_string()),
        Some("dev".to_string()),
        Arc::new(config),
    );

    assert_eq!(ctx.prompt, "deploy order-service");
    assert_eq!(ctx.job_name, Some("ds-pkg".to_string()));
    assert_eq!(ctx.branch, Some("dev".to_string()));
    assert!(ctx.steps.is_empty());
    assert!(ctx.build_number.is_none());
}

/// 测试 JobValidateStep 缺少 job_name 时中止
#[tokio::test]
async fn test_job_validate_missing_job_name() {
    let config = Config::from_file();
    let mut ctx = StepContext::new(
        "test".to_string(),
        TaskType::default(),
        None,
        Some("dev".to_string()),
        Arc::new(config),
    );

    let result = devops_agent::agent::steps::job_validate::JobValidateStep
        .execute(&mut ctx)
        .await;
    match result {
        StepResult::Abort { reason } => assert!(reason.contains("job_name")),
        _ => panic!("Expected Abort, got {:?}", result),
    }
}

/// 测试 JobValidateStep 校验不存在的 Job
#[tokio::test]
#[ignore]
async fn test_job_validate_nonexistent_job() {
    let config = Config::from_file();
    let mut ctx = StepContext::new(
        "test".to_string(),
        TaskType::default(),
        Some("this-job-definitely-does-not-exist-xyz".to_string()),
        Some("dev".to_string()),
        Arc::new(config),
    );

    let result = devops_agent::agent::steps::job_validate::JobValidateStep
        .execute(&mut ctx)
        .await;
    match result {
        StepResult::Failed { error } => assert!(error.contains("不存在")),
        _ => panic!("Expected Failed, got {:?}", result),
    }
}

/// 测试 check_job_exists 函数 — 不存在的 Job
#[tokio::test]
#[ignore]
async fn test_check_job_not_exists() {
    let config = Config::from_file();
    let (exists, _job_type, _name) =
        jenkins::check_job_exists("this-job-definitely-does-not-exist-xyz", &config)
            .await
            .expect("check_job_exists should not error on 404");
    assert!(!exists, "Non-existent job should return exists=false");
}

/// 测试 IntentRouter 识别 deploy 意图
#[tokio::test]
#[ignore]
async fn test_intent_deploy() {
    let config = Config::from_file();
    let cache = Arc::new(JenkinsCacheManager::new(config));
    cache.refresh().await.ok();
    let router = IntentRouter::new(cache);
    let (intent, _) = router.identify("部署 order-service 到 staging 环境").await;
    assert!(matches!(intent, Intent::DeployPipeline { .. }));
}

/// 测试 IntentRouter 识别 build 意图
#[tokio::test]
#[ignore]
async fn test_intent_build() {
    let config = Config::from_file();
    let cache = Arc::new(JenkinsCacheManager::new(config));
    cache.refresh().await.ok();
    let router = IntentRouter::new(cache);
    let (intent, _) = router.identify("构建 ds-pkg 项目").await;
    assert!(matches!(intent, Intent::BuildPipeline { .. }));
}

/// 测试 IntentRouter 识别 query 意图
#[tokio::test]
#[ignore]
async fn test_intent_query() {
    let config = Config::from_file();
    let cache = Arc::new(JenkinsCacheManager::new(config));
    cache.refresh().await.ok();
    let router = IntentRouter::new(cache);
    let (intent, _) = router.identify("查询 ds-pkg dev 分支的构建状态").await;
    assert!(
        matches!(intent, Intent::QueryPipeline { .. }),
        "got: {:?}",
        intent
    );
}

/// 测试 IntentRouter 识别 analyze 意图
#[tokio::test]
#[ignore]
async fn test_intent_analyze() {
    let config = Config::from_file();
    let cache = Arc::new(JenkinsCacheManager::new(config));
    cache.refresh().await.ok();
    let router = IntentRouter::new(cache);
    let (intent, _) = router.identify("分析 ds-pkg dev 分支的构建日志").await;
    assert!(
        matches!(intent, Intent::AnalyzeBuild { .. }),
        "got: {:?}",
        intent
    );
}

/// 测试 IntentRouter 的 StepChain 映射 — DeployPipeline
#[test]
#[ignore]
fn test_chain_deploy_pipeline() {
    let config = Config::from_file();
    let cache = Arc::new(JenkinsCacheManager::new(config));
    let _router = IntentRouter::new(cache);
    let intent = Intent::DeployPipeline {
        job_name: "ds-pkg".to_string(),
        branch: Some("dev".to_string()),
        job_type: Default::default(),
    };
    let _chain = to_chain_with_prompt(&intent, "部署 ds-pkg", None, None);
    // StepChain 内部 steps 是私有字段，无法直接测试数量
    // 但可以通过 execute 端到端验证
}

/// 测试 IntentRouter 的 StepChain 映射 — QueryPipeline
#[test]
#[ignore]
fn test_chain_query_pipeline() {
    let config = Config::from_file();
    let cache = Arc::new(JenkinsCacheManager::new(config));
    let _router = IntentRouter::new(cache);
    let intent = Intent::QueryPipeline {
        job_name: "ds-pkg".to_string(),
        branch: Some("dev".to_string()),
        job_type: Default::default(),
    };
    let _chain = to_chain_with_prompt(&intent, "查询 ds-pkg dev 状态", None, None);
    // 同上，端到端验证
}
