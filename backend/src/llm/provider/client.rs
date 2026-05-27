//! Shared HTTP call logic for LLM providers.
//!
//! Handles request dispatch, response reading, JSON parsing and error mapping.

use std::time;

use crate::llm::LlmError;

/// HTTP response body (raw text + parsed JSON).
#[derive(Debug)]
pub struct HttpResponse {
    pub status: u16,
    pub body: String,
    pub json: serde_json::Value,
}

/// Send an HTTP POST request and return the parsed response.
///
/// Common pattern shared by all providers:
/// 1. Log request
/// 2. POST with headers (configured via closure) + JSON body
/// 3. Read raw response text
/// 4. Parse JSON
/// 5. Return error on status >= 400
pub async fn http_call(
    client: &reqwest::Client,
    url: &str,
    body: &serde_json::Value,
    model: &str,
    provider: &str,
    configure: impl FnOnce(reqwest::RequestBuilder) -> reqwest::RequestBuilder,
) -> Result<HttpResponse, LlmError> {
    let request_id = format!(
        "{}-{}-{}",
        provider,
        chrono::Local::now().timestamp_millis(),
        yunli::util::generate_nonce_ascii_str(8)
    );

    tracing::info!(model = model, "LLM request {}", request_id);
    tracing::info!("LLM Req, body: {}", &body);

    let start = time::Instant::now();
    let request_builder = client
        .post(url)
        .json(body)
        .header("User-Agent", "devops_agent/0.1.0");
    let request_builder = configure(request_builder);

    let response = request_builder.send().await.map_err(|e| {
        if e.is_timeout() {
            LlmError::Timeout
        } else {
            LlmError::ApiError {
                status: 0,
                body: e.to_string(),
            }
        }
    })?;

    let status = response.status().as_u16();
    let raw_body = response.text().await.map_err(|e| LlmError::ParseError {
        detail: format!("Failed to read response body: {}", e),
    })?;

    tracing::info!("LLM Res {}, body: {}", status, &raw_body);
    if status >= 400 {
        return Err(LlmError::ApiError {
            status,
            body: raw_body,
        });
    }

    let json: serde_json::Value =
        serde_json::from_str(&raw_body).map_err(|e| LlmError::ParseError {
            detail: format!("Invalid JSON from {}: {}", model, e),
        })?;

    tracing::info!(
        model = model,
        "LLM done. Duration: {:.2}s. {}",
        start.elapsed().as_secs_f32(),
        request_id
    );

    Ok(HttpResponse {
        status,
        body: raw_body,
        json,
    })
}
