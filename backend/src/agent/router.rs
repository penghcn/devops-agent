use crate::agent::step::StepChain;
use crate::agent::steps::{
    jenkins_trigger::JenkinsTriggerStep,
    jenkins_wait::JenkinsWaitStep,
    jenkins_log::JenkinsLogStep,
    jenkins_status::JenkinsStatusStep,
    claude_analyze::ClaudeAnalyzeStep,
    claude_code::ClaudeCodeStep,
    job_validate::JobValidateStep,
};
use crate::agent::{claude, AgentResponse, StepContext, TaskType};
use crate::tools::jenkins_cache::JenkinsCacheManager;
use serde::Deserialize;
use std::sync::Arc;

fn levenshtein_distance(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let (m, n) = (a.len(), b.len());
    let mut dp = vec![vec![0usize; n + 1]; m + 1];
    for i in 0..=m { dp[i][0] = i; }
    for j in 0..=n { dp[0][j] = j; }
    for i in 1..=m {
        for j in 1..=n {
            let cost = if a[i-1] == b[j-1] { 0 } else { 1 };
            dp[i][j] = (dp[i-1][j]+1).min(dp[i][j-1]+1).min(dp[i-1][j-1]+cost);
        }
    }
    dp[m][n]
}

#[derive(Debug, Clone, PartialEq, Default)]
pub enum JobType {
    Standard,    // 普通 Job
    #[default]
    Branch,      // 多分支 Pipeline
}

#[derive(Debug, Clone, PartialEq)]
pub enum Intent {
    DeployPipeline { job_name: String, branch: Option<String>, job_type: JobType },
    BuildPipeline { job_name: String, branch: Option<String>, job_type: JobType },
    QueryPipeline { job_name: String, branch: Option<String>, job_type: JobType },
    AnalyzeBuild { job_name: String, branch: Option<String>, job_type: JobType },
    General,
}

impl Intent {
    fn branch_is_some(&self) -> bool {
        matches!(self, Intent::DeployPipeline { branch: Some(_), .. }
            | Intent::BuildPipeline { branch: Some(_), .. }
            | Intent::QueryPipeline { branch: Some(_), .. }
            | Intent::AnalyzeBuild { branch: Some(_), .. })
    }
}

pub struct IntentRouter {
    cache: Arc<JenkinsCacheManager>,
}

impl IntentRouter {
    pub fn new(cache: Arc<JenkinsCacheManager>) -> Self {
        Self { cache }
    }

    /// 识别用户意图（分层策略）：
    /// 1. 正则快速匹配：`部署/构建/查询/分析 {job_name}/{branch}` — 不调用 Claude
    /// 2. 自然语言模糊意图 → 调用 Claude
    /// 3. 无法识别 → General
    /// 返回 (Intent, 分支修正提示)
    pub async fn identify(&self, prompt: &str) -> (Intent, Option<(String, String)>) {
        // 第一层：正则快速匹配（不花 token）
        if let Some((action, job_name, branch)) = self.parse_simple(prompt) {
            // 用缓存匹配确认 job_name 是否存在，并拆分组合名称
            if let Some((intent, correction)) = self.resolve_from_simple(&action, &job_name, branch.as_deref()).await {
                return (intent, correction);
            }
        }

        // 第二层：Claude 识别自然语言意图
        match self.parse_with_claude(prompt).await {
            Some(intent) => (self.match_cache(intent).await, None),
            None => (Intent::General, None),
        }
    }

