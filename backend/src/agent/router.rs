use crate::agent::intent::{
    extract_fields, parse_intent_json, replace_branch, Intent, JobType,
};
use crate::agent::chain_mapping::to_chain_with_prompt;
use crate::agent::{AgentResponse, StepContext, TaskType};
use crate::config::Config;
use crate::llm::{LlmProvider, StructuredOutput};
use crate::tools::jenkins_cache::JenkinsCacheManager;
use std::sync::Arc;

fn levenshtein_distance(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let (m, n) = (a.len(), b.len());
    let mut dp = vec![vec![0usize; n + 1]; m + 1];
    for (i, row) in dp.iter_mut().enumerate() {
        row[0] = i;
    }
    for (j, row) in dp[0].iter_mut().enumerate() {
        *row = j;
    }
    for i in 1..=m {
        for j in 1..=n {
            let cost = if a[i - 1] == b[j - 1] { 0 } else { 1 };
            dp[i][j] = (dp[i - 1][j] + 1)
                .min(dp[i][j - 1] + 1)
                .min(dp[i - 1][j - 1] + cost);
        }
    }
    dp[m][n]
}

pub struct IntentRouter {
    cache: Arc<JenkinsCacheManager>,
    llm_provider: Option<Arc<dyn crate::llm::LlmProvider>>,
    llm_model: String,
}

impl IntentRouter {
    pub fn new(cache: Arc<JenkinsCacheManager>) -> Self {
        Self {
            cache,
            llm_provider: None,
            llm_model: "gpt-4o-mini".to_string(),
        }
    }

    pub fn with_llm(
        cache: Arc<JenkinsCacheManager>,
        llm_provider: Arc<dyn crate::llm::LlmProvider>,
        llm_model: impl Into<String>,
    ) -> Self {
        Self {
            cache,
            llm_provider: Some(llm_provider),
            llm_model: llm_model.into(),
        }
    }

    pub async fn identify(&self, prompt: &str) -> (Intent, Option<(String, String)>) {
        if let Some((action, job_name, branch)) = self.parse_simple(prompt)
            && let Some((intent, correction)) = self
                .resolve_from_simple(&action, &job_name, branch.as_deref())
                .await
        {
            return (intent, correction);
        }

        match self.parse_with_llm(prompt).await {
            Some(intent) => (self.match_cache(intent).await, None),
            None => (Intent::General, None),
        }
    }

    async fn resolve_from_simple(
        &self,
        action: &str,
        raw_job: &str,
        branch_hint: Option<&str>,
    ) -> Option<(Intent, Option<(String, String)>)> {
        let cache_data = self.cache.get_cached().await?;

        let (job_name, branch) = if let Some((j, b)) = raw_job.split_once('/') {
            (j.to_string(), Some(b.to_string()))
        } else if let Some(b) = branch_hint {
            (raw_job.to_string(), Some(b.to_string()))
        } else {
            (raw_job.to_string(), None)
        };

        let cached = cache_data.jobs.iter().find(|j| j.name == job_name)?;

        let jt = if cached.job_type == "pipeline_multibranch" {
            JobType::Branch
        } else {
            JobType::Standard
        };

        if cached.job_type == "pipeline_multibranch" {
            let branch = branch.filter(|b| !b.is_empty());
            let mut correction: Option<(String, String)> = None;

            if let Some(b) = &branch
                && !cached.branches.contains(b)
            {
                let matched = cached
                    .branches
                    .iter()
                    .find(|cb| cb.starts_with(b.as_str()))
                    .or_else(|| {
                        cached
                            .branches
                            .iter()
                            .min_by_key(|cb| levenshtein_distance(b, cb))
                            .filter(|cb| levenshtein_distance(b, cb) <= 1)
                    });
                if let Some(best) = matched
                    && best != b.as_str()
                {
                    correction = Some((b.clone(), best.clone()));
                }
            }

            let branch = branch
                .as_ref()
                .and_then(|b| {
                    if cached.branches.contains(b) {
                        return Some(b.clone());
                    }
                    cached
                        .branches
                        .iter()
                        .find(|cb| cb.starts_with(b.as_str()))
                        .or_else(|| {
                            cached
                                .branches
                                .iter()
                                .min_by_key(|cb| levenshtein_distance(b, cb))
                                .filter(|cb| levenshtein_distance(b, cb) <= 1)
                        })
                        .cloned()
                })
                .or(branch);

            tracing::info!(
                "Intent regex match: action='{}', job='{}', branch={:?}, correction={:?} (from cache)",
                action, job_name, branch, correction
            );
            return Some((build_intent(action, &job_name, branch, jt), correction));
        }

        let branch = branch.filter(|b| !b.is_empty());
        tracing::info!(
            "Intent regex match: action='{}', job='{}', branch={:?} (from cache)",
            action, job_name, branch
        );

        Some((build_intent(action, &job_name, branch, jt), None))
    }

