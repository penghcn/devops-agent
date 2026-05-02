//! LLM Config Store — LLM 配置存储。
//!
//! 从环境变量加载 OpenAI / Anthropic 的 base_url 和 api_key。
//! 配置不可运行时修改（只读），前端仅可查看脱敏后的配置。

use std::sync::{Arc, RwLock};

use super::{
    AnthropicConfig, AnthropicProvider, LlmProvider, ModelRouter, ModelRouterConfig, OpenAIConfig,
    OpenAIProvider,
};

/// 单-provider 配置
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ProviderConfig {
    pub api_key: Option<String>,
    pub base_url: Option<String>,
    pub model_flash: Option<String>,
    pub model_pro: Option<String>,
}

impl Default for ProviderConfig {
    fn default() -> Self {
        Self {
            api_key: None,
            base_url: None,
            model_flash: None,
            model_pro: None,
        }
    }
}

/// 当前 LLM 配置快照（用于读取）
#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct LlmConfigSnapshot {
    pub openai: ProviderConfig,
    pub anthropic: ProviderConfig,
}

/// 更新 LLM 配置请求
#[derive(Debug, Clone, Default)]
pub struct LlmConfigUpdate {
    pub openai: Option<ProviderConfig>,
    pub anthropic: Option<ProviderConfig>,
}

/// 返回配置快照，api_key 被脱敏
impl LlmConfigSnapshot {
    pub fn with_masked_keys(&self) -> LlmConfigSnapshot {
        let mut snapshot = self.clone();
        snapshot.openai.api_key = self.openai.api_key.as_ref().map(|k| mask_api_key(k));
        snapshot.anthropic.api_key = self.anthropic.api_key.as_ref().map(|k| mask_api_key(k));
        snapshot
    }
}

/// 可运行时更新的 LLM 配置存储
pub struct LlmConfigStore {
    inner: RwLock<LlmConfigSnapshot>,
    router_config: ModelRouterConfig,
}

impl Default for LlmConfigStore {
    fn default() -> Self {
        Self::new(ModelRouterConfig::default())
    }
}

impl LlmConfigStore {
    pub fn new(router_config: ModelRouterConfig) -> Self {
        Self {
            inner: RwLock::new(LlmConfigSnapshot::default()),
            router_config,
        }
    }

    /// 从环境变量初始化配置（后端启动时调用）
    pub fn from_env(
        openai_api_key: Option<&str>,
        openai_base_url: Option<&str>,
        openai_model_flash: Option<&str>,
        openai_model_pro: Option<&str>,
        anthropic_api_key: Option<&str>,
        anthropic_base_url: Option<&str>,
        anthropic_model_flash: Option<&str>,
        anthropic_model_pro: Option<&str>,
    ) -> Self {
        let mut snapshot = LlmConfigSnapshot::default();

        if let Some(key) = openai_api_key
            && !key.is_empty()
        {
            snapshot.openai.api_key = Some(key.to_string());
            snapshot.openai.base_url = openai_base_url.map(|s| s.to_string());
            snapshot.openai.model_flash = openai_model_flash.map(|s| s.to_string());
            snapshot.openai.model_pro = openai_model_pro.map(|s| s.to_string());
        }

        if let Some(key) = anthropic_api_key
            && !key.is_empty()
        {
            snapshot.anthropic.api_key = Some(key.to_string());
            snapshot.anthropic.base_url = anthropic_base_url.map(|s| s.to_string());
            snapshot.anthropic.model_flash = anthropic_model_flash.map(|s| s.to_string());
            snapshot.anthropic.model_pro = anthropic_model_pro.map(|s| s.to_string());
        }

        Self {
            inner: RwLock::new(snapshot),
            router_config: ModelRouterConfig::default(),
        }
    }

    /// 读取当前配置快照
    pub fn snapshot(&self) -> LlmConfigSnapshot {
        self.inner.read().unwrap().clone()
    }

    /// 更新配置（合并更新，None 字段不修改）
    /// 仅用于单元测试，生产环境配置不可修改
    #[doc(hidden)]
    pub fn update(&self, update: LlmConfigUpdate) {
        let mut config = self.inner.write().unwrap();
        if let Some(openai) = &update.openai {
            if let Some(k) = &openai.api_key {
                config.openai.api_key = Some(k.clone());
            }
            if let Some(u) = &openai.base_url {
                config.openai.base_url = Some(u.clone());
            }
            if let Some(m) = &openai.model_flash {
                config.openai.model_flash = Some(m.clone());
            }
            if let Some(m) = &openai.model_pro {
                config.openai.model_pro = Some(m.clone());
            }
        }
        if let Some(anthropic) = &update.anthropic {
            if let Some(k) = &anthropic.api_key {
                config.anthropic.api_key = Some(k.clone());
            }
            if let Some(u) = &anthropic.base_url {
                config.anthropic.base_url = Some(u.clone());
            }
            if let Some(m) = &anthropic.model_flash {
                config.anthropic.model_flash = Some(m.clone());
            }
            if let Some(m) = &anthropic.model_pro {
                config.anthropic.model_pro = Some(m.clone());
            }
        }
    }

