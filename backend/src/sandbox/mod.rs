pub mod fs_isolation;
pub mod network_whitelist;
pub mod path_check;
pub mod process_sandbox;

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

pub use fs_isolation::{FileSystemIsolator, FsIsolationConfig};
pub use network_whitelist::{NetworkCheckResult, NetworkWhitelist};
pub use path_check::{PathValidation, PathValidator};
pub use process_sandbox::{ProcessResult, ProcessSandbox, ProcessSandboxConfig};
