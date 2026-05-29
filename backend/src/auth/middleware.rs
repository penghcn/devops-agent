//! Axum 认证中间件。
//!
//! 支持两种模式：
//! 1. API Key（向后兼容，管理员权限）
//! 2. JWT Bearer Token（用户级权限）

use std::sync::Arc;

use axum::{
    body::Body,
    extract::State,
    http::{Request, StatusCode},
    middleware::Next,
    response::Response,
};

use crate::api::AppState;
use crate::auth::jwt;

/// 从请求中提取用户身份
#[derive(Debug, Clone)]
pub struct AuthenticatedUser {
    pub username: String,
    pub gitlab_id: String,
    pub role: String,
}

/// 认证守卫中间件
///
/// 检查 Authorization header 或 X-API-Key header。
/// 认证通过后将用户信息注入请求扩展。
pub async fn auth_guard(
    State(state): State<Arc<AppState>>,
    mut req: Request<Body>,
    next: Next,
) -> Result<Response, StatusCode> {
    // 1. 尝试 JWT Bearer Token
    if let Some(token) = extract_bearer_token(&req) {
        if let Ok(claims) = jwt::verify_token::<()>(&token, &state.config.auth.jwt_secret) {
            let user = AuthenticatedUser {
                username: claims.sub,
                gitlab_id: claims.gitlab_id,
                role: claims.role,
            };
            req.extensions_mut().insert(user);
            return Ok(next.run(req).await);
        }
    }

    // 2. 尝试 API Key（向后兼容，管理员权限）
    if let Some(api_key) = extract_api_key(&req) {
        if let Some(config_key) = &state.config.api_key {
            if api_key == *config_key {
                let user = AuthenticatedUser {
                    username: "api_key_user".to_string(),
                    gitlab_id: "0".to_string(),
                    role: "admin".to_string(),
                };
                req.extensions_mut().insert(user);
                return Ok(next.run(req).await);
            }
        }
    }

    // 3. 如果未配置 api_key，放行（内部部署模式）
    if state.config.api_key.is_none() {
        return Ok(next.run(req).await);
    }

    // 4. 公开端点放行（在白名单中）
    let path = req.uri().path().to_string();
    if is_public_path(&path) {
        return Ok(next.run(req).await);
    }

    Err(StatusCode::UNAUTHORIZED)
}

/// 从 Authorization header 提取 Bearer Token
fn extract_bearer_token(req: &Request<Body>) -> Option<String> {
    let auth_header = req.headers().get("Authorization")?.to_str().ok()?;

    if let Some(token) = auth_header.strip_prefix("Bearer ") {
        Some(token.trim().to_string())
    } else {
        None
    }
}

/// 从 X-API-Key header 提取 API Key
fn extract_api_key(req: &Request<Body>) -> Option<String> {
    req.headers()
        .get("X-API-Key")?
        .to_str()
        .ok()
        .map(|s| s.to_string())
}

/// 检查路径是否为公开端点
fn is_public_path(path: &str) -> bool {
    path == "/api/auth/gitlab/login"
        || path == "/api/auth/gitlab/callback"
        || path == "/health"
        || path.starts_with("/api/auth/")
}

/// 从请求扩展中获取用户信息
pub fn get_user_from_extensions(
    ext: &axum::extract::Extension<AuthenticatedUser>,
) -> &AuthenticatedUser {
    &ext.0
}
