use crate::agent::{AgentRequest, AgentResponse, StreamEvent};
use crate::config::Config;
use crate::llm::LlmConfigStore;
use crate::sandbox::SandboxFactory;
use crate::tools::jenkins_cache::{JenkinsCache, JenkinsCacheManager};
use axum::{
    Json, Router,
    body::Body,
    http::{HeaderValue, Request, StatusCode},
    response::sse::{Event, Sse},
    routing::{get, post},
    serve,
};
use futures::Stream;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio_stream::wrappers::ReceiverStream;
use tower_http::cors::{Any, CorsLayer};

#[derive(Clone)]
pub struct AppState {
    pub config: Config,
    pub cache_manager: Arc<JenkinsCacheManager>,
    pub llm_config_store: Arc<LlmConfigStore>,
    pub sandbox_factory: Arc<SandboxFactory>,
}

/// Start the HTTP server
pub async fn run(state: Arc<AppState>) -> anyhow::Result<()> {
    // 构建 CORS 配置：使用配置的 origin 列表，解析失败则回退到 Any
    let cors = build_cors(&state.config.cors_origins);

    let port = state.config.backend_port;
    let app = Router::new()
        .route("/api/agent", post(handle_agent))
        .route("/api/agent/stream", post(handle_agent_stream))
        .route("/api/cache", get(handle_cache))
        .route("/api/llm/config", get(handle_get_llm_config))
        .layer(cors)
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(SocketAddr::from(([0, 0, 0, 0], port))).await?;
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

/// SSE 流式 Agent 处理 — 每个 Step 完成后立即推送
async fn handle_agent_stream(
    axum::extract::State(state): axum::extract::State<Arc<AppState>>,
    req: Request<Body>,
) -> Result<Sse<impl Stream<Item = Result<Event, axum::http::Error>>>, StatusCode> {
    check_api_key(&state.config, &req)?;

    let body = axum::body::to_bytes(req.into_body(), 10 * 1024 * 1024)
        .await
        .map_err(|_| StatusCode::BAD_REQUEST)?;
    let req: AgentRequest = serde_json::from_slice(&body).map_err(|_| StatusCode::BAD_REQUEST)?;

    let config = state.config.clone();
    let cache_manager = state.cache_manager.clone();
    let llm_config_store = state.llm_config_store.clone();

    // Internal channel: Step events
    let (event_tx, event_rx) = tokio::sync::mpsc::channel::<StreamEvent>(32);
    let event_tx = Arc::new(event_tx);
    // External channel: SSE Events
    let (sse_tx, sse_rx) = tokio::sync::mpsc::channel::<Result<Event, axum::http::Error>>(32);

    // Forwarder: convert StreamEvent → SSE Event
    tokio::spawn(async move {
        let mut rx = event_rx;
        while let Some(event) = rx.recv().await {
            let data = serde_json::to_string(&event).unwrap_or_default();
            if sse_tx.send(Ok(Event::default().data(data))).await.is_err() {
                break;
            }
        }
    });

    let tx_clone = event_tx.clone();
    tokio::spawn(async move {
        let response =
            process_request_stream(req, &config, cache_manager, &llm_config_store, tx_clone).await;

        let success = response
            .steps
            .last()
            .map(|step| {
                step.result.contains("成功")
                    && !step.result.contains("失败")
                    && !step.result.contains("中止")
            })
            .unwrap_or(false);

        let output = response
            .steps
            .iter()
            .find(|step| step.result.contains("失败") || step.result.contains("中止"))
            .map(|step| step.result.clone())
            .unwrap_or_else(|| "处理完成".to_string());

        let _ = event_tx
            .send(StreamEvent::Complete {
                success,
                output,
                structured_output: response.structured_output,
                steps: response.steps,
                corrections: response.corrections,
            })
            .await;
    });

    let stream = ReceiverStream::new(sse_rx);
    Ok(Sse::new(stream))
}

async fn process_request_stream(
    req: AgentRequest,
    config: &Config,
    cache: Arc<JenkinsCacheManager>,
    store: &LlmConfigStore,
    sender: Arc<tokio::sync::mpsc::Sender<StreamEvent>>,
) -> AgentResponse {
    let llm_provider = store.build_router();
    let default_model = store
        .snapshot()
        .default_model_flash()
        .unwrap_or_else(|| "gpt-4o-mini".to_string());

    let intent_router =
        crate::agent::IntentRouter::with_llm(cache.clone(), llm_provider.clone(), &default_model);

    let (intent, corrections) = intent_router.identify(&req.prompt).await;

    let chain = crate::agent::chain_mapping::to_chain_with_prompt(
        &intent,
        &req.prompt,
        llm_provider.clone(),
        default_model.clone(),
    );

    let (job_name, branch) = crate::agent::intent::extract_fields(&intent);

    let mut ctx = crate::agent::StepContext::new(
        req.prompt.clone(),
        req.task_type,
        job_name,
        branch,
        Arc::new(config.clone()),
    )
    .with_cache(intent_router.cache().clone())
    .with_llm_provider(llm_provider)
    .with_llm_model(default_model);

    for c in &corrections {
        ctx = ctx.add_correction(c.kind.clone(), c.original.clone(), c.corrected.clone());
    }

    // execute_stream needs Sender, not Arc<Sender> — clone from Arc
    let sender_inner = (*sender).clone();
    let (final_ctx, _steps) = chain.execute_stream(ctx, sender_inner).await;

    let structured_output = final_ctx.structured_analysis.clone();

    AgentResponse {
        success: true,
        output: "".to_string(),
        structured_output,
        steps: final_ctx.steps,
        corrections: final_ctx.corrections.clone(),
    }
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

/// 构建 CORS 中间件。解析配置的 origin 列表，失败则回退到 Any。
fn build_cors(origins: &[String]) -> CorsLayer {
    let allowed: Vec<HeaderValue> = origins.iter().filter_map(|o| o.parse().ok()).collect();

    if allowed.is_empty() {
        CorsLayer::new()
            .allow_methods(Any)
            .allow_headers(Any)
            .allow_origin(Any)
    } else {
        CorsLayer::new()
            .allow_methods(Any)
            .allow_headers(Any)
            .allow_origin(allowed)
    }
}
