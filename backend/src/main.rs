use std::sync::Arc;

use devops_agent::api::AppState;
use devops_agent::config::Config;
use devops_agent::db;
use devops_agent::db::DbPool;
use devops_agent::llm::{ChatRequest, LlmConfigStore};
use devops_agent::sandbox::{CubeSandboxConfig, SandboxFactory};
use devops_agent::tools::jenkins_cache::JenkinsCacheManager;

#[tokio::main]
async fn main() {
    if let Err(e) = run().await {
        tracing::error!("Server error: {}", e);
        std::process::exit(1);
    }
}

async fn run() -> anyhow::Result<()> {
    yunli::setup_logger()?;

    let config = Config::from_file();
    let backend_port = config.backend_port;
    let port = config.port;
    let cache_manager = Arc::new(JenkinsCacheManager::new(config.clone()));
    let llm_config_store = Arc::new(LlmConfigStore::from_providers(
        config.llm_providers.clone(),
        config.default_provider.clone(),
    ));

    // 构建沙箱工厂并初始化（异步检测后端可用性）
    let cube_config = CubeSandboxConfig {
        api_url: config.sandbox.cubesandbox_api_url.clone(),
        api_key: config.sandbox.cubesandbox_api_key.clone(),
        template_id: config.sandbox.cubesandbox_template_id.clone(),
        timeout_secs: config.sandbox.cubesandbox_timeout,
        allow_internet: config.sandbox.cubesandbox_allow_internet,
        envd_port: config.sandbox.cubesandbox_envd_port,
        envd_url_template: config.sandbox.cubesandbox_envd_url_template.clone(),
    };
    let sandbox_factory = Arc::new(create_sandbox_factory(
        &config.sandbox.backends,
        &cube_config,
    ));
    sandbox_factory.init().await;

    // 写端口文件供 run.sh 使用
    let port_dir = std::env::var("DEVOPS_LOG_DIR").unwrap_or_else(|_| "logs".to_string());
    if let Err(e) = std::fs::write(format!("{port_dir}/backend.port"), backend_port.to_string()) {
        tracing::warn!("Failed to write backend.port: {}", e);
    }
    if let Err(e) = std::fs::write(format!("{port_dir}/frontend.port"), port.to_string()) {
        tracing::warn!("Failed to write frontend.port: {}", e);
    }

    log_startup(&llm_config_store);
    spawn_cache_loader(cache_manager.clone());
    spawn_llm_health_check(llm_config_store.clone());
    spawn_cache_refresher(cache_manager.clone());

    // 初始化 PostgreSQL 连接池 + 迁移
    let db = match db::pool::connect(&config.database).await {
        Ok(pool) => {
            db::migrate::run_migrations(&pool)
                .await
                .expect("Database migration failed");
            pool
        }
        Err(e) => {
            tracing::warn!(error = %e, "PostgreSQL connection failed, continuing without DB");
            tracing::warn!("Features requiring database will be unavailable");
            panic!("PostgreSQL is required. Please configure [database] in config.toml");
        }
    };

    let state = Arc::new(AppState {
        config,
        cache_manager,
        llm_config_store,
        sandbox_factory,
        db: db.clone(),
    });

    spawn_knowledge_cleanup(db);

    devops_agent::api::run(state).await
}

fn log_startup(llm_config_store: &LlmConfigStore) {
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
}

fn spawn_cache_loader(cm: Arc<JenkinsCacheManager>) {
    tokio::spawn(async move {
        match cm.refresh().await {
            Ok(()) => {
                if let Some(c) = cm.get_cached().await {
                    tracing::info!(jobs = c.jobs.len(), "Jenkins cache loaded");
                } else {
                    tracing::info!("Jenkins cache loaded (no jobs)");
                }
            }
            Err(e) => tracing::error!("Failed to load Jenkins cache: {}", e),
        }
    });
}

fn spawn_llm_health_check(llm_config_store: Arc<LlmConfigStore>) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(300));
        loop {
            interval.tick().await;
            let router = llm_config_store.build_router();
            let req = ChatRequest::user_prompt("你好".to_string());
            match tokio::time::timeout(std::time::Duration::from_secs(15), router.llm_call(&req))
                .await
            {
                Ok(Ok(_)) => tracing::info!("LLM health check passed"),
                Ok(Err(e)) => tracing::warn!("LLM health check failed: {}", e),
                Err(_) => tracing::warn!("LLM health check timed out"),
            }
        }
    });
}

fn spawn_cache_refresher(cm: Arc<JenkinsCacheManager>) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(60));
        loop {
            interval.tick().await;
            match cm.refresh().await {
                Ok(()) => tracing::info!("Jenkins cache refreshed"),
                Err(e) => tracing::warn!("Jenkins cache refresh failed: {}", e),
            }
        }
    });
}

fn spawn_knowledge_cleanup(db: DbPool) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(86400));
        loop {
            interval.tick().await;
            let deleted = devops_agent::knowledge::store::cleanup_expired(&db).await;
            if deleted > 0 {
                tracing::info!(deleted, "Expired knowledge entries cleaned up");
            }
        }
    });
}

#[cfg(target_os = "linux")]
fn create_sandbox_factory(
    backends: &[devops_agent::sandbox::SandboxBackend],
    cube_config: &CubeSandboxConfig,
) -> SandboxFactory {
    use devops_agent::sandbox::MicrosandboxConfig;
    SandboxFactory::from_config(
        backends.to_vec(),
        MicrosandboxConfig::default(),
        cube_config.clone(),
    )
}

#[cfg(not(target_os = "linux"))]
fn create_sandbox_factory(
    backends: &[devops_agent::sandbox::SandboxBackend],
    cube_config: &CubeSandboxConfig,
) -> SandboxFactory {
    SandboxFactory::from_config(backends.to_vec(), cube_config.clone())
}
