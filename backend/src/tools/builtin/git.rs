use crate::sandbox::{Sandbox, SandboxFactory};
use crate::security::policy::PolicyEngine;
use crate::security::roles::{PolicyDecision, Role, ToolName, ToolRequest};
use std::sync::Arc;

use super::{Tool, ToolInput, ToolOutput};

/// Git 操作封装工具
pub struct GitTool {
    sandbox: Arc<dyn Sandbox>,
    policy_engine: PolicyEngine,
    /// 禁止的 git 子命令
    denied_commands: Vec<String>,
}

impl GitTool {
    pub fn new(sandbox: Arc<dyn Sandbox>, policy_engine: PolicyEngine) -> Self {
        Self {
            sandbox,
            policy_engine,
            denied_commands: default_denied_commands(),
        }
    }

    /// 从工厂创建 GitTool（便捷方法）
    pub fn from_factory(
        factory: &SandboxFactory,
        policy_engine: PolicyEngine,
    ) -> anyhow::Result<Self> {
        let sandbox = factory.create()?;
        Ok(Self::new(sandbox, policy_engine))
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

    fn definition(&self) -> crate::llm::ToolDefinition {
        crate::llm::ToolDefinition {
            name: "Git".to_string(),
            description: "执行 Git 操作。禁止 push/remote/fetch/clone/submodule。".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "subcommand": {"type": "string", "description": "Git 子命令"},
                    "args": {"type": "array", "items": {"type": "string"}, "description": "额外参数"}
                },
                "required": ["subcommand"]
            }),
            cache_control: None,
        }
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
        match decision {
            PolicyDecision::Allow => {}
            PolicyDecision::Deny => {
                return ToolOutput::fail("策略拒绝：无权执行 Git 命令".into());
            }
            PolicyDecision::Prompt => {
                return ToolOutput::fail("策略拦截：Git 命令需要人工确认".into());
            }
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
        let result = match self.sandbox.exec("git", &git_args).await {
            Ok(r) => r,
            Err(e) => {
                return ToolOutput::fail(format!("git 执行失败: {}", e));
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

/// 检查是否为只读 git 命令
fn is_readonly_command(cmd: &str) -> bool {
    matches!(
        cmd,
        "status" | "log" | "diff" | "show" | "branch" | "tag" | "describe" | "rev-parse"
    )
}
