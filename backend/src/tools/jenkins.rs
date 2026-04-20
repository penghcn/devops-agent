use base64::Engine;
use reqwest::{Client, header::{HeaderMap, HeaderValue, AUTHORIZATION}};
use anyhow::{Result, bail};
use crate::config::Config;

/// 校验 Jenkins Job 名称，防止路径注入
/// Jenkins Job 名称只允许: 字母、数字、连字符、下划线、斜杠(文件夹)、点
pub fn sanitize_job_name(name: &str) -> Result<&str> {
    if name.is_empty() {
        bail!("Job name cannot be empty");
    }
    if name.contains("..") || name.contains('\0') || name.contains('\n') {
        bail!("Invalid job name: contains dangerous characters");
    }
    if !name.chars().all(|c| c.is_alphanumeric() || c == '-' || c == '_' || c == '/' || c == '.') {
        bail!("Invalid job name: contains invalid characters (only alphanumeric, -, _, /, . allowed)");
    }
    Ok(name)
}

/// 触发 Jenkins Job
/// 
/// 设计决策：提供原子化的工具函数，让 Claude Code 调用，
/// 而非让 Claude 自己写 curl 命令。原因：
/// 1. 封装认证逻辑，避免泄露 Token
/// 2. 统一错误处理
/// 3. 便于审计和日志记录
pub async fn trigger_job(job_name: &str, params: &serde_json::Value, config: &Config) -> Result<String> {
    let job_name = sanitize_job_name(job_name)?;
    let client = Client::new();

    // 构建 Basic Auth
    let auth_value = format!(
        "{}:{}",
        config.jenkins_user, config.jenkins_token
    );
    let encoded = base64::engine::general_purpose::STANDARD.encode(&auth_value);
    let mut headers = HeaderMap::new();
    headers.insert(
        AUTHORIZATION,
        HeaderValue::from_str(&format!("Basic {}", encoded))?
    );

    // 触发构建
    let url = format!(
        "{}/job/{}/buildWithParameters",
        config.jenkins_url, job_name
    );
    
    let response = client
        .post(&url)
        .headers(headers)
        .json(params)
        .send()
        .await?;
    
    if response.status().is_success() {
        // 获取 Queue ID 用于后续查询
        let location = response
            .headers()
            .get("Location")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        Ok(format!("Job triggered successfully. Queue: {}", location))
    } else {
        anyhow::bail!("Failed to trigger job: {}", response.status())
    }
}

/// 查询 Job 最近一次构建状态
pub async fn get_job_status(job_name: &str, config: &Config) -> Result<serde_json::Value> {
    let job_name = sanitize_job_name(job_name)?;
    let client = Client::new();

    let auth_value = format!("{}:{}", config.jenkins_user, config.jenkins_token);
    let encoded = base64::engine::general_purpose::STANDARD.encode(&auth_value);
    let mut headers = HeaderMap::new();
    headers.insert(
        AUTHORIZATION,
        HeaderValue::from_str(&format!("Basic {}", encoded))?,
    );

    let url = format!("{}/job/{}/api/json?fields=id,status,color", config.jenkins_url, job_name);

    let response = client
        .get(&url)
        .headers(headers)
        .send()
        .await?;

    if response.status().is_success() {
        let body = response.json::<serde_json::Value>().await?;
        Ok(body)
    } else {
        anyhow::bail!("Failed to get job status: {}", response.status())
    }
}

