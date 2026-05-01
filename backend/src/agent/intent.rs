use serde_json::Value;

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
pub fn replace_intent_fields(
    intent: &Intent,
    job_name: String,
    branch: Option<String>,
    job_type: JobType,
) -> Intent {
    match intent {
        Intent::DeployPipeline { .. } => Intent::DeployPipeline {
            job_name,
            branch,
            job_type,
        },
        Intent::BuildPipeline { .. } => Intent::BuildPipeline {
            job_name,
            branch,
            job_type,
        },
        Intent::QueryPipeline { .. } => Intent::QueryPipeline {
            job_name,
            branch,
            job_type,
        },
        Intent::AnalyzeBuild { .. } => Intent::AnalyzeBuild {
            job_name,
            branch,
            job_type,
        },
        Intent::General => Intent::General,
    }
}

/// Error returned when intent JSON parsing fails
#[derive(Debug)]
pub struct ParseIntentError;

impl std::fmt::Display for ParseIntentError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "failed to parse intent JSON")
    }
}

impl std::error::Error for ParseIntentError {}

/// Parse serde_json::Value directly into Intent (no string round-trip).
/// Used by the LLM path where we already have a deserialized Value.
pub fn intent_from_value(json: serde_json::Value) -> Result<Intent, ParseIntentError> {
    let obj = json.as_object().ok_or(ParseIntentError)?;

    let action = obj.get("action").and_then(|v| v.as_str()).unwrap_or("");
    let job_name = obj
        .get("job_name")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .ok_or(ParseIntentError)?;
    let branch = obj
        .get("branch")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let job_type_str = obj
        .get("job_type")
        .and_then(|v| v.as_str())
        .unwrap_or("standard");

    let job_type = match job_type_str {
        "branch" => JobType::Branch,
        _ => JobType::Standard,
    };

    match action {
        "deploy" => Ok(Intent::DeployPipeline {
            job_name,
            branch,
            job_type,
        }),
        "build" => Ok(Intent::BuildPipeline {
            job_name,
            branch,
            job_type,
        }),
        "query" => Ok(Intent::QueryPipeline {
            job_name,
            branch,
            job_type,
        }),
        "analyze" => Ok(Intent::AnalyzeBuild {
            job_name,
            branch,
            job_type,
        }),
        _ => Err(ParseIntentError),
    }
}

/// Parse LLM JSON response string into Intent.
/// Extracts JSON from markdown code blocks if present, then delegates to intent_from_value.
pub fn parse_intent_json(response: &str) -> Result<Intent, ParseIntentError> {
    let json: Value = match serde_json::from_str(response) {
        Ok(v) => v,
        Err(_) => {
            let start = response.find('{').unwrap_or(0);
            let end = response.rfind('}').map(|i| i + 1).unwrap_or(response.len());
            serde_json::from_str(&response[start..end]).map_err(|_| ParseIntentError)?
        }
    };

    intent_from_value(json)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn intent_from_value_deploy() {
        let json = serde_json::json!({
            "action": "deploy",
            "job_name": "backend-api",
            "branch": "main",
            "job_type": "branch"
        });
        let intent = intent_from_value(json).unwrap();
        assert!(matches!(intent, Intent::DeployPipeline { .. }));
        let (job, branch) = extract_fields(&intent);
        assert_eq!(job, Some("backend-api".into()));
        assert_eq!(branch, Some("main".into()));
    }

    #[test]
    fn intent_from_value_build_no_branch() {
        let json = serde_json::json!({
            "action": "build",
            "job_name": "frontend-app"
        });
        let intent = intent_from_value(json).unwrap();
        assert!(matches!(intent, Intent::BuildPipeline { .. }));
        assert!(!intent.branch_is_some());
    }

    #[test]
    fn intent_from_value_query() {
        let json = serde_json::json!({
            "action": "query",
            "job_name": "ds-pkg",
            "branch": "dev"
        });
        let intent = intent_from_value(json).unwrap();
        assert!(matches!(intent, Intent::QueryPipeline { .. }));
    }

    #[test]
    fn intent_from_value_analyze() {
        let json = serde_json::json!({
            "action": "analyze",
            "job_name": "ds-pkg"
        });
        let intent = intent_from_value(json).unwrap();
        assert!(matches!(intent, Intent::AnalyzeBuild { .. }));
    }

    #[test]
    fn intent_from_value_invalid_action() {
        let json = serde_json::json!({
            "action": "unknown",
            "job_name": "test"
        });
        assert!(intent_from_value(json).is_err());
    }

    #[test]
    fn intent_from_value_missing_job_name() {
        let json = serde_json::json!({
            "action": "deploy"
        });
        assert!(intent_from_value(json).is_err());
    }

    #[test]
    fn intent_from_value_not_object() {
        let json = serde_json::json!([1, 2, 3]);
        assert!(intent_from_value(json).is_err());
    }

    #[test]
    fn parse_intent_json_with_markdown() {
        let response = "```json\n{\"action\":\"build\",\"job_name\":\"test-app\"}\n```";
        let intent = parse_intent_json(response).unwrap();
        assert!(matches!(intent, Intent::BuildPipeline { .. }));
    }

    #[test]
    fn parse_intent_json_plain() {
        let response = r#"{"action":"deploy","job_name":"api","branch":"dev","job_type":"branch"}"#;
        let intent = parse_intent_json(response).unwrap();
        assert!(matches!(intent, Intent::DeployPipeline { .. }));
    }
}