    /// 根据当前配置重建 ModelRouter
    pub fn build_router(&self) -> Option<Arc<dyn LlmProvider>> {
        let config = self.inner.read().unwrap();
        let mut router = ModelRouter::new(self.router_config.clone());
        let mut has_any = false;

        // Build OpenAI provider
        if let Some(ref key) = config.openai.api_key
            && !key.is_empty()
        {
            let cfg = OpenAIConfig {
                api_key: key.clone(),
                base_url: config
                    .openai
                    .base_url
                    .clone()
                    .unwrap_or_else(|| "https://api.openai.com".to_string()),
                default_model: config
                    .openai
                    .model_flash
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
        if let Some(ref key) = config.anthropic.api_key
            && !key.is_empty()
        {
            let cfg = AnthropicConfig {
                api_key: key.clone(),
                base_url: config
                    .anthropic
                    .base_url
                    .clone()
                    .unwrap_or_else(|| "https://api.anthropic.com".to_string()),
                default_model: config
                    .anthropic
                    .model_flash
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
            None
        }
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
    fn test_config_store_update() {
        let store = LlmConfigStore::default();
        store.update(LlmConfigUpdate {
            openai: Some(ProviderConfig {
                api_key: Some("sk-test-key".to_string()),
                base_url: Some("https://custom.api.com/v1".to_string()),
                model_flash: None,
                model_pro: None,
            }),
            anthropic: None,
        });

        let snapshot = store.snapshot();
        assert_eq!(snapshot.openai.api_key, Some("sk-test-key".to_string()));
        assert_eq!(
            snapshot.openai.base_url,
            Some("https://custom.api.com/v1".to_string())
        );
        assert!(snapshot.anthropic.api_key.is_none());
    }

    #[test]
    fn test_config_store_partial_update() {
        let store = LlmConfigStore::default();
        // First set both
        store.update(LlmConfigUpdate {
            openai: Some(ProviderConfig {
                api_key: Some("sk-old".to_string()),
                base_url: Some("https://old.api.com".to_string()),
                model_flash: None,
                model_pro: None,
            }),
            anthropic: None,
        });
        // Partial update: only change api_key
        store.update(LlmConfigUpdate {
            openai: Some(ProviderConfig {
                api_key: Some("sk-new".to_string()),
                base_url: None,
                model_flash: None,
                model_pro: None,
            }),
            anthropic: None,
        });

        let snapshot = store.snapshot();
        assert_eq!(snapshot.openai.api_key, Some("sk-new".to_string()));
        // base_url should be preserved
        assert_eq!(
            snapshot.openai.base_url,
            Some("https://old.api.com".to_string())
        );
    }

    #[test]
    fn test_masked_snapshot() {
        let store = LlmConfigStore::default();
        store.update(LlmConfigUpdate {
            openai: Some(ProviderConfig {
                api_key: Some("sk-abcdefghij12345".to_string()),
                base_url: None,
                model_flash: None,
                model_pro: None,
            }),
            anthropic: None,
        });

        let masked = store.snapshot().with_masked_keys();
        assert_eq!(masked.openai.api_key, Some("sk-a****2345".to_string()));
    }

    #[test]
    fn test_model_flash_and_pro_storage() {
        let store = LlmConfigStore::default();
        store.update(LlmConfigUpdate {
            openai: Some(ProviderConfig {
                api_key: Some("sk-test".to_string()),
                base_url: None,
                model_flash: Some("gpt-4o-mini".to_string()),
                model_pro: Some("o3".to_string()),
            }),
            anthropic: None,
        });

        let snapshot = store.snapshot();
        assert_eq!(snapshot.openai.model_flash, Some("gpt-4o-mini".to_string()));
        assert_eq!(snapshot.openai.model_pro, Some("o3".to_string()));
    }

    #[test]
    fn test_from_env_with_models() {
        let store = LlmConfigStore::from_env(
            Some("sk-test"),
            Some("https://custom.api.com"),
            Some("gpt-4o-mini"),
            Some("o3"),
            None,
            None,
            None,
            None,
        );

        let snapshot = store.snapshot();
        assert_eq!(snapshot.openai.api_key, Some("sk-test".to_string()));
        assert_eq!(
            snapshot.openai.base_url,
            Some("https://custom.api.com".to_string())
        );
        assert_eq!(snapshot.openai.model_flash, Some("gpt-4o-mini".to_string()));
        assert_eq!(snapshot.openai.model_pro, Some("o3".to_string()));
        assert!(snapshot.anthropic.api_key.is_none());
    }
}
