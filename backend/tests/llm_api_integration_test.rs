//! LLM API Integration Tests — 通过后端 API 测试双提供商
//!
//! 后端自动从环境变量加载 LLM 配置，测试只需调用 API 即可。
//! 后端必须在 8080 端口运行：cargo run
//!
//! cargo test --test llm_api_integration_test

use serde::Serialize;

// ── Test Helpers ──

#[derive(Serialize, Debug)]
struct AgentRequest {
    prompt: String,
    task_type: String,
}

async fn is_backend_reachable() -> bool {
    let client = reqwest::Client::new();
    client
        .get("http://localhost:8080/api/cache")
        .send()
        .await
        .map(|r| r.status().is_success())
        .unwrap_or(false)
}

// ── LLM Config API Tests ──

/// 测试 LLM 配置 API 只读（GET），验证配置已正确加载
#[tokio::test]
async fn test_llm_config_readonly() {
    if !is_backend_reachable().await {
        println!("SKIP: Backend not running on port 8080");
        return;
    }

    let client = reqwest::Client::new();
    let resp = client
        .get("http://localhost:8080/api/llm/config")
        .send()
        .await
        .expect("Should reach /api/llm/config");

    assert!(resp.status().is_success(), "GET /api/llm/config should succeed");

    let body = resp.json::<serde_json::Value>().await.unwrap();
    assert!(body["success"].as_bool().unwrap(), "Should return success");
    let config = &body["config"];

    // 验证 OpenAI 配置已加载
    let openai = config["openai"].as_object().expect("Should have openai config");
    assert!(
        openai.get("api_key").is_some(),
        "OpenAI api_key should be loaded from env"
    );
    if let Some(api_key) = openai.get("api_key") {
        let key_str = api_key.as_str().unwrap_or("");
        assert!(
            key_str.contains("****"),
            "API key should be masked, got: {}",
            key_str
        );
    }

    // 验证 Anthropic 配置已加载
    let anthropic = config["anthropic"]
        .as_object()
        .expect("Should have anthropic config");
    assert!(
        anthropic.get("api_key").is_some(),
        "Anthropic api_key should be loaded from env"
    );

    println!("LLM config loaded from env: {:?}", body);
}

/// 测试 PUT /api/llm/config 已被移除（配置不可修改）
#[tokio::test]
async fn test_llm_config_no_put_endpoint() {
    if !is_backend_reachable().await {
        println!("SKIP: Backend not running on port 8080");
        return;
    }

    let client = reqwest::Client::new();
    let resp = client
        .put("http://localhost:8080/api/llm/config")
        .json(&serde_json::json!({
            "openai": { "api_key": "test" }
        }))
        .send()
        .await
        .expect("Should get a response");

    // PUT 端点应该不存在（404 或 405）
    assert!(
        resp.status() == 404 || resp.status() == 405,
        "PUT /api/llm/config should not exist, got status {}",
        resp.status()
    );
    println!("PUT endpoint correctly removed (status {})", resp.status());
}

/// 测试双提供商都可用时，配置 API 显示两个 provider
#[tokio::test]
async fn test_both_providers_configured() {
    if !is_backend_reachable().await {
        println!("SKIP: Backend not running on port 8080");
        return;
    }

    let client = reqwest::Client::new();
    let body = client
        .get("http://localhost:8080/api/llm/config")
        .send()
        .await
        .expect("Should reach /api/llm/config")
        .json::<serde_json::Value>()
        .await
        .unwrap();

    let config = &body["config"];

    // OpenAI 应该有 api_key 和 base_url
    let openai = config["openai"].as_object().unwrap();
    assert!(
        openai.get("api_key").map(|v| !v.as_str().unwrap().is_empty()).unwrap_or(false),
        "OpenAI should have api_key"
    );
    assert!(
        openai.get("base_url").is_some(),
        "OpenAI should have base_url"
    );

    // Anthropic 应该有 api_key 和 base_url
    let anthropic = config["anthropic"].as_object().unwrap();
    assert!(
        anthropic.get("api_key").map(|v| !v.as_str().unwrap().is_empty()).unwrap_or(false),
        "Anthropic should have api_key"
    );
    assert!(
        anthropic.get("base_url").is_some(),
        "Anthropic should have base_url"
    );

    println!(
        "Both providers configured: openai={}, anthropic={}",
        openai["base_url"],
        anthropic["base_url"]
    );
}

// ── Agent API Tests ──

/// 测试 Agent API 返回有效响应结构
/// 验证 LLM 被正确调用并返回结构化响应
#[tokio::test]
async fn test_agent_api_returns_valid_response() {
    if !is_backend_reachable().await {
        println!("SKIP: Backend not running on port 8080");
        return;
    }

    let client = reqwest::Client::new();
    let resp = client
        .post("http://localhost:8080/api/agent")
        .json(&AgentRequest {
            prompt: "查询 ds-pkg 状态".to_string(),
            task_type: "Query".to_string(),
        })
        .send()
        .await
        .expect("Should reach /api/agent");

    let status = resp.status();
    assert!(status.is_success(), "Agent API should return 200, got {}", status);

    let body = resp.json::<serde_json::Value>().await.unwrap();
    // 验证响应结构完整
    assert!(body.get("success").is_some(), "Should have 'success' field");
    assert!(body.get("output").is_some(), "Should have 'output' field");
    assert!(body.get("steps").is_some(), "Should have 'steps' field");
    assert!(body["steps"].is_array(), "Steps should be an array");

    println!("Agent API response valid: {:?}", body);
}
