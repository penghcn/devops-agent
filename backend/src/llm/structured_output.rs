//! Structured Output — schema-constrained LLM responses with automatic retry.
//!
//! Wraps LlmProvider to enforce JSON Schema output format.
//! If the LLM response doesn't match the schema, automatically retries
//! with a correction prompt (up to max_retries times).
//!
//! # Example
//!
//! ```ignore
//! #[derive(serde::Deserialize)]
//! struct IntentResult {
//!     action: String,      // "deploy" | "build" | "query" | "analyze"
//!     job_name: String,
//!     branch: Option<String>,
//! }
//!
//! let output = StructuredOutput::new(
//!     provider,
//!     "gpt-4o-mini".into(),
//!     json!({
//!       "type": "object",
//!       "required": ["action", "job_name"],
//!       "properties": {
//!         "action": {"type": "string", "enum": ["deploy","build","query","analyze"]},
//!         "job_name": {"type": "string"},
//!         "branch": {"type": "string"}
//!       }
//!     })
//! ).execute("部署 ds-pkg 到 dev 环境").await?;
//! ```

use std::sync::Arc;

use super::{ChatRequest, LlmError, LlmProvider, Message};

/// Errors that can occur during structured output extraction.
#[derive(Debug)]
pub enum StructuredOutputError {
    /// The underlying LLM call failed.
    LlmError(LlmError),
    /// JSON parsing failed for the given response.
    ParseError {
        /// The raw response from the LLM.
        response: String,
        /// The parse error detail.
        detail: String,
    },
    /// All retry attempts exhausted.
    MaxRetriesExceeded {
        /// Responses from each attempt.
        responses: Vec<String>,
    },
}

impl std::fmt::Display for StructuredOutputError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StructuredOutputError::LlmError(e) => write!(f, "LLM error: {}", e),
            StructuredOutputError::ParseError { response, detail } => {
                write!(f, "Parse error: {} (response: {})", detail, response)
            }
            StructuredOutputError::MaxRetriesExceeded { responses } => {
                write!(
                    f,
                    "Max retries exceeded after {} attempts: {:?}",
                    responses.len(),
                    responses
                        .iter()
                        .map(|r| &r[..r.len().min(50)])
                        .collect::<Vec<_>>()
                )
            }
        }
    }
}

impl std::error::Error for StructuredOutputError {}

/// Schema-constrained LLM output with automatic retry.
///
/// Wraps any `LlmProvider` and enforces that responses conform to
/// a JSON Schema. On parse failure, retries with a correction prompt.
pub struct StructuredOutput {
    /// The provider to call.
    provider: Arc<dyn LlmProvider>,
    /// Model name to use.
    pub model: String,
    /// JSON Schema defining the expected output format.
    schema: serde_json::Value,
    /// Maximum number of retry attempts (default: 3).
    pub max_retries: u32,
    /// System prompt template.
    system_prompt: String,
}

impl StructuredOutput {
    /// Create a new structured output wrapper.
    pub fn new(provider: Arc<dyn LlmProvider>, model: String, schema: serde_json::Value) -> Self {
        Self {
            provider,
            model,
            schema,
            max_retries: 3,
            system_prompt: "你是一个 AI 助手。只输出 JSON，不要输出其他内容。请按照以下 JSON Schema 输出:\n{schema}".to_string(),
        }
    }

    /// Set a custom system prompt.
    pub fn with_system_prompt(mut self, prompt: String) -> Self {
        self.system_prompt = prompt;
        self
    }

    /// Set maximum retry count.
    pub fn with_max_retries(mut self, retries: u32) -> Self {
        self.max_retries = retries;
        self
    }

    /// Execute the structured output request.
    ///
    /// Sends the user prompt to the LLM with schema constraints,
    /// parses the response, and retries on failure.
    pub async fn execute<T: serde::de::DeserializeOwned>(
        &self,
        user_prompt: &str,
    ) -> Result<T, StructuredOutputError> {
        let mut failed_responses: Vec<String> = Vec::new();

        for attempt in 0..self.max_retries {
            // Build request
            let system_content = self
                .system_prompt
                .replace("{schema}", &self.schema.to_string());

            let messages = if attempt == 0 {
                vec![
                    Message::System {
                        content: system_content,
                    },
                    Message::User {
                        content: user_prompt.to_string(),
                    },
                ]
            } else {
                // Retry: show the LLM its previous (failed) output so it can self-correct
                let last_response = failed_responses.last().map(|s| s.as_str()).unwrap_or("");
                let last_error = match serde_json::from_str::<serde_json::Value>(last_response) {
                    Ok(_) => "上一次输出格式不符合预期 schema".to_string(),
                    Err(e) => format!("上一次输出不是有效的 JSON: {}", e),
                };

                vec![
                    Message::System {
                        content: system_content.clone(),
                    },
                    Message::User {
                        content: user_prompt.to_string(),
                    },
                    Message::Assistant {
                        content: last_response.to_string(),
                        tool_calls: Vec::new(),
                    },
                    Message::User {
                        content: format!(
                            "你的上一次输出不符合 JSON Schema。错误: {}。\n\n请重新输出符合以下 Schema 的 JSON:\n{}",
                            last_error, self.schema
                        ),
                    },
                ]
            };

            let request = ChatRequest {
                model: self.model.clone(),
                messages,
                tools: None,
                temperature: Some(0.0),
            };

            // Call provider
            let response = match self.provider.chat(&request).await {
                Ok(r) => r,
                Err(e) => return Err(StructuredOutputError::LlmError(e)),
            };

            // Try to extract JSON
            match self.extract_and_parse(&response.content) {
                Ok(parsed) => return Ok(parsed),
                Err(parse_error) => {
                    failed_responses.push(response.content.clone());
                    tracing::warn!(
                        attempt = attempt + 1,
                        max_retries = self.max_retries,
                        error = %parse_error,
                        "Structured output parse failed, will retry"
                    );
                }
            }
        }

        Err(StructuredOutputError::MaxRetriesExceeded {
            responses: failed_responses,
        })
    }

