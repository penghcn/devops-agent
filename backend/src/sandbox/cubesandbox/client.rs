use anyhow::Result;

/// 创建沙箱响应
#[derive(Debug, serde::Deserialize)]
pub struct SandboxCreateResponse {
    #[serde(rename = "sandboxID")]
    pub sandbox_id: String,
    #[allow(dead_code)]
    #[serde(rename = "templateID")]
    pub template_id: String,
}

/// 创建命令响应
#[derive(Debug, serde::Deserialize)]
pub struct CommandCreateResponse {
    #[serde(rename = "commandId")]
    pub command_id: String,
}

/// 命令执行结果
#[derive(Debug, serde::Deserialize)]
pub struct CommandResult {
    #[serde(rename = "exitCode")]
    pub exit_code: i32,
    pub stdout: String,
    pub stderr: String,
}

pub struct E2BClient {
    http: reqwest::Client,
    api_url: String,
    api_key: String,
}

impl E2BClient {
    pub fn new(api_url: String, api_key: String) -> Self {
        Self {
            http: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(30))
                .build()
                .expect("reqwest client build"),
            api_url: api_url.trim_end_matches('/').to_string(),
            api_key,
        }
    }

    /// 创建沙箱
    pub async fn create_sandbox(
        &self,
        template_id: &str,
        timeout_secs: i32,
    ) -> Result<SandboxCreateResponse> {
        let resp = self
            .http
            .post(format!("{}/sandboxes", self.api_url))
            .header("Authorization", format!("Bearer {}", self.api_key))
            .json(&serde_json::json!({
                "templateID": template_id,
                "timeout": timeout_secs,
            }))
            .send()
            .await?;

        if !resp.status().is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!(
                "创建沙箱失败: HTTP {} - {}",
                resp.status(),
                body
            ));
        }

        let body: SandboxCreateResponse = resp.json().await?;
        Ok(body)
    }

    /// 执行命令（通过控制平面 REST API + 轮询）
    pub async fn exec_command(
        &self,
        sandbox_id: &str,
        command: &str,
        args: &[String],
    ) -> Result<CommandResult> {
        let mut full_cmd = command.to_string();
        for arg in args {
            full_cmd.push(' ');
            full_cmd.push_str(arg);
        }

        let resp = self
            .http
            .post(format!(
                "{}/sandboxes/{}/commands",
                self.api_url, sandbox_id
            ))
            .header("Authorization", format!("Bearer {}", self.api_key))
            .json(&serde_json::json!({
                "command": full_cmd,
            }))
            .send()
            .await?;

        if !resp.status().is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!(
                "执行命令失败: HTTP {} - {}",
                resp.status(),
                body
            ));
        }

        let cmd_resp: CommandCreateResponse = resp.json().await?;

        // 轮询等待命令完成（最多 60 次，每次 1 秒）
        for _ in 0..60 {
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            let status_resp = self
                .http
                .get(format!(
                    "{}/sandboxes/{}/commands/{}",
                    self.api_url, sandbox_id, cmd_resp.command_id
                ))
                .header("Authorization", format!("Bearer {}", self.api_key))
                .send()
                .await?;

            if !status_resp.status().is_success() {
                continue;
            }

            let result: CommandResult = status_resp.json().await?;
            if result.exit_code != -1 {
                return Ok(result);
            }
        }

        Err(anyhow::anyhow!("命令执行超时"))
    }

    /// 销毁沙箱
    pub async fn kill_sandbox(&self, sandbox_id: &str) -> Result<()> {
        let resp = self
            .http
            .delete(format!("{}/sandboxes/{}", self.api_url, sandbox_id))
            .header("Authorization", format!("Bearer {}", self.api_key))
            .send()
            .await?;

        if resp.status().is_success() || resp.status().is_client_error() {
            Ok(())
        } else {
            Err(anyhow::anyhow!("销毁沙箱失败: HTTP {}", resp.status()))
        }
    }

    /// 健康检查：3 秒超时探测 API 是否可达
    pub async fn health_check(api_url: &str) -> bool {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(3))
            .no_proxy()
            .build()
            .ok();

        if let Some(client) = client {
            client
                .get(format!("{}/", api_url.trim_end_matches('/')))
                .send()
                .await
                .map(|r| r.status().is_success())
                .unwrap_or(false)
        } else {
            false
        }
    }
}
