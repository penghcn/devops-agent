use serde::{Deserialize, Serialize};

pub mod bash;
pub mod git;
pub mod read;
pub mod write;

pub use bash::BashTool;
pub use git::GitTool;
pub use read::ReadTool;
pub use write::WriteTool;

use crate::security::roles::Role;

/// 工具输入参数
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolInput {
    /// 目标文件路径
    pub path: Option<String>,
    /// 写入内容（WriteTool 用）
    pub content: Option<String>,
    /// 命令参数（BashTool/GitTool 用）
    pub arguments: Vec<String>,
    /// 调用者角色
    pub user_role: Role,
}

/// 工具执行输出
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolOutput {
    /// 是否成功
    pub success: bool,
    /// 结果内容
    pub result: String,
    /// 错误信息（失败时）
    pub error: Option<String>,
}

impl ToolOutput {
    pub fn success(result: String) -> Self {
        Self {
            success: true,
            result,
            error: None,
        }
    }

    pub fn fail(error: String) -> Self {
        Self {
            success: false,
            result: String::new(),
            error: Some(error),
        }
    }
}

/// 工具执行接口
#[async_trait::async_trait]
pub trait Tool {
    /// 工具名称
    fn name(&self) -> &str;

    /// 执行工具
    async fn execute(&self, input: &ToolInput) -> ToolOutput;
}
