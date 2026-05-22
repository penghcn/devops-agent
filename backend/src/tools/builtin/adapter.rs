//! 适配器 — 将 builtin Tool trait 适配为 ToolUseLoop 的 ToolExecutor

use std::path::PathBuf;
use std::sync::Arc;

use crate::llm::ToolDefinition;
use crate::llm::tool_use_loop::{ToolCallResult, ToolExecutor, ToolRegistration};
use crate::sandbox::{
    FileSystemIsolator, FsIsolationConfig, NetworkWhitelist, PathValidator, ProcessBackend,
    Sandbox, SandboxFactory,
};
use crate::security::audit::AuditLog;
use crate::security::policy::PolicyEngine;
use crate::security::roles::Role;

use super::{Tool, ToolInput, ToolOutput};

/// 注册所有内置简单工具（不依赖沙箱的工具）到 ToolExecutor。
///
/// 这些工具不需要沙箱、策略引擎等基础设施，适合在 ToolUseLoop 中直接使用。
pub fn register_all_builtin(executor: &mut ToolExecutor, config: &crate::config::Config) {
    let config = Arc::new(config.clone());
    executor.register(
        "get_time",
        ToolRegistration::safe(|args| {
            let args = args.clone();
            async move {
                let tool = super::GetTimeTool::new();
                let input = to_tool_input(&args);
                let output = tool.execute(&input).await;
                to_call_result(output)
            }
        }),
    );

    executor.register(
        "get_env",
        ToolRegistration::safe(|args| {
            let args = args.clone();
            async move {
                let tool = super::GetEnvTool::new();
                let input = to_tool_input(&args);
                let output = tool.execute(&input).await;
                to_call_result(output)
            }
        }),
    );

    executor.register(
        "get_config",
        ToolRegistration::safe({
            let config = config.clone();
            move |args| {
                let args = args.clone();
                let config = config.clone();
                async move {
                    let tool = super::GetConfigTool::new(&config);
                    let input = to_tool_input(&args);
                    let output = tool.execute(&input).await;
                    to_call_result(output)
                }
            }
        }),
    );
}

/// 创建默认沙箱实例（Process 降级）
fn create_default_sandbox() -> Arc<dyn Sandbox> {
    Arc::new(ProcessBackend::new())
}

/// 注册重型工具（Read/Write/Bash/Git）到 ToolExecutor。
///
/// 这些工具需要沙箱、策略引擎等基础设施。
pub fn register_heavy_tools(executor: &mut ToolExecutor) {
    let workspace_root = std::env::current_dir()
        .unwrap_or_else(|_| PathBuf::from("/workspace"))
        .to_string_lossy()
        .to_string();

    // 构造公共依赖（用 Arc 包装以便在 Fn 闭包中共享）
    let validator = Arc::new(PathValidator::new(&workspace_root));
    let isolator = Arc::new(FileSystemIsolator::new(FsIsolationConfig {
        workspace_root: PathBuf::from(&workspace_root),
        tmp_dir: PathBuf::from("/tmp"),
        output_dir: PathBuf::from(&workspace_root),
        read_only_mounts: Vec::new(),
    }));
    let audit_log = Arc::new(AuditLog::new());
    let policy_engine = Arc::new(PolicyEngine::new(audit_log));

    // 注册 Read
    executor.register(
        "Read",
        ToolRegistration::safe({
            let v = validator.clone();
            let i = isolator.clone();
            let p = policy_engine.clone();
            move |args| {
                let args = args.clone();
                let v_inner = v.as_ref().clone();
                let i_inner = i.as_ref().clone();
                let p_inner = p.as_ref().clone();
                async move {
                    let tool = super::ReadTool::new(v_inner, i_inner, p_inner);
                    let input = to_tool_input(&args);
                    let output = tool.execute(&input).await;
                    to_call_result(output)
                }
            }
        }),
    );

    // 注册 Write
    executor.register(
        "Write",
        ToolRegistration::safe({
            let v = validator.clone();
            let i = isolator.clone();
            let p = policy_engine.clone();
            move |args| {
                let args = args.clone();
                let v_inner = v.as_ref().clone();
                let i_inner = i.as_ref().clone();
                let p_inner = p.as_ref().clone();
                async move {
                    let tool = super::WriteTool::new(v_inner, i_inner, p_inner);
                    let input = to_tool_input(&args);
                    let output = tool.execute(&input).await;
                    to_call_result(output)
                }
            }
        }),
    );

    // 注册 Bash
    executor.register(
        "Bash",
        ToolRegistration::safe({
            let p = policy_engine.clone();
            move |args| {
                let args = args.clone();
                let p_inner = p.as_ref().clone();
                async move {
                    let factory = SandboxFactory::new();
                    let sandbox: Arc<dyn Sandbox> = factory
                        .create()
                        .unwrap_or_else(|_| create_default_sandbox());
                    let tool = super::BashTool::new(sandbox, NetworkWhitelist::new(), p_inner);
                    let input = to_tool_input(&args);
                    let output = tool.execute(&input).await;
                    to_call_result(output)
                }
            }
        }),
    );

    // 注册 Git
    executor.register(
        "Git",
        ToolRegistration::safe({
            let p = policy_engine.clone();
            move |args| {
                let args = args.clone();
                let p_inner = p.as_ref().clone();
                async move {
                    let factory = SandboxFactory::new();
                    let sandbox: Arc<dyn Sandbox> = factory
                        .create()
                        .unwrap_or_else(|_| create_default_sandbox());
                    let tool = super::GitTool::new(sandbox, p_inner);
                    let input = to_tool_input(&args);
                    let output = tool.execute(&input).await;
                    to_call_result(output)
                }
            }
        }),
    );
}

