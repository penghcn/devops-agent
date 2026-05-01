use std::env;

#[derive(Debug, Clone)]
pub struct Config {
    pub jenkins_url: String,
    pub jenkins_user: String,
    pub jenkins_token: String,
    pub gitlab_url: String,
    pub gitlab_token: String,
    pub claude_code_path: String,
    pub openai_api_key: Option<String>,
    pub openai_base_url: Option<String>,
    pub anthropic_api_key: Option<String>,
    pub anthropic_base_url: Option<String>,
}

impl Config {
    pub fn from_env() -> Self {
        // 尝试从多个位置加载 .env 文件
        // 1. 当前工作目录
        // 2. backend/.env（项目结构）
        // 3. ../backend/.env（从 frontend 目录运行）
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
            openai_api_key: env::var("OPENAI_API_KEY").ok(),
            openai_base_url: env::var("OPENAI_BASE_URL").ok(),
            anthropic_api_key: env::var("ANTHROPIC_API_KEY")
                .or_else(|_| env::var("ANTHROPIC_AUTH_TOKEN"))
                .ok(),
            anthropic_base_url: env::var("ANTHROPIC_BASE_URL").ok(),
        }
        .validate_llm()
    }

    /// Validate that at least one LLM provider is configured.
    fn validate_llm(self) -> Self {
        let has_openai = self.openai_api_key.as_ref().is_some_and(|k| !k.is_empty());
        let has_anthropic = self
            .anthropic_api_key
            .as_ref()
            .is_some_and(|k| !k.is_empty());
        if !has_openai && !has_anthropic {
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
            openai_api_key: Some("test-openai-key".to_string()),
            openai_base_url: None,
            anthropic_api_key: None,
            anthropic_base_url: None,
        }
    }
}
