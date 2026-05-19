pub mod cubesandbox;
pub mod factory;
#[cfg(target_os = "linux")]
pub mod microsandbox_backend;
pub mod process_backend;
pub mod process_sandbox;
pub mod trait_sandbox;

// 保留原有模块作为降级方案的内部实现
pub mod fs_isolation;
pub mod network_whitelist;
pub mod path_check;

use std::fmt;

/// 沙箱统一错误类型
#[derive(Debug)]
pub enum SandboxError {
    /// 路径穿越
    PathTraversal(String),
    /// 敏感文件访问
    SensitiveFile(String),
    /// 超出工作区
    OutsideWorkspace(String),
    /// 超时
    TimeoutExceeded(String),
    /// 网络拦截
    NetworkBlocked(String),
    /// IO 错误
    IoError(String),
}

impl fmt::Display for SandboxError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SandboxError::PathTraversal(msg) => write!(f, "路径穿越: {}", msg),
            SandboxError::SensitiveFile(msg) => write!(f, "敏感文件访问: {}", msg),
            SandboxError::OutsideWorkspace(msg) => write!(f, "超出工作区: {}", msg),
            SandboxError::TimeoutExceeded(msg) => write!(f, "超时: {}", msg),
            SandboxError::NetworkBlocked(msg) => write!(f, "网络拦截: {}", msg),
            SandboxError::IoError(msg) => write!(f, "IO 错误: {}", msg),
        }
    }
}

impl std::error::Error for SandboxError {}

// 新抽象层导出
pub use cubesandbox::CubeSandboxConfig;
pub use factory::{MicrosandboxConfig, SandboxBackend, SandboxFactory};
pub use process_backend::ProcessBackend;
pub use trait_sandbox::{ExecResult, Sandbox};

// 保留旧导出以兼容现有代码
pub use fs_isolation::{FileSystemIsolator, FsIsolationConfig};
pub use network_whitelist::{NetworkCheckResult, NetworkWhitelist};
pub use path_check::{PathValidation, PathValidator};
pub use process_sandbox::{ProcessResult, ProcessSandbox, ProcessSandboxConfig};
