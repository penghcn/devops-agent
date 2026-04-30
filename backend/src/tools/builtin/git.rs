use crate::sandbox::ProcessSandbox;
use crate::security::policy::PolicyEngine;
use crate::security::roles::{PolicyDecision, Role, ToolName, ToolRequest};

use super::{Tool, ToolInput, ToolOutput};

/// Git 操作封装工具
pub struct GitTool {
    sandbox: ProcessSandbox,
    policy_engine: PolicyEngine,
    /// 禁止的 git 子命令
    denied_commands: Vec<String>,
}

impl GitTool {
    pub fn new(sandbox: ProcessSandbox, policy_engine: PolicyEngine) -> Self {
        Self {
            sandbox,
            policy_engine,
            denied_commands: default_denied_commands(),
        }
    }
}

/// 默认禁止的 git 子命令列表
fn default_denied_commands() -> Vec<String> {
    vec![
        "push".into(),
        "remote".into(),
        "fetch".into(),
        "clone".into(),
        "submodule".into(),
    ]
}

#[async_trait::async_trait]
impl Tool for GitTool {
    fn name(&self) -> &str {
        "Git"
    }

    async fn execute(&self, input: &ToolInput) -> ToolOutput {
        if input.arguments.is_empty() {
            return ToolOutput::fail("缺少 git 子命令".into());
        }

        let subcommand = &input.arguments[0];

        // 策略检查
        let request = ToolRequest::new(
            input.user_role,
            ToolName::Git,
            input.path.clone(),
            input.arguments.clone(),
        );
        let decision = self.policy_engine.check(&request);
        if decision == PolicyDecision::Deny {
            return ToolOutput::fail("策略拒绝：无权执行 Git 命令".into());
        }

        // Viewer 只能执行只读命令
        if input.user_role == Role::Viewer && !is_readonly_command(subcommand) {
            return ToolOutput::fail(format!("Viewer 角色不允许执行 git {}", subcommand));
        }

        // 检查禁止命令
        if self.denied_commands.contains(subcommand) {
            return ToolOutput::fail(format!("禁止的 git 子命令: {}", subcommand));
        }

        // 构建 git 命令参数
        let mut git_args = vec![subcommand.clone()];
        if input.arguments.len() > 1 {
            git_args.extend(input.arguments[1..].iter().cloned());
        }

        // 执行 git 命令
        let result = match self.sandbox.execute_async("git", &git_args).await {
            Ok(r) => r,
            Err(e) => {
                return ToolOutput::fail(format!("git 执行失败: {}", e));
            }
        };

        let success = !result.timed_out && result.exit_code == 0;
        let mut output = result.stdout;
        if result.truncated {
            output.push_str(" [...truncated]");
        }

        let error = if result.timed_out {
            Some("git 命令执行超时".into())
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

/// 检查是否为只读 git 命令
fn is_readonly_command(cmd: &str) -> bool {
    matches!(
        cmd,
        "status" | "log" | "diff" | "show" | "branch" | "tag" | "describe" | "rev-parse"
    )
}
