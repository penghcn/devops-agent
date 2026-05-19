use anyhow::Result;

/// 创建沙箱响应（控制平面）
#[derive(Debug, serde::Deserialize)]
pub struct SandboxCreateResponse {
    #[serde(rename = "sandboxID")]
    pub sandbox_id: String,
    #[serde(rename = "templateID")]
    pub template_id: String,
    /// envd 访问令牌，用于数据平面通信
    #[serde(rename = "envdAccessToken", default)]
    pub envd_access_token: Option<String>,
}

/// 沙箱详细信息（用于 connect/resume 后获取 envd_access_token）
#[derive(Debug, serde::Deserialize)]
pub struct SandboxInfo {
    #[serde(rename = "sandboxID")]
    pub sandbox_id: String,
    #[serde(rename = "envdAccessToken", default)]
    pub envd_access_token: Option<String>,
    /// envd 服务地址（通过 CubeProxy 路由）
    /// 格式: {port}-{sandboxID}.{domain}
    #[serde(default)]
    pub sandbox_domain: Option<String>,
}

/// 命令执行结果（从 Process/Start 流式响应中聚合）
#[derive(Debug)]
pub struct CommandResult {
    pub exit_code: i32,
    pub stdout: String,
    pub stderr: String,
}

/// 控制平面客户端 — 与 CubeAPI 交互（创建/销毁/查询沙箱）
pub struct ControlPlaneClient {
    http: reqwest::Client,
    api_url: String,
    api_key: String,
}

/// 数据平面客户端 — 与 envd 交互（命令执行/文件操作）
#[derive(Debug)]
pub struct EnvdClient {
    http: reqwest::Client,
    /// envd 端点地址，通过 CubeProxy 路由
    /// 自托管时格式: http://{port}-{sandboxID}.{domain}
    envd_url: String,
    /// envd 访问令牌（创建沙箱时获取）
    access_token: String,
}

impl ControlPlaneClient {
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

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!("创建沙箱失败: HTTP {} - {}", status, body));
        }

        let body: SandboxCreateResponse = resp.json().await?;
        Ok(body)
    }

    /// 获取沙箱详情（获取 envd_access_token）
    pub async fn get_sandbox(&self, sandbox_id: &str) -> Result<SandboxInfo> {
        let resp = self
            .http
            .get(format!("{}/sandboxes/{}", self.api_url, sandbox_id))
            .header("Authorization", format!("Bearer {}", self.api_key))
            .send()
            .await?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!(
                "获取沙箱详情失败: HTTP {} - {}",
                status,
                body
            ));
        }

        let body: SandboxInfo = resp.json().await?;
        Ok(body)
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

impl EnvdClient {
    /// 创建 envd 客户端
    /// - envd_url: envd 端点地址（通过 CubeProxy），如 http://49983-{sandboxID}.{domain}
    /// - access_token: 从创建沙箱响应中获取的 envd_access_token
    pub fn new(envd_url: String, access_token: String) -> Self {
        Self {
            http: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(120))
                .build()
                .expect("reqwest client build"),
            envd_url: envd_url.trim_end_matches('/').to_string(),
            access_token,
        }
    }

    /// 执行命令 — 通过 Connect RPC `connect+json` 协议调用 Process/Start
    ///
    /// Process/Start 是服务端流式 RPC，响应格式为：
    /// - 二进制帧: 1 字节 flags + 4 字节大端长度 + JSON body
    /// - 第一个事件: StartEvent { pid }
    /// - 中间事件: DataEvent { stdout / stderr / pty }
    /// - 最后事件: EndEvent { exit_code, exited } 或 flags=0x80 结束帧
    pub async fn exec_command(&self, command: &str, args: &[String]) -> Result<CommandResult> {
        // 构建 ProcessConfig — E2B SDK 通过 /bin/bash -c 执行完整命令
        let mut full_cmd = command.to_string();
        for arg in args {
            full_cmd.push(' ');
            full_cmd.push_str(arg);
        }

        let request_body = serde_json::json!({
            "process": {
                "cmd": "/bin/bash",
                "args": ["-l", "-c", &full_cmd],
            },
            "stdin": false,
        });

        let resp = self
            .http
            .post(format!("{}/process.Process/Start", self.envd_url))
            .header("Content-Type", "application/connect+json")
            .header("Connect-Protocol-Version", "1")
            .header("X-Access-Token", self.access_token.to_string())
            .body(request_body.to_string())
            .send()
            .await?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!("执行命令失败: HTTP {} - {}", status, body));
        }

        // 解析 connect+json 流式响应
        Self::parse_connect_json_stream(resp, command).await
    }

    /// 解析 Connect RPC `connect+json` 服务端流式响应
    async fn parse_connect_json_stream(
        resp: reqwest::Response,
        _command: &str,
    ) -> Result<CommandResult> {
        let mut stdout = String::new();
        let mut stderr = String::new();
        let mut exit_code: i32 = -1;

        // 读取整个响应体
        let bytes = resp.bytes().await?;
        let mut data = bytes.as_ref();

        while !data.is_empty() {
            // 解析帧: 1 字节 flags + 4 字节大端长度 + JSON body
            if data.len() < 5 {
                break;
            }

            let flags = data[0];
            let length = u32::from_be_bytes([data[1], data[2], data[3], data[4]]) as usize;
            data = &data[5..];

            if data.len() < length {
                break;
            }

            let json_bytes = &data[..length];
            data = &data[length..];

            // flags=0x80 是结束帧，包含错误信息
            if flags & 0x80 != 0 {
                if length > 0 {
                    if let Ok(error_resp) = serde_json::from_slice::<serde_json::Value>(json_bytes)
                    {
                        if let Some(msg) = error_resp.get("message").and_then(|m| m.as_str()) {
                            if !msg.is_empty() {
                                tracing::debug!("Connect RPC error trailer: {}", msg);
                            }
                        }
                    }
                }
                break;
            }

            // 解析 ProcessEvent
            if let Ok(event) = serde_json::from_slice::<serde_json::Value>(json_bytes) {
                if let Some(peek_event) = event.get("event") {
                    // start 事件
                    if let Some(_start) = peek_event.get("start") {
                        // PID 信息，暂不处理
                    }
                    // data 事件
                    else if let Some(data_event) = peek_event.get("data") {
                        if let Some(stdout_data) = data_event.get("stdout") {
                            if let Some(raw) = stdout_data.as_array() {
                                let mut bytes = Vec::new();
                                for v in raw {
                                    if let Some(n) = v.as_u64() {
                                        bytes.push(n as u8);
                                    }
                                }
                                stdout.push_str(&String::from_utf8_lossy(&bytes));
                            }
                        }
                        if let Some(stderr_data) = data_event.get("stderr") {
                            if let Some(raw) = stderr_data.as_array() {
                                let mut bytes = Vec::new();
                                for v in raw {
                                    if let Some(n) = v.as_u64() {
                                        bytes.push(n as u8);
                                    }
                                }
                                stderr.push_str(&String::from_utf8_lossy(&bytes));
                            }
                        }
                    }
                    // end 事件
                    else if let Some(end_event) = peek_event.get("end") {
                        if let Some(code) = end_event.get("exitCode") {
                            exit_code = code.as_i64().unwrap_or(-1) as i32;
                        }
                        if let Some(exited) = end_event.get("exited") {
                            if exited.as_bool().unwrap_or(false) {
                                // 进程已退出
                            }
                        }
                    }
                }
            }
        }

        Ok(CommandResult {
            stdout,
            stderr,
            exit_code,
        })
    }
}

// 保留旧名称作为别名，向后兼容
pub type E2BClient = ControlPlaneClient;
