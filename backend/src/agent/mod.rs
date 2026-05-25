pub mod chain_mapping;
pub mod intent;
mod router;
mod step;
pub mod steps;

pub use intent::{Intent, JobType, ParseIntentError};
pub use router::IntentRouter;
pub use step::{Step, StepChain, StepContext, StepResult};

pub mod claude;

use crate::config::Config;
use crate::llm::provider::build_model_router;
use crate::llm::{LlmConfigStore, LlmProvider};
use crate::tools::jenkins_cache::JenkinsCacheManager;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Debug, Deserialize)]
pub struct AgentRequest {
    pub prompt: String,
    #[serde(default)]
    pub task_type: TaskType,
    /// Jenkins Pipeline 项目名称（如 ds-pkg）
    #[serde(default)]
    pub job_name: Option<String>,
    /// 分支名称（如 dev）
    #[serde(default)]
    pub branch: Option<String>,
}

#[derive(Debug, Deserialize, Default, PartialEq)]
pub enum TaskType {
    #[default]
    Auto, // 自动识别
    Deploy,
    Build,
    Query,
}

#[derive(Debug, Serialize)]
pub struct AgentResponse {
    pub success: bool,
    pub output: String,
    pub steps: Vec<AgentStep>, // 展示思考过程
    #[serde(skip_serializing_if = "Option::is_none")]
    pub structured_output: Option<serde_json::Value>, // Claude 结构化分析结果
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub corrections: Vec<Correction>, // job/branch 模糊修正提示
}

/// 模糊修正记录（job 名或分支名）
#[derive(Debug, Clone, Serialize)]
pub struct Correction {
    pub kind: String,
    pub original: String,
    pub corrected: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct AgentStep {
    pub action: String,
    pub result: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub elapsed: Option<f64>,
}

/// SSE 流式推送事件
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type")]
pub enum StreamEvent {
    /// Step 开始执行
    StepStart {
        step_index: usize,
        action: String,
        description: String,
    },
    /// Step 完成
    StepDone {
        step_index: usize,
        action: String,
        result: String,
        elapsed: f64,
    },
    /// 最终完成
    Complete {
        success: bool,
        output: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        structured_output: Option<serde_json::Value>,
        steps: Vec<AgentStep>,
        #[serde(skip_serializing_if = "Vec::is_empty")]
        corrections: Vec<Correction>,
    },
}

/// 主 Agent 入口 — 基于步骤链架构
pub async fn process_request(
    req: AgentRequest,
    config: &Config,
    cache: Arc<JenkinsCacheManager>,
) -> AgentResponse {
    let llm_provider: Arc<dyn LlmProvider> =
        build_model_router(&config.llm_providers, &config.default_provider);
    let default_model = resolve_default_model(config);

    let intent_router =
        IntentRouter::with_llm(cache.clone(), llm_provider.clone(), default_model.as_str());

    intent_router
        .execute(
            &req.prompt,
            req.task_type,
            Arc::new(config.clone()),
            llm_provider,
            default_model,
        )
        .await
}

/// 主 Agent 入口 — 使用 LlmConfigStore 获取 Provider（支持运行时配置）
pub async fn process_request_with_store(
    req: AgentRequest,
    config: &Config,
    cache: Arc<JenkinsCacheManager>,
    store: &LlmConfigStore,
) -> AgentResponse {
    let llm_provider = store.build_router();
    let default_model = resolve_default_model_from_store(store);

    let intent_router = IntentRouter::with_llm(cache.clone(), llm_provider.clone(), &default_model);

    intent_router
        .execute(
            &req.prompt,
            req.task_type,
            Arc::new(config.clone()),
            llm_provider,
            default_model,
        )
        .await
}

/// Resolve the default model: look up the default_provider's model_flash.
/// Falls back to "gpt-4o-mini" if not configured.
fn resolve_default_model(config: &Config) -> String {
    config
        .llm_providers
        .iter()
        .find(|p| p.id == config.default_provider)
        .and_then(|p| p.model_flash.clone())
        .unwrap_or_else(|| "gpt-4o-mini".to_string())
}

/// Resolve the default model from LlmConfigStore.
/// Falls back to "gpt-4o-mini" if not configured.
fn resolve_default_model_from_store(store: &LlmConfigStore) -> String {
    store
        .snapshot()
        .default_model_flash()
        .unwrap_or_else(|| "gpt-4o-mini".to_string())
}
