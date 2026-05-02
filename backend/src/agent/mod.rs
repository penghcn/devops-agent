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
use crate::llm::{
    AnthropicConfig, AnthropicProvider, LlmConfigStore, LlmProvider, ModelRouter,
    ModelRouterConfig, OpenAIConfig, OpenAIProvider,
};
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
    let default_model = llm_provider.as_ref().map(|_| "gpt-4o-mini".to_string());

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
    let default_model = llm_provider.as_ref().map(|_| "gpt-4o-mini".to_string());

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

/// Build LLM providers from config. Returns a ModelRouter if multiple providers
/// are configured, a single provider if only one is available, or None if neither.
fn build_llm_provider(config: &Config) -> Option<Arc<dyn LlmProvider>> {
    let mut router = ModelRouter::new(ModelRouterConfig::default());
    let mut has_any = false;

    // Build OpenAI provider
    if let Some(ref key) = config.openai_api_key
        && !key.is_empty()
    {
        let cfg = OpenAIConfig {
            api_key: key.clone(),
            base_url: config
                .openai_base_url
                .clone()
                .unwrap_or_else(|| "https://api.openai.com".to_string()),
            default_model: config
                .openai_model_flash
                .clone()
                .unwrap_or_else(|| "gpt-4o-mini".to_string()),
            timeout_secs: 60,
        };
        match OpenAIProvider::new(cfg) {
            Ok(provider) => {
                router.register_provider("openai".into(), Arc::new(provider));
                has_any = true;
            }
            Err(e) => {
                tracing::warn!(error = %e, "Failed to create OpenAI provider");
            }
        }
    }

    // Build Anthropic provider
    if let Some(ref key) = config.anthropic_api_key
        && !key.is_empty()
    {
        let cfg = AnthropicConfig {
            api_key: key.clone(),
            base_url: config
                .anthropic_base_url
                .clone()
                .unwrap_or_else(|| "https://api.anthropic.com".to_string()),
            default_model: config
                .anthropic_model_flash
                .clone()
                .unwrap_or_else(|| "claude-sonnet-4-20250514".to_string()),
            timeout_secs: 60,
        };
        match AnthropicProvider::new(cfg) {
            Ok(provider) => {
                router.register_provider("anthropic".into(), Arc::new(provider));
                has_any = true;
            }
            Err(e) => {
                tracing::warn!(error = %e, "Failed to create Anthropic provider");
            }
        }
    }

    if has_any {
        Some(Arc::new(router))
    } else {
        tracing::error!("No LLM provider configured. Set OPENAI_API_KEY or ANTHROPIC_API_KEY.");
        None
    }
}
