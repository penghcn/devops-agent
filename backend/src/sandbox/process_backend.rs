use anyhow::Result;

use super::process_sandbox::{ProcessSandbox, ProcessSandboxConfig};
use super::trait_sandbox::{ExecResult, Sandbox};

/// 使用现有进程沙箱的 `Sandbox` trait 实现（降级方案）
pub struct ProcessBackend {
    inner: ProcessSandbox,
}

impl ProcessBackend {
    pub fn new() -> Self {
        Self {
            inner: ProcessSandbox::new(),
        }
    }

    pub fn with_config(config: ProcessSandboxConfig) -> Self {
        Self {
            inner: ProcessSandbox::with_config(config),
        }
    }
}

#[async_trait::async_trait]
impl Sandbox for ProcessBackend {
    async fn exec(&self, command: &str, args: &[String]) -> Result<ExecResult> {
        let result = self.inner.execute_async(command, args).await?;

        Ok(ExecResult {
            stdout: result.stdout,
            stderr: result.stderr,
            exit_code: result.exit_code,
        })
    }

    async fn read_file(&self, path: &str) -> Result<String> {
        let result = self.exec("cat", &[path.to_string()]).await?;
        if result.exit_code == 0 {
            Ok(result.stdout)
        } else {
            return Err(anyhow::anyhow!("cat {} failed: {}", path, result.stderr));
        }
    }

    async fn write_file(&self, path: &str, content: &str) -> Result<()> {
        let cmd = format!("printf '%s' '{}' > '{}'", content, path);
        let result = self.exec("sh", &["-c".into(), cmd]).await?;
        if result.exit_code == 0 {
            Ok(())
        } else {
            return Err(anyhow::anyhow!("write {} failed: {}", path, result.stderr));
        }
    }

    async fn stop(&self) -> Result<()> {
        Ok(())
    }
}
