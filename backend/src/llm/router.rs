//! Model Router — L1/L2 task classification and provider routing.
//!
//! Routes LLM requests based on task complexity:
//! - L1: Simple tasks (intent recognition, status queries, short text) → fast/cheap model
//! - L2: Complex tasks (code analysis, log analysis, long text) → powerful model

use std::collections::HashMap;
use std::sync::Arc;

use super::{ChatRequest, ChatResponse, LlmError, LlmProvider};

/// Task complexity level.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum TaskLevel {
    /// Simple tasks: intent recognition, status queries, short text generation.
    /// Uses fast, low-cost models (e.g., gpt-4o-mini).
    #[default]
    L1,
    /// Complex tasks: code analysis, log analysis, long text understanding.
    /// Uses powerful reasoning models (e.g., claude-sonnet-4).
    L2,
}

/// Configuration for model routing.
#[derive(Debug, Clone)]
pub struct ModelRouterConfig {
    /// Model name for L1 tasks (default: "gpt-4o-mini").
    pub l1_model: String,
    /// Model name for L2 tasks (default: "claude-sonnet-4-20250514").
    pub l2_model: String,
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
            l1_model: "gpt-4o-mini".to_string(),
            l2_model: "claude-sonnet-4-20250514".to_string(),
            default_level: TaskLevel::L1,
            max_tokens_l1: 1024,
            max_tokens_l2: 4096,
        }
    }
}

/// Routes LLM requests to the appropriate model based on task complexity.
///
/// Supports registering multiple providers and automatically selects the
/// right one based on the target model name prefix (gpt-* → openai,
/// claude-* → anthropic).
pub struct ModelRouter {
    config: ModelRouterConfig,
    providers: HashMap<String, Arc<dyn LlmProvider>>,
    provider_order: Vec<String>,
}

impl Default for ModelRouter {
    fn default() -> Self {
        Self::new(ModelRouterConfig::default())
    }
}

impl ModelRouter {
    /// Create a new router with the given configuration.
    pub fn new(config: ModelRouterConfig) -> Self {
        Self {
            config,
            providers: HashMap::new(),
            provider_order: Vec::new(),
        }
    }

    /// Register a provider with a stable ID.
    pub fn register_provider(&mut self, id: String, provider: Arc<dyn LlmProvider>) {
        self.provider_order.push(id.clone());
        self.providers.insert(id, provider);
    }

    /// Classify a prompt into L1 (simple) or L2 (complex).
    ///
    /// Classification rules:
    /// - Prompt length >= 500 characters → L2
    /// - Prompt contains complex keywords → L2
    /// - Otherwise → L1
    pub fn classify_task(&self, prompt: &str) -> TaskLevel {
        // Long prompts are complex
        if prompt.len() >= 500 {
            return TaskLevel::L2;
        }

        // Complex keywords indicate L2
        let complex_keywords = [
            "分析", "analyze", "日志", "log", "debug", "故障", "root cause",
        ];
        if complex_keywords.iter().any(|kw| prompt.contains(kw)) {
            return TaskLevel::L2;
        }

        TaskLevel::L1
    }

    /// Select the model name for a given task level.
    pub fn select_model(&self, level: TaskLevel) -> &str {
        match level {
            TaskLevel::L1 => &self.config.l1_model,
            TaskLevel::L2 => &self.config.l2_model,
        }
    }

    /// Extract the user prompt from chat request messages.
    fn extract_prompt(messages: &[super::Message]) -> String {
        messages
            .iter()
            .filter_map(|m| match m {
                super::Message::User { content } => Some(content.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Find the best provider for a target model name.
    ///
    /// Strategy:
    /// 1. Prefix match: gpt-* → openai, claude-* → anthropic
    /// 2. Fallback: first registered provider in order
    fn find_provider_for_model(&self, model: &str) -> Option<Arc<dyn LlmProvider>> {
        // Try prefix-based matching
        let preferred_id = if model.starts_with("gpt") {
            "openai"
        } else if model.starts_with("claude") {
            "anthropic"
        } else {
            // No known prefix, use first provider
            return self
                .provider_order
                .first()
                .and_then(|id| self.providers.get(id).cloned());
        };

        // Find the preferred provider in registration order
        for id in &self.provider_order {
            if id.as_str() == preferred_id {
                return self.providers.get(id).cloned();
            }
        }

        // Fallback to first available provider
        self.provider_order
            .first()
            .and_then(|id| self.providers.get(id).cloned())
    }

    /// Route a chat request through the appropriate model and provider.
    ///
    /// Process:
    /// 1. Classify task complexity from user prompt
    /// 2. Select model for that level
    /// 3. Find matching provider
    /// 4. Build new ChatRequest with selected model
    /// 5. Call provider.chat()
    pub async fn route(&self, request: &ChatRequest) -> Result<ChatResponse, LlmError> {
        // Extract prompt from user messages for classification
        let prompt = Self::extract_prompt(&request.messages);
        let level = self.classify_task(&prompt);
        let model = self.select_model(level);

        tracing::debug!(
            task_level = ?level,
            model = %model,
            prompt_length = prompt.len(),
            "Routing LLM request"
        );

        // Find provider for target model
        let provider = self.find_provider_for_model(model).ok_or_else(|| LlmError::NotFound {
            model: model.to_string(),
        })?;

        // Build new request with selected model
        let mut routed_request = request.clone();
        routed_request.model = model.to_string();

        // Call the provider
        provider.chat(&routed_request).await
    }
}
