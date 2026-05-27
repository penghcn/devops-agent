//! LLM Config Store — LLM 配置存储。
//!
//! 从 config.toml 加载 provider 配置。
//! 配置不可运行时修改（只读），前端仅可查看脱敏后的配置。

use std::sync::{Arc, RwLock};

use super::base::BaseConfig;
use super::{
    AnthropicProvider, DeepSeekProvider, LLaMAProvider, NVIDIAProvider, OpenAIProvider,
    VLLMProvider,
};
use crate::llm::router::build_dummy_provider;
use crate::llm::{LlmError, LlmProvider, ModelRouter, ModelRouterConfig, ProviderModels};

/// 单-provider 配置
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct ProviderConfig {
    pub id: String,
    pub api_key: Option<String>,
    pub base_url: Option<String>,
    pub model_flash: Option<String>,
    pub model_pro: Option<String>,
}

/// 当前 LLM 配置快照（用于读取）
#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct LlmConfigSnapshot {
    pub providers: Vec<ProviderConfig>,
    pub default_provider: String,
}

impl LlmConfigSnapshot {
    /// 返回配置快照，api_key 被脱敏
    pub fn with_masked_keys(&self) -> LlmConfigSnapshot {
        let mut snapshot = self.clone();
        for pc in &mut snapshot.providers {
            pc.api_key = pc.api_key.as_ref().map(|k| mask_api_key(k));
        }
        snapshot
    }

    /// 根据 provider id 查找配置
    pub fn get_provider(&self, id: &str) -> Option<&ProviderConfig> {
        self.providers.iter().find(|p| p.id == id)
    }

    /// 获取默认 provider 的 model_flash
    pub fn default_model_flash(&self) -> Option<String> {
        self.get_provider(&self.default_provider)
            .and_then(|p| p.model_flash.clone())
    }
}

/// 可运行时更新的 LLM 配置存储
pub struct LlmConfigStore {
    inner: RwLock<LlmConfigSnapshot>,
}

impl Default for LlmConfigStore {
    fn default() -> Self {
        Self {
            inner: RwLock::new(LlmConfigSnapshot::default()),
        }
    }
}

impl LlmConfigStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// 从 ProviderConfig 列表初始化配置
    pub fn from_providers(providers: Vec<ProviderConfig>, default_provider: String) -> Self {
        Self {
            inner: RwLock::new(LlmConfigSnapshot {
                providers,
                default_provider,
            }),
        }
    }

    /// 读取当前配置快照
    pub fn snapshot(&self) -> LlmConfigSnapshot {
        self.inner.read().unwrap().clone()
    }

    /// 根据当前配置重建 ModelRouter。无有效配置时返回 dummy provider。
    pub fn build_router(&self) -> Arc<dyn LlmProvider> {
        let snapshot = self.inner.read().unwrap().clone();
        build_model_router(&snapshot.providers, &snapshot.default_provider)
    }
}

/// 共享的 ModelRouter 构建逻辑（config.rs 和 agent/mod.rs 共用）。
/// 无有效配置时返回 dummy provider。
pub fn build_model_router(
    providers: &[ProviderConfig],
    default_provider: &str,
) -> Arc<dyn LlmProvider> {
    // 把 default_provider 排到最前面注册，确保优先路由
    let mut sorted = providers.to_vec();
    sorted.sort_by(|a, b| {
        let a_is_default = a.id == default_provider;
        let b_is_default = b.id == default_provider;
        b_is_default.cmp(&a_is_default)
    });

    let mut router = ModelRouter::new(ModelRouterConfig::default());

    for pc in &sorted {
        let Some(ref key) = pc.api_key else { continue };
        if key.is_empty() {
            continue;
        }

        let base_url = pc.base_url.clone().ok_or_else(|| {
            tracing::warn!(
                provider = %pc.id,
                "Skipping provider: base_url is required but not configured"
            );
        });
        let base_url = match base_url {
            Ok(url) => url,
            Err(_) => continue,
        };

        let flash = pc.model_flash.clone();

        let base_config = BaseConfig {
            api_key: key.clone(),
            base_url,
            default_model: flash.clone().unwrap_or_default(),
            timeout_secs: 60,
        };

        let result: Result<Arc<dyn LlmProvider>, LlmError> = match pc.id.as_str() {
            "openai" => OpenAIProvider::new(base_config).map(|p| Arc::new(p) as _),
            "anthropic" => AnthropicProvider::new(base_config).map(|p| Arc::new(p) as _),
            "nvidia" => NVIDIAProvider::new(base_config).map(|p| Arc::new(p) as _),
            "deepseek" => DeepSeekProvider::new(base_config).map(|p| Arc::new(p) as _),
            "llama" => LLaMAProvider::new(base_config).map(|p| Arc::new(p) as _),
            "vllm" => VLLMProvider::new(base_config).map(|p| Arc::new(p) as _),
            _ => {
                tracing::warn!(provider = %pc.id, "Unknown provider, skipping");
                continue;
            }
        };

        let provider = match result {
            Ok(p) => p,
            Err(e) => {
                tracing::warn!(
                    provider = %pc.id,
                    error = %e,
                    "Failed to create provider"
                );
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

/// 脱敏 API Key: 显示前 4 位和后 4 位，中间用 **** 代替
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
