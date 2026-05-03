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
use crate::llm::provider::{AnthropicProvider, BaseConfig, OpenAIProvider};
use crate::llm::{LlmConfigStore, LlmProvider, ModelRouter, ModelRouterConfig, ProviderModels};
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub branch_correction: Option<String>, // 分支名模糊修正提示
}

#[derive(Debug, Clone, Serialize)]
pub struct AgentStep {
    pub action: String,
    pub result: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub elapsed: Option<f64>,
}

/// 主 Agent 入口 — 基于步骤链架构
pub async fn process_request(
    req: AgentRequest,
    config: &Config,
    cache: Arc<JenkinsCacheManager>,
) -> AgentResponse {
    let llm_provider: Option<Arc<dyn LlmProvider>> = build_llm_provider(config);
    let default_model = resolve_default_model(config);

    let intent_router = if let Some(ref provider) = llm_provider {
        IntentRouter::with_llm(
            cache.clone(),
            provider.clone(),
            default_model.as_deref().unwrap_or("gpt-4o-mini"),
        )
    } else {
        IntentRouter::new(cache)
    };

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

    let intent_router = if let Some(ref provider) = llm_provider {
        IntentRouter::with_llm(
            cache.clone(),
            provider.clone(),
            default_model.as_deref().unwrap_or("gpt-4o-mini"),
        )
    } else {
        IntentRouter::new(cache)
    };

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
fn resolve_default_model(config: &Config) -> Option<String> {
    config
        .llm_providers
        .iter()
        .find(|p| p.id == config.default_provider)
        .and_then(|p| p.model_flash.clone())
}

/// Resolve the default model from LlmConfigStore.
fn resolve_default_model_from_store(store: &LlmConfigStore) -> Option<String> {
    store.snapshot().default_model_flash()
}

/// Build LLM providers from config. Iterates over the unified provider list.
fn build_llm_provider(config: &Config) -> Option<Arc<dyn LlmProvider>> {
    let mut router = ModelRouter::new(ModelRouterConfig::default());
    let mut has_any = false;

    for pc in &config.llm_providers {
        let Some(ref key) = pc.api_key else { continue };
        if key.is_empty() {
            continue;
        }

        let flash = pc.model_flash.clone();

        let base_config = BaseConfig {
            api_key: key.clone(),
            base_url: pc.base_url.clone().unwrap_or_else(|| {
                if pc.id == "openai" {
                    "https://api.openai.com".to_string()
                } else {
                    "https://api.anthropic.com".to_string()
                }
            }),
            default_model: flash.clone().unwrap_or_default(),
            timeout_secs: 60,
        };

        let provider: Arc<dyn LlmProvider> = if pc.id == "openai" {
            match OpenAIProvider::new(base_config) {
                Ok(p) => Arc::new(p),
                Err(e) => {
                    tracing::warn!(error = %e, "Failed to create OpenAI provider");
                    continue;
                }
            }
        } else if pc.id == "anthropic" {
            match AnthropicProvider::new(base_config) {
                Ok(p) => Arc::new(p),
                Err(e) => {
                    tracing::warn!(error = %e, "Failed to create Anthropic provider");
                    continue;
                }
            }
        } else {
            tracing::warn!(provider = %pc.id, "Unknown provider, skipping");
            continue;
        };

        router.register_provider(
            pc.id.clone(),
            provider,
            ProviderModels {
                model_flash: flash.clone(),
                model_pro: pc.model_pro.clone(),
                default_model: flash,
            },
        );
        has_any = true;
    }

    if has_any {
        Some(Arc::new(router))
    } else {
        tracing::error!("No LLM provider configured. Set OPENAI_API_KEY or ANTHROPIC_API_KEY.");
        None
    }
}
