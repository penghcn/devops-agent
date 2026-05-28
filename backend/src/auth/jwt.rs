//! JWT 签发与验证。

use chrono::{Duration, Utc};
use jsonwebtoken::{Algorithm, DecodingKey, EncodingKey, Header, decode, decode_header, encode};
use serde::{Deserialize, Serialize};

/// JWT 声明
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Claims {
    /// 用户名
    pub sub: String,
    /// GitLab ID
    pub gitlab_id: String,
    /// 角色
    pub role: String,
    /// 过期时间
    pub exp: u64,
    /// 签发时间
    pub iat: u64,
}

/// JWT 相关错误
#[derive(Debug)]
pub enum JwtError {
    Expired,
    InvalidToken(jsonwebtoken::errors::Error),
    MissingClaim,
}

impl std::fmt::Display for JwtError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            JwtError::Expired => write!(f, "Token expired"),
            JwtError::InvalidToken(e) => write!(f, "Invalid token: {}", e),
            JwtError::MissingClaim => write!(f, "Missing required claim"),
        }
    }
}

impl std::error::Error for JwtError {}

/// 签发 Access Token（24 小时有效）
pub fn create_access_token(
    username: &str,
    gitlab_id: &str,
    role: &str,
    secret: &str,
) -> Result<String, jsonwebtoken::errors::Error> {
    let now = Utc::now();
    let claims = Claims {
        sub: username.to_string(),
        gitlab_id: gitlab_id.to_string(),
        role: role.to_string(),
        exp: (now + Duration::hours(24)).timestamp() as u64,
        iat: now.timestamp() as u64,
    };

    encode(
        &Header::new(Algorithm::HS256),
        &claims,
        &EncodingKey::from_secret(secret.as_bytes()),
    )
}

/// 验证并解析 JWT
pub fn verify_token<T>(token: &str, secret: &str) -> Result<Claims, JwtError>
where
    T: serde::de::DeserializeOwned,
{
    let token_data = decode::<Claims>(
        token,
        &DecodingKey::from_secret(secret.as_bytes()),
        &jsonwebtoken::Validation::new(Algorithm::HS256),
    )
    .map_err(JwtError::InvalidToken)?;

    Ok(token_data.claims)
}

/// 仅解析 Header，不验证签名（用于调试）
pub fn parse_header(token: &str) -> Result<Algorithm, JwtError> {
    decode_header(token)
        .map(|h| h.alg)
        .map_err(JwtError::InvalidToken)
}
