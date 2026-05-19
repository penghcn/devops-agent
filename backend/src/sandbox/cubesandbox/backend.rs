use anyhow::Result;
use std::sync::Arc;
use tokio::sync::RwLock;

use super::client::{ControlPlaneClient, EnvdClient};
use super::config::CubeSandboxConfig;
use crate::sandbox::trait_sandbox::{ExecResult, Sandbox};

/// CubeSandbox 后端实现
///
/// 支持懒初始化（首次 exec/read/write 时创建沙箱）、
/// 持久化（同一实例复用沙箱）、以及 stop 时清理。
pub struct CubeSandboxBackend {
    control: ControlPlaneClient,
    config: CubeSandboxConfig,
    /// 懒初始化: 首次使用时创建沙箱
    state: Arc<RwLock<BackendState>>,
}

#[derive(Debug, Default)]
struct BackendState {
    sandbox_id: Option<String>,
    envd: Option<std::sync::Arc<EnvdClient>>,
}

impl CubeSandboxBackend {
    pub fn new(config: CubeSandboxConfig) -> Self {
        let api_url = config.api_url.clone();
        let api_key = config.api_key.clone();
        Self {
            control: ControlPlaneClient::new(api_url, api_key),
            config,
            state: Arc::new(RwLock::new(BackendState::default())),
        }
    }

    /// 确保沙箱正在运行，返回 envd 客户端引用
    async fn ensure_running(&self) -> Result<String> {
        let mut guard = self.state.write().await;

        if let Some(ref _envd) = guard.envd {
            // 已就绪，返回命令执行用的 envd
            let id = guard.sandbox_id.clone().unwrap_or_default();
            drop(guard);
            return Ok(id);
        }

        // 创建沙箱
        let resp = self
            .control
            .create_sandbox(&self.config.template_id, self.config.timeout_secs)
            .await?;

        let sandbox_id = resp.sandbox_id.clone();

        // 获取 envd 访问令牌
        let info = self.control.get_sandbox(&sandbox_id).await?;
        let access_token = info
            .envd_access_token
            .unwrap_or_else(|| "dummy".to_string());

        // 构建 envd 地址
        // 自托管时通过 CubeProxy 路由: {port}-{sandboxID}.{api_host}
        let envd_url = Self::build_envd_url(&self.config, &sandbox_id);

        let envd = std::sync::Arc::new(EnvdClient::new(envd_url, access_token));

        guard.sandbox_id = Some(sandbox_id.clone());
        guard.envd = Some(envd);

        drop(guard);
        Ok(sandbox_id)
    }

    /// 构建 envd 端点地址
    ///
    /// E2B 官方: https://{port}-{sandboxID}.e2b.dev
    /// 自托管 CubeSandbox: http://{port}-{sandboxID}.{domain}
    /// 默认端口 49983
    fn build_envd_url(config: &CubeSandboxConfig, sandbox_id: &str) -> String {
        if !config.envd_url_template.is_empty() {
            return config.envd_url_template.replace("{sandbox_id}", sandbox_id);
        }
        let host = config
            .api_url
            .strip_prefix("http://")
            .or_else(|| config.api_url.strip_prefix("https://"))
            .unwrap_or(&config.api_url);
        format!("http://49983-{}.{}", sandbox_id, host)
    }

    /// 获取当前 envd 客户端
    async fn envd_client(&self) -> Result<std::sync::Arc<EnvdClient>> {
        let guard = self.state.read().await;
        let envd = guard
            .envd
            .clone()
            .ok_or_else(|| anyhow::anyhow!("沙箱未初始化，请先调用 exec"));
        drop(guard);
        envd
    }
}

#[async_trait::async_trait]
impl Sandbox for CubeSandboxBackend {
    async fn exec(&self, command: &str, args: &[String]) -> Result<ExecResult> {
        self.ensure_running().await?;
        let envd = self.envd_client().await?;
        let result = envd.exec_command(command, args).await?;
        Ok(ExecResult {
            stdout: result.stdout,
            stderr: result.stderr,
            exit_code: result.exit_code,
        })
    }

    async fn read_file(&self, path: &str) -> Result<String> {
        let result = self.exec("cat", &[path.to_string()]).await?;
        if result.exit_code == 0 {
            Ok(result.stdout)
        } else {
            Err(anyhow::anyhow!("cat {} failed: {}", path, result.stderr))
        }
    }

    async fn write_file(&self, path: &str, content: &str) -> Result<()> {
        let encoded = yunli::hash::b64(content.as_bytes());
        let cmd = format!("echo '{}' | base64 -d > '{}'", encoded, path);
        let result = self.exec("sh", &["-c".into(), cmd]).await?;
        if result.exit_code == 0 {
            Ok(())
        } else {
            Err(anyhow::anyhow!("write {} failed: {}", path, result.stderr))
        }
    }

    async fn stop(&self) -> Result<()> {
        let id = {
            let guard = self.state.read().await;
            guard.sandbox_id.clone()
        };
        if let Some(id) = id {
            let _ = self.control.kill_sandbox(&id).await;
            let mut guard = self.state.write().await;
            guard.sandbox_id = None;
            guard.envd = None;
        }
        Ok(())
    }
}
