/// CubeSandbox 后端配置
#[derive(Debug, Clone)]
pub struct CubeSandboxConfig {
    /// 控制平面 API 地址（CubeAPI），如 http://127.0.0.1:3000
    pub api_url: String,
    /// API 密钥（自托管时可用 dummy）
    pub api_key: String,
    /// 沙箱模板 ID
    pub template_id: String,
    /// 沙箱超时时间（秒），默认 1800
    pub timeout_secs: i32,
    /// 是否允许沙箱访问互联网
    pub allow_internet: bool,
    /// envd 端口，默认 49983
    pub envd_port: u16,
    /// envd 端点地址模板，占位符 {sandbox_id} 会被替换。
    /// 留空则自动从 api_url 推导: http://{envd_port}-{sandbox_id}.{api_host}
    pub envd_url_template: String,
}

impl Default for CubeSandboxConfig {
    fn default() -> Self {
        Self {
            api_url: String::new(),
            api_key: "dummy".to_string(),
            template_id: String::new(),
            timeout_secs: 1800,
            allow_internet: true,
            envd_port: 49983,
            envd_url_template: String::new(),
        }
    }
}

impl CubeSandboxConfig {
    /// 配置是否完整（api_url 和 template_id 非空）
    pub fn is_complete(&self) -> bool {
        !self.api_url.is_empty() && !self.template_id.is_empty()
    }
}
