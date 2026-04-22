use anyhow::Result;
use reqwest::Client;

/// GitLab API 封装
/// 提供原子化的 GitLab 操作函数，让 Claude Code 调用
pub async fn get_pipeline_status(
    project_id: &str,
    pipeline_id: &str,
    token: &str,
) -> Result<serde_json::Value> {
    let client = Client::new();
    let url = format!(
        "https://gitlab.com/api/v4/projects/{}/pipelines/{}/status",
        project_id, pipeline_id
    );

    let response = client
        .get(&url)
        .header("PRIVATE-TOKEN", token)
        .send()
        .await?;

    if response.status().is_success() {
        let body = response.json::<serde_json::Value>().await?;
        Ok(body)
    } else {
        anyhow::bail!("Failed to get pipeline status: {}", response.status())
    }
}

pub async fn create_merge_request(
    project_id: &str,
    source_branch: &str,
    target_branch: &str,
    title: &str,
    token: &str,
) -> Result<String> {
    let client = Client::new();
    let url = format!(
        "https://gitlab.com/api/v4/projects/{}/merge_requests",
        project_id
    );

    let response = client
        .post(&url)
        .header("PRIVATE-TOKEN", token)
        .json(&serde_json::json!({
            "source_branch": source_branch,
            "target_branch": target_branch,
            "title": title,
        }))
        .send()
        .await?;

    if response.status().is_success() {
        let body = response.json::<serde_json::Value>().await?;
        Ok(format!("Merge Request created: {}", body["web_url"]))
    } else {
        let error = response.text().await?;
        anyhow::bail!("Failed to create MR: {}", error)
    }
}
