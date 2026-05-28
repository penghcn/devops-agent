//! GitLab OAuth 登录流程。
//!
//! 流程：
//! 1. 前端重定向到 GitLab 授权页面
//! 2. 用户授权后 GitLab 回调我们的 /auth/gitlab/callback
//! 3. 我们用 code 换取 access_token
//! 4. 查询用户信息，签发 JWT

use serde::Deserialize;

use crate::config::AuthConfig;

/// GitLab OAuth 授权码换取 Token 的响应
#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct GitLabTokenResponse {
    access_token: String,
    token_type: String,
    expires_in: Option<u64>,
    refresh_token: Option<String>,
    scope: String,
}

/// GitLab 用户信息
#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct GitLabUser {
    id: i64,
    username: String,
    name: String,
    avatar_url: String,
    state: String,
}

/// 用户登录结果
#[derive(Debug)]
pub struct LoginResult {
    pub username: String,
    pub gitlab_id: String,
    pub avatar_url: String,
    pub access_token: String, // GitLab access_token，后续调用 API 用
}

/// 生成 GitLab 授权 URL，前端重定向到此地址
pub fn auth_url(config: &AuthConfig, redirect_uri: &str) -> String {
    format!(
        "{}/oauth/authorize?response_type=code&client_id={}&redirect_uri={}&scope=read_api",
        config.gitlab_url.trim_end_matches('/'),
        config.gitlab_client_id,
        urlencoding(redirect_uri)
    )
}

/// 用授权码换取 Access Token，并获取用户信息
pub async fn exchange_code(
    config: &AuthConfig,
    code: &str,
    redirect_uri: &str,
) -> anyhow::Result<LoginResult> {
    let token_url = format!("{}/oauth/token", config.gitlab_url.trim_end_matches('/'));

    let client = reqwest::Client::new();
    let token_resp: GitLabTokenResponse = client
        .post(&token_url)
        .form(&[
            ("grant_type", "authorization_code_code"),
            ("code", code),
            ("redirect_uri", redirect_uri),
            ("client_id", &config.gitlab_client_id),
            ("client_secret", &config.gitlab_client_secret),
        ])
        .send()
        .await?
        .json()
        .await?;

    // 获取用户信息
    let user: GitLabUser = client
        .get(format!(
            "{}/api/v4/user",
            config.gitlab_url.trim_end_matches('/')
        ))
        .bearer_auth(&token_resp.access_token)
        .send()
        .await?
        .json()
        .await?;

    if user.state != "active" {
        return Err(anyhow::anyhow!("GitLab user is not active"));
    }

    Ok(LoginResult {
        username: user.username,
        gitlab_id: user.id.to_string(),
        avatar_url: user.avatar_url,
        access_token: token_resp.access_token,
    })
}

/// URL 编码
fn urlencoding(s: &str) -> String {
    let mut result = String::new();
    for c in s.chars() {
        match c {
            'A'..='Z' | 'a'..='z' | '0'..='9' | '-' | '_' | '.' | '~' => result.push(c),
            ' ' => result.push_str("%20"),
            _ => {
                let bytes = c.to_string().into_bytes();
                for b in bytes {
                    result.push_str(&format!("%{:02X}", b));
                }
            }
        }
    }
    result
}
