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
use devops_agent::llm::{ChatRequest, Message};
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

    // 打印启动配置概览
    let snapshot = llm_config_store.snapshot();
    let mut provider_strs = Vec::new();
    for pc in &snapshot.providers {
        if pc.api_key.is_some() {
            let model = pc.model_flash.as_deref().unwrap_or("(not set)");
            let base = pc.base_url.as_deref().unwrap_or("(default)");
            provider_strs.push(format!("{}(model={}, base={})", pc.id, model, base));
        }
    }
    tracing::info!(
        version = "0.1.0",
        default_provider = %snapshot.default_provider,
        providers = provider_strs.join(", "),
        "DevOps Agent starting"
    );

    // 启动时异步加载缓存
    let cache_clone = cache_manager.clone();
    tokio::spawn(async move {
        match cache_clone.refresh().await {
            Ok(()) => {
                // 加载完成后输出缓存概览
                let cached = cache_clone.get_cached().await;
                if let Some(c) = cached {
                    tracing::info!(jobs = c.jobs.len(), "Jenkins cache loaded");
                } else {
                    tracing::info!("Jenkins cache loaded (no jobs)");
                }
            }
            Err(e) => tracing::error!("Failed to load Jenkins cache: {}", e),
        }
    });

    // 每 5 分钟异步校验 LLM 是否正常工作
    let llm_store_clone = llm_config_store.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(300));
        loop {
            interval.tick().await;
            let router = match llm_store_clone.build_router() {
                Some(r) => r,
                None => {
                    tracing::warn!("No LLM provider configured for health check");
                    continue;
                }
            };
            let req = ChatRequest {
                model: String::new(),
                messages: vec![Message::User {
                    content: "你好".to_string(),
                }],
                tools: None,
                temperature: Some(0.0),
            };
            match tokio::time::timeout(std::time::Duration::from_secs(15), router.llm_call(&req))
                .await
            {
                Ok(Ok(_)) => {
                    tracing::info!("LLM health check passed");
                }
                Ok(Err(e)) => {
                    tracing::warn!("LLM health check failed: {}", e);
                }
                Err(_) => {
                    tracing::warn!("LLM health check timed out");
                }
            }
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
    LlmConfigStore::from_providers(
        config.llm_providers.clone(),
        config.default_provider.clone(),
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