    /// Extract JSON from LLM response and parse into target type.
    ///
    /// Extraction strategy:
    /// 1. Try direct JSON parse
    /// 2. Try extracting from ```json ... ``` code block
    /// 3. Try extracting from ``` ... ``` code block
    /// 4. Try extracting outermost { ... } braces
    /// 5. Return error with detail
    fn extract_and_parse<T: serde::de::DeserializeOwned>(
        &self,
        content: &str,
    ) -> Result<T, String> {
        // Strategy 1: Try direct parse
        if let Ok(result) = serde_json::from_str::<T>(content.trim()) {
            return Ok(result);
        }

        // Strategy 2: Try ```json ... ``` code block
        if let Some(json_str) = Self::extract_json_codeblock(content, Some("json"))
            && let Ok(result) = serde_json::from_str::<T>(&json_str)
        {
            return Ok(result);
        }

        // Strategy 3: Try ``` ... ``` code block (without language tag)
        if let Some(json_str) = Self::extract_codeblock(content)
            && let Ok(result) = serde_json::from_str::<T>(&json_str)
        {
            return Ok(result);
        }

        // Strategy 4: Try outermost { ... } braces
        if let Some(json_str) = Self::extract_braces(content)
            && let Ok(result) = serde_json::from_str::<T>(&json_str)
        {
            return Ok(result);
        }

        // Strategy 5: Try parsing as JSON value to get error detail
        let trimmed = content.trim();
        match serde_json::from_str::<serde_json::Value>(trimmed) {
            Ok(v) => {
                // Valid JSON but doesn't match target type
                let parse_err = match serde_json::from_value::<T>(v.clone()) {
                    Ok(_) => "JSON matched but deserialization failed".to_string(),
                    Err(e) => e.to_string(),
                };
                Err(format!(
                    "JSON is valid but doesn't match expected type: {} (value: {})",
                    parse_err,
                    v.to_string().chars().take(100).collect::<String>()
                ))
            }
            Err(e) => Err(format!(
                "Failed to parse JSON: {} (content preview: {})",
                e,
                trimmed.chars().take(80).collect::<String>()
            )),
        }
    }

    /// Extract content from a ```json ... ``` code block.
    fn extract_json_codeblock(content: &str, _language: Option<&str>) -> Option<String> {
        // Match ```json\n...\n```
        let marker = "```json";
        if let Some(start) = content.find(marker) {
            let after_marker = &content[start + marker.len()..];
            // Find the closing ```
            if let Some(end) = after_marker.find("```") {
                let json_str = after_marker[..end].trim().to_string();
                if !json_str.is_empty() {
                    return Some(json_str);
                }
            }
        }
        None
    }

    /// Extract content from any ``` ... ``` code block.
    fn extract_codeblock(content: &str) -> Option<String> {
        let marker = "```";
        if let Some(start) = content.find(marker) {
            let after_marker = &content[start + marker.len()..];
            // Skip optional language tag (e.g., "json\n" or "\n")
            let after_lang = if let Some(nl) = after_marker.find('\n') {
                &after_marker[nl + 1..]
            } else {
                after_marker
            };
            // Find the closing ```
            if let Some(end) = after_lang.find(marker) {
                let inner = after_lang[..end].trim().to_string();
                if !inner.is_empty() {
                    return Some(inner);
                }
            }
        }
        None
    }

    /// Extract the outermost { ... } from the content.
    fn extract_braces(content: &str) -> Option<String> {
        let start = content.find('{')?;
        // Find matching closing brace by counting
        let mut depth = 0i32;
        let mut end = None;
        for (i, c) in content[start..].char_indices() {
            match c {
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        end = Some(start + i + 1);
                        break;
                    }
                }
                _ => {}
            }
        }
        end.map(|e| content[start..e].to_string())
    }
}
