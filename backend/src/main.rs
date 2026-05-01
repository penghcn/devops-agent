use axum::serve;
use axum::{
    Json, Router,
    extract::State,
    response::IntoResponse,
    routing::{get, post},
};
use std::sync::Arc;
use tower_http::cors::{Any, CorsLayer};

use devops_agent::llm::LlmConfigStore;
use devops_agent::tools::jenkins_cache::{JenkinsCache, JenkinsCacheManager};

#[derive(Clone)]
struct AppState {
    config: devops_agent::config::Config,
    cache_manager: Arc<JenkinsCacheManager>,
    llm_config_store: Arc<LlmConfigStore>,
}

#[tokio::main]
async fn main() {
    // 初始化日志（使用本地时区）
    if let Err(e) = yunli::setup_logger() {
        tracing::error!("Init log error: {}", e);
    }

    // 加载配置
    let config = devops_agent::config::Config::from_env();
    let cache_manager = Arc::new(JenkinsCacheManager::new(config.clone()));

    // 初始化 LLM 配置存储（从环境变量加载）
    let llm_config_store = Arc::new(init_llm_config_store(&config));

    // 启动时异步加载缓存
    let cache_clone = cache_manager.clone();
    tokio::spawn(async move {
        match cache_clone.refresh().await {
            Ok(()) => tracing::info!("Jenkins cache loaded successfully"),
            Err(e) => tracing::error!("Failed to load Jenkins cache: {}", e),
        }
    });

    // 每分钟刷新缓存
    let cache_refresh = cache_manager.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(60));
        loop {
            interval.tick().await;
            match cache_refresh.refresh().await {
                Ok(()) => tracing::info!("Jenkins cache refreshed"),
                Err(e) => tracing::warn!("Jenkins cache refresh failed: {}", e),
            }
        }
    });

    let state = Arc::new(AppState {
        config,
        cache_manager,
        llm_config_store,
    });

    // 构建路由
    let app = Router::new()
        .route("/api/agent", post(handle_agent))
        .route("/api/cache", get(handle_cache))
        .route("/api/llm/config", get(handle_get_llm_config))
        .layer(CorsLayer::new().allow_origin(Any))
        .with_state(state);

    tracing::info!("Server running on http://localhost:8080");
    let listener = match tokio::net::TcpListener::bind("0.0.0.0:8080").await {
        Ok(l) => l,
        Err(e) => {
            tracing::error!("Failed to bind to port 8080: {}", e);
            eprintln!("错误: 无法绑定端口 8080 (端口可能已被占用)");
            std::process::exit(1);
        }
    };
    if let Err(e) = serve(listener, app).await {
        tracing::error!("Server error: {}", e);
        eprintln!("服务器运行出错: {}", e);
        std::process::exit(1);
    }
}

/// 从环境变量初始化 LlmConfigStore
fn init_llm_config_store(config: &devops_agent::config::Config) -> LlmConfigStore {
    LlmConfigStore::from_env(
        config.openai_api_key.as_deref(),
        config.openai_base_url.as_deref(),
        config.anthropic_api_key.as_deref(),
        config.anthropic_base_url.as_deref(),
    )
}

async fn handle_agent(
    State(state): State<Arc<AppState>>,
    Json(req): Json<devops_agent::agent::AgentRequest>,
) -> impl IntoResponse {
    let response = devops_agent::agent::process_request_with_store(
        req,
        &state.config,
        state.cache_manager.clone(),
        &state.llm_config_store,
    )
    .await;
    Json(response)
}

async fn handle_cache(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let cache = state.cache_manager.get_cached().await;
    match cache {
        Some(c) => Json(c),
        None => Json(JenkinsCache {
            jobs: vec![],
            last_refresh: "未加载".to_string(),
        }),
    }
}

/// GET /api/llm/config — 获取当前 LLM 配置（api_key 已脱敏）
async fn handle_get_llm_config(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let snapshot = state.llm_config_store.snapshot().with_masked_keys();
    Json(serde_json::json!({
        "success": true,
        "config": snapshot
    }))
}

