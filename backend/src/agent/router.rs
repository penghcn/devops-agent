use crate::agent::chain_mapping::to_chain_with_prompt;
use crate::agent::intent::{
    Intent, JobType, extract_fields, intent_from_value, replace_intent_fields,
};
use crate::agent::{AgentResponse, StepContext, TaskType};
use crate::config::Config;
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

/// Find the best matching job name in the cache.
/// Returns (matched_name, was_corrected).
fn find_job_match(user_job: &str, cached_jobs: &[String]) -> (String, bool) {
    if let Some(found) = cached_jobs.iter().find(|j| *j == user_job) {
        return (found.clone(), false);
    }
    if let Some(found) = cached_jobs.iter().find(|j| j.starts_with(user_job)) {
        return (found.clone(), true);
    }
    if let Some((best, dist)) = cached_jobs
        .iter()
        .map(|j| (j.as_str(), levenshtein_distance(user_job, j)))
        .min_by_key(|(_, d)| *d)
        && dist <= 1
    {
        return (best.to_string(), true);
    }
    (user_job.to_string(), false)
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

    pub fn cache(&self) -> &Arc<JenkinsCacheManager> {
        &self.cache
    }

    pub async fn identify(&self, prompt: &str) -> (Intent, Vec<crate::agent::Correction>) {
        if let Some((action, job_name, branch)) = self.parse_simple(prompt) {
            if let Some((intent, corrections)) = self
                .resolve_from_simple(&action, &job_name, branch.as_deref())
                .await
            {
                return (intent, corrections);
            }
            let (job, br) = if let Some((j, b)) = job_name.split_once('/') {
                (j.to_string(), Some(b.to_string()))
            } else if let Some(b) = &branch {
                (job_name, Some(b.clone()))
            } else {
                (job_name, None)
            };
            return (
                build_intent(&action, &job, br, JobType::Standard),
                Vec::new(),
            );
        }

        match self.parse_with_llm(prompt).await {
            Some(intent) => {
                let (resolved, corrections) = self.match_cache(intent).await;
                (resolved, corrections)
            }
            None => (Intent::General, Vec::new()),
        }
    }

    async fn resolve_from_simple(
        &self,
        action: &str,
        raw_job: &str,
        branch_hint: Option<&str>,
    ) -> Option<(Intent, Vec<crate::agent::Correction>)> {
        use crate::agent::Correction;

        let cache_data = self.cache.get_cached().await?;
        let job_names: Vec<String> = cache_data.jobs.iter().map(|j| j.name.clone()).collect();

        let (job_name, branch) = if let Some((j, b)) = raw_job.split_once('/') {
            (j.to_string(), Some(b.to_string()))
        } else if let Some(b) = branch_hint {
            (raw_job.to_string(), Some(b.to_string()))
        } else {
            (raw_job.to_string(), None)
        };

        let (matched_job, job_corrected) = find_job_match(&job_name, &job_names);
        let cached = cache_data.jobs.iter().find(|j| j.name == matched_job)?;
        let mut corrections: Vec<Correction> = Vec::new();

        if job_corrected {
            corrections.push(Correction {
                kind: "job".into(),
                original: job_name.clone(),
                corrected: matched_job.clone(),
            });
        }

        let jt = if cached.job_type == "pipeline_multibranch" {
            JobType::Branch
        } else {
            JobType::Standard
        };

        if cached.job_type == "pipeline_multibranch" {
            let branch = branch.filter(|b| !b.is_empty());
            let branch = if let Some(b) = &branch {
                let (matched, was_corrected) = find_branch_match(b, &cached.branches);
                if was_corrected {
                    corrections.push(Correction {
                        kind: "branch".into(),
                        original: b.clone(),
                        corrected: matched.clone(),
                    });
                }
                Some(matched)
            } else {
                branch
            };

            tracing::info!(
                "Intent regex match: action='{}', job='{}', branch={:?}, corrections={:?} (from cache)",
                action,
                matched_job,
                branch,
                corrections
            );
            return Some((build_intent(action, &matched_job, branch, jt), corrections));
        }

        let branch = branch.filter(|b| !b.is_empty());
        tracing::info!(
            "Intent regex match: action='{}', job='{}', branch={:?}, corrections={:?} (from cache)",
            action,
            matched_job,
            branch,
            corrections
        );

        Some((build_intent(action, &matched_job, branch, jt), corrections))
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

    async fn match_cache(&self, intent: Intent) -> (Intent, Vec<crate::agent::Correction>) {
        use crate::agent::Correction;

        let (raw_job, raw_branch) = extract_fields(&intent);
        let Some(raw_job) = raw_job else {
            return (intent, Vec::new());
        };

        let cache_data = match self.cache.get_cached().await {
            Some(c) => c,
            None => return (intent, Vec::new()),
        };

        let job_names: Vec<String> = cache_data.jobs.iter().map(|j| j.name.clone()).collect();

        // 先尝试从 job_name 中拆分 branch（ds-pkg/dev 格式）
        if let Some((job, branch)) = raw_job.split_once('/') {
            let (matched_job, job_corrected) = find_job_match(job, &job_names);
            if let Some(cached) = cache_data.jobs.iter().find(|j| j.name == matched_job) {
                let jt = if cached.job_type == "pipeline_multibranch" {
                    JobType::Branch
                } else {
                    JobType::Standard
                };

                let mut corrections: Vec<Correction> = Vec::new();
                if job_corrected {
                    corrections.push(Correction {
                        kind: "job".into(),
                        original: job.to_string(),
                        corrected: matched_job.clone(),
                    });
                }

                if cached.job_type == "pipeline_multibranch" {
                    let (matched_branch, branch_corrected) =
                        find_branch_match(branch, &cached.branches);
                    if branch_corrected {
                        corrections.push(Correction {
                            kind: "branch".into(),
                            original: branch.to_string(),
                            corrected: matched_branch.clone(),
                        });
                    }
                    tracing::info!(
                        "Intent cache match: '{}' -> job='{}', branch='{}' (from cache, slash split){}",
                        raw_job,
                        matched_job,
                        matched_branch,
                        if job_corrected || branch_corrected {
                            " [corrected]"
                        } else {
                            ""
                        }
                    );
                    return (
                        replace_intent_fields(&intent, matched_job, Some(matched_branch), jt),
                        corrections,
                    );
                }

                tracing::info!(
                    "Intent cache match: '{}' -> job='{}', branch='{}' (from cache, slash split){}",
                    raw_job,
                    matched_job,
                    branch,
                    if job_corrected { " [corrected]" } else { "" }
                );
                return (
                    replace_intent_fields(&intent, matched_job, Some(branch.to_string()), jt),
                    corrections,
                );
            }
        }

        // 按空格拆分 job/branch
        let parts: Vec<&str> = raw_job.split_whitespace().collect();
        if parts.len() >= 2 {
            for i in 0..parts.len() - 1 {
                let job = parts[..=i].join(" ");
                let branch = parts[i + 1..].join(" ");
                let (matched_job, job_corrected) = find_job_match(&job, &job_names);
                if let Some(cached) = cache_data.jobs.iter().find(|j| j.name == matched_job) {
                    let jt = if cached.job_type == "pipeline_multibranch" {
                        JobType::Branch
                    } else {
                        JobType::Standard
                    };

                    let mut corrections: Vec<Correction> = Vec::new();
                    if job_corrected {
                        corrections.push(Correction {
                            kind: "job".into(),
                            original: job.clone(),
                            corrected: matched_job.clone(),
                        });
                    }

                    if cached.job_type == "pipeline_multibranch" {
                        let (matched_branch, branch_corrected) =
                            find_branch_match(&branch, &cached.branches);
                        if branch_corrected {
                            corrections.push(Correction {
                                kind: "branch".into(),
                                original: branch.clone(),
                                corrected: matched_branch.clone(),
                            });
                        }
                        tracing::info!(
                            "Intent cache match: '{}' -> job='{}', branch='{}' (from cache, space split){}",
                            raw_job,
                            matched_job,
                            matched_branch,
                            if job_corrected || branch_corrected {
                                " [corrected]"
                            } else {
                                ""
                            }
                        );
                        return (
                            replace_intent_fields(&intent, matched_job, Some(matched_branch), jt),
                            corrections,
                        );
                    }

                    tracing::info!(
                        "Intent cache match: '{}' -> job='{}', branch='{}' (from cache, space split){}",
                        raw_job,
                        matched_job,
                        branch,
                        if job_corrected { " [corrected]" } else { "" }
                    );
                    return (
                        replace_intent_fields(&intent, matched_job, Some(branch), jt),
                        corrections,
                    );
                }
            }
        }

        // LLM 解析的 branch 字段 + job 名模糊匹配
        {
            let (matched_job, job_corrected) = find_job_match(&raw_job, &job_names);
            if let Some(cached) = cache_data.jobs.iter().find(|j| j.name == matched_job)
                && cached.job_type == "pipeline_multibranch"
                && let Some(branch) = &raw_branch
            {
                let (matched_branch, branch_corrected) =
                    find_branch_match(branch, &cached.branches);
                let mut corrections: Vec<Correction> = Vec::new();
                if job_corrected {
                    corrections.push(Correction {
                        kind: "job".into(),
                        original: raw_job.clone(),
                        corrected: matched_job.clone(),
                    });
                }
                if branch_corrected {
                    corrections.push(Correction {
                        kind: "branch".into(),
                        original: branch.clone(),
                        corrected: matched_branch.clone(),
                    });
                }
                let jt = JobType::Branch;
                tracing::info!(
                    "Intent cache match: job='{}', branch='{}' -> '{}' (from cache, LLM branch){}",
                    matched_job,
                    branch,
                    matched_branch,
                    if job_corrected || branch_corrected {
                        " [corrected]"
                    } else {
                        ""
                    }
                );
                return (
                    replace_intent_fields(&intent, matched_job, Some(matched_branch), jt),
                    corrections,
                );
            }
        }

        if cache_data.jobs.iter().any(|j| j.name == raw_job) {
            return (intent, Vec::new());
        }

        (intent, Vec::new())
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
        let (intent, corrections) = self.identify(prompt).await;
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
        for c in &corrections {
            ctx = ctx.add_correction(c.kind.clone(), c.original.clone(), c.corrected.clone());
        }

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
            corrections: final_ctx.corrections.clone(),
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

    // ── find_job_match tests ──

    #[test]
    fn job_match_exact() {
        let jobs = vec!["ds-pkg".into(), "backend-api".into()];
        let (matched, corrected) = find_job_match("ds-pkg", &jobs);
        assert_eq!(matched, "ds-pkg");
        assert!(!corrected);
    }

    #[test]
    fn job_match_prefix() {
        let jobs = vec!["ds-package".into(), "backend".into()];
        let (matched, corrected) = find_job_match("ds", &jobs);
        assert_eq!(matched, "ds-package");
        assert!(corrected);
    }

    #[test]
    fn job_match_levenshtein_one_insertion() {
        let jobs = vec!["ds-pkg".into(), "backend".into()];
        let (matched, corrected) = find_job_match("ds-pk", &jobs);
        assert_eq!(matched, "ds-pkg");
        assert!(corrected);
    }

    #[test]
    fn job_match_levenshtein_one_substitution() {
        let jobs = vec!["frontend-app".into(), "backend-api".into()];
        let (matched, corrected) = find_job_match("backend-apl", &jobs);
        assert_eq!(matched, "backend-api");
        assert!(corrected);
    }

    #[test]
    fn job_match_beyond_threshold() {
        let jobs = vec!["alpha".into(), "beta".into()];
        let (matched, corrected) = find_job_match("gamma", &jobs);
        assert_eq!(matched, "gamma");
        assert!(!corrected);
    }

    #[test]
    fn job_match_empty_list() {
        let jobs: Vec<String> = vec![];
        let (matched, corrected) = find_job_match("anything", &jobs);
        assert_eq!(matched, "anything");
        assert!(!corrected);
    }

    // ── parse_simple integration tests ──

    async fn make_router_with_mock_cache() -> IntentRouter {
        use crate::tools::jenkins_cache::{CachedJob, JenkinsCache};

        let cache_data = JenkinsCache {
            jobs: vec![CachedJob {
                name: "ds-pkg".into(),
                job_type: "pipeline_multibranch".into(),
                url: "http://jenkins/job/ds-pkg".into(),
                branches: vec!["dev".into(), "main".into(), "feature/x".into()],
            }],
            last_refresh: "now".into(),
        };

        let cache_mgr = crate::tools::jenkins_cache::JenkinsCacheManager::new(
            crate::config::Config::test_default(),
        );
        {
            let rw = cache_mgr.cache();
            let mut guard = rw.write().await;
            *guard = Some(cache_data);
        }
        IntentRouter::new(std::sync::Arc::new(cache_mgr))
    }

    #[tokio::test]
    async fn identify_ds_pkg_dev_exact_match() {
        let router = make_router_with_mock_cache().await;
        let (intent, correction) = router.identify("部署 ds-pkg/dev").await;
        let (job, branch) = crate::agent::intent::extract_fields(&intent);
        assert_eq!(job, Some("ds-pkg".into()));
        assert_eq!(branch, Some("dev".into()));
        assert!(correction.is_empty(), "精确匹配不应有修正");
    }

    #[tokio::test]
    async fn identify_ds_pkg_de_branch_corrected() {
        let router = make_router_with_mock_cache().await;
        let (intent, correction) = router.identify("部署 ds-pkg/de").await;
        let (job, branch) = crate::agent::intent::extract_fields(&intent);
        assert_eq!(job, Some("ds-pkg".into()));
        assert_eq!(branch, Some("dev".into()));
        assert!(!correction.is_empty(), "分支 'de' 应该被修正为 'dev'");
    }

    #[tokio::test]
    async fn identify_ds_pk_de_both_corrected() {
        let router = make_router_with_mock_cache().await;
        let (intent, correction) = router.identify("部署 ds-pk/de").await;
        let (job, branch) = crate::agent::intent::extract_fields(&intent);
        assert_eq!(job, Some("ds-pkg".into()));
        assert_eq!(branch, Some("dev".into()));
        assert!(
            !correction.is_empty(),
            "job 'ds-pk' 和分支 'de' 都应该被修正"
        );
    }

    #[tokio::test]
    async fn identify_ds_pkg_dev_space_separated() {
        let router = make_router_with_mock_cache().await;
        let (intent, correction) = router.identify("部署 ds-pkg dev").await;
        let (job, branch) = crate::agent::intent::extract_fields(&intent);
        assert_eq!(job, Some("ds-pkg".into()));
        assert_eq!(branch, Some("dev".into()));
        assert!(correction.is_empty(), "精确匹配不应有修正");
    }

    #[tokio::test]
    async fn identify_ds_pk_space_de_both_corrected() {
        let router = make_router_with_mock_cache().await;
        let (intent, correction) = router.identify("部署 ds-pk de").await;
        let (job, branch) = crate::agent::intent::extract_fields(&intent);
        assert_eq!(job, Some("ds-pkg".into()));
        assert_eq!(branch, Some("dev".into()));
        assert!(!correction.is_empty(), "job 和分支都应该被修正");
    }

    // ── 步骤链一致性：三个关键词组必须生成相同的步骤链 ──

    #[tokio::test]
    async fn step_chain_consistency_three_variants() {
        use crate::agent::chain_mapping::to_chain_with_prompt;

        let router = make_router_with_mock_cache().await;

        // 解析三个关键词组
        let (intent_exact, _c1) = router.identify("部署 ds-pkg/dev").await;
        let (intent_branch_fix, _c2) = router.identify("部署 ds-pkg/de").await;
        let (intent_both_fix, _c3) = router.identify("部署 ds-pk/de").await;

        // 三个 intent 必须完全等价（job, branch, action, job_type）
        let (job1, branch1) = crate::agent::intent::extract_fields(&intent_exact);
        let (job2, branch2) = crate::agent::intent::extract_fields(&intent_branch_fix);
        let (job3, branch3) = crate::agent::intent::extract_fields(&intent_both_fix);

        assert_eq!(job1, job2, "job 名应一致: ds-pkg/dev vs ds-pkg/de");
        assert_eq!(job1, job3, "job 名应一致: ds-pkg/dev vs ds-pk/de");
        assert_eq!(branch1, branch2, "branch 应一致: ds-pkg/dev vs ds-pkg/de");
        assert_eq!(branch1, branch3, "branch 应一致: ds-pkg/dev vs ds-pk/de");

        // 验证 intent 类型相同（DeployPipeline）
        assert!(
            matches!(intent_exact, crate::agent::Intent::DeployPipeline { .. }),
            "应该是 DeployPipeline"
        );
        assert!(
            matches!(
                intent_branch_fix,
                crate::agent::Intent::DeployPipeline { .. }
            ),
            "应该是 DeployPipeline"
        );
        assert!(
            matches!(intent_both_fix, crate::agent::Intent::DeployPipeline { .. }),
            "应该是 DeployPipeline"
        );

        // 意图等价 → to_chain_with_prompt 生成的步骤链也等价
        // 因为 chain_mapping 只根据 Intent 枚举类型决定步骤链
        let _chain1 = to_chain_with_prompt(&intent_exact, "", None, None);
        let _chain2 = to_chain_with_prompt(&intent_branch_fix, "", None, None);
        let _chain3 = to_chain_with_prompt(&intent_both_fix, "", None, None);
    }

    #[tokio::test]
    async fn step_chain_consistency_query_variants() {
        use crate::agent::chain_mapping::to_chain_with_prompt;

        let router = make_router_with_mock_cache().await;

        let (intent1, _) = router.identify("查询 ds-pkg/dev").await;
        let (intent2, _) = router.identify("查询 ds-pkg/de").await;
        let (intent3, _) = router.identify("查询 ds-pk/de").await;

        let (job1, branch1) = crate::agent::intent::extract_fields(&intent1);
        let (job2, branch2) = crate::agent::intent::extract_fields(&intent2);
        let (job3, branch3) = crate::agent::intent::extract_fields(&intent3);

        assert_eq!(job1, job2);
        assert_eq!(job1, job3);
        assert_eq!(branch1, branch2);
        assert_eq!(branch1, branch3);

        // 验证都是 QueryPipeline 类型
        assert!(
            matches!(intent1, crate::agent::Intent::QueryPipeline { .. }),
            "应该是 QueryPipeline"
        );
        assert!(
            matches!(intent2, crate::agent::Intent::QueryPipeline { .. }),
            "应该是 QueryPipeline"
        );
        assert!(
            matches!(intent3, crate::agent::Intent::QueryPipeline { .. }),
            "应该是 QueryPipeline"
        );

        // 步骤链也一致
        let chain1 = to_chain_with_prompt(&intent1, "", None, None);
        let chain2 = to_chain_with_prompt(&intent2, "", None, None);
        let chain3 = to_chain_with_prompt(&intent3, "", None, None);
        assert!(
            std::mem::discriminant(&intent1) == std::mem::discriminant(&intent2)
                && std::mem::discriminant(&intent1) == std::mem::discriminant(&intent3),
            "三个 query intent 类型必须相同"
        );
        drop(chain1);
        drop(chain2);
        drop(chain3);
    }

    // ── 缓存未命中时，action 仍由 parse_simple 决定（不被 LLM 覆盖） ──

    async fn make_router_with_empty_cache() -> IntentRouter {
        let cache_mgr = crate::tools::jenkins_cache::JenkinsCacheManager::new(
            crate::config::Config::test_default(),
        );
        // 缓存保持为空（不注入数据）
        IntentRouter::new(std::sync::Arc::new(cache_mgr))
    }

    #[tokio::test]
    async fn cache_miss_preserves_action_deploy() {
        let router = make_router_with_empty_cache().await;
        let (intent, _correction) = router.identify("部署 ds-pkg/de").await;
        // 即使缓存为空，action 必须是 deploy，不能变成 query
        assert!(
            matches!(intent, crate::agent::Intent::DeployPipeline { .. }),
            "缓存未命中时应保留 deploy action，实际: {:?}",
            std::mem::discriminant(&intent)
        );
        let (job, branch) = crate::agent::intent::extract_fields(&intent);
        assert_eq!(job, Some("ds-pkg".into()));
        assert_eq!(branch, Some("de".into()));
    }

    #[tokio::test]
    async fn cache_miss_preserves_action_query() {
        let router = make_router_with_empty_cache().await;
        let (intent, _) = router.identify("查询 ds-pkg/de").await;
        assert!(
            matches!(intent, crate::agent::Intent::QueryPipeline { .. }),
            "缓存未命中时应保留 query action"
        );
    }

    #[tokio::test]
    async fn cache_miss_ds_pk_de_still_deploy() {
        let router = make_router_with_empty_cache().await;
        let (intent, _) = router.identify("部署 ds-pk/de").await;
        assert!(
            matches!(intent, crate::agent::Intent::DeployPipeline { .. }),
            "缓存未命中时应保留 deploy action"
        );
        let (job, branch) = crate::agent::intent::extract_fields(&intent);
        assert_eq!(job, Some("ds-pk".into()));
        assert_eq!(branch, Some("de".into()));
    }
}
