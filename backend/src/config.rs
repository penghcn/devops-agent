//! 应用配置加载。
//!
//! 支持三种配置源（优先级从高到低）：
//! 1. 系统环境变量
//! 2. `.env` 文件
//! 3. `config.toml`（支持 `${VAR}` 和 `$VAR` 展开）

use std::collections::HashMap;
use std::env;
use std::path::Path;

use serde::Deserialize;

use crate::llm::ProviderConfig;

// ── TOML 原始结构 ──

#[derive(Debug, Deserialize)]
struct RawConfig {
    #[serde(default = "default_log")]
    log: RawLog,
    #[serde(default)]
    providers: HashMap<String, RawProvider>,
    #[serde(default = "default_default_provider")]
    default_provider: String,
    jenkins: RawJenkins,
    gitlab: RawGitlab,
    #[serde(default)]
    dev: RawDev,
    #[serde(default)]
    server: RawServer,
}

#[derive(Debug, Deserialize)]
struct RawLog {
    #[serde(default = "default_log_level")]
    level: String,
}

#[derive(Debug, Deserialize)]
struct RawProvider {
    model_flash: Option<String>,
    model_pro: Option<String>,
    base_url: Option<String>,
    api_key: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
struct RawJenkins {
    url: Option<String>,
    user: Option<String>,
    token: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
struct RawGitlab {
    #[serde(default = "default_gitlab_url")]
    url: String,
    token: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
struct RawDev {
    #[serde(default = "default_claude_path")]
    claude_code_path: String,
}

#[derive(Debug, Deserialize, Default)]
struct RawServer {
    #[serde(default = "default_cors_origins")]
    cors_origins: Vec<String>,
    api_key: Option<String>,
}

// ── Default 函数 ──

fn default_log() -> RawLog {
    RawLog {
        level: "info".to_string(),
    }
}

fn default_log_level() -> String {
    "info".to_string()
}

fn default_default_provider() -> String {
    "openai".to_string()
}

fn default_gitlab_url() -> String {
    "https://gitlab.com".to_string()
}

fn default_claude_path() -> String {
    "claude".to_string()
}

fn default_cors_origins() -> Vec<String> {
    vec!["http://localhost:5173".to_string()]
}

// ── 公开配置结构 ──

#[derive(Debug, Clone)]
pub struct Config {
    pub log_level: String,
    pub llm_providers: Vec<ProviderConfig>,
    pub default_provider: String,
    pub jenkins_url: String,
    pub jenkins_user: String,
    pub jenkins_token: String,
    pub gitlab_url: String,
    pub gitlab_token: String,
    pub claude_code_path: String,
    pub cors_origins: Vec<String>,
    pub api_key: Option<String>,
}

impl Config {
    /// 从 config.toml 加载配置。
    ///
    /// 配置加载顺序：
    /// 1. 加载 `.env` 文件（dotenv）
    /// 2. 读取 config.toml 并展开 `${VAR}` / `$VAR`
    /// 3. 系统环境变量覆盖（通过 DEVOPS_ 前缀）
    ///
    /// 查找路径：config.toml → backend/config.toml → ../config.toml
    pub fn from_file() -> Self {
        // 1. 加载 .env 文件
        dotenv::dotenv().ok();
        let _ = dotenv::from_path("backend/.env");
        let _ = dotenv::from_path("../backend/.env");

        // 2. 读取 TOML → 展开 ${VAR} → 反序列化
        let config_path = Self::find_config();
        let content = std::fs::read_to_string(config_path)
            .unwrap_or_else(|_| panic!("Failed to read config file: {}", config_path));

        let raw_value: toml::Value = toml::from_str(&content)
            .unwrap_or_else(|e| panic!("Failed to parse {}: {}", config_path, e));
        let expanded = expand_toml_value(raw_value);
        let json = toml_to_json(expanded);

        let raw: RawConfig = serde_json::from_value(json)
            .unwrap_or_else(|e| panic!("Failed to deserialize config: {}", e));

        let providers = Self::build_providers(raw.providers);
        let config = Self {
            log_level: raw.log.level,
            llm_providers: providers,
            default_provider: raw.default_provider,
            jenkins_url: raw
                .jenkins
                .url
                .expect("jenkins.url not set in config.toml")
                .trim_end_matches('/')
                .to_string(),
            jenkins_user: raw
                .jenkins
                .user
                .expect("jenkins.user not set in config.toml"),
            jenkins_token: raw
                .jenkins
                .token
                .expect("jenkins.token not set in config.toml"),
            gitlab_url: raw.gitlab.url,
            gitlab_token: raw
                .gitlab
                .token
                .expect("gitlab.token not set in config.toml"),
            claude_code_path: raw.dev.claude_code_path,
            cors_origins: raw.server.cors_origins,
            api_key: raw.server.api_key,
        };

        config.validate_llm()
    }

