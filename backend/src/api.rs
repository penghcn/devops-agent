use crate::agent::{AgentRequest, AgentResponse, StreamEvent};
use crate::config::Config;
use crate::db::DbPool;
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
    pub db: DbPool,
}

/// Start the HTTP server
pub async fn run(state: Arc<AppState>) -> anyhow::Result<()> {
    let cors = build_cors(&state.config.cors_origins);

    let port = state.config.backend_port;

    // 公开路由（无需认证）
    let auth_routes = Router::new()
        .route("/api/auth/gitlab/login", get(handle_gitlab_login))
        .route("/api/auth/gitlab/callback", get(handle_gitlab_callback));

    // 受保护路由（JWT Bearer 或 X-API-Key 双模式）
    let protected_routes = Router::new()
        .route("/api/agent", post(handle_agent))
        .route("/api/agent/stream", post(handle_agent_stream))
        .route("/api/cache", get(handle_cache))
        .route("/api/llm/config", get(handle_get_llm_config))
        .route("/api/knowledge/feedback", post(handle_knowledge_feedback))
        .route("/api/knowledge/learn", post(handle_knowledge_learn))
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            crate::auth::middleware::auth_guard,
        ));

    let app = Router::new()
        .merge(auth_routes)
        .merge(protected_routes)
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
    let body = axum::body::to_bytes(req.into_body(), 10 * 1024 * 1024)
        .await
        .map_err(|_| StatusCode::BAD_REQUEST)?;
    let req: AgentRequest = serde_json::from_slice(&body).map_err(|_| StatusCode::BAD_REQUEST)?;

    let config = state.config.clone();
    let cache_manager = state.cache_manager.clone();
    let llm_config_store = state.llm_config_store.clone();
    let db = state.db.clone();

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
            process_request_stream(req, &config, cache_manager, &llm_config_store, db, tx_clone).await;

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
                knowledge_hit: None,
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
    db: DbPool,
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

    // 构造知识库检索器
    let retriever = crate::knowledge::KnowledgeRetriever::new(
        db,
        config.embedding_api_key.clone().unwrap_or_default(),
    );

    let chain = crate::agent::chain_mapping::to_chain_with_prompt(
        &intent,
        &req.prompt,
        llm_provider.clone(),
        default_model.clone(),
        Some(Arc::new(retriever)),
    );

    let (job_name, branch) = crate::agent::intent::extract_fields(&intent);

    let mut ctx = crate::agent::StepContext::new(
        req.prompt.clone(),
        req.task_type,
        job_name,
        branch,
        Arc::new(config.clone()),
    )
    .with_cache(intent_router.cache().clone());

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
        knowledge_hit: None,
    }
}

async fn handle_cache(
    axum::extract::State(state): axum::extract::State<Arc<AppState>>,
    _req: Request<Body>,
) -> Result<Json<JenkinsCache>, StatusCode> {
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
    _req: Request<Body>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let snapshot = state.llm_config_store.snapshot().with_masked_keys();
    Ok(Json(serde_json::json!({
        "success": true,
        "config": snapshot
    })))
}

/// POST /api/knowledge/feedback
/// 接收用户对知识库条目的点赞/点踩反馈
async fn handle_knowledge_feedback(
    axum::extract::State(state): axum::extract::State<Arc<AppState>>,
    req: Request<Body>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let body = axum::body::to_bytes(req.into_body(), 1024)
        .await
        .map_err(|_| StatusCode::BAD_REQUEST)?;
    let feedback: FeedbackRequest =
        serde_json::from_slice(&body).map_err(|_| StatusCode::BAD_REQUEST)?;

    let learner = crate::knowledge::KnowledgeLearner::new(
        state.db.clone(),
        state
            .config
            .embedding_api_key
            .clone()
            .unwrap_or_default(),
    );

    match feedback.action.as_str() {
        "confirm" => {
            learner.confirm_entry(feedback.entry_id).await;
        }
        "deny" => {
            learner.deny_entry(feedback.entry_id).await;
        }
        _ => {
            return Err(StatusCode::BAD_REQUEST);
        }
    }

    Ok(Json(serde_json::json!({
        "success": true,
        "message": format!("Feedback recorded for entry {}", feedback.entry_id)
    })))
}

