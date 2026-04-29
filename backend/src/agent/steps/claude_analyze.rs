use super::super::claude;
use super::super::step::{Step, StepContext, StepResult};

pub struct ClaudeAnalyzeStep;

impl Default for ClaudeAnalyzeStep {
    fn default() -> Self {
        Self
    }
}

#[async_trait::async_trait]
impl Step for ClaudeAnalyzeStep {
    fn name(&self) -> &str {
        "ClaudeAnalyze"
    }

    async fn execute(&self, ctx: &mut StepContext) -> StepResult {
        let log = match &ctx.build_log {
            Some(log) if !log.is_empty() => log,
            _ => {
                return StepResult::Abort {
                    reason: "没有可分析的构建日志".to_string(),
                };
            }
        };

        let result = ctx
            .pipeline_status
            .as_ref()
            .and_then(|s| s.get("result"))
            .and_then(|r| r.as_str())
            .unwrap_or("");

        let prompt = if result == "SUCCESS" {
            build_deploy_check_prompt(log)
        } else {
            build_failure_analysis_prompt(log, result)
        };

        match claude::call_claude_code(&prompt, "Bash,Read,Write,Grep,Glob").await {
            Ok(raw_result) => {
                // 尝试从 Claude 响应中提取 JSON 块
                let json_str = extract_json(&raw_result);
                match serde_json::from_str::<serde_json::Value>(json_str) {
                    Ok(structured) => {
                        ctx.structured_analysis = Some(structured.clone());
                        // 同时生成一段人类可读的文本
                        let human_text = format_structured_output(&structured);
                        ctx.analysis_result = Some(human_text);
                        StepResult::Success {
                            message: "分析完成".to_string(),
                        }
                    }
                    Err(_) => {
                        // 回退：原始文本
                        ctx.analysis_result = Some(raw_result);
                        StepResult::Success {
                            message: "分析完成".to_string(),
                        }
                    }
                }
            }
            Err(e) => StepResult::Failed {
                error: e.to_string(),
            },
        }
    }
}

fn extract_json(text: &str) -> &str {
    // 尝试从 ```json ... ``` 块中提取
    if let Some(start) = text.find("```json") {
        let after_marker = &text[start + 7..];
        if let Some(end) = after_marker.find("```") {
            return after_marker[..end].trim();
        }
    }
    // 尝试从 ``` ... ``` 块中提取
    if let Some(start) = text.find("```") {
        let after_marker = &text[start + 3..];
        if let Some(end) = after_marker.find("```") {
            return after_marker[..end].trim();
        }
    }
    // 尝试找最外层的 {} 块
    if let Some(start) = text.find('{')
        && let Some(end) = text.rfind('}')
    {
        return &text[start..end + 1];
    }
    text
}

fn format_structured_output(data: &serde_json::Value) -> String {
    match data.get("deploy_status") {
        Some(s) if s.as_str() == Some("success") => {
            let servers_array: Vec<&str> = data
                .get("servers")
                .and_then(|v| v.as_array())
                .map(|arr| arr.iter().filter_map(|v| v.as_str()).collect())
                .unwrap_or_default();
            let summary = data.get("summary").and_then(|v| v.as_str()).unwrap_or("");
            format!(
                "部署成功。目标服务器: {}\n摘要: {}",
                servers_array.join(", "),
                summary
            )
        }
        Some(s) if s.as_str() == Some("failed") => {
            let error = data
                .get("error")
                .and_then(|v| v.as_str())
                .unwrap_or("未知错误");
            let suggestion = data
                .get("suggestion")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            format!("部署失败: {}\n建议: {}", error, suggestion)
        }
        _ => {
            // 构建分析结果
            let build_status = data
                .get("build_status")
                .and_then(|v| v.as_str())
                .unwrap_or("UNKNOWN");
            let error = data.get("error").and_then(|v| v.as_str()).unwrap_or("");
            let suggestion = data
                .get("suggestion")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            format!(
                "构建状态: {}\n错误: {}\n建议: {}",
                build_status, error, suggestion
            )
        }
    }
}

fn build_deploy_check_prompt(log: &str) -> String {
    format!(
        "你是一个 DevOps 工程师。请分析以下 Jenkins 构建日志中的 SSH deploy 阶段，给出结构化结果。\n\n只输出 JSON，不要输出其他内容：\n```json\n{{\"deploy_status\":\"success|failed\",\"servers\":[\"服务器1\",\"服务器2\"],\"summary\":\"部署关键日志摘要\",\"error\":\"失败原因（如果成功则为空）\",\"suggestion\":\"后续操作建议\"}}\n```\n\n构建日志:\n{}",
        log
    )
}

fn build_failure_analysis_prompt(log: &str, result: &str) -> String {
    format!(
        "你是一个 DevOps 工程师。请分析以下构建结果，给出结构化结果。\n\n只输出 JSON，不要输出其他内容：\n```json\n{{\"build_status\":\"failed|success\",\"error\":\"失败原因分析\",\"suggestion\":\"修复建议\"}}\n```\n\n构建状态: {}\n构建日志:\n{}",
        result, log
    )
}
