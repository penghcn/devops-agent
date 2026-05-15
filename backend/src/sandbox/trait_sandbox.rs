use anyhow::Result;

/// 沙箱执行结果
#[derive(Debug, Clone)]
pub struct ExecResult {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
}

/// 沙箱统一接口
#[async_trait::async_trait]
pub trait Sandbox: Send + Sync {
    /// 执行命令
    async fn exec(&self, command: &str, args: &[String]) -> Result<ExecResult>;

    /// 读取沙箱内文件内容
    async fn read_file(&self, path: &str) -> Result<String>;

    /// 写入文件到沙箱内
    async fn write_file(&self, path: &str, content: &str) -> Result<()>;

    /// 停止并销毁沙箱
    async fn stop(&self) -> Result<()>;
}
