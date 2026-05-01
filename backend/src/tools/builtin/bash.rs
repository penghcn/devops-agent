use crate::sandbox::{NetworkCheckResult, NetworkWhitelist, ProcessSandbox};
use crate::security::policy::PolicyEngine;
use crate::security::roles::{PolicyDecision, ToolName, ToolRequest};

use super::{Tool, ToolInput, ToolOutput};

/// 基于进程沙箱的命令执行工具
pub struct BashTool {
    sandbox: ProcessSandbox,
    network_check: NetworkWhitelist,
    policy_engine: PolicyEngine,
}

impl BashTool {
    pub fn new(
        sandbox: ProcessSandbox,
        network_check: NetworkWhitelist,
        policy_engine: PolicyEngine,
    ) -> Self {
        Self {
            sandbox,
            network_check,
            policy_engine,
        }
    }
}

#[async_trait::async_trait]
impl Tool for BashTool {
    fn name(&self) -> &str {
        "Bash"
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
        let result = match self.sandbox.execute_async(cmd, &args_slice).await {
            Ok(r) => r,
            Err(e) => {
                return ToolOutput::fail(format!("命令执行失败: {}", e));
            }
        };

        let success = !result.timed_out && result.exit_code == 0;
        let mut output = result.stdout;
        if result.truncated {
            output.push_str(" [...truncated]");
        }

        let error = if result.timed_out {
            Some("命令执行超时".into())
        } else if !result.stderr.is_empty() {
            Some(result.stderr)
        } else {
            None
        };

        if success {
            ToolOutput::success(output)
        } else {
            ToolOutput::fail(error.unwrap_or_else(|| format!("exit code: {}", result.exit_code)))
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
