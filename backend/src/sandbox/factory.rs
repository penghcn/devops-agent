use anyhow::Result;
use std::sync::Arc;

use super::microsandbox_backend::{MicrosandboxBackend, MicrosandboxConfig};
use super::process_backend::ProcessBackend;
use super::trait_sandbox::Sandbox;

/// 沙箱后端类型
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SandboxBackend {
    Microsandbox,
    Process,
}

impl SandboxBackend {
    /// 从环境变量自动检测后端类型
    pub fn from_env() -> Self {
        match std::env::var("SANDBOX_BACKEND").ok().as_deref() {
            Some("process") => Self::Process,
            Some("microsandbox") => Self::Microsandbox,
            _ => {
                if cfg!(target_os = "macos") || cfg!(target_os = "linux") {
                    Self::Microsandbox
                } else {
                    Self::Process
                }
            }
        }
    }
}

/// 沙箱工厂 — 根据配置创建对应的后端
pub struct SandboxFactory {
    backend: SandboxBackend,
    micro_config: MicrosandboxConfig,
}

impl SandboxFactory {
    pub fn new() -> Self {
        Self {
            backend: SandboxBackend::from_env(),
            micro_config: MicrosandboxConfig::default(),
        }
    }

    pub fn with_backend(mut self, backend: SandboxBackend) -> Self {
        self.backend = backend;
        self
    }

    pub fn with_micro_config(mut self, config: MicrosandboxConfig) -> Self {
        self.micro_config = config;
        self
    }

    /// 创建沙箱实例
    pub fn create(&self) -> Result<Arc<dyn Sandbox>> {
        match self.backend {
            SandboxBackend::Microsandbox => {
                tracing::info!("使用 microsandbox 后端");
                Ok(Arc::new(MicrosandboxBackend::new(
                    self.micro_config.clone(),
                )))
            }
            SandboxBackend::Process => {
                tracing::info!("使用 process 后端（降级）");
                Ok(Arc::new(ProcessBackend::new()))
            }
        }
    }
}