    /// 查找 config.toml 文件
    fn find_config() -> &'static str {
        let candidates = ["config.toml", "backend/config.toml", "../config.toml"];
        for path in &candidates {
            if Path::new(path).exists() {
                return path;
            }
        }
        "config.toml"
    }

    /// 将 RawProvider 转换为 ProviderConfig
    fn build_providers(raw: HashMap<String, RawProvider>) -> Vec<ProviderConfig> {
        let mut providers = Vec::new();
        let mut entries: Vec<_> = raw.into_iter().collect();
        entries.sort_by_key(|(k, _)| k.clone());
        for (id, p) in entries {
            providers.push(ProviderConfig {
                id,
                api_key: p.api_key,
                base_url: p.base_url,
                model_flash: p.model_flash,
                model_pro: p.model_pro,
            });
        }
        providers
    }

    /// Validate that at least one LLM provider is configured.
    fn validate_llm(self) -> Self {
        let has_provider = self
            .llm_providers
            .iter()
            .any(|p| p.api_key.as_ref().is_some_and(|k| !k.is_empty()));
        if !has_provider {
            panic!("At least one LLM provider must be configured with api_key in config.toml");
        }
        self
    }

    /// Create a default config for testing.
    /// Note: This bypasses LLM validation. Only use for tests that don't need LLM.
    #[doc(hidden)]
    pub fn test_default() -> Self {
        Self {
            log_level: "info".to_string(),
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

// ── TOML ${VAR} 展开 ──
//
// toml crate 不支持 ${VAR} 展开。TOML 解析器会把 "${VAR}" 当作普通字符串。
// 因此在解析后需要递归遍历 value tree，展开字符串中的环境变量引用。

fn expand_toml_value(v: toml::Value) -> toml::Value {
    match v {
        toml::Value::String(s) => toml::Value::String(expand_env_vars(&s)),
        toml::Value::Integer(n) => toml::Value::Integer(n),
        toml::Value::Float(f) => toml::Value::Float(f),
        toml::Value::Boolean(b) => toml::Value::Boolean(b),
        toml::Value::Array(arr) => {
            toml::Value::Array(arr.into_iter().map(expand_toml_value).collect())
        }
        toml::Value::Table(table) => {
            let expanded: toml::map::Map<String, toml::Value> = table
                .into_iter()
                .map(|(k, v)| (k, expand_toml_value(v)))
                .collect();
            toml::Value::Table(expanded)
        }
        toml::Value::Datetime(dt) => toml::Value::Datetime(dt),
    }
}

/// 展开字符串中的 `${VAR}` 和 `$VAR` 环境变量引用。
///
/// 匹配规则：
/// - `${VAR_NAME}` → 环境变量值
/// - `$VAR_NAME` → 环境变量值（VAR_NAME 为 [A-Za-z_][A-Za-z0-9_]*）
fn expand_env_vars(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();

    while let Some(c) = chars.next() {
        if c == '$' {
            let next = chars.peek();
            if next == Some(&'{') {
                // ${VAR} 形式
                chars.next(); // consume '{'
                let mut var_name = String::new();
                let mut found_close = false;
                for ch in &mut chars {
                    if ch == '}' {
                        found_close = true;
                        break;
                    }
                    var_name.push(ch);
                }
                if found_close && is_valid_var_name(&var_name) {
                    result.push_str(&env::var(&var_name).unwrap_or_default());
                } else {
                    // 无法解析，保留原文
                    result.push('$');
                    result.push('{');
                    result.push_str(&var_name);
                    if !found_close {
                        // chars 已被消费，不追加
                    }
                }
            } else if next.is_some_and(|&c| c.is_ascii_alphabetic() || c == '_') {
                // $VAR 形式
                let mut var_name = String::new();
                for ch in &mut chars {
                    if ch.is_ascii_alphanumeric() || ch == '_' {
                        var_name.push(ch);
                    } else {
                        break;
                    }
                }
                if !var_name.is_empty() {
                    result.push_str(&env::var(&var_name).unwrap_or_default());
                } else {
                    result.push('$');
                }
            } else {
                result.push('$');
            }
        } else {
            result.push(c);
        }
    }

    result
}

fn is_valid_var_name(name: &str) -> bool {
    if name.is_empty() {
        return false;
    }
    let mut chars = name.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() || c == '_' => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

// ── toml::Value → serde_json::Value 转换 ──
//
// toml::Value 没有实现 serde::Serialize，无法直接反序列化为自定义结构。
// 通过转换为 serde_json::Value 再利用 serde 的反序列化能力。

fn toml_to_json(v: toml::Value) -> serde_json::Value {
    match v {
        toml::Value::String(s) => serde_json::Value::String(s),
        toml::Value::Integer(n) => serde_json::Value::Number(n.into()),
        toml::Value::Float(f) => serde_json::Number::from_f64(f)
            .map_or(serde_json::Value::Null, serde_json::Value::Number),
        toml::Value::Boolean(b) => serde_json::Value::Bool(b),
        toml::Value::Array(arr) => {
            serde_json::Value::Array(arr.into_iter().map(toml_to_json).collect())
        }
        toml::Value::Table(table) => {
            let obj: serde_json::Map<String, serde_json::Value> = table
                .into_iter()
                .map(|(k, v)| (k, toml_to_json(v)))
                .collect();
            serde_json::Value::Object(obj)
        }
        toml::Value::Datetime(dt) => serde_json::Value::String(dt.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_expand_braced_var() {
        let result = expand_env_vars("url = ${HOME}");
        assert!(result.starts_with("url = /"));
    }

    #[test]
    fn test_expand_plain_var() {
        let result = expand_env_vars("key = $HOME");
        assert!(result.starts_with("key = /"));
    }

    #[test]
    fn test_expand_missing_var() {
        let result = expand_env_vars("key = ${NONEXISTENT_VAR_XYZ}");
        assert_eq!(result, "key = ");
    }

    #[test]
    fn test_expand_multiple_vars() {
        let result = expand_env_vars("${HOME}-${HOME}");
        assert!(result.contains("-"));
    }

    #[test]
    fn test_expand_no_vars() {
        let result = expand_env_vars("normal = value");
        assert_eq!(result, "normal = value");
    }

    #[test]
    fn test_expand_concatenated() {
        let result = expand_env_vars("prefix-${HOME}-suffix");
        assert!(result.starts_with("prefix-"));
        assert!(result.ends_with("-suffix"));
    }

    #[test]
    fn test_expand_toml_string_value() {
        let v = toml::Value::String("${HOME}/test".to_string());
        let expanded = expand_toml_value(v);
        if let toml::Value::String(s) = expanded {
            assert!(s.starts_with("/"));
            assert!(s.ends_with("/test"));
        } else {
            panic!("Expected string value");
        }
    }

    #[test]
    fn test_expand_toml_table() {
        let mut table = toml::map::Map::new();
        table.insert(
            "key".to_string(),
            toml::Value::String("${HOME}".to_string()),
        );
        let v = toml::Value::Table(table);
        let expanded = expand_toml_value(v);
        if let toml::Value::Table(t) = expanded {
            if let toml::Value::String(s) = &t["key"] {
                assert!(s.starts_with("/"));
            } else {
                panic!("Expected string");
            }
        } else {
            panic!("Expected table");
        }
    }

    #[test]
    fn test_valid_var_name() {
        assert!(is_valid_var_name("HOME"));
        assert!(is_valid_var_name("_private"));
        assert!(is_valid_var_name("VAR_1"));
        assert!(!is_valid_var_name("1bad"));
        assert!(!is_valid_var_name(""));
    }

    #[test]
    fn test_toml_to_json_string() {
        let v = toml::Value::String("hello".to_string());
        let j = toml_to_json(v);
        assert_eq!(j, serde_json::json!("hello"));
    }

    #[test]
    fn test_toml_to_json_array() {
        let arr = toml::Value::Array(vec![
            toml::Value::Integer(1),
            toml::Value::String("a".to_string()),
        ]);
        let j = toml_to_json(arr);
        assert_eq!(j, serde_json::json!([1, "a"]));
    }
}
