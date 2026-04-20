use base64::Engine;
use reqwest::Client;
use std::env;

// 测试前加载 .env 文件
#[ctor::ctor]
fn init_env() {
    dotenv::dotenv().ok();
}

/// 获取 Jenkins 配置
fn get_jenkins_config() -> (String, String, String) {
    let url = env::var("JENKINS_URL").expect("JENKINS_URL not set");
    let user = env::var("JENKINS_USER").expect("JENKINS_USER not set");
    let token = env::var("JENKINS_TOKEN").expect("JENKINS_TOKEN not set");
    (url, user, token)
}

/// 构建 Basic Auth Header
fn build_auth_header(user: &str, token: &str) -> String {
    let auth = format!("{}:{}", user, token);
    format!("Basic {}", base64::engine::general_purpose::STANDARD.encode(&auth))
}

#[tokio::test]
async fn test_jenkins_connectivity() {
    let (url, user, token) = get_jenkins_config();
    let client = Client::new();
    let auth = build_auth_header(&user, &token);

    let response = client
        .get(&format!("{}/api/json", url))
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
    let (url, user, token) = get_jenkins_config();
    let client = Client::new();
    let auth = build_auth_header(&user, &token);

    let response = client
        .get(&format!("{}/job/ds-pkg/api/json", url))
        .header("Authorization", &auth)
        .send()
        .await
        .expect("Failed to get ds-pkg job info");

    assert!(response.status().is_success(), "ds-pkg job not found");
    let body: serde_json::Value = response.json().await.unwrap();
    println!("Job name: {:?}", body.get("displayName"));
    println!("Job type: {:?}", body.get("_class"));

    // ds-pkg 应该是 Pipeline 多分支项目
    let class: &str = body.get("_class").and_then(|v| v.as_str()).unwrap_or("");
    assert!(
        class.contains("WorkflowMultiBranchProject"),
        "ds-pkg should be a Pipeline Multi-Branch project, got: {}",
        class
    );
}

#[tokio::test]
async fn test_ds_pkg_dev_branch_exists() {
    let (url, user, token) = get_jenkins_config();
    let client = Client::new();
    let auth = build_auth_header(&user, &token);

    let response = client
        .get(&format!("{}/job/ds-pkg/job/dev/api/json", url))
        .header("Authorization", &auth)
        .send()
        .await
        .expect("Failed to get ds-pkg/dev branch info");

    assert!(response.status().is_success(), "ds-pkg dev branch not found");
    let body: serde_json::Value = response.json().await.unwrap();
    println!("Branch name: {:?}", body.get("displayName"));
    println!("Branch type: {:?}", body.get("_class"));

    // dev 分支应该是 Pipeline Job
    let class: &str = body.get("_class").and_then(|v| v.as_str()).unwrap_or("");
    assert!(
        class.contains("WorkflowJob"),
        "dev branch should be a Pipeline job, got: {}",
        class
    );
}

#[tokio::test]
async fn test_trigger_ds_pkg_dev_build() {
    let (url, user, token) = get_jenkins_config();
    let client = Client::new();
    let auth = build_auth_header(&user, &token);

    // 触发构建（Jenkins 需要 POST 空表单）
    let response = client
        .post(&format!("{}/job/ds-pkg/job/dev/build", url))
        .header("Authorization", &auth)
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body("")
        .send()
        .await
        .expect("Failed to trigger build");

    assert!(
        response.status().is_success(),
        "Build trigger failed: {}",
        response.text().await.unwrap()
    );

    // 获取 Location header 来提取构建号
    let location = response
        .headers()
        .get("Location")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    println!("Build triggered. Location: {}", location);

    // 从 URL 中提取构建号
    // 例如: http://host/jenkins/job/ds-pkg/job/dev/123/
    let build_number = location
        .split('/')
        .filter(|s| s.parse::<u32>().is_ok())
        .next()
        .and_then(|s| s.parse::<u32>().ok());

    assert!(
        build_number.is_some(),
        "Could not extract build number from location: {}",
        location
    );

    let build_num = build_number.unwrap();
    println!("Build #{} triggered successfully", build_num);

    // 等待构建完成（最多 10 分钟）
    let max_wait = 600u64;
    let poll_interval = 10u64;
    let mut elapsed = 0u64;

    while elapsed < max_wait {
        tokio::time::sleep(tokio::time::Duration::from_secs(poll_interval)).await;
        elapsed += poll_interval;

        let status_response = client
            .get(&format!("{}/job/ds-pkg/job/dev/{}/api/json", url, build_num))
            .header("Authorization", &auth)
            .send()
            .await
            .expect("Failed to get build status");

        if !status_response.status().is_success() {
            continue;
        }

        let status: serde_json::Value = status_response.json().await.unwrap();
        let in_progress = status.get("inProgress").and_then(|v| v.as_bool()).unwrap_or(true);
        let result = status.get("result").and_then(|v| v.as_str());

        println!(
            "Build #{} - Elapsed: {}s, InProgress: {}, Result: {:?}",
            build_num, elapsed, in_progress, result
        );

        if !in_progress {
            // 构建完成
            if let Some(r) = result {
                println!("Build #{} completed with result: {}", build_num, r);
                assert!(
                    r == "SUCCESS",
                    "Build #{} failed with result: {}",
                    build_num,
                    r
                );
            } else {
                println!("Build #{} is still running but inProgress=false", build_num);
            }
            break;
        }
    }

    if elapsed >= max_wait {
        panic!("Build #{} did not complete within {} seconds", build_num, max_wait);
    }
}

#[tokio::test]
async fn test_get_latest_build_status() {
    let (url, user, token) = get_jenkins_config();
    let client = Client::new();
    let auth = build_auth_header(&user, &token);

    // 获取最新构建信息
    let response = client
        .get(&format!("{}/job/ds-pkg/job/dev/api/json?fields=lastBuild,number", url))
        .header("Authorization", &auth)
        .send()
        .await
        .expect("Failed to get latest build info");

    assert!(response.status().is_success());
    let body: serde_json::Value = response.json().await.unwrap();

    println!("Latest build info: {:?}", serde_json::to_string_pretty(&body).unwrap());

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
