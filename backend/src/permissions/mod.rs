//! 权限控制模块。
//!
//! 从 TOML 加载项目白名单，校验用户是否有权访问某个项目。

pub mod checker;
pub mod config;

pub use checker::PermissionChecker;
