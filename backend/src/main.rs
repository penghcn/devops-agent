use std::sync::Arc;

use devops_agent::api::AppState;
use devops_agent::config::Config;
use devops_agent::llm::{ChatRequest, LlmConfigStore, Message};
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

    let config = Config::from_env();
    let cache_manager = Arc::new(JenkinsCacheManager::new(config.clone()));
    let llm_config_store = Arc::new(LlmConfigStore::from_providers(
        config.llm_providers.clone(),
        config.default_provider.clone(),
    ));

    log_startup(&llm_config_store);
    spawn_cache_loader(cache_manager.clone());
    spawn_llm_health_check(llm_config_store.clone());
    spawn_cache_refresher(cache_manager.clone());

    let state = Arc::new(AppState {
        config,
        cache_manager,
        llm_config_store,
    });

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
            let router = match llm_config_store.build_router() {
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
