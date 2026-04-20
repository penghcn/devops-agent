use base64::Engine;
use reqwest::{Client, header::{HeaderMap, HeaderValue, AUTHORIZATION}};
use anyhow::{Result, bail, Context};
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
/// 策略：先尝试 HTTP API，若失败（如 Tomcat POST bug 返回 400）则 fallback 到 Jenkins CLI。
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

    // 策略 1: 先尝试 HTTP API 触发
    let http_job_path = match branch {
        Some(b) => {
            let b = sanitize_job_name(b)?;
            format!("{}/{}", job_name, b)
        }
        None => job_name.to_string(),
    };
    let http_url = format!("{}/job/{}/build", config.jenkins_url, http_job_path);

    let auth_value = format!("{}:{}", config.jenkins_user, config.jenkins_token);
    let encoded = base64::engine::general_purpose::STANDARD.encode(&auth_value);
    let mut headers = HeaderMap::new();
    headers.insert(
        AUTHORIZATION,
        HeaderValue::from_str(&format!("Basic {}", encoded))?,
    );

    let client = Client::new();
    let trigger_result = client
        .post(&http_url)
        .headers(headers)
        .body("")
        .send()
        .await;

    match trigger_result {
        Ok(resp) if resp.status().is_success() => {
            tracing::info!("Pipeline triggered via HTTP API: {}", job_name);
            let build_num = get_latest_build_number(job_name, branch, config).await?;
            let branch_str = branch.unwrap_or("");
            let url = format!(
                "{}/job/{}/job/{}/{}",
                config.jenkins_url, job_name, branch_str, build_num
            );
            return Ok(format!("Pipeline triggered successfully. Build URL: {}", url));
        }
        Ok(resp) => {
            tracing::warn!(
                "HTTP API trigger failed ({}), falling back to Jenkins CLI",
                resp.status()
            );
        }
        Err(e) => {
            tracing::warn!("HTTP API trigger error ({:?}), falling back to Jenkins CLI", e);
        }
    }

    // 策略 2: HTTP API 失败，使用 Jenkins CLI fallback
    let job_path = match branch {
        Some(b) => format!("{}/{}", job_name, sanitize_job_name(b)?),
        None => job_name.to_string(),
    };

    tracing::info!("Triggering pipeline via Jenkins CLI fallback: {}", job_path);

    let cli_jar = get_cli_jar(config).await?;
    let auth_owned = format!("{}:{}", config.jenkins_user, config.jenkins_token);
    let url_owned = config.jenkins_url.clone();
    let job_path_owned = job_path.clone();
    let cli_jar_owned = cli_jar.clone();

    let cli_output = tokio::task::spawn_blocking(move || {
        std::process::Command::new("java")
            .arg("-jar")
            .arg(&cli_jar_owned)
            .arg("-s")
            .arg(&url_owned)
            .arg("-auth")
            .arg(&auth_owned)
            .arg("build")
            .arg(&job_path_owned)
            .output()
    })
    .await?
    .context("Failed to execute Jenkins CLI")?;

    if !cli_output.status.success() {
        let stderr = String::from_utf8_lossy(&cli_output.stderr);
        anyhow::bail!("Jenkins CLI build failed: {}", stderr.trim());
    }

    tracing::info!("Pipeline triggered via Jenkins CLI fallback: {}", job_path);

    // CLI 触发成功后，通过 HTTP API 获取最新构建号
    let build_num = get_latest_build_number(job_name, branch, config).await?;

    let branch_str = branch.unwrap_or("");
    let url = format!(
        "{}/job/{}/job/{}/{}",
        config.jenkins_url, job_name, branch_str, build_num
    );

    Ok(format!("Pipeline triggered successfully. Build URL: {}", url))
}

/// 下载/缓存 jenkins-cli.jar
fn get_cli_cache_path() -> std::path::PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    std::path::PathBuf::from(home).join(".cache/jenkins-cli.jar")
}

/// 获取/下载 jenkins-cli.jar 缓存路径
///
/// 如果本地缓存不存在，则从 Jenkins HTTP API 下载。
pub async fn get_cli_jar(config: &Config) -> Result<String> {
    let cache_path = get_cli_cache_path();

    if cache_path.exists() {
        return Ok(cache_path.to_string_lossy().to_string());
    }

    // 创建缓存目录
    if let Some(parent) = cache_path.parent() {
        std::fs::create_dir_all(parent).context("Failed to create jenkins-cli cache directory")?;
    }

    let jar_url = format!("{}/jnlpJars/jenkins-cli.jar", config.jenkins_url);
    tracing::info!("Downloading jenkins-cli.jar from: {}", jar_url);

    let client = Client::new();
    let auth_value = format!("{}:{}", config.jenkins_user, config.jenkins_token);
    let encoded = base64::engine::general_purpose::STANDARD.encode(&auth_value);
    let mut headers = HeaderMap::new();
    headers.insert(
        AUTHORIZATION,
        HeaderValue::from_str(&format!("Basic {}", encoded))?,
    );

    let response = client
        .get(&jar_url)
        .headers(headers)
        .send()
        .await?;

    if !response.status().is_success() {
        anyhow::bail!("Failed to download jenkins-cli.jar: {}", response.status());
    }

    let bytes = response.bytes().await?;
    std::fs::write(&cache_path, &bytes).context("Failed to write jenkins-cli.jar to cache")?;

    tracing::info!("jenkins-cli.jar downloaded to: {}", cache_path.display());
    Ok(cache_path.to_string_lossy().to_string())
}

/// 获取最新构建号（通过 HTTP API GET，不受 POST bug 影响）
async fn get_latest_build_number(
    job_name: &str,
    branch: Option<&str>,
    config: &Config,
) -> Result<u32> {
    let client = Client::new();
    let auth_value = format!("{}:{}", config.jenkins_user, config.jenkins_token);
    let encoded = base64::engine::general_purpose::STANDARD.encode(&auth_value);
    let mut headers = HeaderMap::new();
    headers.insert(
        AUTHORIZATION,
        HeaderValue::from_str(&format!("Basic {}", encoded))?,
    );

    let url = match branch {
        Some(b) => format!(
            "{}/job/{}/job/{}/api/json?fields=lastBuild",
            config.jenkins_url, job_name, b
        ),
        None => format!(
            "{}/job/{}/api/json?fields=lastBuild",
            config.jenkins_url, job_name
        ),
    };

    let response = client
        .get(&url)
        .headers(headers)
        .send()
        .await?;

    if !response.status().is_success() {
        anyhow::bail!("Failed to get latest build number: {}", response.status());
    }

    let body: serde_json::Value = response.json().await?;
    let build_num = body
        .get("lastBuild")
        .and_then(|b| b.get("number"))
        .and_then(|n| n.as_u64())
        .map(|n| n as u32)
        .context("No lastBuild found")?;

    Ok(build_num)
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

/// 获取指定构建的 console 日志
pub async fn get_build_log(
    job_name: &str,
    branch: &str,
    build_number: u32,
    config: &Config,
) -> Result<String> {
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
        "{}/job/{}/job/{}/{}/consoleText",
        config.jenkins_url, job_name, branch, build_number
    );

    let response = client
        .get(&url)
        .headers(headers)
        .send()
        .await?;

    if response.status().is_success() {
        let log = response.text().await?;
        Ok(log)
    } else {
        anyhow::bail!("Failed to get build log: {} ({})", response.status(), response.text().await?)
    }
}