    pub fn parse_simple(&self, prompt: &str) -> Option<(String, String, Option<String>)> {
        let action = if prompt.contains("部署") || prompt.contains("发布") {
            "deploy"
        } else if prompt.contains("分析")
            || prompt.contains("查看日志")
            || prompt.contains("看日志")
        {
            "analyze"
        } else if prompt.contains("查询") || prompt.contains("查看") || prompt.contains("状态") {
            "query"
        } else if prompt.contains("构建") || prompt.contains("编译") {
            "build"
        } else {
            return None;
        };

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

        if let Some((job, branch)) = cleaned.split_once('/') {
            let job = job.trim().to_string();
            let branch = branch.trim().to_string();
            if !job.is_empty() {
                return Some((action.to_string(), job, Some(branch)));
            }
        }

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

        let job = cleaned.trim().to_string();
        if !job.is_empty() {
            return Some((action.to_string(), job, None));
        }

        None
    }

    async fn parse_with_llm(&self, prompt: &str) -> Option<Intent> {
        let provider = self.llm_provider.as_ref()?;

        let intent_prompt = format!(
            "判断以下用户意图，只输出一个JSON，不要输出其他内容：\n{}\n\nJSON格式：{{\"action\":\"deploy|build|query|analyze\",\"job_name\":\"项目名称\",\"branch\":\"分支名或null\",\"job_type\":\"standard|branch\"}}",
            prompt
        );

        let so = StructuredOutput::new(
            provider.clone(),
            self.llm_model.clone(),
            serde_json::json!({
                "type": "object",
                "required": ["action", "job_name"],
                "properties": {
                    "action": {"type": "string", "enum": ["deploy", "build", "query", "analyze"]},
                    "job_name": {"type": "string"},
                    "branch": {"type": "string", "nullable": true},
                    "job_type": {"type": "string", "enum": ["standard", "branch"]}
                }
            })
        );

        match so.execute::<serde_json::Value>(&intent_prompt).await {
            Ok(json) => parse_intent_json(&json.to_string()).ok(),
            Err(_) => None,
        }
    }

    async fn match_cache(&self, intent: Intent) -> Intent {
        if intent.branch_is_some() {
            return intent;
        }

        let (raw_job, _) = extract_fields(&intent);
        let Some(raw_job) = raw_job else {
            return intent;
        };

        let cache_data = match self.cache.get_cached().await {
            Some(c) => c,
            None => return intent,
        };

        if let Some((job, branch)) = raw_job.split_once('/')
            && let Some(cached) = cache_data.jobs.iter().find(|j| j.name == job)
        {
            tracing::info!(
                "Intent cache match: '{}' -> job='{}', branch='{}' (from cache, slash split)",
                raw_job, job, branch
            );
            return replace_branch(
                &intent,
                job.to_string(),
                Some(branch.to_string()),
                &cached.job_type,
            );
        }

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
                    return replace_branch(&intent, job, Some(branch), &cached.job_type);
                }
            }
        }

        if cache_data.jobs.iter().any(|j| j.name == raw_job) {
            return intent;
        }

        intent
    }

    pub async fn execute(
        &self,
        prompt: &str,
        task_type: TaskType,
        config: Arc<Config>,
        llm_provider: Option<Arc<dyn LlmProvider>>,
        llm_model: Option<String>,
    ) -> AgentResponse {
        let start = std::time::Instant::now();
        let (intent, branch_correction) = self.identify(prompt).await;
        let identify_elapsed = start.elapsed().as_millis() as f64 / 1000.0;

        let chain = to_chain_with_prompt(&intent, prompt, llm_provider.clone(), llm_model.clone());

        let (job_name, branch) = extract_fields(&intent);

        let mut ctx = StepContext::new(
            prompt.to_string(),
            task_type,
            job_name,
            branch,
            config,
        )
        .with_cache(self.cache.clone())
        .with_identify_elapsed(identify_elapsed);

        if let Some(provider) = llm_provider {
            ctx = ctx.with_llm_provider(provider);
        }
        if let Some(model) = llm_model {
            ctx = ctx.with_llm_model(model);
        }
        let ctx = if let Some((orig, corrected)) = &branch_correction {
            ctx.with_branch_correction(format!("原始分支 '{}' 已修正为 '{}'", orig, corrected))
        } else {
            ctx
        };

        let (final_ctx, steps) = chain.execute(ctx).await;

        let success = final_ctx
            .steps
            .iter()
            .any(|s| s.result.contains("成功") || s.result.contains("完成"));

        let output = final_ctx
            .steps
            .iter()
            .find(|s| s.result.contains("失败") || s.result.contains("中止"))
            .map(|s| s.result.clone())
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

fn build_intent(
    action: &str,
    job_name: &str,
    branch: Option<String>,
    job_type: JobType,
) -> Intent {
    match action {
        "deploy" => Intent::DeployPipeline {
            job_name: job_name.to_string(),
            branch,
            job_type,
        },
        "build" => Intent::BuildPipeline {
            job_name: job_name.to_string(),
            branch,
            job_type,
        },
        "query" => Intent::QueryPipeline {
            job_name: job_name.to_string(),
            branch,
            job_type,
        },
        "analyze" => Intent::AnalyzeBuild {
            job_name: job_name.to_string(),
            branch,
            job_type,
        },
        _ => Intent::General,
    }
}