    /// 从正则解析结果中解析 Intent，需要异步获取缓存
    /// 返回 (Intent, 分支修正提示)，如 Some(("de5", "dev")) 表示 de5 被修正为 dev
    async fn resolve_from_simple(&self, action: &str, raw_job: &str, branch_hint: Option<&str>) -> Option<(Intent, Option<(String, String)>)> {
        let cache_data = self.cache.get_cached().await?;

        // 尝试拆分：斜杠分隔（raw_job 中已含 /）
        let (job_name, branch) = if let Some((j, b)) = raw_job.split_once('/') {
            (j.to_string(), Some(b.to_string()))
        } else if let Some(b) = branch_hint {
            (raw_job.to_string(), Some(b.to_string()))
        } else {
            (raw_job.to_string(), None)
        };

        // 在缓存中查找 job
        let cached = cache_data.jobs.iter().find(|j| j.name == job_name)?;

        let jt = if cached.job_type == "pipeline_multibranch" {
            JobType::Branch
        } else {
            JobType::Standard
        };

        // 多分支 Pipeline 校验 branch
        if cached.job_type == "pipeline_multibranch" {
            let branch = branch.filter(|b| !b.is_empty());
            let mut correction: Option<(String, String)> = None;

            if let Some(b) = &branch {
                if !cached.branches.contains(b) {
                    // starts_with 优先
                    let matched = cached.branches.iter().find(|cb| cb.starts_with(b.as_str()))
                        .or_else(|| cached.branches.iter().min_by_key(|cb| levenshtein_distance(b, cb)).filter(|cb| levenshtein_distance(b, cb) <= 1));
                    if let Some(best) = matched {
                        if best != b.as_str() {
                            correction = Some((b.clone(), best.clone()));
                        }
                    }
                }
            }

            let branch = branch.as_ref().and_then(|b| {
                if cached.branches.contains(b) {
                    return Some(b.clone());
                }
                cached.branches.iter().find(|cb| cb.starts_with(b.as_str()))
                    .or_else(|| cached.branches.iter().min_by_key(|cb| levenshtein_distance(b, cb)).filter(|cb| levenshtein_distance(b, cb) <= 1))
                    .cloned()
            }).or(branch);

            tracing::info!(
                "Intent regex match: action='{}', job='{}', branch={:?}, correction={:?} (from cache)",
                action, job_name, branch, correction
            );
            return Some((match action {
                "deploy" => Intent::DeployPipeline { job_name, branch, job_type: jt },
                "build" => Intent::BuildPipeline { job_name, branch, job_type: jt },
                "query" => Intent::QueryPipeline { job_name, branch, job_type: jt },
                "analyze" => Intent::AnalyzeBuild { job_name, branch, job_type: jt },
                _ => return None,
            }, correction.map(|(orig, best)| (orig, best))));
        }

        // 非多分支，无分支要求
        let branch = branch.filter(|b| !b.is_empty());
        tracing::info!(
            "Intent regex match: action='{}', job='{}', branch={:?} (from cache)",
            action, job_name, branch
        );

        Some((match action {
            "deploy" => Intent::DeployPipeline { job_name, branch, job_type: jt },
            "build" => Intent::BuildPipeline { job_name, branch, job_type: jt },
            "query" => Intent::QueryPipeline { job_name, branch, job_type: jt },
            "analyze" => Intent::AnalyzeBuild { job_name, branch, job_type: jt },
            _ => return None,
        }, None))
    }

    /// 第一层：正则快速解析（不花 token）
    /// 匹配模式：
    ///   - "部署 ds-pkg/dev 到 staging" → job=ds-pkg, branch=dev
    ///   - "构建 ds-pkg dev" → job=ds-pkg, branch=dev
    ///   - "查询 ds-pkg" → job=ds-pkg, branch=null
    pub fn parse_simple(&self, prompt: &str) -> Option<(String, String, Option<String>)> {
        // 动作关键词（注意顺序：query/analyze 在 build 之前，避免"查询构建状态"被误判为 build）
        let action = if prompt.contains("部署") || prompt.contains("发布") {
            "deploy"
        } else if prompt.contains("分析") || prompt.contains("查看日志") || prompt.contains("看日志") {
            "analyze"
        } else if prompt.contains("查询") || prompt.contains("查看") || prompt.contains("状态") {
            "query"
        } else if prompt.contains("构建") || prompt.contains("编译") {
            "build"
        } else {
            return None;
        };

        // 去掉动作词和环境词，剩余部分解析 job_name/branch
        let cleaned = prompt
            .replace("部署", "")
            .replace("发布", "")
            .replace("构建", "")
            .replace("编译", "")
            .replace("查询", "")
            .replace("查看", "")
            .replace("分析", "")
            .replace("看日志", "")
            .replace("分支", "")
            .replace("的", "")
            .replace("到", "")
            .replace("在", "")
            .replace("staging", "")
            .replace("production", "")
            .replace("prod", "")
            .replace("测试", "")
            .replace("环境", "")
            .replace("最近", "")
            .replace("一下", "")
            .replace("帮我", "")
            .replace("日志", "")
            .replace("状态", "")
            .trim()
            .to_string();

        // 尝试 "job/branch" 格式
        if let Some((job, branch)) = cleaned.split_once('/') {
            let job = job.trim().to_string();
            let branch = branch.trim().to_string();
            if !job.is_empty() {
                return Some((action.to_string(), job, Some(branch)));
            }
        }

        // 尝试 "job branch" 格式
        let parts: Vec<&str> = cleaned.split_whitespace().collect();
        if parts.len() >= 2 {
            for i in 0..parts.len() - 1 {
                let job = parts[..=i].join(" ");
                let branch = parts[i + 1..].join(" ");
                if !job.is_empty() {
                    return Some((action.to_string(), job, Some(branch)));
                }
            }
        }

        // 单个 job name（无分支）
        let job = cleaned.trim().to_string();
        if !job.is_empty() {
            return Some((action.to_string(), job, None));
        }

        None
    }

