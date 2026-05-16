/// CubeSandbox 后端配置
#[derive(Debug, Clone)]
pub struct CubeSandboxConfig {
    pub api_url: String,
    pub api_key: String,
    pub template_id: String,
    pub timeout_secs: i32,
    pub allow_internet: bool,
}

impl Default for CubeSandboxConfig {
    fn default() -> Self {
        Self {
            api_url: String::new(),
            api_key: "dummy".to_string(),
            template_id: String::new(),
            timeout_secs: 1800,
            allow_internet: true,
        }
    }
}

impl CubeSandboxConfig {
    /// 配置是否完整（api_url 和 template_id 非空）
    pub fn is_complete(&self) -> bool {
        !self.api_url.is_empty() && !self.template_id.is_empty()
    }
}
