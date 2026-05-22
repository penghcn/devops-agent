//! 适配器 — 将 builtin Tool trait 适配为 ToolUseLoop 的 ToolExecutor

use crate::llm::tool_use_loop::{ToolCallResult, ToolExecutor, ToolRegistration};
use crate::security::roles::Role;

use super::{Tool, ToolInput, ToolOutput};

/// 注册所有内置简单工具（不依赖沙箱的工具）到 ToolExecutor。
///
/// 这些工具不需要沙箱、策略引擎等基础设施，适合在 ToolUseLoop 中直接使用。
pub fn register_all_builtin(executor: &mut ToolExecutor) {
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
        ToolRegistration::safe(move |args| {
            let args = args.clone();
            async move {
                let config = crate::config::Config::test_default();
                let tool = super::GetConfigTool::new(&config);
                let input = to_tool_input(&args);
                let output = tool.execute(&input).await;
                to_call_result(output)
            }
        }),
    );
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
