use crate::agent::chain_mapping::to_chain_with_prompt;
use crate::agent::intent::{
    Intent, JobType, extract_fields, intent_from_value, replace_intent_fields,
};
use crate::agent::{AgentResponse, StepContext, TaskType};
use crate::app_config::Config;
use crate::llm::{LlmProvider, StructuredOutput};
use crate::tools::jenkins_cache::JenkinsCacheManager;
use std::sync::Arc;

/// Strip structural filler words from the leading and trailing edges of a string.
/// Only removes complete words at boundaries, never embedded text.
pub(crate) fn strip_fillers(s: &str) -> String {
    let fillers = ["分支", "的", "到", "在", "最近", "一下", "帮我"];
    let mut result = s.to_string();

    // Strip leading fillers
    for _ in 0..fillers.len() {
        if let Some(rest) = fillers.iter().find_map(|f| result.strip_prefix(*f)) {
            result = rest.trim_start().to_string();
        } else {
            break;
        }
    }

    // Strip trailing fillers
    for _ in 0..fillers.len() {
        if let Some(rest) = fillers.iter().find_map(|f| result.strip_suffix(*f)) {
            result = rest.trim_end().to_string();
        } else {
            break;
        }
    }

    result
}

fn levenshtein_distance(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let (m, n) = (a.len(), b.len());
    let mut prev = (0..=n).collect::<Vec<usize>>();
    let mut curr = vec![0usize; n + 1];
    for i in 1..=m {
        curr[0] = i;
        for j in 1..=n {
            let cost = if a[i - 1] == b[j - 1] { 0 } else { 1 };
            curr[j] = (prev[j] + 1).min(curr[j - 1] + 1).min(prev[j - 1] + cost);
        }
        std::mem::swap(&mut prev, &mut curr);
    }
    prev[n]
}

/// Find the best matching branch name in the cache.
/// Returns (matched_branch, was_corrected) where was_corrected is true
/// if the matched branch differs from the input.
fn find_branch_match(user_branch: &str, cached_branches: &[String]) -> (String, bool) {
    // Exact match
    if let Some(found) = cached_branches.iter().find(|cb| cb == &user_branch) {
        return (found.clone(), false);
    }

    // Prefix match
    if let Some(found) = cached_branches
        .iter()
        .find(|cb| cb.starts_with(user_branch))
    {
        return (found.clone(), true);
    }

    // Levenshtein distance match (threshold: 1) — compute distance once per candidate
    if let Some((best, dist)) = cached_branches
        .iter()
        .map(|cb| (cb.as_str(), levenshtein_distance(user_branch, cb)))
        .min_by_key(|(_, d)| *d)
        && dist <= 1
    {
        return (best.to_string(), true);
    }

    // No match — return original
    (user_branch.to_string(), false)
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

            let branch = if let Some(b) = &branch {
                let (matched, was_corrected) = find_branch_match(b, &cached.branches);
                if was_corrected {
                    correction = Some((b.clone(), matched.clone()));
                }
                Some(matched)
            } else {
                branch
            };

            tracing::info!(
                "Intent regex match: action='{}', job='{}', branch={:?}, correction={:?} (from cache)",
                action,
                job_name,
                branch,
                correction
            );
            return Some((build_intent(action, &job_name, branch, jt), correction));
        }

        let branch = branch.filter(|b| !b.is_empty());
        tracing::info!(
            "Intent regex match: action='{}', job='{}', branch={:?} (from cache)",
            action,
            job_name,
            branch
        );

        Some((build_intent(action, &job_name, branch, jt), None))
    }

    pub fn parse_simple(&self, prompt: &str) -> Option<(String, String, Option<String>)> {
        // Detect action and find keyword position to extract entity after it.
        // This avoids destructive .replace() that corrupts job names containing
        // Chinese keywords (e.g., "部署工具" would become "工具").
        let (action, action_end) = if prompt.contains("部署") {
            let pos = prompt.find("部署").unwrap();
            ("deploy", pos + "部署".len())
        } else if prompt.contains("发布") {
            let pos = prompt.find("发布").unwrap();
            ("deploy", pos + "发布".len())
        } else if prompt.contains("查看日志") {
            let pos = prompt.find("查看日志").unwrap();
            ("analyze", pos + "查看日志".len())
        } else if prompt.contains("看日志") {
            let pos = prompt.find("看日志").unwrap();
            ("analyze", pos + "看日志".len())
        } else if prompt.contains("分析") {
            let pos = prompt.find("分析").unwrap();
            ("analyze", pos + "分析".len())
        } else if prompt.contains("查询") {
            let pos = prompt.find("查询").unwrap();
            ("query", pos + "查询".len())
        } else if prompt.contains("查看") {
            let pos = prompt.find("查看").unwrap();
            ("query", pos + "查看".len())
        } else if prompt.contains("状态") {
            let pos = prompt.find("状态").unwrap();
            ("query", pos + "状态".len())
        } else if prompt.contains("构建") {
            let pos = prompt.find("构建").unwrap();
            ("build", pos + "构建".len())
        } else if prompt.contains("编译") {
            let pos = prompt.find("编译").unwrap();
            ("build", pos + "编译".len())
        } else {
            return None;
        };

        // Extract entity portion (everything after the matched action keyword)
        let entity = prompt[action_end..].trim().to_string();
        if entity.is_empty() {
            return None;
        }

        // Strip structural filler words from entity boundaries only.
        // Using .replace() would corrupt job/branch names containing
        // these characters (e.g. "我的测试" → "我测试").
        let cleaned = strip_fillers(&entity).trim().to_string();

        if cleaned.is_empty() {
            return None;
        }

        // Parse job/branch from cleaned entity
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

        Some((action.to_string(), cleaned, None))
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
            }),
        );

        match so.execute::<serde_json::Value>(&intent_prompt).await {
            Ok(json) => intent_from_value(json).ok(),
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
                raw_job,
                job,
                branch
            );
            return replace_intent_fields(
                &intent,
                job.to_string(),
                Some(branch.to_string()),
                if cached.job_type == "pipeline_multibranch" {
                    JobType::Branch
                } else {
                    JobType::Standard
                },
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
                        raw_job,
                        job,
                        branch
                    );
                    return replace_intent_fields(
                        &intent,
                        job,
                        Some(branch),
                        if cached.job_type == "pipeline_multibranch" {
                            JobType::Branch
                        } else {
                            JobType::Standard
                        },
                    );
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

        let mut ctx = StepContext::new(prompt.to_string(), task_type, job_name, branch, config)
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

        let success = final_ctx.steps.last().is_some_and(|s| {
            s.result.contains("成功") && !s.result.contains("失败") && !s.result.contains("中止")
        });

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

