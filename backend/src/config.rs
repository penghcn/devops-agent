use dotenv::dotenv;
use std::env;

#[derive(Debug, Clone)]
pub struct Config {
    pub jenkins_url: String,
    pub jenkins_user: String,
    pub jenkins_token: String,
    pub gitlab_url: String,
    pub gitlab_token: String,
    pub claude_code_path: String,
}

impl Config {
    pub fn from_env() -> Self {
        dotenv().ok();
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
        }
    }

    #[cfg(test)]
    pub fn test_default() -> Self {
        Self {
            jenkins_url: "http://localhost:8080".to_string(),
            jenkins_user: "test-user".to_string(),
            jenkins_token: "test-token".to_string(),
            gitlab_url: "https://gitlab.com".to_string(),
            gitlab_token: "test-token".to_string(),
            claude_code_path: "claude".to_string(),
        }
    }
}
