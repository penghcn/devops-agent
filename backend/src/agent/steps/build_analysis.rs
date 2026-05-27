use super::super::claude;
use super::super::step::{Step, StepContext, StepResult};
use crate::llm::{LlmProvider, PromptBuilder};
use std::sync::Arc;
use std::time::Instant;

//#[derive(Default)]
pub struct BuildAnalysisStep {
    llm_provider: Arc<dyn LlmProvider>,
    llm_model: String,
}

impl BuildAnalysisStep {
    pub fn with_provider(provider: Arc<dyn LlmProvider>, model: String) -> Self {
        Self {
            llm_provider: provider,
            llm_model: model,
        }
    }
}

#[async_trait::async_trait]
impl Step for BuildAnalysisStep {
    fn name(&self) -> &str {
        "BuildAnalysis"
    }

    fn description(&self, _ctx: &StepContext) -> String {
        format!("AI 分析构建结果 ({})", &self.llm_model)
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

        // let raw_result = if let Some(provider) = &self.llm_provider {
        //     let model = self
        //         .llm_model
        //         .as_deref()
        //         .unwrap_or("gpt-4o-mini")
        //         .to_string();
        //     match provider
        //         .llm_call(&ChatRequest {
        //             model,
        //             messages: vec![Message::User {
        //                 content: prompt.clone(),
        //             }],
        //             tools: None,
        //             temperature: Some(0.0),
        //             tool_choice: None,
        //             stop_sequences: None,
        //             prefill: None,
        //         })
        //         .await
        //     {
        //         Ok(response) => response.content,
        //         Err(e) => {
        //             tracing::warn!(error = %e, model = %self.llm_model.as_deref().unwrap_or("default"), "LlmProvider failed, falling back to Claude Code CLI");
        //             match claude::call_claude_code(&prompt, "Bash,Read,Write,Grep,Glob").await {
        //                 Ok(r) => r,
        //                 Err(e) => {
        //                     return StepResult::Failed {
        //                         error: e.to_string(),
        //                     };
        //                 }
        //             }
        //         }
        //     }
        // } else {
        //     match claude::call_claude_code(&prompt, "Bash,Read,Write,Grep,Glob").await {
        //         Ok(r) => r,
        //         Err(e) => {
        //             return StepResult::Failed {
        //                 error: e.to_string(),
        //             };
        //         }
        //     }
        // };

        let raw_result = match call(&self.llm_provider, &self.llm_model, &prompt).await {
            Ok(r) => r,
            Err(e) => {
                return StepResult::Failed {
                    error: e.to_string(),
                };
            }
        };
        tracing::info!("raw_result: {}", &raw_result);

        let json_str = extract_json(&raw_result);
        match serde_json::from_str::<serde_json::Value>(json_str) {
            Ok(structured) => {
                ctx.structured_analysis = Some(structured.clone());
                let human_text = format_structured_output(&structured);
                ctx.analysis_result = Some(human_text);
                StepResult::Success {
                    message: "分析完成".to_string(),
                }
            }
            Err(_) => {
                ctx.analysis_result = Some(raw_result);
                StepResult::Success {
                    message: "分析完成".to_string(),
                }
            }
        }
    }
}

async fn call(
    provider: &Arc<dyn LlmProvider>,
    model: &str,
    prompt: &str,
) -> anyhow::Result<String> {
    let start = Instant::now();
    let cc_res = claude::call_claude_code(&prompt, "Bash,Read,Write,Grep,Glob").await?;
    tracing::info!("cc耗时{:.2}s", start.elapsed().as_secs());

    let builder = PromptBuilder::with_static_tools(vec![]);
    let cr = builder
        .build_simple(prompt.to_string())
        .with_model(model.to_string());

    let start = Instant::now();
    let llm_res = provider.llm_call(&cr).await?;
    tracing::info!("llm耗时{:.2}s", start.elapsed().as_secs());

    // 对比实验：Claude Code CLI vs LLM Provider，随机返回其一
    Ok(if yunli::util::random_f32() < 0.5 {
        cc_res
    } else {
        llm_res.content
    })
}

fn extract_json(text: &str) -> &str {
    if let Some(start) = text.find("```json") {
        let after_marker = &text[start + 7..];
        if let Some(end) = after_marker.find("```") {
            return after_marker[..end].trim();
        }
    }
    if let Some(start) = text.find("```") {
        let after_marker = &text[start + 3..];
        if let Some(end) = after_marker.find("```") {
            return after_marker[..end].trim();
        }
    }
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