/// 触发 Pipeline 多分支构建
///
/// Jenkins Pipeline 多分支的 URL 模式:
/// /job/{job_name}/job/{branch}/build  — 触发指定分支构建
/// /job/{job_name}/build              — 触发默认分支构建
pub async fn trigger_pipeline(
    job_name: &str,
    branch: Option<&str>,
    config: &Config,
) -> Result<String> {
    let job_name = sanitize_job_name(job_name)?;
    let client = Client::new();

    let auth_value = format!("{}:{}", config.jenkins_user, config.jenkins_token);
    let encoded = base64::engine::general_purpose::STANDARD.encode(&auth_value);
    let mut headers = HeaderMap::new();
    headers.insert(
        AUTHORIZATION,
        HeaderValue::from_str(&format!("Basic {}", encoded))?,
    );

    // 构建 URL
    let url = match branch {
        Some(b) => {
            let b = sanitize_job_name(b)?;
            format!("{}/job/{}/job/{}/build", config.jenkins_url, job_name, b)
        }
        None => format!("{}/job/{}/build", config.jenkins_url, job_name),
    };

    tracing::info!("Triggering pipeline: {}", url);

    let response = client
        .post(&url)
        .headers(headers)
        .body("")
        .send()
        .await?;

    if response.status().is_success() {
        let location = response
            .headers()
            .get("Location")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        Ok(format!("Pipeline triggered successfully. Build URL: {}", location))
    } else {
        anyhow::bail!("Failed to trigger pipeline: {} ({})", response.status(), response.text().await?)
    }
}

/// 查询 Pipeline 构建状态
///
/// Jenkins Pipeline 状态在 `inProgress` 字段中:
/// - inProgress: true 表示构建中
/// - result: null 表示未开始/进行中
/// - result: SUCCESS/FAILURE/ABORTED 表示已完成
pub async fn get_pipeline_status(
    job_name: &str,
    branch: &str,
    build_number: u32,
    config: &Config,
) -> Result<serde_json::Value> {
    let job_name = sanitize_job_name(job_name)?;
    let branch = sanitize_job_name(branch)?;
    let client = Client::new();

    let auth_value = format!("{}:{}", config.jenkins_user, config.jenkins_token);
    let encoded = base64::engine::general_purpose::STANDARD.encode(&auth_value);
    let mut headers = HeaderMap::new();
    headers.insert(
        AUTHORIZATION,
        HeaderValue::from_str(&format!("Basic {}", encoded))?,
    );

    let url = format!(
        "{}/job/{}/job/{}/{}",
        config.jenkins_url, job_name, branch, build_number
    );

    let response = client
        .get(&url)
        .header("Accept", "application/json")
        .headers(headers)
        .send()
        .await?;

    if response.status().is_success() {
        let body = response.json::<serde_json::Value>().await?;
        Ok(body)
    } else {
        anyhow::bail!("Failed to get pipeline status: {}", response.status())
    }
}

/// 等待 Pipeline 完成（轮询模式）
///
/// 默认每 5 秒轮询一次，最多等待 30 分钟
pub async fn wait_for_pipeline(
    job_name: &str,
    branch: &str,
    build_number: u32,
    config: &Config,
    poll_interval_secs: u64,
    max_wait_secs: u64,
) -> Result<serde_json::Value> {
    let mut elapsed = 0u64;
    let sleep_duration = std::time::Duration::from_secs(poll_interval_secs);

    while elapsed < max_wait_secs {
        let status = get_pipeline_status(job_name, branch, build_number, config).await?;

        // 检查 inProgress 字段
        if let Some(in_progress) = status.get("inProgress").and_then(|v| v.as_bool()) {
            if !in_progress {
                // 构建完成
                tracing::info!("Pipeline #{} completed in {}s", build_number, elapsed);
                return Ok(status);
            }
        }

        // 检查 result 字段（某些 Job 类型可能没有 inProgress）
        if let Some(result) = status.get("result").and_then(|v| v.as_str()) {
            if !result.is_empty() {
                tracing::info!("Pipeline #{} completed with result: {} in {}s", build_number, result, elapsed);
                return Ok(status);
            }
        }

        // 等待后继续轮询
        tokio::time::sleep(sleep_duration).await;
        elapsed += poll_interval_secs;
    }

    anyhow::bail!("Pipeline #{} did not complete within {} seconds", build_number, max_wait_secs)
}