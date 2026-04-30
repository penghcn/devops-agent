use serde::Deserialize;

#[derive(Debug, Clone, PartialEq, Default)]
pub enum JobType {
    Standard,
    #[default]
    Branch,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Intent {
    DeployPipeline {
        job_name: String,
        branch: Option<String>,
        job_type: JobType,
    },
    BuildPipeline {
        job_name: String,
        branch: Option<String>,
        job_type: JobType,
    },
    QueryPipeline {
        job_name: String,
        branch: Option<String>,
        job_type: JobType,
    },
    AnalyzeBuild {
        job_name: String,
        branch: Option<String>,
        job_type: JobType,
    },
    General,
}

impl Intent {
    pub fn branch_is_some(&self) -> bool {
        matches!(
            self,
            Intent::DeployPipeline {
                branch: Some(_),
                ..
            } | Intent::BuildPipeline {
                branch: Some(_),
                ..
            } | Intent::QueryPipeline {
                branch: Some(_),
                ..
            } | Intent::AnalyzeBuild {
                branch: Some(_),
                ..
            }
        )
    }
}

/// Extract job_name and branch from an Intent
pub fn extract_fields(intent: &Intent) -> (Option<String>, Option<String>) {
    match intent {
        Intent::DeployPipeline {
            job_name, branch, ..
        }
        | Intent::BuildPipeline {
            job_name, branch, ..
        }
        | Intent::QueryPipeline {
            job_name, branch, ..
        }
        | Intent::AnalyzeBuild {
            job_name, branch, ..
        } => (Some(job_name.clone()), branch.clone()),
        Intent::General => (None, None),
    }
}

/// Replace job_name/branch/job_type in an Intent
pub fn replace_branch(
    intent: &Intent,
    job_name: String,
    branch: Option<String>,
    job_type: &str,
) -> Intent {
    let jt = if job_type == "pipeline_multibranch" || job_type == "branch" {
        JobType::Branch
    } else {
        JobType::Standard
    };
    match intent {
        Intent::DeployPipeline { .. } => Intent::DeployPipeline {
            job_name,
            branch,
            job_type: jt,
        },
        Intent::BuildPipeline { .. } => Intent::BuildPipeline {
            job_name,
            branch,
            job_type: jt,
        },
        Intent::QueryPipeline { .. } => Intent::QueryPipeline {
            job_name,
            branch,
            job_type: jt,
        },
        Intent::AnalyzeBuild { .. } => Intent::AnalyzeBuild {
            job_name,
            branch,
            job_type: jt,
        },
        Intent::General => Intent::General,
    }
}

/// Parse LLM JSON response into Intent
pub fn parse_intent_json(response: &str) -> Result<Intent, ()> {
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
