//! Model Router — L1/L2 task classification and provider routing.

use std::sync::Arc;

use super::{ChatRequest, ChatResponse, LlmError, LlmProvider, Message};

/// Task complexity level.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum TaskLevel {
    /// Simple tasks: intent recognition, status queries, short text generation.
    #[default]
    L1,
    /// Complex tasks: code analysis, log analysis, long text understanding.
    L2,
}

/// Model configuration for a single provider.
#[derive(Debug, Clone, Default)]
pub struct ProviderModels {
    /// Fast/cheap model for L1 tasks.
    pub model_flash: Option<String>,
    /// Powerful model for L2 tasks.
    pub model_pro: Option<String>,
    /// Default model (fallback when flash/pro not set). Defaults to model_flash.
    pub default_model: Option<String>,
}

impl ProviderModels {
    /// Select model for a task level.
    pub fn select(&self, level: TaskLevel) -> Result<String, LlmError> {
        let candidate = match level {
            TaskLevel::L1 => &self.model_flash,
            TaskLevel::L2 => &self.model_pro,
        };

        candidate
            .clone()
            .or_else(|| self.default_model.clone())
            .ok_or_else(|| LlmError::InvalidRequest {
                message: format!("no model configured for {:?}", level),
            })
    }
}

/// Configuration for model routing.
#[derive(Debug, Clone)]
pub struct ModelRouterConfig {
    pub default_level: TaskLevel,
    pub max_tokens_l1: u32,
    pub max_tokens_l2: u32,
}

impl Default for ModelRouterConfig {
    fn default() -> Self {
        Self {
            default_level: TaskLevel::L1,
            max_tokens_l1: 1024,
            max_tokens_l2: 4096,
        }
    }
}

/// A dummy provider that always returns an error.
pub struct DummyProvider;

#[async_trait::async_trait]
impl LlmProvider for DummyProvider {
    async fn llm_call(&self, _request: &ChatRequest) -> Result<ChatResponse, LlmError> {
        Err(LlmError::Provider {
            provider: "dummy".to_string(),
            status: Some(503),
            code: None,
            message: "No LLM provider configured".to_string(),
        })
    }

    fn provider_id(&self) -> &str {
        "dummy"
    }
}

/// Build a dummy provider for fallback.
pub fn build_dummy_provider() -> Arc<dyn LlmProvider> {
    Arc::new(DummyProvider)
}

/// Routes LLM requests to the appropriate provider and model.
#[derive(Default)]
pub struct ModelRouter {
    providers: Vec<(String, Arc<dyn LlmProvider>, ProviderModels)>,
}

impl ModelRouter {
    #[allow(dead_code)]
    pub fn new(_config: ModelRouterConfig) -> Self {
        Self {
            providers: Vec::new(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.providers.is_empty()
    }

    pub fn register_provider(
        &mut self,
        id: String,
        provider: Arc<dyn LlmProvider>,
        models: ProviderModels,
    ) {
        self.providers.push((id, provider, models));
    }

    pub fn classify_task(&self, prompt: &str) -> TaskLevel {
        if prompt.len() >= 500 {
            return TaskLevel::L2;
        }

        let complex_keywords = [
            "分析", "analyze", "日志", "log", "debug", "故障", "root cause",
        ];
        if complex_keywords.iter().any(|kw| prompt.contains(kw)) {
            return TaskLevel::L2;
        }

        TaskLevel::L1
    }

    fn resolve(&self, level: TaskLevel) -> Result<(Arc<dyn LlmProvider>, String), LlmError> {
        for (_, provider, models) in &self.providers {
            if let Ok(model) = models.select(level) {
                return Ok((provider.clone(), model));
            }
        }

        Err(LlmError::InvalidRequest {
            message: format!("no provider has a model for {:?}", level),
        })
    }

    fn extract_prompt(messages: &[Message]) -> String {
        messages
            .iter()
            .filter_map(|m| match m {
                Message::User { .. } => {
                    let text = m.content()
                        .iter()
                        .filter_map(|b| b.as_text().map(|s| s.to_string()))
                        .collect::<Vec<_>>()
                        .join("");
                    if text.is_empty() { None } else { Some(text) }
                }
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    pub async fn route(&self, request: &ChatRequest) -> Result<ChatResponse, LlmError> {
        let prompt = Self::extract_prompt(&request.messages);
        let level = self.classify_task(&prompt);
        let (provider, model) = self.resolve(level)?;

        tracing::debug!(
            task_level = ?level,
            model = %model,
            prompt_length = prompt.len(),
            "Routing LLM request"
        );

        let mut routed_request = request.clone();
        routed_request.model = model;

        provider.llm_call(&routed_request).await
    }

    fn find_provider_by_model(&self, model: &str) -> Option<Arc<dyn LlmProvider>> {
        for (_, provider, models) in &self.providers {
            let matched = models
                .model_flash
                .as_ref()
                .is_some_and(|m| m.starts_with(model))
                || models
                    .model_pro
                    .as_ref()
                    .is_some_and(|m| m.starts_with(model))
                || models
                    .default_model
                    .as_ref()
                    .is_some_and(|m| m.starts_with(model));
            if matched {
                return Some(provider.clone());
            }
        }
        tracing::warn!(
            model = %model,
            "No provider found for model, falling back to first registered provider"
        );
        self.providers.first().map(|(_, p, _)| p.clone())
    }
}

#[async_trait::async_trait]
impl LlmProvider for ModelRouter {
    async fn llm_call(&self, request: &ChatRequest) -> Result<ChatResponse, LlmError> {
        if !request.model.is_empty()
            && let Some(provider) = self.find_provider_by_model(&request.model)
        {
            return provider.llm_call(request).await;
        }

        self.route(request).await
    }

    fn provider_id(&self) -> &str {
        "router"
    }
}
