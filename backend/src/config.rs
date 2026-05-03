use std::env;

use crate::llm::{ProviderConfig, load_llm_providers};

#[derive(Debug, Clone)]
pub struct Config {
    pub jenkins_url: String,
    pub jenkins_user: String,
    pub jenkins_token: String,
    pub gitlab_url: String,
    pub gitlab_token: String,
    pub claude_code_path: String,
    pub llm_providers: Vec<ProviderConfig>,
    pub default_provider: String,
    /// CORS 允许的域名列表，逗号分隔。默认允许 localhost。
    pub cors_origins: Vec<String>,
    /// API 访问密钥，用于保护后端接口。为空则不启用认证。
    pub api_key: Option<String>,
}

impl Config {
    pub fn from_env() -> Self {
        // 尝试从多个位置加载 .env 文件
        dotenv::dotenv().ok();
        let _ = dotenv::from_path("backend/.env");
        let _ = dotenv::from_path("../backend/.env");
        Self {
            jenkins_url: env::var("JENKINS_URL")
                .expect("JENKINS_URL not set")
                .trim_end_matches('/')
                .to_string(),
            jenkins_user: env::var("JENKINS_USER").expect("JENKINS_USER not set"),
            jenkins_token: env::var("JENKINS_TOKEN").expect("JENKINS_TOKEN not set"),
            gitlab_url: env::var("GITLAB_URL").unwrap_or_else(|_| "https://gitlab.com".to_string()),
            gitlab_token: env::var("GITLAB_TOKEN").expect("GITLAB_TOKEN not set"),
            claude_code_path: env::var("CLAUDE_CODE_PATH").unwrap_or_else(|_| "claude".to_string()),
            llm_providers: load_llm_providers(),
            default_provider: env::var("DEFAULT_PROVIDER")
                .unwrap_or_else(|_| "anthropic".to_string()),
            cors_origins: env::var("CORS_ORIGINS")
                .map(|v| v.split(',').map(|s| s.trim().to_string()).collect())
                .unwrap_or_else(|_| vec!["http://localhost:5173".to_string()]),
            api_key: env::var("API_KEY").ok(),
        }
        .validate_llm()
    }

    /// Validate that at least one LLM provider is configured.
    fn validate_llm(self) -> Self {
        let has_provider = self
            .llm_providers
            .iter()
            .any(|p| p.api_key.as_ref().is_some_and(|k| !k.is_empty()));
        if !has_provider {
            panic!(
                "At least one LLM provider must be configured. \
                 Set OPENAI_API_KEY or ANTHROPIC_API_KEY (optionally with \
                 OPENAI_BASE_URL / ANTHROPIC_BASE_URL for custom endpoints)."
            );
        }
        self
    }

    /// Create a default config for testing.
    /// Note: This bypasses LLM validation. Only use for tests that don't need LLM.
    #[doc(hidden)]
    pub fn test_default() -> Self {
        Self {
            jenkins_url: "http://localhost:8080".to_string(),
            jenkins_user: "test-user".to_string(),
            jenkins_token: "test-token".to_string(),
            gitlab_url: "https://gitlab.com".to_string(),
            gitlab_token: "test-token".to_string(),
            claude_code_path: "claude".to_string(),
            llm_providers: vec![ProviderConfig {
                id: "openai".to_string(),
                api_key: Some("test-openai-key".to_string()),
                base_url: None,
                model_flash: None,
                model_pro: None,
            }],
            default_provider: "openai".to_string(),
            cors_origins: vec!["http://localhost:5173".to_string()],
            api_key: None,
        }
    }
}
