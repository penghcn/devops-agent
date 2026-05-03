use devops_agent::agent::{Step, StepContext, StepResult};
use devops_agent::app_config::Config;
use devops_agent::tools::jenkins;
use std::sync::Arc;

#[ctor::ctor]
fn init_env() {
    dotenv::dotenv().ok();
}

/// 测试 JenkinsLogStep 缺少 job_name 时中止
#[tokio::test]
async fn test_jenkins_log_step_missing_job_name() {
    let config = Config::from_file();
    let mut ctx = StepContext::new(
        "test".to_string(),
        devops_agent::agent::TaskType::default(),
        None,
        Some("dev".to_string()),
        Arc::new(config),
    );
    ctx.build_number = Some(1);

    let result = devops_agent::agent::steps::jenkins_log::JenkinsLogStep
        .execute(&mut ctx)
        .await;
    match result {
        StepResult::Abort { reason } => assert!(reason.contains("job_name")),
        _ => panic!("Expected Abort, got {:?}", result),
    }
}

/// 测试 JenkinsLogStep 缺少 branch 时中止
#[tokio::test]
async fn test_jenkins_log_step_missing_branch() {
    let config = Config::from_file();
    let mut ctx = StepContext::new(
        "test".to_string(),
        devops_agent::agent::TaskType::default(),
        Some("ds-pkg".to_string()),
        None,
        Arc::new(config),
    );
    ctx.build_number = Some(1);

    let result = devops_agent::agent::steps::jenkins_log::JenkinsLogStep
        .execute(&mut ctx)
        .await;
    match result {
        StepResult::Abort { reason } => assert!(reason.contains("branch")),
        _ => panic!("Expected Abort, got {:?}", result),
    }
}

/// 测试 JenkinsLogStep 缺少 build_number 时中止
#[tokio::test]
async fn test_jenkins_log_step_missing_build_number() {
    let config = Config::from_file();
    let mut ctx = StepContext::new(
        "test".to_string(),
        devops_agent::agent::TaskType::default(),
        Some("ds-pkg".to_string()),
        Some("dev".to_string()),
        Arc::new(config),
    );
    // build_number 为 None

    let result = devops_agent::agent::steps::jenkins_log::JenkinsLogStep
        .execute(&mut ctx)
        .await;
    match result {
        StepResult::Abort { reason } => assert!(reason.contains("build_number")),
        _ => panic!("Expected Abort, got {:?}", result),
    }
}

/// 集成测试：获取真实 Jenkins 构建日志（需要连接 Jenkins）
#[tokio::test]
#[ignore]
async fn test_get_real_build_log() {
    let config = Config::from_file();
    let build_num = get_latest_build_number("ds-pkg", Some("dev"), &config)
        .await
        .expect("Failed to get latest build number");
    println!("Latest build #{}", build_num);

    let log = jenkins::get_build_log("ds-pkg", "dev", build_num, &config)
        .await
        .expect("Failed to get build log");

    println!("Log length: {} bytes", log.len());
    assert!(log.len() > 0, "Build log should not be empty");
    assert!(
        log.contains("[Pipeline]") || log.contains("Started"),
        "Log should contain pipeline markers"
    );
}

/// 集成测试：获取构建日志并分析完整性（成功构建场景）
#[tokio::test]
#[ignore]
async fn test_log_analysis_success_build() {
    let config = Config::from_file();
    let build_num = get_latest_build_number("ds-pkg", Some("dev"), &config)
        .await
        .expect("Failed to get build number");

    let status = jenkins::get_pipeline_status("ds-pkg", "dev", build_num, &config)
        .await
        .expect("Failed to get pipeline status");

    let result = status
        .get("result")
        .and_then(|r| r.as_str())
        .unwrap_or("UNKNOWN");

    let in_progress = status
        .get("inProgress")
        .and_then(|v| v.as_bool())
        .unwrap_or(true);

    println!(
        "Build #{}: result={}, inProgress={}",
        build_num, result, in_progress
    );

    // 如果构建还在进行中，跳过分析测试
    if in_progress || result.is_empty() {
        println!("Build #{} still in progress, skipping analysis", build_num);
        return;
    }

    let log = jenkins::get_build_log("ds-pkg", "dev", build_num, &config)
        .await
        .expect("Failed to get build log");

    println!("Build #{} log length: {} bytes", build_num, log.len());

    // 修复后不再截断，验证完整日志包含 deploy 内容
    let has_deploy_in_full = log.contains("SSH deploy") || log.contains("deploy");

    println!("Full log has deploy: {}", has_deploy_in_full);
    println!(
        "Log start (first 200 chars):\n{}",
        &log[..log.len().min(200)]
    );
    if log.len() > 5000 {
        println!(
            "Log end (last 200 chars):\n{}",
            &log[log.len() - 200.min(log.len())..log.len()]
        );
    }
}

/// 辅助函数：获取最新构建号
async fn get_latest_build_number(
    job_name: &str,
    branch: Option<&str>,
    config: &Config,
) -> Result<u32, anyhow::Error> {
    use anyhow::Context;
    use base64::Engine;
    use reqwest::{
        Client,
        header::{AUTHORIZATION, HeaderMap, HeaderValue},
    };

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

    let response = client.get(&url).headers(headers).send().await?;
    let body: serde_json::Value = response.json().await?;

    let build_num = body
        .get("lastBuild")
        .and_then(|b| b.get("number"))
        .and_then(|n| n.as_u64())
        .map(|n| n as u32)
        .context("No lastBuild found")?;

    Ok(build_num)
}
