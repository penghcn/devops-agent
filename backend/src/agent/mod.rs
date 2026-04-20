mod claude;

use serde::{Deserialize, Serialize};
use crate::config::Config;
use crate::tools::jenkins;
use claude::{call_with_skill, call_claude_code};

#[derive(Debug, Deserialize)]
pub struct AgentRequest {
    pub prompt: String,
    #[serde(default)]
    pub task_type: TaskType,
    /// Jenkins Pipeline 项目名称（如 ds-pkg）
    #[serde(default)]
    pub job_name: Option<String>,
    /// 分支名称（如 dev）
    #[serde(default)]
    pub branch: Option<String>,
}

#[derive(Debug, Deserialize, Default, PartialEq)]
pub enum TaskType {
    #[default]
    Auto,      // 自动识别
    Deploy,
    Build,
    Query,
}

#[derive(Debug, Serialize)]
pub struct AgentResponse {
    pub success: bool,
    pub output: String,
    pub steps: Vec<AgentStep>,  // 展示思考过程
}

#[derive(Debug, Serialize)]
pub struct AgentStep {
    pub action: String,
    pub result: String,
}

/// 主 Agent 入口
/// 
/// 设计决策：采用 "意图识别 → 执行" 两步流水线
/// 第一步：调用轻量模型（或 Claude 快速模式）识别意图
/// 第二步：根据意图调用对应工具或 Claude Skill
pub async fn process_request(req: AgentRequest, _config: &Config) -> AgentResponse {
    let mut steps = Vec::new();
    
    // Step 1: 意图识别（用 Claude 快速判断）
    steps.push(AgentStep {
        action: "意图识别".to_string(),
        result: "正在分析...".to_string(),
    });
    
    let intent = match req.task_type {
        TaskType::Deploy => "deploy",
        TaskType::Build => "build",
        TaskType::Query => "query",
        TaskType::Auto => {
            // 调用 Claude 快速判断意图
            let intent_prompt = format!(
                "判断以下用户意图，只输出一个词（deploy/build/query）：\n{}",
                req.prompt
            );
            match claude::call_claude_code(&intent_prompt, "None").await {
                Ok(i) if i.contains("deploy") => "deploy",
                Ok(i) if i.contains("build") => "build",
                Ok(i) if i.contains("query") => "query",
                _ => "unknown",
            }
        }
    };
    
    steps[0].result = intent.to_string();
    
    // Step 2: 执行任务
    steps.push(AgentStep {
        action: format!("执行 {}", intent),
        result: "".to_string(),
    });
    
    let output: Result<String, anyhow::Error> = match intent {
        "deploy" | "build" => {
            // 优先使用 Jenkins Pipeline API（如果提供了 job_name）
            if let Some(ref job_name) = req.job_name {
                let branch = req.branch.as_deref();
                match jenkins::trigger_pipeline(job_name, branch, _config).await {
                    Ok(msg) => {
                        // 启动后台等待任务完成
                        let job_name_clone = job_name.clone();
                        let branch_clone = branch.map(String::from);
                        let config_clone = _config.clone();
                        tokio::spawn(async move {
                            if let Some(branch) = branch_clone {
                                if let Ok(status) = jenkins::wait_for_pipeline(
                                    &job_name_clone, &branch, 1, &config_clone, 10, 1800
                                ).await {
                                    tracing::info!("Pipeline completed: {:?}", status);
                                }
                            }
                        });
                        Ok(msg)
                    }
                    Err(e) => Err(e),
                }
            } else {
                // 回退到 Claude Skill
                call_with_skill("devops-deploy", &req.prompt).await
            }
        }
        "query" => {
            // 优先使用 Jenkins Pipeline API 查询
            if let (Some(ref job_name), Some(ref branch)) = (&req.job_name, &req.branch) {
                match jenkins::get_job_status(job_name, _config).await {
                    Ok(job_status) => {
                        if let Some(last_build) = job_status.get("lastBuild")
                            .and_then(|b| b.get("number"))
                            .and_then(|n| n.as_u64())
                        {
                            let build_num = last_build as u32;
                            match jenkins::get_pipeline_status(job_name, branch, build_num, _config).await {
                                Ok(status) => {
                                    let result = status.get("result").and_then(|r| r.as_str()).unwrap_or("RUNNING");
                                    let in_progress = status.get("inProgress").and_then(|v| v.as_bool()).unwrap_or(true);
                                    Ok(format!(
                                        "Pipeline #{} [{}]: {} ({}s)",
                                        build_num,
                                        branch,
                                        result,
                                        if in_progress { "构建中" } else { "已完成" }
                                    ))
                                }
                                Err(e) => Err(anyhow::anyhow!("Failed to get pipeline status: {}", e))
                            }
                        } else {
                            Ok(format!("No builds found for {}/{}", job_name, branch))
                        }
                    }
                    Err(e) => Err(anyhow::anyhow!("Failed to get job status: {}", e))
                }
            } else {
                call_with_skill("devops-query", &req.prompt).await
            }
        }
        _ => {
            // 未知意图，直接把 prompt 交给 Claude 处理
            claude::call_claude_code(&req.prompt, "Bash,Read,Write").await
        }
    };
    
    steps[1].result = match &output {
        Ok(ref s) => s.clone(),
        Err(e) => format!("错误: {}", e),
    };
    
    AgentResponse {
        success: output.is_ok(),
        output: output.unwrap_or_else(|e| e.to_string()),
        steps,
    }
}