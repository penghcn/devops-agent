//! Provider 公共基础设施。
//!
//! BaseConfig — 公共配置
//! ProviderAdapter — 各 Provider 的 API 格式适配 trait
//! GenericProvider — 通用 LlmProvider 实现

use std::sync::Arc;

use async_trait::async_trait;

use super::client::http_call;
use crate::llm::{ChatRequest, ChatResponse, LlmError, LlmProvider};

/// 公共 LLM 配置
#[derive(Clone)]
pub struct BaseConfig {
    pub api_key: String,
    pub base_url: String,
    pub default_model: String,
    pub timeout_secs: u64,
}

impl std::fmt::Debug for BaseConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BaseConfig")
            .field("api_key", &"****")
            .field("base_url", &self.base_url)
            .field("default_model", &self.default_model)
            .field("timeout_secs", &self.timeout_secs)
            .finish()
    }
}

/// 各 Provider 的 API 格式适配 trait
pub trait ProviderAdapter: Send + Sync {
    fn id(&self) -> &str;
    fn endpoint(&self, base: &str) -> String;
    fn headers(&self, api_key: &str, builder: reqwest::RequestBuilder) -> reqwest::RequestBuilder;
    fn build_request(&self, request: &ChatRequest, default_model: &str) -> serde_json::Value;
    fn parse_response(&self, raw: &serde_json::Value) -> Result<ChatResponse, LlmError>;
}

/// 通用 Provider — 实现 LlmProvider trait
pub struct GenericProvider<T: ProviderAdapter> {
    config: Arc<BaseConfig>,
    client: reqwest::Client,
    adapter: T,
}

impl<T: ProviderAdapter> GenericProvider<T> {
    pub fn new(config: BaseConfig, adapter: T) -> Result<Self, LlmError> {
        if config.api_key.is_empty() {
            return Err(LlmError::MissingApiKey {
                provider: adapter.id().to_string(),
            });
        }

        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(config.timeout_secs))
            .build()
            .map_err(|e| LlmError::ParseError {
                detail: format!("Failed to build HTTP client: {}", e),
            })?;

        Ok(Self {
            config: Arc::new(config),
            client,
            adapter,
        })
    }
}

#[async_trait]
impl<T: ProviderAdapter> LlmProvider for GenericProvider<T> {
    async fn llm_call(&self, request: &ChatRequest) -> Result<ChatResponse, LlmError> {
        let body = self
            .adapter
            .build_request(request, &self.config.default_model);
        let url = self.adapter.endpoint(&self.config.base_url);
        let api_key = self.config.api_key.clone();
        let model = &self.config.default_model;

        let resp = http_call(&self.client, &url, &body, model, &self.adapter.id(), |b| {
            self.adapter.headers(&api_key, b)
        })
        .await?;

        self.adapter.parse_response(&resp.json)
    }

    fn provider_id(&self) -> &str {
        self.adapter.id()
    }
}

impl<T: std::fmt::Debug + ProviderAdapter> std::fmt::Debug for GenericProvider<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GenericProvider")
            .field("config", &self.config)
            .finish_non_exhaustive()
    }
}