    /// 第二层：调用 Claude 识别自然语言意图
    async fn parse_with_claude(&self, prompt: &str) -> Option<Intent> {
        let intent_prompt = format!(
            "判断以下用户意图，只输出一个 JSON，不要输出其他内容：\n{}\n\nJSON 格式：{{\"action\":\"deploy|build|query|analyze\",\"job_name\":\"项目名称\",\"branch\":\"分支名或null\",\"job_type\":\"standard|branch\"}}",
            prompt
        );

        match claude::call_claude_code(&intent_prompt, "").await {
            Ok(response) => {
                parse_intent_json(&response).ok()
            }
            Err(_) => None,
        }
    }

    /// 用缓存匹配拆分组合名称（async，需要获取缓存）
    /// 支持格式：
    ///   - "ds-pkg/dev" → job=ds-pkg, branch=dev
    ///   - "ds-pkg dev" → job=ds-pkg, branch=dev
    async fn match_cache(&self, intent: Intent) -> Intent {
        // 如果 Claude 已经正确拆分了 branch，直接返回
        if intent.branch_is_some() {
            return intent;
        }

        let (raw_job, _) = Self::extract_fields(&intent);
        let Some(raw_job) = raw_job else { return intent };

        let cache_data = match self.cache.get_cached().await {
            Some(c) => c,
            None => return intent,
        };

        // 尝试拆分：斜杠分隔
        if let Some((job, branch)) = raw_job.split_once('/') {
            if let Some(cached) = cache_data.jobs.iter().find(|j| j.name == job) {
                tracing::info!(
                    "Intent cache match: '{}' -> job='{}', branch='{}' (from cache, slash split)",
                    raw_job, job, branch
                );
                return self.replace_branch(&intent, job.to_string(), Some(branch.to_string()), &cached.job_type);
            }
        }

        // 尝试拆分：空格分隔
        let parts: Vec<&str> = raw_job.split_whitespace().collect();
        if parts.len() >= 2 {
            for i in 0..parts.len() - 1 {
                let job = parts[..=i].join(" ");
                let branch = parts[i + 1..].join(" ");
                if let Some(cached) = cache_data.jobs.iter().find(|j| j.name == job) {
                    tracing::info!(
                        "Intent cache match: '{}' -> job='{}', branch='{}' (from cache, space split)",
                        raw_job, job, branch
                    );
                    return self.replace_branch(&intent, job, Some(branch), &cached.job_type);
                }
            }
        }

        // 精确匹配：raw_job 本身就是 job name
        if cache_data.jobs.iter().any(|j| j.name == raw_job) {
            return intent;
        }

        intent
    }

    fn replace_branch(&self, intent: &Intent, job_name: String, branch: Option<String>, job_type: &str) -> Intent {
        let jt = if job_type == "pipeline_multibranch" || job_type == "branch" {
            JobType::Branch
        } else {
            JobType::Standard
        };
        match intent {
            Intent::DeployPipeline { .. } => Intent::DeployPipeline { job_name, branch, job_type: jt },
            Intent::BuildPipeline { .. } => Intent::BuildPipeline { job_name, branch, job_type: jt },
            Intent::QueryPipeline { .. } => Intent::QueryPipeline { job_name, branch, job_type: jt },
            Intent::AnalyzeBuild { .. } => Intent::AnalyzeBuild { job_name, branch, job_type: jt },
            Intent::General => Intent::General,
        }
    }

