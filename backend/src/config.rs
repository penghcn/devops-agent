//! 应用配置加载。
//!
//! 基于 `yunli::config::load()` 加载 flat key-value 配置。
//! 优先级: TOML 显式值 > 系统环境变量 > .env 文件

use std::collections::{BTreeMap, BTreeSet};

use crate::llm::ProviderConfig;

// ── 公开配置结构 ──

/// 沙箱相关配置
#[derive(Debug, Clone)]
pub struct SandboxConfig {
    pub backends: Vec<crate::sandbox::SandboxBackend>,
    pub timeout_secs: u64,
    pub image: String,
    pub cpus: u32,
    pub memory: u64,
    /// CubeSandbox 控制平面 API 地址
    pub cubesandbox_api_url: String,
    pub cubesandbox_api_key: String,
    pub cubesandbox_template_id: String,
    pub cubesandbox_timeout: i32,
    pub cubesandbox_allow_internet: bool,
    pub cubesandbox_envd_port: u16,
    pub cubesandbox_envd_url_template: String,
}

impl Default for SandboxConfig {
    fn default() -> Self {
        Self {
            backends: vec![crate::sandbox::SandboxBackend::Microsandbox],
            timeout_secs: 30,
            image: "debian".to_string(),
            cpus: 1,
            memory: 512,
            cubesandbox_api_url: String::new(),
            cubesandbox_api_key: "dummy".to_string(),
            cubesandbox_template_id: String::new(),
            cubesandbox_timeout: 1800,
            cubesandbox_allow_internet: true,
            cubesandbox_envd_port: 49983,
            cubesandbox_envd_url_template: String::new(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Config {
    pub log_level: String,
    pub llm_providers: Vec<ProviderConfig>,
    pub default_provider: String,
    pub port: u16,
    pub backend_port: u16,
    pub jenkins_url: String,
    pub jenkins_user: String,
    pub jenkins_token: String,
    pub gitlab_url: String,
    pub gitlab_token: String,
    pub claude_code_path: String,
    pub cors_origins: Vec<String>,
    pub api_key: Option<String>,
    pub sandbox: SandboxConfig,
}

/// 过滤无效值：空字符串和占位符视为 None。
fn effective_value(v: &str) -> Option<String> {
    if v.is_empty() || yunli::config::is_placeholder(v) {
        None
    } else {
        Some(v.to_string())
    }
}

/// 从 flat map 中提取某个 key 的值（如果有效）。
fn conf_get(conf: &BTreeMap<String, String>, key: &str) -> Option<String> {
    conf.get(key).and_then(|v| effective_value(v))
}

/// 从 flat map 中提取 CORS origins 数组。
/// yunli 平铺后: server.cors.origins.0, server.cors.origins.1, ...
fn parse_cors_origins(conf: &BTreeMap<String, String>, port: u16) -> Vec<String> {
    let mut origins = Vec::new();
    let mut i = 0;
    loop {
        match conf.get(&format!("server.cors.origins.{i}")) {
            Some(v) if !v.is_empty() => origins.push(v.clone()),
            _ => break,
        }
        i += 1;
    }
    if origins.is_empty() {
        vec![format!("http://localhost:{port}")]
    } else {
        origins
    }
}

/// 从 flat map 中提取 sandbox backend 数组。
/// yunli 平铺后: sandbox.backend.0, sandbox.backend.1, ...
/// 也兼容单个 sandbox.backend 字符串。
fn parse_sandbox_backends(conf: &BTreeMap<String, String>) -> Vec<crate::sandbox::SandboxBackend> {
    use crate::sandbox::SandboxBackend;
    let mut backends = Vec::new();
    let mut i = 0;
    loop {
        match conf.get(&format!("sandbox.backend.{}", i)) {
            Some(v) => {
                let b = match v.as_str() {
                    "cubesandbox" => SandboxBackend::CubeSandbox,
                    "process" => SandboxBackend::Process,
                    _ => SandboxBackend::Microsandbox,
                };
                backends.push(b);
            }
            None => break,
        }
        i += 1;
    }
    if backends.is_empty() {
        if let Some(v) = conf.get("sandbox.backend") {
            let b = match v.as_str() {
                "cubesandbox" => SandboxBackend::CubeSandbox,
                "process" => SandboxBackend::Process,
                _ => SandboxBackend::Microsandbox,
            };
            backends.push(b);
        }
    }
    if backends.is_empty() {
        backends.push(SandboxBackend::Microsandbox);
    }
    backends
}

/// 从 flat map 中提取所有 ProviderConfig。
/// yunli 平铺后: providers.{id}.api.key, providers.{id}.base.url, 等
fn extract_providers(conf: &BTreeMap<String, String>) -> Vec<ProviderConfig> {
    // 收集所有 provider id
    let mut ids = BTreeSet::new();
    for key in conf.keys() {
        if let Some(rest) = key.strip_prefix("providers.") {
            if let Some(id) = rest.split('.').next() {
                ids.insert(id.to_string());
            }
        }
    }

    let mut providers = Vec::new();
    for id in ids {
        let prefix = format!("providers.{}.", id);
        providers.push(ProviderConfig {
            id,
            api_key: conf_get(conf, &format!("{prefix}api.key")),
            base_url: conf_get(conf, &format!("{prefix}base.url")),
            model_flash: conf_get(conf, &format!("{prefix}model.flash")),
            model_pro: conf_get(conf, &format!("{prefix}model.pro")),
        });
    }
    providers
}

impl Config {
    /// 从配置源加载配置。
    ///
    /// 委托给 `yunli::config::load_with()`，自动处理：
    /// - 逐级加载 .env 文件
    /// - 查找项目根目录的 config.toml
    /// - 展开 ${VAR} / $VAR 环境变量引用
    /// - 优先级: TOML 显式值 > 系统环境变量 > .env 文件
    pub fn from_file() -> Self {
        let conf = yunli::config::load().unwrap_or_else(|e| panic!("Failed to load config: {}", e));

        // 基础字段
        let log_level = conf
            .get("log.level")
            .map(|s| s.as_str())
            .unwrap_or("info")
            .to_string();
        let default_provider = conf
            .get("default.provider")
            .map(|s| s.as_str())
            .unwrap_or("openai")
            .to_string();
        let port = conf
            .get("server.port")
            .and_then(|s| s.parse().ok())
            .unwrap_or(3000);
        let backend_port = conf
            .get("server.backend.port")
            .and_then(|s| s.parse().ok())
            .unwrap_or(8080);

        // Jenkins
        let jenkins_url = conf
            .get("jenkins.url")
            .map(|s| s.trim_end_matches('/').to_string())
            .unwrap_or_default();
        let jenkins_user = conf.get("jenkins.user").cloned().unwrap_or_default();
        let jenkins_token = conf.get("jenkins.token").cloned().unwrap_or_default();

        // GitLab
        let gitlab_url = conf
            .get("gitlab.url")
            .map(|s| s.as_str())
            .unwrap_or("https://gitlab.com")
            .to_string();
        let gitlab_token = conf.get("gitlab.token").cloned().unwrap_or_default();

        // Dev
        let claude_code_path = conf
            .get("dev.claude.code.path")
            .map(|s| s.as_str())
            .unwrap_or("claude")
            .to_string();

        // Server
        let api_key = conf_get(&conf, "server.api.key");
        let cors_origins = parse_cors_origins(&conf, port);

        // Providers
        let llm_providers = extract_providers(&conf);

        // Sandbox
        let sandbox_backends = parse_sandbox_backends(&conf);
        let sandbox_timeout_secs = conf
            .get("sandbox.timeout_secs")
            .and_then(|s| s.parse().ok())
            .unwrap_or(30);
        let sandbox_image = conf
            .get("sandbox.microsandbox.image")
            .cloned()
            .unwrap_or_else(|| "debian".to_string());
        let sandbox_cpus = conf
            .get("sandbox.microsandbox.cpus")
            .and_then(|s| s.parse().ok())
            .unwrap_or(1);
        let sandbox_memory = conf
            .get("sandbox.microsandbox.memory")
            .and_then(|s| s.parse().ok())
            .unwrap_or(512);

        // CubeSandbox
        let cubesandbox_api_url =
            conf_get(&conf, "sandbox.cubesandbox.api_url").unwrap_or_default();
        let cubesandbox_api_key =
            conf_get(&conf, "sandbox.cubesandbox.api_key").unwrap_or_else(|| "dummy".to_string());
        let cubesandbox_template_id =
            conf_get(&conf, "sandbox.cubesandbox.template_id").unwrap_or_default();
        let cubesandbox_timeout = conf
            .get("sandbox.cubesandbox.timeout")
            .and_then(|s| s.parse().ok())
            .unwrap_or(1800i32);
        let cubesandbox_allow_internet = conf
            .get("sandbox.cubesandbox.allow_internet")
            .and_then(|s| s.parse().ok())
            .unwrap_or(true);
        let cubesandbox_envd_port = conf
            .get("sandbox.cubesandbox.envd_port")
            .and_then(|s| s.parse().ok())
            .unwrap_or(49983u16);
        let cubesandbox_envd_url_template =
            conf_get(&conf, "sandbox.cubesandbox.envd_url_template").unwrap_or_default();

        let sandbox = SandboxConfig {
            backends: sandbox_backends,
            timeout_secs: sandbox_timeout_secs,
            image: sandbox_image,
            cpus: sandbox_cpus,
            memory: sandbox_memory,
            cubesandbox_api_url,
            cubesandbox_api_key,
            cubesandbox_template_id,
            cubesandbox_timeout,
            cubesandbox_allow_internet,
            cubesandbox_envd_port,
            cubesandbox_envd_url_template,
        };

        let config = Self {
            log_level,
            llm_providers,
            default_provider,
            backend_port,
            port,
            jenkins_url,
            jenkins_user,
            jenkins_token,
            gitlab_url,
            gitlab_token,
            claude_code_path,
            cors_origins,
            api_key,
            sandbox,
        };

        config.validate_llm()
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
            port: 3000,
            backend_port: 8080,
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
            cors_origins: vec!["http://localhost:3000".to_string()],
            api_key: None,
            sandbox: SandboxConfig {
                backends: vec![crate::sandbox::SandboxBackend::Process],
                timeout_secs: 30,
                image: "debian".to_string(),
                cpus: 1,
                memory: 512,
                cubesandbox_api_url: String::new(),
                cubesandbox_api_key: "dummy".to_string(),
                cubesandbox_template_id: String::new(),
                cubesandbox_timeout: 1800,
                cubesandbox_allow_internet: true,
                cubesandbox_envd_port: 49983,
                cubesandbox_envd_url_template: String::new(),
            },
        }
    }
}