/// 返回重型工具的 LLM 定义（Read/Write/Bash/Git）
pub fn get_heavy_tool_definitions() -> Vec<ToolDefinition> {
    let workspace_root = std::env::current_dir()
        .unwrap_or_else(|_| PathBuf::from("/workspace"))
        .to_string_lossy()
        .to_string();

    let validator = PathValidator::new(&workspace_root);
    let isolator = FileSystemIsolator::new(FsIsolationConfig {
        workspace_root: PathBuf::from(&workspace_root),
        tmp_dir: PathBuf::from("/tmp"),
        output_dir: PathBuf::from(&workspace_root),
        read_only_mounts: Vec::new(),
    });
    let audit_log = Arc::new(AuditLog::new());
    let policy_engine = PolicyEngine::new(audit_log);
    let sandbox: Arc<dyn Sandbox> = Arc::new(ProcessBackend::new());

    let mut defs = Vec::new();
    defs.push(
        super::ReadTool::new(validator.clone(), isolator.clone(), policy_engine.clone())
            .definition(),
    );
    defs.push(
        super::WriteTool::new(validator.clone(), isolator.clone(), policy_engine.clone())
            .definition(),
    );
    defs.push(
        super::BashTool::new(
            sandbox.clone(),
            NetworkWhitelist::new(),
            policy_engine.clone(),
        )
        .definition(),
    );
    defs.push(super::GitTool::new(sandbox, policy_engine).definition());
    defs
}

/// 将 LLM 的工具调用参数转换为 ToolInput
fn to_tool_input(args: &serde_json::Value) -> ToolInput {
    let obj = args.as_object();
    let mut arguments = Vec::new();
    let mut path = None;
    let mut content = None;

    if let Some(obj) = obj {
        if let Some(p) = obj.get("path") {
            path = Some(p.as_str().unwrap_or("").to_string());
        }
        if let Some(c) = obj.get("content") {
            content = Some(c.as_str().unwrap_or("").to_string());
        }
        if let Some(cmd) = obj.get("command") {
            arguments.push(cmd.as_str().unwrap_or("").to_string());
        }
        if let Some(subcmd) = obj.get("subcommand") {
            arguments.push(subcmd.as_str().unwrap_or("").to_string());
        }
        if let Some(name) = obj.get("name") {
            arguments.push(name.as_str().unwrap_or("").to_string());
        }
        // 通用 args 字段
        if let Some(arr) = obj.get("args").and_then(|v| v.as_array()) {
            for item in arr {
                if let Some(s) = item.as_str() {
                    arguments.push(s.to_string());
                }
            }
        }
    }

    ToolInput {
        path,
        content,
        arguments,
        user_role: Role::Admin,
    }
}

/// 将 ToolOutput 转换为 ToolCallResult
fn to_call_result(output: ToolOutput) -> ToolCallResult {
    if output.success {
        ToolCallResult::Ok(output.result)
    } else {
        ToolCallResult::Err(output.error.unwrap_or_else(|| "执行失败".to_string()))
    }
}