    /// 根据 Intent 返回对应的 StepChain
    pub fn to_chain_with_prompt(&self, intent: &Intent, prompt: &str) -> StepChain {
        match intent {
            Intent::DeployPipeline { .. } | Intent::BuildPipeline { .. } => {
                StepChain::new(vec![
                    Box::new(JobValidateStep),
                    Box::new(JenkinsTriggerStep),
                    Box::new(JenkinsWaitStep::default()),
                    Box::new(JenkinsLogStep),
                    Box::new(ClaudeAnalyzeStep::default()),
                ])
            }
            Intent::QueryPipeline { .. } => {
                StepChain::new(vec![
                    Box::new(JobValidateStep),
                    Box::new(JenkinsStatusStep),
                ])
            }
            Intent::AnalyzeBuild { .. } => {
                StepChain::new(vec![
                    Box::new(JobValidateStep),
                    Box::new(JenkinsLogStep),
                    Box::new(ClaudeAnalyzeStep::default()),
                ])
            }
            Intent::General => {
                StepChain::new(vec![
                    Box::new(ClaudeCodeStep {
                        prompt: prompt.to_string(),
                        allowed_tools: "Bash,Read,Write".to_string(),
                    }),
                ])
            }
        }
    }

    /// 从 Intent 中提取 job_name 和 branch
    fn extract_fields(intent: &Intent) -> (Option<String>, Option<String>) {
        match intent {
            Intent::DeployPipeline { job_name, branch, .. }
            | Intent::BuildPipeline { job_name, branch, .. }
            | Intent::QueryPipeline { job_name, branch, .. }
            | Intent::AnalyzeBuild { job_name, branch, .. } => (Some(job_name.clone()), branch.clone()),
            Intent::General => (None, None),
        }
    }

    /// 完整流程：识别意图 + 执行 StepChain
    pub async fn execute(
        &self,
        prompt: &str,
        task_type: TaskType,
    ) -> AgentResponse {
        let start = std::time::Instant::now();
        let (intent, branch_correction) = self.identify(prompt).await;
        let identify_elapsed = start.elapsed().as_millis() as f64 / 1000.0;

        let chain = self.to_chain_with_prompt(&intent, prompt);

        let (job_name, branch) = Self::extract_fields(&intent);

        let ctx = StepContext::new(
            prompt.to_string(),
            task_type,
            job_name,
            branch,
            Arc::new(crate::config::Config::from_env()),
        ).with_cache(self.cache.clone()).with_identify_elapsed(identify_elapsed);
        let ctx = if let Some((orig, corrected)) = &branch_correction {
            ctx.with_branch_correction(format!("原始分支 '{}' 已修正为 '{}'", orig, corrected))
        } else {
            ctx
        };

        let (final_ctx, steps) = chain.execute(ctx).await;

        let success = final_ctx.steps.iter().any(|s| {
            s.result.contains("成功") || s.result.contains("完成")
        });

        // 优先返回第一条失败/中止信息，否则返回分析结果或最后一步
        let output = final_ctx.steps.iter().find(|s| {
            s.result.contains("失败") || s.result.contains("中止")
        }).map(|s| s.result.clone())
        .or_else(|| final_ctx.analysis_result.clone())
        .unwrap_or_else(|| {
            final_ctx
                .steps
                .last()
                .map(|s| s.result.clone())
                .unwrap_or_else(|| "处理完成".to_string())
        });

        AgentResponse {
            success,
            output,
            structured_output: final_ctx.structured_analysis.clone(),
            steps,
            branch_correction: final_ctx.branch_correction.clone(),
        }
    }
}

/// 解析 Claude 返回的 JSON，构建 Intent
fn parse_intent_json(response: &str) -> Result<Intent, ()> {
    #[derive(Deserialize)]
    struct IntentJson {
        action: String,
        job_name: String,
        branch: Option<String>,
        job_type: String,
    }

    let parsed: IntentJson = match serde_json::from_str::<IntentJson>(response) {
        Ok(v) => v,
        Err(_) => {
            // 尝试从 response 中找 JSON 片段
            let start = response.find('{').unwrap_or(0);
            let end = response.rfind('}').map(|i| i + 1).unwrap_or(response.len());
            serde_json::from_str(&response[start..end]).map_err(|_| ())?
        }
    };

    let job_type = match parsed.job_type.as_str() {
        "branch" => JobType::Branch,
        _ => JobType::Standard,
    };

    let intent = match parsed.action.as_str() {
        "deploy" => Intent::DeployPipeline {
            job_name: parsed.job_name,
            branch: parsed.branch,
            job_type,
        },
        "build" => Intent::BuildPipeline {
            job_name: parsed.job_name,
            branch: parsed.branch,
            job_type,
        },
        "query" => Intent::QueryPipeline {
            job_name: parsed.job_name,
            branch: parsed.branch,
            job_type,
        },
        "analyze" => Intent::AnalyzeBuild {
            job_name: parsed.job_name,
            branch: parsed.branch,
            job_type,
        },
        _ => return Err(()),
    };

    Ok(intent)
}
