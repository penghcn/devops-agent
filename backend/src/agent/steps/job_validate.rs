use super::super::step::{Step, StepContext, StepResult};
use crate::tools::jenkins;

pub struct JobValidateStep;

#[async_trait::async_trait]
impl Step for JobValidateStep {
    fn name(&self) -> &str {
        "JobValidate"
    }

    async fn execute(&self, ctx: &mut StepContext) -> StepResult {
        let job_name = match &ctx.job_name {
            Some(j) => j.clone(),
            None => {
                return StepResult::Abort {
                    reason: "缺少 job_name".to_string(),
                };
            }
        };

        let (exists, job_type, display_name) =
            match jenkins::check_job_exists(&job_name, &ctx.config).await {
                Ok(info) => info,
                Err(e) => {
                    return StepResult::Failed {
                        error: e.to_string(),
                    };
                }
            };

        if !exists {
            return StepResult::Failed {
                error: format!("Jenkins 中不存在 Job: {}", display_name),
            };
        }

        // 多分支 Pipeline：检查 branch 是否在缓存的分支列表中
        if matches!(job_type, jenkins::JobTypeInfo::MultiBranchPipeline) {
            let branch = match &ctx.branch {
                Some(b) if !b.is_empty() => b,
                Some(_) => {
                    return StepResult::Abort {
                        reason: "分支名为空".to_string(),
                    };
                }
                None => {
                    return StepResult::Abort {
                        reason: "多分支 Pipeline 缺少 branch，需指定分支".to_string(),
                    };
                }
            };

            // 从缓存中检查分支是否存在
            if let Some(cache_mgr) = &ctx.cache {
                let branches = cache_mgr.get_branches(&display_name).await;
                if !branches.contains(branch) {
                    let suggestion =
                        if let Some(suggestion) = find_closest_branch(branch, &branches) {
                            format!("，是否想使用 '{}'?", suggestion)
                        } else {
                            String::new()
                        };
                    return StepResult::Failed {
                        error: format!("分支 '{}' 不存在{}", branch, suggestion),
                    };
                }
            }
        }

        // 将校验结果存入 pipeline_status（复用已有字段）
        use serde_json::json;
        ctx.pipeline_status = Some(json!({
            "job_exists": true,
            "job_type": match job_type {
                jenkins::JobTypeInfo::MultiBranchPipeline => "pipeline_multibranch",
                jenkins::JobTypeInfo::Pipeline => "pipeline",
                jenkins::JobTypeInfo::Job => "job",
            },
            "job_name": display_name,
        }));

        StepResult::Success {
            message: format!(
                "Job 校验通过: {} (类型: {})",
                display_name,
                match job_type {
                    jenkins::JobTypeInfo::MultiBranchPipeline => "Pipeline 多分支",
                    jenkins::JobTypeInfo::Pipeline => "Pipeline",
                    jenkins::JobTypeInfo::Job => "Job",
                }
            ),
        }
    }
}

/// 模糊匹配最接近的分支名
fn find_closest_branch(branch: &str, branches: &[String]) -> Option<String> {
    branches
        .iter()
        .min_by_key(|b| levenshtein_distance(branch, b))
        .filter(|b| levenshtein_distance(branch, b) <= 1)
        .cloned()
}

/// Levenshtein 编辑距离
fn levenshtein_distance(a: &str, b: &str) -> usize {
    let a_len = a.len();
    let b_len = b.len();
    let mut dp = vec![vec![0usize; b_len + 1]; a_len + 1];

 for (i, row) in dp.iter_mut().enumerate() {
        row[0] = i;
    }
    for (j, cell) in dp[0].iter_mut().enumerate() {
        *cell = j;
    }

    for i in 1..=a_len {
        for j in 1..=b_len {
            let cost = if a.as_bytes()[i - 1] == b.as_bytes()[j - 1] {
                0
            } else {
                1
            };
            dp[i][j] = (dp[i - 1][j] + 1)
                .min(dp[i][j - 1] + 1)
                .min(dp[i - 1][j - 1] + cost);
        }
    }

    dp[a_len][b_len]
}