fn build_intent(action: &str, job_name: &str, branch: Option<String>, job_type: JobType) -> Intent {
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

#[cfg(test)]
mod tests {
    use super::*;

    // ── strip_fillers tests ──

    #[test]
    fn strip_fillers_removes_leading() {
        assert_eq!(strip_fillers("最近的 dev"), "dev");
        assert_eq!(strip_fillers("帮我 dev"), "dev");
    }

    #[test]
    fn strip_fillers_removes_trailing() {
        assert_eq!(strip_fillers("dev 分支"), "dev");
        assert_eq!(strip_fillers("main 一下"), "main");
    }

    #[test]
    fn strip_fillers_removes_both_edges() {
        assert_eq!(strip_fillers("帮我 dev 分支"), "dev");
    }

    #[test]
    fn strip_fillers_preserves_embedded_text() {
        // "的" in the middle should NOT be removed
        assert_eq!(strip_fillers("我的测试"), "我的测试");
        assert_eq!(strip_fillers("部署工具"), "部署工具");
    }

    #[test]
    fn strip_fillers_multiple_leading() {
        assert_eq!(strip_fillers("帮我最近的 dev"), "dev");
    }

    // ── levenshtein_distance tests ──

    #[test]
    fn levenshtein_identical() {
        assert_eq!(levenshtein_distance("main", "main"), 0);
    }

    #[test]
    fn levenshtein_one_substitution() {
        assert_eq!(levenshtein_distance("dev", "de5"), 1);
    }

    #[test]
    fn levenshtein_one_insertion() {
        assert_eq!(levenshtein_distance("dev", "deve"), 1);
    }

    #[test]
    fn levenshtein_empty_strings() {
        assert_eq!(levenshtein_distance("", ""), 0);
        assert_eq!(levenshtein_distance("abc", ""), 3);
    }

    #[test]
    fn levenshtein_unicode() {
        assert_eq!(levenshtein_distance("开发", "开发"), 0);
        assert_eq!(levenshtein_distance("开发", "发布"), 2);
    }

    // ── find_branch_match tests ──

    #[test]
    fn branch_match_exact() {
        let branches = vec!["main".into(), "dev".into(), "feature/x".into()];
        let (matched, corrected) = find_branch_match("dev", &branches);
        assert_eq!(matched, "dev");
        assert!(!corrected);
    }

    #[test]
    fn branch_match_prefix() {
        let branches = vec!["main".into(), "feature/login".into()];
        let (matched, corrected) = find_branch_match("feature", &branches);
        assert_eq!(matched, "feature/login");
        assert!(corrected);
    }

    #[test]
    fn branch_match_levenshtein_within_threshold() {
        let branches = vec!["main".into(), "dev".into()];
        let (matched, corrected) = find_branch_match("de5", &branches);
        assert_eq!(matched, "dev");
        assert!(corrected);
    }

    #[test]
    fn branch_match_levenshtein_beyond_threshold() {
        let branches = vec!["main".into(), "develop".into()];
        let (matched, corrected) = find_branch_match("hotfix", &branches);
        assert_eq!(matched, "hotfix");
        assert!(!corrected);
    }

    #[test]
    fn branch_match_no_match_returns_original() {
        let branches: Vec<String> = vec![];
        let (matched, corrected) = find_branch_match("unknown", &branches);
        assert_eq!(matched, "unknown");
        assert!(!corrected);
    }
}
