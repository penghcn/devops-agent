use devops_agent::config::Config;
use devops_agent::tools::jenkins;
use reqwest::Client;
use std::path::Path;

/// 检查配置文件是否存在，不存在则 panic 带清晰提示（测试框架会捕获）
fn require_config() {
    let candidates = ["config.toml", "backend/config.toml", "../config.toml"];
    assert!(
        candidates.iter().any(|p| Path::new(p).exists()),
        "Jenkins 测试需要 config.toml。请放在项目根目录、backend/ 或 ../ 下（相对于 CWD）"
    );
}

/// 获取 Jenkins 配置引用（通过 Config 三层加载：TOML > .env > 环境变量）
fn get_jenkins_config(config: &Config) -> (&str, &str, &str) {
    (
        &config.jenkins_url,
        &config.jenkins_user,
        &config.jenkins_token,
    )
}

/// 构建 Basic Auth Header
fn build_auth_header(user: &str, token: &str) -> String {
    let auth = format!("{}:{}", user, token);
    format!("Basic {}", yunli::hash::b64(auth.as_bytes()))
}

#[tokio::test]
async fn test_jenkins_connectivity() {
    require_config();
    let config = Config::from_file();
    let (url, user, token) = get_jenkins_config(&config);
    let client = Client::new();
    let auth = build_auth_header(user, token);

    let response = client
        .get(format!("{}/api/json", url))
        .header("Authorization", &auth)
        .send()
        .await
        .expect("Failed to connect to Jenkins");

    assert!(response.status().is_success(), "Jenkins connection failed");
    let body: serde_json::Value = response.json().await.unwrap();
    println!("Jenkins version: {:?}", body.get("version"));
    println!("Node count: {:?}", body.get("numExecutors"));
}

#[tokio::test]
async fn test_ds_pkg_job_exists() {
    require_config();
    let config = Config::from_file();
    let (url, user, token) = get_jenkins_config(&config);
    let client = Client::new();
    let auth = build_auth_header(user, token);

    let response = client
        .get(format!("{}/job/ds-pkg/api/json", url))
        .header("Authorization", &auth)
        .send()
        .await
        .expect("Failed to get ds-pkg job info");

    assert!(response.status().is_success(), "ds-pkg job not found");
    let body: serde_json::Value = response.json().await.unwrap();
    println!("Job name: {:?}", body.get("displayName"));
    println!("Job type: {:?}", body.get("_class"));

    // ds-pkg 应该是 Pipeline 多分支项目
    let class = body
        .get("_class")
        .and_then(|v| v.as_str())
        .expect("ds-pkg 响应缺少 _class 字段");
    assert!(
        class.contains("WorkflowMultiBranchProject"),
        "ds-pkg 应该是 Pipeline Multi-Branch 项目，实际: {}",
        class
    );
}

#[tokio::test]
#[ignore]
async fn test_ds_pkg_dev_branch_exists() {
    require_config();
    let config = Config::from_file();
    let (url, user, token) = get_jenkins_config(&config);
    let client = Client::new();
    let auth = build_auth_header(user, token);

    let response = client
        .get(format!("{}/job/ds-pkg/job/dev/api/json", url))
        .header("Authorization", &auth)
        .send()
        .await
        .expect("Failed to get ds-pkg/dev branch info");

    assert!(
        response.status().is_success(),
        "ds-pkg dev branch not found"
    );
    let body: serde_json::Value = response.json().await.unwrap();
    println!("Branch name: {:?}", body.get("displayName"));
    println!("Branch type: {:?}", body.get("_class"));

    // dev 分支应该是 Pipeline Job
    let class = body
        .get("_class")
        .and_then(|v| v.as_str())
        .expect("dev 分支响应缺少 _class 字段");
    assert!(
        class.contains("WorkflowJob"),
        "dev 分支应该是 Pipeline Job，实际: {}",
        class
    );
}

#[tokio::test]
#[ignore]
async fn test_trigger_ds_pkg_dev_build() {
    require_config();
    let _ = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .with_ansi(false)
        .with_timer(tracing_subscriber::fmt::time::LocalTime::rfc_3339())
        .try_init();
    let config = Config::from_file();

    // 触发构建 + 自动等待完成（封装在 trigger_pipeline + wait_for_pipeline 中）
    let message = jenkins::trigger_pipeline("ds-pkg", Some("dev"), &config)
        .await
        .expect("trigger_pipeline failed");
    println!("{}", message);

    // 从消息中提取构建号
    let build_num = message
        .split('/')
        .find(|s| s.parse::<u32>().is_ok())
        .and_then(|s| s.parse::<u32>().ok())
        .expect("No build number in message");
    println!("Build #{} triggered", build_num);

    // 等待构建完成（最多 30 分钟，每 10 秒轮询）
    let status = jenkins::wait_for_pipeline("ds-pkg", "dev", build_num, &config, 10, 1800)
        .await
        .expect("wait_for_pipeline failed");

    let result = status
        .get("result")
        .and_then(|r| r.as_str())
        .unwrap_or("UNKNOWN");
    let in_progress = status
        .get("inProgress")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    println!(
        "Build #{} completed: result={}, inProgress={}",
        build_num, result, in_progress
    );

    if result != "SUCCESS" && result != "UNKNOWN" {
        panic!("Build #{} failed with result: {}", build_num, result);
    }
}

#[tokio::test]
#[ignore]
async fn test_get_latest_build_status() {
    require_config();
    let config = Config::from_file();
    let (url, user, token) = get_jenkins_config(&config);
    let client = Client::new();
    let auth = build_auth_header(user, token);

    // 获取最新构建信息
    let response = client
        .get(format!(
            "{}/job/ds-pkg/job/dev/api/json?fields=lastBuild,number",
            url
        ))
        .header("Authorization", &auth)
        .send()
        .await
        .expect("Failed to get latest build info");

    assert!(response.status().is_success());
    let body: serde_json::Value = response.json().await.unwrap();

    println!(
        "Latest build info: {:?}",
        serde_json::to_string_pretty(&body).unwrap()
    );

    if let Some(last_build) = body.get("lastBuild") {
        let build_num = last_build.get("number").and_then(|n| n.as_u64());
        let build_url = last_build.get("url").and_then(|u| u.as_str());

        println!(
            "Latest build: #{} at {}",
            build_num.unwrap_or(0),
            build_url.unwrap_or("N/A")
        );
    } else {
        println!("No builds found for ds-pkg/dev");
    }
}
