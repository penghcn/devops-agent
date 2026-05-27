use crate::sandbox::{NetworkCheckResult, NetworkWhitelist, Sandbox, SandboxFactory};
use crate::security::policy::PolicyEngine;
use crate::security::roles::{PolicyDecision, ToolName, ToolRequest};
use std::sync::Arc;

use super::{Tool, ToolInput, ToolOutput};

/// 基于沙箱的命令执行工具
pub struct BashTool {
    sandbox: Arc<dyn Sandbox>,
    network_check: NetworkWhitelist,
    policy_engine: PolicyEngine,
}

impl BashTool {
    pub fn new(
        sandbox: Arc<dyn Sandbox>,
        network_check: NetworkWhitelist,
        policy_engine: PolicyEngine,
    ) -> Self {
        Self {
            sandbox,
            network_check,
            policy_engine,
        }
    }

    /// 从工厂创建 BashTool（便捷方法）
    pub fn from_factory(
        factory: &SandboxFactory,
        network_check: NetworkWhitelist,
        policy_engine: PolicyEngine,
    ) -> anyhow::Result<Self> {
        let sandbox = factory.create()?;
        Ok(Self::new(sandbox, network_check, policy_engine))
    }
}

#[async_trait::async_trait]
impl Tool for BashTool {
    fn name(&self) -> &str {
        "Bash"
    }

    fn definition(&self) -> crate::llm::ToolDefinition {
        crate::llm::ToolDefinition {
            name: "Bash".to_string(),
            description: "在沙箱中执行 Shell 命令。仅允许白名单命令。".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "command": {"type": "string", "description": "要执行的命令"},
                    "args": {"type": "array", "items": {"type": "string"}, "description": "命令参数"}
                },
                "required": ["command"]
            }),
            cache_control: None,
        }
    }

    async fn execute(&self, input: &ToolInput) -> ToolOutput {
        if input.arguments.is_empty() {
            return ToolOutput::fail("缺少命令参数".into());
        }

        // 策略检查
        let request = ToolRequest::new(
            input.user_role,
            ToolName::Bash,
            input.path.clone(),
            input.arguments.clone(),
        );
        let decision = self.policy_engine.check(&request);
        match decision {
            PolicyDecision::Allow => {}
            PolicyDecision::Deny => {
                return ToolOutput::fail("策略拒绝：无权执行 Bash 命令".into());
            }
            PolicyDecision::Prompt => {
                return ToolOutput::fail("策略拦截：Bash 命令需要人工确认".into());
            }
        }

        let cmd = &input.arguments[0];

        // 命令白名单检查：防止 PATH 劫持
        let cmd_name = cmd.split('/').next_back().unwrap_or(cmd).to_lowercase();
        if !is_allowed_command(&cmd_name) {
            return ToolOutput::fail(format!("命令不在允许列表中: {}", cmd));
        }

        // 网络白名单检查
        let args_slice: Vec<String> = if input.arguments.len() > 1 {
            input.arguments[1..].to_vec()
        } else {
            Vec::new()
        };
        if self.network_check.check(cmd, &args_slice) == NetworkCheckResult::Blocked {
            return ToolOutput::fail(format!("网络命令被拦截: {}", cmd));
        }

        // 执行命令
        let result = match self.sandbox.exec(cmd, &args_slice).await {
            Ok(r) => r,
            Err(e) => {
                return ToolOutput::fail(format!("命令执行失败: {}", e));
            }
        };

        let success = result.exit_code == 0;
        let output = result.stdout;

        let error = if !result.stderr.is_empty() {
            Some(result.stderr)
        } else if !success {
            Some(format!("exit code: {}", result.exit_code))
        } else {
            None
        };

        if success {
            ToolOutput::success(output)
        } else {
            ToolOutput::fail(error.unwrap_or_default())
        }
    }
}

/// 检查命令是否在允许列表中（防止 PATH 劫持）
fn is_allowed_command(cmd: &str) -> bool {
    matches!(
        cmd,
        "ls" | "cat"
            | "echo"
            | "grep"
            | "find"
            | "diff"
            | "wc"
            | "head"
            | "tail"
            | "sort"
            | "uniq"
            | "tr"
            | "cut"
            | "sed"
            | "awk"
            | "basename"
            | "dirname"
            | "pwd"
            | "mkdir"
            | "touch"
            | "rm"
            | "cp"
            | "mv"
            | "ln"
            | "chmod"
            | "chown"
            | "stat"
            | "file"
            | "du"
            | "df"
            | "tar"
            | "zip"
            | "unzip"
            | "gzip"
            | "gunzip"
            | "md5sum"
            | "sha256sum"
            | "xxd"
            | "hexdump"
            | "env"
            | "printenv"
            | "which"
            | "type"
            | "test"
            | "true"
            | "false"
            | "sleep"
            | "date"
            | "id"
            | "whoami"
            | "uname"
            | "hostname"
            | "ps"
            | "kill"
            | "pgrep"
            | "pkill"
            | "tree"
            | "less"
            | "more"
            | "column"
            | "join"
            | "paste"
            | "comm"
            | "shuf"
            | "nl"
            | "fold"
    )
}
