use anyhow::Result;
use std::sync::Arc;
use tokio::sync::RwLock;

use super::client::E2BClient;
use super::config::CubeSandboxConfig;
use crate::sandbox::trait_sandbox::{ExecResult, Sandbox};

/// CubeSandbox 后端实现
///
/// 支持懒初始化（首次 exec/read/write 时创建沙箱）、
/// 持久化（同一实例复用沙箱）、以及 stop 时清理。
pub struct CubeSandboxBackend {
    client: E2BClient,
    config: CubeSandboxConfig,
    /// 懒初始化: 首次使用时创建沙箱
    sandbox_id: Arc<RwLock<Option<String>>>,
}

impl CubeSandboxBackend {
    pub fn new(config: CubeSandboxConfig) -> Self {
        let api_url = config.api_url.clone();
        let api_key = config.api_key.clone();
        Self {
            client: E2BClient::new(api_url, api_key),
            config,
            sandbox_id: Arc::new(RwLock::new(None)),
        }
    }

    /// 确保沙箱正在运行，返回 sandbox_id
    async fn ensure_running(&self) -> Result<String> {
        let id = {
            let mut guard = self.sandbox_id.write().await;
            if let Some(ref id) = *guard {
                id.clone()
            } else {
                let resp = self
                    .client
                    .create_sandbox(&self.config.template_id, self.config.timeout_secs)
                    .await?;
                let id = resp.sandbox_id;
                *guard = Some(id.clone());
                id
            }
        };
        Ok(id)
    }
}

#[async_trait::async_trait]
impl Sandbox for CubeSandboxBackend {
    async fn exec(&self, command: &str, args: &[String]) -> Result<ExecResult> {
        let id = self.ensure_running().await?;
        let result = self.client.exec_command(&id, command, args).await?;
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
        // 使用 yunli::hash::b64 编码避免 shell 转义问题
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
            let guard = self.sandbox_id.read().await;
            guard.clone()
        };
        if let Some(id) = id {
            let _ = self.client.kill_sandbox(&id).await;
            *self.sandbox_id.write().await = None;
        }
        Ok(())
    }
}
