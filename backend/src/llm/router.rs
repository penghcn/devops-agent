//! Model Router — L1/L2 task classification and provider routing.
//!
//! Routing flow:
//! 1. Find Provider (by registration order)
//! 2. Select model by task level: L1 → model_flash, L2 → model_pro
//! 3. Fallback to default (model_flash) if flash/pro not configured
//! 4. Error if default is also missing

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
    /// L1 → model_flash, L2 → model_pro. Falls back to default_model.
    /// Returns error if nothing is configured.
    pub fn select(&self, level: TaskLevel) -> Result<String, LlmError> {
        let candidate = match level {
            TaskLevel::L1 => &self.model_flash,
            TaskLevel::L2 => &self.model_pro,
        };

        candidate
            .clone()
            .or_else(|| self.default_model.clone())
            .ok_or_else(|| LlmError::NotFound {
                model: format!("no model configured for {:?}", level),
            })
    }
}

/// Configuration for model routing.
#[derive(Debug, Clone)]
pub struct ModelRouterConfig {
    /// Default task level when classification is uncertain (default: L1).
    pub default_level: TaskLevel,
    /// Maximum tokens for L1 tasks (default: 1024).
    pub max_tokens_l1: u32,
    /// Maximum tokens for L2 tasks (default: 4096).
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

/// Routes LLM requests to the appropriate provider and model.
///
/// Consumers call `chat()` without knowing which provider or model handles the request.
#[derive(Default)]
pub struct ModelRouter {
    /// Registered providers in order: (id, provider, models).
    providers: Vec<(String, Arc<dyn LlmProvider>, ProviderModels)>,
}

impl ModelRouter {
    /// Create a new router. Configuration is reserved for future use.
    #[allow(dead_code)]
    pub fn new(_config: ModelRouterConfig) -> Self {
        Self {
            providers: Vec::new(),
        }
    }

    /// Register a provider with its model configuration.
    pub fn register_provider(
        &mut self,
        id: String,
        provider: Arc<dyn LlmProvider>,
        models: ProviderModels,
    ) {
        self.providers.push((id, provider, models));
    }

    /// Classify a prompt into L1 (simple) or L2 (complex).
    pub fn classify_task(&self, prompt: &str) -> TaskLevel {
        if prompt.len() >= 500 {
            return TaskLevel::L2;
        }

        let complex_keywords = [
            "分析",
            "analyze",
            "日志",
            "log",
            "debug",
            "故障",
            "root cause",
        ];
        if complex_keywords.iter().any(|kw| prompt.contains(kw)) {
            return TaskLevel::L2;
        }

        TaskLevel::L1
    }

    /// Find provider + resolve model for a task level.
    /// Iterates providers in registration order, returns the first one
    /// that has a model configured for the requested level.
    fn resolve(&self, level: TaskLevel) -> Result<(Arc<dyn LlmProvider>, String), LlmError> {
        for (_, provider, models) in &self.providers {
            if let Ok(model) = models.select(level) {
                return Ok((provider.clone(), model));
            }
        }

        Err(LlmError::NotFound {
            model: format!("no provider has a model for {:?}", level),
        })
    }

    /// Extract the user prompt from chat request messages.
    fn extract_prompt(messages: &[Message]) -> String {
        messages
            .iter()
            .filter_map(|m| match m {
                Message::User { content } => Some(content.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Route a chat request: classify → resolve provider+model → call.
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

    /// Find provider by model name prefix (gpt-* → openai, claude-* → anthropic).
    fn find_provider_by_model(&self, model: &str) -> Option<Arc<dyn LlmProvider>> {
        for (id, provider, _) in &self.providers {
            if model.starts_with("gpt-") && id == "openai" {
                return Some(provider.clone());
            }
            if model.starts_with("claude-") && id == "anthropic" {
                return Some(provider.clone());
            }
        }
        // Fallback to first provider if no prefix match.
        self.providers.first().map(|(_, p, _)| p.clone())
    }
}

#[async_trait::async_trait]
impl LlmProvider for ModelRouter {
    async fn llm_call(&self, request: &ChatRequest) -> Result<ChatResponse, LlmError> {
        // Caller specified a model — route by prefix, fallback to first provider.
        if !request.model.is_empty()
            && let Some(provider) = self.find_provider_by_model(&request.model)
        {
            return provider.llm_call(request).await;
        }

        // No model specified — classify task, resolve provider + model.
        self.route(request).await
    }

    fn provider_id(&self) -> &str {
        "router"
    }
}
