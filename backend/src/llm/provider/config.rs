//! LLM Config Store — LLM 配置存储。

use std::sync::{Arc, Mutex, RwLock};

use crate::llm::router::build_dummy_provider;
use crate::llm::{LlmProvider, ModelRouter, ModelRouterConfig, ProviderModels};

/// 单-provider 配置
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct ProviderConfig {
    pub id: String,
    pub api_key: Option<String>,
    pub base_url: Option<String>,
    pub model_flash: Option<String>,
    pub model_pro: Option<String>,
}

/// 当前 LLM 配置快照
#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct LlmConfigSnapshot {
    pub providers: Vec<ProviderConfig>,
    pub default_provider: String,
}

impl LlmConfigSnapshot {
    pub fn with_masked_keys(&self) -> LlmConfigSnapshot {
        let mut snapshot = self.clone();
        for pc in &mut snapshot.providers {
            pc.api_key = pc.api_key.as_ref().map(|k| mask_api_key(k));
        }
        snapshot
    }

    pub fn get_provider(&self, id: &str) -> Option<&ProviderConfig> {
        self.providers.iter().find(|p| p.id == id)
    }

    pub fn default_model_flash(&self) -> Option<String> {
        self.get_provider(&self.default_provider)
            .and_then(|p| p.model_flash.clone())
    }
}

/// 可运行时更新的 LLM 配置存储
pub struct LlmConfigStore {
    inner: RwLock<LlmConfigSnapshot>,
    cached_router: Mutex<Option<Arc<dyn LlmProvider>>>,
}

impl Default for LlmConfigStore {
    fn default() -> Self {
        Self {
            inner: RwLock::new(LlmConfigSnapshot::default()),
            cached_router: Mutex::new(None),
        }
    }
}

impl LlmConfigStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn from_providers(providers: Vec<ProviderConfig>, default_provider: String) -> Self {
        Self {
            inner: RwLock::new(LlmConfigSnapshot {
                providers,
                default_provider,
            }),
            cached_router: Mutex::new(None),
        }
    }

    pub fn snapshot(&self) -> LlmConfigSnapshot {
        self.inner.read().unwrap().clone()
    }

    pub fn build_router(&self) -> Arc<dyn LlmProvider> {
        {
            let cache = self.cached_router.lock().unwrap();
            if let Some(ref router) = *cache {
                return router.clone();
            }
        }

        let snapshot = self.inner.read().unwrap().clone();
        let router = build_model_router(&snapshot.providers, &snapshot.default_provider);

        let mut cache = self.cached_router.lock().unwrap();
        *cache = Some(router.clone());
        router
    }

    pub fn invalidate_router_cache(&self) {
        let mut cache = self.cached_router.lock().unwrap();
        *cache = None;
    }
}

