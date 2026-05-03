use axum::{
    Json, Router,
    body::Body,
    http::{Request, StatusCode},
    routing::{get, post},
    serve,
};
use std::{net::SocketAddr, sync::Arc};
use tower_http::cors::{Any, CorsLayer};

use crate::agent::{AgentRequest, AgentResponse};
use crate::config::Config;
use crate::llm::LlmConfigStore;
use crate::tools::jenkins_cache::{JenkinsCache, JenkinsCacheManager};

#[derive(Clone)]
pub struct AppState {
    pub config: Config,
    pub cache_manager: Arc<JenkinsCacheManager>,
    pub llm_config_store: Arc<LlmConfigStore>,
}

/// Start the HTTP server
pub async fn run(state: Arc<AppState>) -> anyhow::Result<()> {
    let cors = CorsLayer::new()
        .allow_methods(Any)
        .allow_headers(Any)
        .allow_origin(Any); // TODO: 生产环境使用 config.cors_origins

    let app = Router::new()
        .route("/api/agent", post(handle_agent))
        .route("/api/cache", get(handle_cache))
        .route("/api/llm/config", get(handle_get_llm_config))
        .layer(cors)
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(SocketAddr::from(([0, 0, 0, 0], 8080))).await?;
    tracing::info!("Server running on http://{}", listener.local_addr()?);
    serve(listener, app).await?;
    Ok(())
}

// ============ Handlers ============

async fn handle_agent(
    axum::extract::State(state): axum::extract::State<Arc<AppState>>,
    req: Request<Body>,
) -> Result<Json<AgentResponse>, StatusCode> {
    check_api_key(&state.config, &req)?;

    let body = axum::body::to_bytes(req.into_body(), 10 * 1024 * 1024)
        .await
        .map_err(|_| StatusCode::BAD_REQUEST)?;
    let req: AgentRequest = serde_json::from_slice(&body).map_err(|_| StatusCode::BAD_REQUEST)?;

    let response = crate::agent::process_request_with_store(
        req,
        &state.config,
        state.cache_manager.clone(),
        &state.llm_config_store,
    )
    .await;
    Ok(Json(response))
}

async fn handle_cache(
    axum::extract::State(state): axum::extract::State<Arc<AppState>>,
    req: Request<Body>,
) -> Result<Json<JenkinsCache>, StatusCode> {
    check_api_key(&state.config, &req)?;

    let cache = state.cache_manager.get_cached().await;
    match cache {
        Some(c) => Ok(Json(c)),
        None => Ok(Json(JenkinsCache {
            jobs: vec![],
            last_refresh: "未加载".to_string(),
        })),
    }
}

async fn handle_get_llm_config(
    axum::extract::State(state): axum::extract::State<Arc<AppState>>,
    req: Request<Body>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    check_api_key(&state.config, &req)?;

    let snapshot = state.llm_config_store.snapshot().with_masked_keys();
    Ok(Json(serde_json::json!({
        "success": true,
        "config": snapshot
    })))
}

/// Check API key from request headers
fn check_api_key(config: &Config, req: &Request<Body>) -> Result<(), StatusCode> {
    if let Some(ref api_key) = config.api_key {
        let valid = req
            .headers()
            .get("X-API-Key")
            .and_then(|h| h.to_str().ok())
            .map(|k| k == api_key.as_str())
            .unwrap_or(false);

        if valid {
            return Ok(());
        }
        return Err(StatusCode::UNAUTHORIZED);
    }
    Ok(())
}
