//! GitLab OAuth 认证模块。
//!
//! JWT 签发验证 + GitLab OAuth 登录流程 + Axum 中间件。

pub mod gitlab_oauth;
pub mod jwt;
pub mod middleware;

pub use jwt::{Claims, JwtError};
pub use middleware::auth_guard;
