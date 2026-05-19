use chrono::Local;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

use super::{Tool, ToolInput, ToolOutput};

/// 工具结果缓存条目
#[derive(Debug)]
struct CacheEntry {
    value: String,
    expire_at: std::time::Instant,
}

impl CacheEntry {
    fn is_valid(&self) -> bool {
        std::time::Instant::now() < self.expire_at
    }
}

/// 会话级工具结果缓存（带 TTL）
#[derive(Debug, Clone, Default)]
pub struct ToolCache {
    inner: Arc<RwLock<HashMap<String, CacheEntry>>>,
    /// 默认 TTL 秒数（默认 10 分钟）
    default_ttl_secs: u64,
}

impl ToolCache {
    pub fn new(default_ttl_secs: u64) -> Self {
        Self {
            inner: Arc::new(RwLock::new(HashMap::new())),
            default_ttl_secs,
        }
    }

    /// 获取缓存结果。命中且未过期返回 Some，否则 None。
    pub async fn get(&self, key: &str) -> Option<String> {
        let guard = self.inner.read().await;
        guard.get(key).and_then(|e| {
            if e.is_valid() {
                Some(e.value.clone())
            } else {
                None
            }
        })
    }

    /// 写入缓存，指定 TTL。
    pub async fn put(&self, key: String, value: String, ttl_secs: u64) {
        let mut guard = self.inner.write().await;
        guard.insert(
            key,
            CacheEntry {
                value,
                expire_at: std::time::Instant::now() + std::time::Duration::from_secs(ttl_secs),
            },
        );
    }

    /// 写入缓存，使用默认 TTL。
    pub async fn put_default(&self, key: String, value: String) {
        self.put(key, value, self.default_ttl_secs).await;
    }

    /// 清空缓存。
    pub async fn clear(&self) {
        self.inner.write().await.clear();
    }
}

/// 环境变量白名单
const ENV_WHITELIST: &[&str] = &[
    "PATH", "HOME", "USER", "LANG", "LC_ALL", "TZ", "TERM", "SHELL", "EDITOR", "VISUAL", "PWD",
    "HOSTNAME", "USER", "LOGNAME", "EDITOR",
];

/// 配置路径白名单前缀
const CONFIG_WHITELIST_PREFIXES: &[&str] = &[
    "server.",
    "sandbox.timeout_secs",
    "sandbox.backend",
    "dev.",
    "log.",
];

/// 获取当前时间的工具
pub struct GetTimeTool {
    cache: ToolCache,
}

impl GetTimeTool {
    pub fn new() -> Self {
        Self {
            cache: ToolCache::new(60), // get_time 特殊 TTL: 1 分钟
        }
    }
}

#[async_trait::async_trait]
impl Tool for GetTimeTool {
    fn name(&self) -> &str {
        "get_time"
    }

    async fn execute(&self, _input: &ToolInput) -> ToolOutput {
        let cache_key = "get_time:now".to_string();

        if let Some(cached) = self.cache.get(&cache_key).await {
            return ToolOutput::success(cached);
        }

        let now = Local::now();
        let result = format!(
            "{{\"iso8601\":\"{}\",\"unix_timestamp\":{},\"timezone\":\"{}\"}}",
            now.format("%Y-%m-%dT%H:%M:%S%.3f%z"),
            now.timestamp(),
            now.format("%Z")
        );

        self.cache.put(cache_key, result.clone(), 60).await;
        ToolOutput::success(result)
    }
}

/// 读取环境变量的工具
pub struct GetEnvTool {
    cache: ToolCache,
}

impl GetEnvTool {
    pub fn new() -> Self {
        Self {
            cache: ToolCache::new(600), // 默认 10 分钟
        }
    }
}

#[async_trait::async_trait]
impl Tool for GetEnvTool {
    fn name(&self) -> &str {
        "get_env"
    }

    async fn execute(&self, input: &ToolInput) -> ToolOutput {
        let key = input.arguments.first().map(|s| s.as_str()).unwrap_or("");

        if key.is_empty() {
            return ToolOutput::fail("缺少环境变量名称".into());
        }

        // 白名单检查
        if !ENV_WHITELIST.contains(&key) {
            return ToolOutput::fail(format!(
                "环境变量 '{}' 不在白名单中。使用 get_env 查询可用列表",
                key
            ));
        }

        let cache_key = format!("get_env:{}", key);

        if let Some(cached) = self.cache.get(&cache_key).await {
            return ToolOutput::success(cached);
        }

        let result = std::env::var(key).unwrap_or_default();
        self.cache.put_default(cache_key, result.clone()).await;
        ToolOutput::success(result)
    }
}

/// 读取项目配置的工具
pub struct GetConfigTool {
    cache: ToolCache,
    /// 配置数据（从 Config 传入）
    config_map: Arc<RwLock<HashMap<String, String>>>,
}

