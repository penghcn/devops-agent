use anyhow::Result;
use std::sync::{Arc, Mutex};

#[cfg(target_os = "linux")]
use super::microsandbox_backend::{MicrosandboxBackend, MicrosandboxConfig};
#[cfg(not(target_os = "linux"))]
#[derive(Clone, Default)]
pub struct MicrosandboxConfig;
use super::cubesandbox::{CubeSandboxBackend, CubeSandboxConfig};
use super::process_backend::ProcessBackend;
use super::trait_sandbox::Sandbox;

/// 沙箱后端类型
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SandboxBackend {
    Microsandbox,
    CubeSandbox,
    Process,
}

impl SandboxBackend {
    /// 从环境变量自动检测后端类型
    pub fn from_env() -> Self {
        match std::env::var("SANDBOX_BACKEND").ok().as_deref() {
            Some("process") => Self::Process,
            Some("microsandbox") => Self::Microsandbox,
            Some("cubesandbox") => Self::CubeSandbox,
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
    backends: Vec<SandboxBackend>,
    selected: Mutex<Option<SandboxBackend>>,
    #[cfg(target_os = "linux")]
    micro_config: MicrosandboxConfig,
    cube_config: CubeSandboxConfig,
}

impl SandboxFactory {
    /// 从配置构建工厂
    pub fn from_config(
        backends: Vec<SandboxBackend>,
        #[cfg(target_os = "linux")] micro_config: MicrosandboxConfig,
        cube_config: CubeSandboxConfig,
    ) -> Self {
        Self {
            backends,
            selected: Mutex::new(None),
            #[cfg(target_os = "linux")]
            micro_config,
            cube_config,
        }
    }

    pub fn new() -> Self {
        Self {
            backends: vec![SandboxBackend::from_env()],
            selected: Mutex::new(None),
            #[cfg(target_os = "linux")]
            micro_config: MicrosandboxConfig::default(),
            cube_config: CubeSandboxConfig::default(),
        }
    }

    pub fn with_backend(mut self, backend: SandboxBackend) -> Self {
        self.backends = vec![backend];
        self.selected = Mutex::new(None);
        self
    }

    #[cfg(target_os = "linux")]
    pub fn with_micro_config(mut self, config: MicrosandboxConfig) -> Self {
        self.micro_config = config;
        self
    }

    /// 异步初始化：按优先级检测后端可用性，缓存选择结果
    /// 应在应用启动时调用一次
    pub async fn init(&self) {
        self._select_backend().await
    }

    /// 重置选择结果并重新检测。
    /// 当网络拓扑变化（如 CubeSandbox 延迟启动）时调用。
    pub async fn retry_init(&self) {
        *self.selected.lock().unwrap() = None;
        tracing::info!("重新检测沙箱后端");
        self._select_backend().await
    }

    async fn _select_backend(&self) {
        for backend in &self.backends {
            if Self::check(
                backend,
                #[cfg(target_os = "linux")]
                &self.micro_config,
                &self.cube_config,
            )
            .await
            {
                tracing::info!("选中沙箱后端: {:?}", backend);
                *self.selected.lock().unwrap() = Some(*backend);
                return;
            }
        }
        tracing::warn!("配置的后端都不可用，降级为 process");
        *self.selected.lock().unwrap() = Some(SandboxBackend::Process);
    }

    /// 轻量可用性检测
    async fn check(
        backend: &SandboxBackend,
        #[cfg(target_os = "linux")] _micro_config: &MicrosandboxConfig,
        cube_config: &CubeSandboxConfig,
    ) -> bool {
        match backend {
            SandboxBackend::Microsandbox => {
                cfg!(target_os = "linux") && std::path::Path::new("/dev/kvm").exists()
            }
            SandboxBackend::CubeSandbox => {
                if !cube_config.is_complete() {
                    tracing::warn!("CubeSandbox 配置不完整，跳过");
                    return false;
                }
                let healthy =
                    super::cubesandbox::client::E2BClient::health_check(&cube_config.api_url).await;
                if !healthy {
                    tracing::debug!("CubeSandbox API {} 健康检查失败", cube_config.api_url);
                }
                healthy
            }
            SandboxBackend::Process => true,
        }
    }

    /// 创建沙箱实例
    pub fn create(&self) -> Result<Arc<dyn Sandbox>> {
        let backend = self
            .selected
            .lock()
            .unwrap()
            .unwrap_or(SandboxBackend::Process);
        match backend {
            #[cfg(target_os = "linux")]
            SandboxBackend::Microsandbox => {
                tracing::info!("使用 microsandbox 后端");
                Ok(Arc::new(MicrosandboxBackend::new(
                    self.micro_config.clone(),
                )))
            }
            #[cfg(not(target_os = "linux"))]
            SandboxBackend::Microsandbox => {
                tracing::warn!("microsandbox 仅支持 Linux，降级为 process 后端");
                Ok(Arc::new(ProcessBackend::new()))
            }
            SandboxBackend::CubeSandbox => {
                Ok(Arc::new(CubeSandboxBackend::new(self.cube_config.clone())))
            }
            SandboxBackend::Process => {
                tracing::info!("使用 process 后端（降级）");
                Ok(Arc::new(ProcessBackend::new()))
            }
        }
    }
}
