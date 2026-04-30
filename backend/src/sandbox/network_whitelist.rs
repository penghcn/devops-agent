/// 网络检查结果
#[derive(Debug, Clone, PartialEq)]
pub enum NetworkCheckResult {
    /// 非网络命令或已在白名单
    Allowed,
    /// 网络命令被拦截
    Blocked,
}

/// 网络白名单，拦截网络命令
pub struct NetworkWhitelist {
    blocked_commands: Vec<String>,
    pub allowed_hosts: Vec<String>,
}

impl Default for NetworkWhitelist {
    fn default() -> Self {
        Self {
            blocked_commands: default_blocked_commands(),
            allowed_hosts: Vec::new(),
        }
    }
}

impl NetworkWhitelist {
    pub fn new() -> Self {
        Self::default()
    }

    /// 检查命令是否需要拦截
    pub fn check(&self, command: &str, _args: &[String]) -> NetworkCheckResult {
        let cmd_name = command
            .split('/')
            .last()
            .unwrap_or(command)
            .to_lowercase();

        if self.blocked_commands.contains(&cmd_name) {
            return NetworkCheckResult::Blocked;
        }

        NetworkCheckResult::Allowed
    }

    /// 添加允许的主机
    pub fn allow_host(&mut self, host: &str) {
        if !self.allowed_hosts.contains(&host.to_string()) {
            self.allowed_hosts.push(host.to_string());
        }
    }
}

fn default_blocked_commands() -> Vec<String> {
    vec![
        "curl".into(),
        "wget".into(),
        "ssh".into(),
        "scp".into(),
        "nc".into(),
        "netcat".into(),
        "telnet".into(),
        "ftp".into(),
        "sftp".into(),
    ]
}