impl GetConfigTool {
    pub fn new(config: &crate::config::Config) -> Self {
        let mut map = HashMap::new();
        // 暴露安全的配置项（不含敏感数据）
        map.insert("server.port".into(), config.port.to_string());
        map.insert(
            "server.backend_port".into(),
            config.backend_port.to_string(),
        );
        map.insert("log.level".into(), config.log_level.clone());
        map.insert(
            "dev.claude_code_path".into(),
            config.claude_code_path.clone(),
        );
        map.insert(
            "sandbox.timeout_secs".into(),
            config.sandbox.timeout_secs.to_string(),
        );
        map.insert("sandbox.image".into(), config.sandbox.image.clone());

        Self {
            cache: ToolCache::new(600),
            config_map: Arc::new(RwLock::new(map)),
        }
    }
}

#[async_trait::async_trait]
impl Tool for GetConfigTool {
    fn name(&self) -> &str {
        "get_config"
    }

    async fn execute(&self, input: &ToolInput) -> ToolOutput {
        let path = input.arguments.first().map(|s| s.as_str()).unwrap_or("");

        if path.is_empty() {
            let keys: Vec<String> = self.config_map.read().await.keys().cloned().collect();
            return ToolOutput::success(format!("可用配置项:\n{}", keys.join("\n")));
        }

        // 白名单检查
        if !CONFIG_WHITELIST_PREFIXES
            .iter()
            .any(|&prefix| path.starts_with(prefix))
        {
            return ToolOutput::fail(format!("配置路径 '{}' 不在白名单中", path));
        }

        let cache_key = format!("get_config:{}", path);

        if let Some(cached) = self.cache.get(&cache_key).await {
            return ToolOutput::success(cached);
        }

        let guard = self.config_map.read().await;
        let result = match guard.get(path) {
            Some(v) => v.clone(),
            None => format!("配置项 '{}' 不存在", path),
        };

        drop(guard);
        self.cache.put_default(cache_key, result.clone()).await;
        ToolOutput::success(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_tool_cache_hit() {
        let cache = ToolCache::new(600);
        cache.put("key1".into(), "value1".into(), 600).await;

        assert_eq!(cache.get("key1").await, Some("value1".into()));
        assert_eq!(cache.get("key2").await, None);
    }

    #[tokio::test]
    async fn test_tool_cache_expiry() {
        let cache = ToolCache::new(600);
        cache.put("key1".into(), "value1".into(), 0).await; // 0 秒过期

        // 短暂等待后过期
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        assert_eq!(cache.get("key1").await, None);
    }

    #[tokio::test]
    async fn test_get_time_tool() {
        let tool = GetTimeTool::new();
        let output = tool
            .execute(&ToolInput {
                path: None,
                content: None,
                arguments: vec![],
                user_role: crate::security::roles::Role::Admin,
            })
            .await;

        assert!(output.success);
        assert!(output.result.contains("iso8601"));
        assert!(output.result.contains("unix_timestamp"));
    }

    #[tokio::test]
    async fn test_get_env_whitelist() {
        let tool = GetEnvTool::new();

        // 白名单内的 key
        let output = tool
            .execute(&ToolInput {
                path: None,
                content: None,
                arguments: vec!["PATH".into()],
                user_role: crate::security::roles::Role::Admin,
            })
            .await;
        assert!(output.success);

        // 不在白名单的 key
        let output = tool
            .execute(&ToolInput {
                path: None,
                content: None,
                arguments: vec!["SECRET_KEY".into()],
                user_role: crate::security::roles::Role::Admin,
            })
            .await;
        assert!(!output.success);
    }

    #[tokio::test]
    async fn test_get_config_tool() {
        let config = crate::config::Config::test_default();
        let tool = GetConfigTool::new(&config);

        let output = tool
            .execute(&ToolInput {
                path: None,
                content: None,
                arguments: vec!["server.port".into()],
                user_role: crate::security::roles::Role::Admin,
            })
            .await;
        assert!(output.success);
        assert_eq!(output.result, "3000");
    }

    #[tokio::test]
    async fn test_get_config_list_keys() {
        let config = crate::config::Config::test_default();
        let tool = GetConfigTool::new(&config);

        let output = tool
            .execute(&ToolInput {
                path: None,
                content: None,
                arguments: vec![],
                user_role: crate::security::roles::Role::Admin,
            })
            .await;
        assert!(output.success);
        assert!(output.result.contains("server.port"));
    }

    #[tokio::test]
    async fn test_get_config_whitelist() {
        let config = crate::config::Config::test_default();
        let tool = GetConfigTool::new(&config);

        // 不在白名单前缀中
        let output = tool
            .execute(&ToolInput {
                path: None,
                content: None,
                arguments: vec!["jenkins.token".into()],
                user_role: crate::security::roles::Role::Admin,
            })
            .await;
        assert!(!output.success);
    }
}