#[derive(serde::Deserialize)]
struct FeedbackRequest {
    entry_id: i32,
    action: String, // "confirm" | "deny"
}

/// POST /api/knowledge/learn
/// 用户点赞 LLM 生成的方案 → 写入知识库（Flow B 闭环）
async fn handle_knowledge_learn(
    axum::extract::State(state): axum::extract::State<Arc<AppState>>,
    req: Request<Body>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let body = axum::body::to_bytes(req.into_body(), 64 * 1024)
        .await
        .map_err(|_| StatusCode::BAD_REQUEST)?;
    let learn_req: LearnRequest =
        serde_json::from_slice(&body).map_err(|_| StatusCode::BAD_REQUEST)?;

    let learner = crate::knowledge::KnowledgeLearner::new(
        state.db.clone(),
        state
            .config
            .embedding_api_key
            .clone()
            .unwrap_or_default(),
    );

    // 使用方案文本作为错误文本提取指纹（不完美，但可用）
    let result = learner
        .on_confirm(&learn_req.solution, &learn_req.solution, None)
        .await;

    match result {
        Ok(_) => Ok(Json(serde_json::json!({
            "success": true,
            "message": "Knowledge entry created from AI solution"
        }))),
        Err(e) => {
            tracing::error!(error = %e, "Failed to learn from AI solution");
            Ok(Json(serde_json::json!({
                "success": true,
                "message": "Learned with warning"
            })))
        }
    }
}

#[derive(serde::Deserialize)]
struct LearnRequest {
    solution: String,
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

// ============ Auth Handlers ============

/// GET /api/auth/gitlab/login
/// 返回 GitLab 授权 URL，前端重定向
async fn handle_gitlab_login(
    axum::extract::State(state): axum::extract::State<Arc<AppState>>,
    axum::extract::Query(params): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> Json<serde_json::Value> {
    let redirect_uri = params
        .get("redirect_uri")
        .map(|s| s.as_str())
        .unwrap_or("http://localhost:3000");

    let auth_url = crate::auth::gitlab_oauth::auth_url(&state.config.auth, redirect_uri);
    Json(serde_json::json!({ "auth_url": auth_url }))
}

/// GET /api/auth/gitlab/callback?code=xxx&redirect_uri=yyy
/// GitLab OAuth 回调，用授权码换取 JWT
async fn handle_gitlab_callback(
    axum::extract::State(state): axum::extract::State<Arc<AppState>>,
    axum::extract::Query(params): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let code = match params.get("code") {
        Some(c) => c,
        None => return Err(StatusCode::BAD_REQUEST),
    };

    let redirect_uri = params
        .get("redirect_uri")
        .map(|s| s.as_str())
        .unwrap_or("http://localhost:3000");

    match crate::auth::gitlab_oauth::exchange_code(&state.config.auth, code, redirect_uri).await {
        Ok(login_result) => {
            let access_token = crate::auth::jwt::create_access_token(
                &login_result.username,
                &login_result.gitlab_id,
                "user",
                &state.config.auth.jwt_secret,
            );

            match access_token {
                Ok(token) => Ok(Json(serde_json::json!({
                    "access_token": token,
                    "username": login_result.username,
                    "avatar_url": login_result.avatar_url,
                }))),
                Err(e) => {
                    tracing::error!(error = %e, "Failed to create JWT token");
                    Err(StatusCode::INTERNAL_SERVER_ERROR)
                }
            }
        }
        Err(e) => {
            tracing::error!(error = %e, "GitLab OAuth exchange failed");
            Err(StatusCode::UNAUTHORIZED)
        }
    }
}
