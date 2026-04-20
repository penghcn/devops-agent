use devops_agent::agent::{Intent, IntentRouter, StepContext, TaskType};
use devops_agent::config::Config;
use std::sync::Arc;

#[ctor::ctor]
fn init_env() {
    dotenv::dotenv().ok();
}

/// 测试 StepContext 创建
#[tokio::test]
async fn test_step_context_creation() {
    let config = Config::from_env();
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

/// 测试 IntentRouter 识别 deploy 意图
#[tokio::test]
async fn test_intent_deploy() {
    let router = IntentRouter;
    let intent = router.identify("部署 order-service 到 staging 环境").await;
    assert!(matches!(intent, Intent::DeployPipeline { .. }));
}

/// 测试 IntentRouter 识别 build 意图
#[tokio::test]
async fn test_intent_build() {
    let router = IntentRouter;
    let intent = router.identify("构建 ds-pkg 项目").await;
    assert!(matches!(intent, Intent::BuildPipeline { .. }));
}

/// 测试 IntentRouter 识别 query 意图
#[tokio::test]
async fn test_intent_query() {
    let router = IntentRouter;
    let intent = router.identify("查询 ds-pkg dev 分支的构建状态").await;
    assert!(matches!(intent, Intent::QueryPipeline { .. }));
}

/// 测试 IntentRouter 识别 analyze 意图
#[tokio::test]
async fn test_intent_analyze() {
    let router = IntentRouter;
    let intent = router.identify("分析 ds-pkg dev 分支的构建日志").await;
    assert!(matches!(intent, Intent::AnalyzeBuild { .. }));
}

/// 测试 IntentRouter 的 StepChain 映射 — DeployPipeline
#[test]
fn test_chain_deploy_pipeline() {
    let router = IntentRouter;
    let intent = Intent::DeployPipeline {
        job_name: "ds-pkg".to_string(),
        branch: Some("dev".to_string()),
    };
    let _chain = router.to_chain_with_prompt(&intent, "部署 ds-pkg");
    // StepChain 内部 steps 是私有字段，无法直接测试数量
    // 但可以通过 execute 端到端验证
}

/// 测试 IntentRouter 的 StepChain 映射 — QueryPipeline
#[test]
fn test_chain_query_pipeline() {
    let router = IntentRouter;
    let intent = Intent::QueryPipeline {
        job_name: "ds-pkg".to_string(),
        branch: Some("dev".to_string()),
    };
    let _chain = router.to_chain_with_prompt(&intent, "查询 ds-pkg dev 状态");
    // 同上，端到端验证
}