/// 构建 ModelRouter — 使用 lellm-provider 的 CodecProvider 创建 provider。
pub fn build_model_router(
    providers: &[ProviderConfig],
    _default_provider: &str,
) -> Arc<dyn LlmProvider> {
    // Sort: default provider first
    let mut sorted = providers.to_vec();
    sorted.sort_by(|a, b| {
        let a_is_default = a.id == _default_provider;
        let b_is_default = b.id == _default_provider;
        b_is_default.cmp(&a_is_default)
    });

    let mut router = ModelRouter::new(ModelRouterConfig::default());

    for pc in &sorted {
        let Some(ref key) = pc.api_key else { continue };
        if key.is_empty() {
            continue;
        }
        let Some(ref base_url) = pc.base_url else {
            tracing::warn!(
                provider = %pc.id,
                "Skipping provider: base_url is required but not configured"
            );
            continue;
        };

        let flash = pc.model_flash.clone();

        // Create a LlmProvider adapter using lellm-provider's CodecProvider
        let provider: Option<Arc<dyn LlmProvider>> = match pc.id.as_str() {
            "openai" | "deepseek" | "nvidia" | "llama" | "vllm" => {
                create_openai_compat_provider(key, base_url, flash.as_deref(), &pc.id)
            }
            "anthropic" => create_anthropic_provider(key, base_url, flash.as_deref()),
            _ => {
                tracing::warn!(provider = %pc.id, "Unknown provider, skipping");
                continue;
            }
        };

        let provider = match provider {
            Some(p) => p,
            None => {
                tracing::warn!(provider = %pc.id, "Failed to create provider");
                continue;
            }
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
    }

    if router.is_empty() {
        tracing::warn!("No LLM provider configured, returning dummy provider");
        build_dummy_provider()
    } else {
        Arc::new(router)
    }
}

/// Create an OpenAI-compatible provider using lellm-provider's CodecProvider.
fn create_openai_compat_provider(
    api_key: &str,
    base_url: &str,
    _default_model: Option<&str>,
    provider_id: &str,
) -> Option<Arc<dyn LlmProvider>> {
    use lellm_provider::providers::base::CodecProvider;
    use lellm_provider::providers::openai_compat::OpenAICompatCodec;

    let codec = OpenAICompatCodec {
        provider_id: provider_id.to_string(),
    };
    let provider = CodecProvider::builder(codec)
        .base_url(base_url)
        .api_key(api_key)
        .timeout(std::time::Duration::from_secs(60))
        .build()
        .ok()?;
    Some(Arc::new(LlmProviderAdapter(provider)) as Arc<dyn LlmProvider>)
}

/// Create an Anthropic provider using lellm-provider's CodecProvider.
fn create_anthropic_provider(
    api_key: &str,
    base_url: &str,
    _default_model: Option<&str>,
) -> Option<Arc<dyn LlmProvider>> {
    use lellm_provider::providers::anthropic::AnthropicCodec;
    use lellm_provider::providers::base::CodecProvider;

    let codec = AnthropicCodec;
    let provider = CodecProvider::builder(codec)
        .base_url(base_url)
        .api_key(api_key)
        .timeout(std::time::Duration::from_secs(60))
        .build()
        .ok()?;
    Some(Arc::new(LlmProviderAdapter(provider)) as Arc<dyn LlmProvider>)
}

/// Adapter: wraps lellm_provider::LlmProvider into our project's LlmProvider trait.
struct LlmProviderAdapter<T: lellm_provider::LlmProvider>(T);

#[async_trait::async_trait]
impl<T: lellm_provider::LlmProvider + Send + Sync + 'static> crate::llm::LlmProvider
    for LlmProviderAdapter<T>
{
    async fn llm_call(
        &self,
        request: &crate::llm::ChatRequest,
    ) -> Result<crate::llm::ChatResponse, crate::llm::LlmError> {
        // Convert our ChatRequest to lellm's ChatRequest
        let lellm_request = convert_request(request);
        self.0.call(&lellm_request).await
    }

    fn provider_id(&self) -> &str {
        self.0.provider_id()
    }
}

/// Convert project's ChatRequest to lellm's ChatRequest.
fn convert_request(req: &crate::llm::ChatRequest) -> lellm_core::ChatRequest {
    lellm_core::ChatRequest {
        model: req.model.clone(),
        messages: req.messages.clone(),
        tools: req.tools.clone(),
        temperature: req.temperature,
        tool_choice: req.tool_choice.clone(),
        stop_sequences: req.stop_sequences.clone(),
        prefill: req.prefill.clone(),
        ..Default::default()
    }
}

fn mask_api_key(key: &str) -> String {
    if key.len() <= 8 {
        "****".to_string()
    } else {
        format!("{}****{}", &key[..4], &key[key.len() - 4..])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mask_api_key_long() {
        assert_eq!(mask_api_key("sk-abc123def456ghi"), "sk-a****6ghi");
    }

    #[test]
    fn test_mask_api_key_short() {
        assert_eq!(mask_api_key("short"), "****");
    }

    #[test]
    fn test_mask_api_key_exact_8() {
        assert_eq!(mask_api_key("12345678"), "****");
    }

    #[test]
    fn test_provider_config_lookup() {
        let snapshot = LlmConfigSnapshot {
            providers: vec![
                ProviderConfig {
                    id: "openai".to_string(),
                    api_key: Some("sk-test".to_string()),
                    base_url: Some("https://custom.api.com".to_string()),
                    model_flash: Some("gpt-4o-mini".to_string()),
                    model_pro: Some("o3".to_string()),
                },
                ProviderConfig {
                    id: "anthropic".to_string(),
                    api_key: Some("sk-ant-test".to_string()),
                    base_url: Some("https://api.anthropic.com".to_string()),
                    model_flash: Some("Qwen3.6".to_string()),
                    model_pro: None,
                },
            ],
            default_provider: "anthropic".to_string(),
        };

        assert!(snapshot.get_provider("openai").is_some());
        assert!(snapshot.get_provider("anthropic").is_some());
        assert!(snapshot.get_provider("unknown").is_none());
        assert_eq!(snapshot.default_model_flash(), Some("Qwen3.6".to_string()));
    }

    #[test]
    fn test_masked_snapshot() {
        let snapshot = LlmConfigSnapshot {
            providers: vec![ProviderConfig {
                id: "openai".to_string(),
                api_key: Some("sk-abcdefghij12345".to_string()),
                base_url: Some("https://api.openai.com".to_string()),
                model_flash: None,
                model_pro: None,
            }],
            default_provider: "openai".to_string(),
        };

        let masked = snapshot.with_masked_keys();
        assert_eq!(
            masked.providers[0].api_key,
            Some("sk-a****2345".to_string())
        );
    }

    #[test]
    fn test_from_providers() {
        let store = LlmConfigStore::from_providers(
            vec![ProviderConfig {
                id: "openai".to_string(),
                api_key: Some("sk-test".to_string()),
                base_url: Some("https://custom.api.com".to_string()),
                model_flash: Some("gpt-4o-mini".to_string()),
                model_pro: Some("o3".to_string()),
            }],
            "openai".to_string(),
        );

        let snapshot = store.snapshot();
        assert_eq!(snapshot.providers.len(), 1);
        assert_eq!(snapshot.providers[0].id, "openai");
        assert_eq!(snapshot.providers[0].api_key, Some("sk-test".to_string()));
        assert_eq!(snapshot.default_provider, "openai");
    }
}
