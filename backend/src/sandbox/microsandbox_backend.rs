use anyhow::Result;
use std::sync::Arc;
use tokio::sync::RwLock;

use super::trait_sandbox::{ExecResult, Sandbox};

/// Microsandbox 后端配置
#[derive(Clone)]
pub struct MicrosandboxConfig {
    /// OCI 镜像引用（如 "debian", "alpine:3.19"）
    pub image: String,
    /// CPU 核心数
    pub cpus: u8,
    /// 内存大小（MiB）
    pub memory_mib: u32,
}

impl Default for MicrosandboxConfig {
    fn default() -> Self {
        Self {
            image: "debian".to_string(),
            cpus: 1,
            memory_mib: 512,
        }
    }
}

/// 使用 microsandbox SDK 的 Sandbox 实现
pub struct MicrosandboxBackend {
    /// 懒初始化：首次 exec 时创建沙箱
    sandbox: Arc<RwLock<Option<microsandbox::Sandbox>>>,
    config: MicrosandboxConfig,
}

impl MicrosandboxBackend {
    pub fn new(config: MicrosandboxConfig) -> Self {
        Self {
            sandbox: Arc::new(RwLock::new(None)),
            config,
        }
    }

    /// 懒初始化沙箱 — 首次调用时创建，后续复用
    async fn ensure_running(
        &self,
    ) -> Result<Arc<microsandbox::Sandbox>, anyhow::Error> {
        let sb = {
            let mut guard = self.sandbox.write().await;
            if guard.is_some() {
                let sb = guard.as_ref().unwrap().clone();
                Arc::new(sb)
            } else {
                let name = format!("devops-{}", std::process::id());
                let sb = microsandbox::Sandbox::builder(&name)
                    .image(&self.config.image)
                    .cpus(self.config.cpus)
                    .memory(self.config.memory_mib)
                    .create()
                    .await
                    .map_err(|e| anyhow::anyhow!("microsandbox 创建失败: {}", e))?;
                *guard = Some(sb.clone());
                drop(guard);
                Arc::new(sb)
            }
        };
        Ok(sb)
    }
}

#[async_trait::async_trait]
impl Sandbox for MicrosandboxBackend {
    async fn exec(&self, command: &str, args: &[String]) -> Result<ExecResult> {
        let sb = self.ensure_running().await?;

        let output = sb
            .exec(command, args)
            .await
            .map_err(|e| anyhow::anyhow!("microsandbox exec 失败: {}", e))?;

        let status = output.status();
        let stdout = output.stdout().unwrap_or_default();
        let stderr = output.stderr().unwrap_or_default();

        Ok(ExecResult {
            stdout,
            stderr,
            exit_code: status.code,
        })
    }

    async fn read_file(&self, path: &str) -> Result<String> {
        let sb = self.ensure_running().await?;

        sb.fs()
            .read_to_string(path)
            .await
            .map_err(|e| anyhow::anyhow!("microsandbox read {} 失败: {}", path, e))
    }

    async fn write_file(&self, path: &str, content: &str) -> Result<()> {
        let sb = self.ensure_running().await?;

        sb.fs()
            .write(path, content.as_bytes())
            .await
            .map_err(|e| anyhow::anyhow!("microsandbox write {} 失败: {}", path, e))?;

        Ok(())
    }

    async fn stop(&self) -> Result<()> {
        let sb = {
            let guard = self.sandbox.read().await;
            guard.as_ref().map(|s| s.clone())
        };

        if let Some(sb) = sb {
            sb.stop_and_wait()
                .await
                .map_err(|e| anyhow::anyhow!("microsandbox 停止失败: {}", e))?;
            // 清除引用
            *self.sandbox.write().await = None;
        }

        Ok(())
    }
}
