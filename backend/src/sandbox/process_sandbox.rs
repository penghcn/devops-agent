use std::collections::{HashMap, HashSet};
use std::process::Stdio;
use std::time::Duration;
use tokio::time::timeout;

/// 进程沙箱配置
pub struct ProcessSandboxConfig {
    pub timeout_secs: u64,
    pub max_output_bytes: usize,
    pub allowed_env_keys: Vec<String>,
}

/// 进程执行结果
#[derive(Debug, Clone)]
pub struct ProcessResult {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
    pub timed_out: bool,
    pub truncated: bool,
}

/// 进程沙箱，提供超时控制、环境变量净化、输出截断
pub struct ProcessSandbox {
    config: ProcessSandboxConfig,
}

impl Default for ProcessSandbox {
    fn default() -> Self {
        Self {
            config: ProcessSandboxConfig {
                timeout_secs: 30,
                max_output_bytes: 1_048_576, // 1MB
                allowed_env_keys: vec![
                    "PATH".into(),
                    "HOME".into(),
                    "LANG".into(),
                    "LC_ALL".into(),
                ],
            },
        }
    }
}

impl ProcessSandbox {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_config(config: ProcessSandboxConfig) -> Self {
        Self { config }
    }

    /// 执行命令并返回结果
    pub fn execute(&self, command: &str, args: &[String]) -> anyhow::Result<ProcessResult> {
        let rt = tokio::runtime::Runtime::new()?;
        rt.block_on(self.execute_async(command, args))
    }

    async fn execute_async(
        &self,
        command: &str,
        args: &[String],
    ) -> anyhow::Result<ProcessResult> {
        let allowed_keys: HashSet<String> =
            self.config.allowed_env_keys.iter().cloned().collect();
        let clean_env: HashMap<String, String> = std::env::vars()
            .filter(|(k, _)| allowed_keys.contains(k))
            .collect();

        let mut cmd = tokio::process::Command::new(command);
        cmd.args(args)
            .env_clear()
            .envs(&clean_env)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        let child = cmd.spawn()?;

        let result = timeout(
            Duration::from_secs(self.config.timeout_secs),
            child.wait_with_output(),
        )
        .await;

        match result {
            Ok(Ok(output)) => {
                let max = self.config.max_output_bytes;
                let truncated =
                    output.stdout.len() > max || output.stderr.len() > max;

                let stdout = build_output(&output.stdout, max);
                let stderr = build_output(&output.stderr, max);

                Ok(ProcessResult {
                    stdout,
                    stderr,
                    exit_code: output.status.code().unwrap_or(-1),
                    timed_out: false,
                    truncated,
                })
            }
            Ok(Err(e)) => Ok(ProcessResult {
                stdout: String::new(),
                stderr: e.to_string(),
                exit_code: -1,
                timed_out: false,
                truncated: false,
            }),
            Err(_) => Ok(ProcessResult {
                stdout: String::new(),
                stderr: String::from("timeout"),
                exit_code: -1,
                timed_out: true,
                truncated: false,
            }),
        }
    }
}

fn build_output(bytes: &[u8], max: usize) -> String {
    if bytes.len() > max {
        let s = String::from_utf8_lossy(&bytes[..max]);
        format!("{}[...truncated]", s)
    } else {
        String::from_utf8_lossy(bytes).to_string()
    }
}
