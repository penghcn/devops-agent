use axum::{
    extract::State,
    routing::post,
    Router,
    Json,
    response::IntoResponse,
};
use axum::serve;
use tower_http::cors::{CorsLayer, Any};
use std::sync::Arc;
use devops_agent;

#[derive(Clone)]
struct AppState {
    config: devops_agent::config::Config,
}

#[tokio::main]
async fn main() {
    // 初始化日志（使用本地时区）
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .with_ansi(false)
        .with_timer(tracing_subscriber::fmt::time::LocalTime::rfc_3339())
        .init();
    
    // 加载配置
    let config = devops_agent::config::Config::from_env();
    let state = Arc::new(AppState { config });
    
    // 构建路由
    let app = Router::new()
        .route("/api/agent", post(handle_agent))
        .layer(CorsLayer::new().allow_origin(Any))
        .with_state(state);
    
    tracing::info!("Server running on http://localhost:3000");
    let listener = match tokio::net::TcpListener::bind("0.0.0.0:3000").await {
        Ok(l) => l,
        Err(e) => {
            tracing::error!("Failed to bind to port 3000: {}", e);
            eprintln!("错误: 无法绑定端口 3000 (端口可能已被占用)");
            std::process::exit(1);
        }
    };
    if let Err(e) = serve(listener, app).await {
        tracing::error!("Server error: {}", e);
        eprintln!("服务器运行出错: {}", e);
        std::process::exit(1);
    }
}

async fn handle_agent(
    State(state): State<Arc<AppState>>,
    Json(req): Json<devops_agent::agent::AgentRequest>,
) -> impl IntoResponse {
    let response = devops_agent::agent::process_request(req, &state.config).await;
    Json(response)
}