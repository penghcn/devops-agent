# Provider 抽象实现计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 消除 OpenAIConfig/AnthropicConfig 和 OpenAIProvider/AnthropicProvider 之间的重复代码，提取 BaseConfig + ProviderAdapter trait + GenericProvider。

**Architecture:** 提取 `BaseConfig` 作为公共配置，定义 `ProviderAdapter` trait 封装各 provider 的 API 格式差异，用 `GenericProvider<T>` 统一实现 `LlmProvider` trait。对外通过 type alias 保持类型名不变。

**Tech Stack:** Rust, async-trait, reqwest, serde_json

---

### Task 1: 创建 base.rs — BaseConfig + ProviderAdapter trait + GenericProvider

**Files:**
- Create: `backend/src/llm/provider/base.rs`
- Modify: `backend/src/llm/provider/mod.rs` (添加 `pub mod base`)

- [ ] **Step 1: 创建 base.rs**

```rust
//! Provider 公共基础设施。
//!
//! BaseConfig — 公共配置
//! ProviderAdapter — 各 Provider 的 API 格式适配 trait
//! GenericProvider — 通用 LlmProvider 实现

use async_trait::async_trait;
use std::sync::Arc;

use super::http_client::http_call;
use crate::llm::{ChatRequest, ChatResponse, LlmError, LlmProvider, TokenUsage};

/// 公共 LLM 配置
#[derive(Debug, Clone)]
pub struct BaseConfig {
    pub api_key: String,
    pub base_url: String,
    pub default_model: String,
    pub timeout_secs: u64,
}

/// 各 Provider 的 API 格式适配 trait
pub trait ProviderAdapter: Send + Sync {
    /// Provider 唯一标识（如 "openai", "anthropic"）
    fn id(&self) -> &str;

    /// 构建 API 端点 URL
    fn endpoint(&self, base: &str) -> String;

    /// 配置请求 headers
    fn headers(&self, api_key: &str, builder: reqwest::RequestBuilder) -> reqwest::RequestBuilder;

    /// 将统一 ChatRequest 转换为 provider 特定的 JSON body
    fn build_request(&self, request: &ChatRequest, default_model: &str) -> serde_json::Value;

    /// 将 provider 特定的 JSON 响应解析为统一 ChatResponse
    fn parse_response(&self, raw: &serde_json::Value) -> Result<ChatResponse, LlmError>;
}

/// 通用 Provider — 实现 LlmProvider trait
pub struct GenericProvider<T: ProviderAdapter> {
    config: BaseConfig,
    client: reqwest::Client,
    adapter: T,
}

impl<T: ProviderAdapter> GenericProvider<T> {
    pub fn new(config: BaseConfig, adapter: T) -> Result<Self, LlmError> {
        if config.api_key.is_empty() {
            return Err(LlmError::MissingApiKey {
                provider: adapter.id().to_string(),
            });
        }

        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(config.timeout_secs))
            .build()
            .map_err(|e| LlmError::ParseError {
                detail: format!("Failed to build HTTP client: {}", e),
            })?;

        Ok(Self { config, client, adapter })
    }
}

#[async_trait]
impl<T: ProviderAdapter> LlmProvider for GenericProvider<T> {
    async fn llm_call(&self, request: &ChatRequest) -> Result<ChatResponse, LlmError> {
        let body = self.adapter.build_request(request, &self.config.default_model);
        let url = self.adapter.endpoint(&self.config.base_url);
        let api_key = self.config.api_key.clone();

        let resp = http_call(&self.client, &url, &body, self.adapter.id(), |b| {
            self.adapter.headers(&api_key, b)
        })
        .await?;

        self.adapter.parse_response(&resp.json)
    }

    fn provider_id(&self) -> &str {
        self.adapter.id()
    }
}

impl<T: std::fmt::Debug + ProviderAdapter> std::fmt::Debug for GenericProvider<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GenericProvider")
            .field("config", &self.config)
            .finish_non_exhaustive()
    }
}
```

- [ ] **Step 2: 更新 provider/mod.rs**

在文件顶部添加 `pub mod base;`，在 exports 中添加：

```rust
pub use base::{BaseConfig, GenericProvider, ProviderAdapter};
```

- [ ] **Step 3: cargo check 验证编译**

```bash
cargo check
```

Expected: PASS（无错误）

- [ ] **Step 4: 提交**

```bash
git add src/llm/provider/base.rs src/llm/provider/mod.rs
git commit -m "feat: 提取 BaseConfig + ProviderAdapter trait + GenericProvider"
```

---

### Task 2: 重构 OpenAI — 删除 OpenAIConfig/OpenAIProvider，新增 OpenAIAdapter

**Files:**
- Modify: `backend/src/llm/provider/openai.rs`
- Modify: `backend/src/llm/provider/mod.rs` (更新 exports)

- [ ] **Step 1: 重写 openai.rs**

保留 `build_request`、`parse_response` 的核心逻辑，改为实现 `ProviderAdapter`：

```rust
//! OpenAI Adapter — implements ProviderAdapter for the OpenAI chat completions API.

use super::base::ProviderAdapter;
use crate::llm::{ChatRequest, ChatResponse, LlmError, Message, TokenUsage, ToolCall};

#[derive(Debug, Default)]
pub struct OpenAIAdapter;

impl ProviderAdapter for OpenAIAdapter {
    fn id(&self) -> &str {
        "openai"
    }

    fn endpoint(&self, base: &str) -> String {
        format!("{}/v1/chat/completions", base)
    }

    fn headers(&self, api_key: &str, builder: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        builder.header("Authorization", format!("Bearer {}", api_key))
    }

    fn build_request(&self, request: &ChatRequest, default_model: &str) -> serde_json::Value {
        let model = if request.model.is_empty() {
            default_model
        } else {
            &request.model
        };

        let messages: Vec<serde_json::Value> = request
            .messages
            .iter()
            .filter_map(|msg| self.message_to_openai(msg))
            .collect();

        let mut body = serde_json::json!({
            "model": model,
            "messages": messages,
            "temperature": request.temperature.unwrap_or(0.0),
        });

        if let Some(ref tools) = request.tools {
            let openai_tools: Vec<serde_json::Value> = tools
                .iter()
                .map(|t| {
                    serde_json::json!({
                        "type": "function",
                        "function": {
                            "name": t.name,
                            "description": t.description,
                            "parameters": t.parameters,
                        }
                    })
                })
                .collect();
            body["tools"] = serde_json::json!(openai_tools);
        }

        body
    }

    fn parse_response(&self, raw: &serde_json::Value) -> Result<ChatResponse, LlmError> {
        let content = raw
            .get("choices")
            .and_then(|c| c.get(0))
            .and_then(|c| c.get("message"))
            .and_then(|m| m.get("content"))
            .and_then(|c| c.as_str())
            .unwrap_or("")
            .to_string();

        let tool_calls: Vec<ToolCall> = raw
            .get("choices")
            .and_then(|c| c.get(0))
            .and_then(|c| c.get("message"))
            .and_then(|m| m.get("tool_calls"))
            .and_then(|tc| tc.as_array())
            .map(|calls| {
                calls
                    .iter()
                    .filter_map(|call| {
                        let id = call.get("id")?.as_str()?.to_string();
                        let name = call.get("function")?.get("name")?.as_str()?.to_string();
                        let args_str = call
                            .get("function")?
                            .get("arguments")?
                            .as_str()?
                            .to_string();
                        let arguments = serde_json::from_str(&args_str).ok()?;
                        Some(ToolCall { id, name, arguments })
                    })
                    .collect()
            })
            .unwrap_or_default();

        let usage = TokenUsage {
            prompt_tokens: raw
                .get("usage")
                .and_then(|u| u.get("prompt_tokens"))
                .and_then(|t| t.as_u64())
                .unwrap_or(0) as u32,
            completion_tokens: raw
                .get("usage")
                .and_then(|u| u.get("completion_tokens"))
                .and_then(|t| t.as_u64())
                .unwrap_or(0) as u32,
            total_tokens: raw
                .get("usage")
                .and_then(|u| u.get("total_tokens"))
                .and_then(|t| t.as_u64())
                .unwrap_or(0) as u32,
        };

        Ok(ChatResponse {
            content,
            tool_calls,
            usage,
            raw: raw.clone(),
        })
    }
}

impl OpenAIAdapter {
    fn message_to_openai(&self, msg: &Message) -> Option<serde_json::Value> {
        match msg {
            Message::System { content } => Some(serde_json::json!({
                "role": "system",
                "content": content,
            })),
            Message::User { content } => Some(serde_json::json!({
                "role": "user",
                "content": content,
            })),
            Message::Assistant { content, tool_calls } => {
                if content.is_empty() && tool_calls.is_empty() {
                    return None;
                }

                let mut msg_obj = serde_json::json!({ "role": "assistant" });

                if !content.is_empty() {
                    msg_obj["content"] = serde_json::json!(content);
                }

                if !tool_calls.is_empty() {
                    let calls: Vec<serde_json::Value> = tool_calls
                        .iter()
                        .map(|tc| {
                            serde_json::json!({
                                "id": tc.id,
                                "type": "function",
                                "function": {
                                    "name": tc.name,
                                    "arguments": tc.arguments.to_string(),
                                }
                            })
                        })
                        .collect();
                    msg_obj["tool_calls"] = serde_json::json!(calls);
                }

                Some(msg_obj)
            }
        }
    }
}
```

- [ ] **Step 2: 更新 provider/mod.rs exports**

替换：
```rust
pub use openai::OpenAIAdapter;
```

移除 `OpenAIConfig` 和 `OpenAIProvider` 的导出。

- [ ] **Step 3: cargo check 验证编译**

```bash
cargo check
```

Expected: 会有编译错误（consumer 还在引用 OpenAIConfig/OpenAIProvider）— 这是预期的，Task 3/4/5 会修复。

- [ ] **Step 4: 提交**

```bash
git add src/llm/provider/openai.rs src/llm/provider/mod.rs
git commit -m "refactor: OpenAI 迁移到 ProviderAdapter"
```

---

### Task 3: 重构 Anthropic — 删除 AnthropicConfig/AnthropicProvider，新增 AnthropicAdapter

**Files:**
- Modify: `backend/src/llm/provider/anthropic.rs`
- Modify: `backend/src/llm/provider/mod.rs` (更新 exports)

- [ ] **Step 1: 重写 anthropic.rs**

```rust
//! Anthropic Adapter — implements ProviderAdapter for the Anthropic messages API.

use super::base::ProviderAdapter;
use crate::llm::{ChatRequest, ChatResponse, LlmError, LlmProvider, Message, TokenUsage, ToolCall};

#[derive(Debug, Default)]
pub struct AnthropicAdapter;

impl ProviderAdapter for AnthropicAdapter {
    fn id(&self) -> &str {
        "anthropic"
    }

    fn endpoint(&self, base: &str) -> String {
        format!("{}/v1/messages", base)
    }

    fn headers(&self, api_key: &str, builder: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        builder
            .header("x-api-key", api_key)
            .header("anthropic-version", "2023-10-01")
    }

    fn build_request(&self, request: &ChatRequest, default_model: &str) -> serde_json::Value {
        let model = if request.model.is_empty() {
            default_model
        } else {
            &request.model
        };

        let system = request.messages.iter().find_map(|msg| match msg {
            Message::System { content } => Some(content.clone()),
            _ => None,
        });

        let messages: Vec<serde_json::Value> = request
            .messages
            .iter()
            .filter_map(|msg| self.message_to_anthropic(msg))
            .collect();

        let mut body = serde_json::json!({
            "model": model,
            "max_tokens": 4096,
            "messages": messages,
            "temperature": request.temperature.unwrap_or(0.0),
        });

        if let Some(sys) = system {
            body["system"] = serde_json::json!(sys);
        }

        if let Some(ref tools) = request.tools {
            let anthropic_tools: Vec<serde_json::Value> = tools
                .iter()
                .map(|t| {
                    serde_json::json!({
                        "name": t.name,
                        "description": t.description,
                        "input_schema": t.parameters,
                    })
                })
                .collect();
            body["tools"] = serde_json::json!(anthropic_tools);
        }

        body
    }

    fn parse_response(&self, raw: &serde_json::Value) -> Result<ChatResponse, LlmError> {
        let mut content_parts = Vec::new();
        let mut tool_calls = Vec::new();

        if let Some(content_array) = raw.get("content").and_then(|c| c.as_array()) {
            for block in content_array {
                let block_type = block.get("type").and_then(|t| t.as_str());
                match block_type {
                    Some("text") => {
                        if let Some(text) = block.get("text").and_then(|t| t.as_str()) {
                            content_parts.push(text.to_string());
                        }
                    }
                    Some("tool_use") => {
                        let id = block.get("id").and_then(|i| i.as_str()).unwrap_or("").to_string();
                        let name = block.get("name").and_then(|n| n.as_str()).unwrap_or("").to_string();
                        let input = block.get("input").cloned().unwrap_or_else(|| serde_json::json!({}));
                        tool_calls.push(ToolCall { id, name, arguments: input });
                    }
                    _ => {}
                }
            }
        }

        let usage = TokenUsage {
            prompt_tokens: raw
                .get("usage")
                .and_then(|u| u.get("input_tokens"))
                .and_then(|t| t.as_u64())
                .unwrap_or(0) as u32,
            completion_tokens: raw
                .get("usage")
                .and_then(|u| u.get("output_tokens"))
                .and_then(|t| t.as_u64())
                .unwrap_or(0) as u32,
            total_tokens: 0,
        };

        let total_tokens = usage.prompt_tokens + usage.completion_tokens;

        Ok(ChatResponse {
            content: content_parts.join("\n"),
            tool_calls,
            usage: TokenUsage { total_tokens, ..usage },
            raw: raw.clone(),
        })
    }
}

impl AnthropicAdapter {
    fn message_to_anthropic(&self, msg: &Message) -> Option<serde_json::Value> {
        match msg {
            Message::System { .. } => None,
            Message::User { content } => Some(serde_json::json!({
                "role": "user",
                "content": content,
            })),
            Message::Assistant { content, tool_calls } => {
                if content.is_empty() && tool_calls.is_empty() {
                    return None;
                }

                let mut blocks = Vec::new();

                if !content.is_empty() {
                    blocks.push(serde_json::json!({
                        "type": "text",
                        "text": content,
                    }));
                }

                for tc in tool_calls {
                    blocks.push(serde_json::json!({
                        "type": "tool_use",
                        "id": tc.id,
                        "name": tc.name,
                        "input": tc.arguments,
                    }));
                }

                Some(serde_json::json!({
                    "role": "assistant",
                    "content": blocks,
                }))
            }
        }
    }
}
```

- [ ] **Step 2: 更新 provider/mod.rs exports**

替换：
```rust
pub use anthropic::AnthropicAdapter;
```

移除 `AnthropicConfig` 和 `AnthropicProvider` 的导出。

- [ ] **Step 3: 提交**

```bash
git add src/llm/provider/anthropic.rs src/llm/provider/mod.rs
git commit -m "refactor: Anthropic 迁移到 ProviderAdapter"
```

---

### Task 4: 更新 llm/provider/config.rs — build_router 使用 BaseConfig + GenericProvider

**Files:**
- Modify: `backend/src/llm/provider/config.rs`

- [ ] **Step 1: 重写 build_router 方法**

将原来分别构建 `OpenAIConfig`/`AnthropicConfig` 和 `OpenAIProvider`/`AnthropicProvider` 的逻辑替换为：

```rust
    pub fn build_router(&self) -> Option<Arc<dyn LlmProvider>> {
        let snapshot = self.inner.read().unwrap().clone();
        let default_provider = snapshot.default_provider.clone();
        let providers = snapshot.providers;

        let mut sorted_providers = providers;
        sorted_providers.sort_by(|a, b| {
            let a_is_default = a.id == default_provider;
            let b_is_default = b.id == default_provider;
            b_is_default.cmp(&a_is_default)
        });

        let mut router = ModelRouter::new(ModelRouterConfig::default());
        let mut has_any = false;

        for pc in &sorted_providers {
            let Some(ref key) = pc.api_key else { continue };
            if key.is_empty() { continue; }

            let flash = pc.model_flash.clone();
            let base_config = BaseConfig {
                api_key: key.clone(),
                base_url: pc.base_url.clone().unwrap_or_else(|| {
                    if pc.id == "openai" { "https://api.openai.com".to_string() }
                    else { "https://api.anthropic.com".to_string() }
                }),
                default_model: flash.clone().unwrap_or_default(),
                timeout_secs: 60,
            };

            let provider: Arc<dyn LlmProvider> = if pc.id == "openai" {
                match GenericProvider::<OpenAIAdapter>::new(base_config, OpenAIAdapter::default()) {
                    Ok(p) => Arc::new(p),
                    Err(e) => { tracing::warn!(error = %e, "Failed to create OpenAI provider"); continue; }
                }
            } else if pc.id == "anthropic" {
                match GenericProvider::<AnthropicAdapter>::new(base_config, AnthropicAdapter::default()) {
                    Ok(p) => Arc::new(p),
                    Err(e) => { tracing::warn!(error = %e, "Failed to create Anthropic provider"); continue; }
                }
            } else {
                tracing::warn!(provider = %pc.id, "Unknown provider, skipping");
                continue;
            };

            router.register_provider(
                pc.id.clone(),
                provider,
                ProviderModels {
                    model_flash: flash.clone(),
                    model_pro: pc.model_pro.clone(),
                    default_model: flash,
                },
            );
            has_any = true;
        }

        if has_any { Some(Arc::new(router)) } else { None }
    }
```

更新 imports：
```rust
use super::base::{BaseConfig, GenericProvider, ProviderAdapter};
use super::{AnthropicAdapter, OpenAIAdapter};
```

移除不再需要的 `AnthropicConfig`、`AnthropicProvider`、`OpenAIConfig`、`OpenAIProvider` 的 import。

- [ ] **Step 2: cargo check 验证编译**

```bash
cargo check
```

Expected: PASS（或仅有 agent/mod.rs 的编译错误）

- [ ] **Step 3: 提交**

```bash
git add src/llm/provider/config.rs
git commit -m "refactor: build_router 使用 BaseConfig + GenericProvider"
```

---

### Task 5: 更新 agent/mod.rs — build_llm_provider 使用 BaseConfig + GenericProvider

**Files:**
- Modify: `backend/src/agent/mod.rs`

- [ ] **Step 1: 重写 build_llm_provider 函数**

将原来分别构建 `OpenAIConfig`/`AnthropicConfig` 和 `OpenAIProvider`/`AnthropicProvider` 的逻辑替换为：

```rust
fn build_llm_provider(config: &Config) -> Option<Arc<dyn LlmProvider>> {
    use crate::llm::provider::{AnthropicAdapter, BaseConfig, GenericProvider, OpenAIAdapter, ProviderAdapter};

    let mut router = ModelRouter::new(ModelRouterConfig::default());
    let mut has_any = false;

    for pc in &config.llm_providers {
        let Some(ref key) = pc.api_key else { continue };
        if key.is_empty() { continue; }

        let flash = pc.model_flash.clone();
        let base_config = BaseConfig {
            api_key: key.clone(),
            base_url: pc.base_url.clone().unwrap_or_else(|| {
                if pc.id == "openai" { "https://api.openai.com".to_string() }
                else { "https://api.anthropic.com".to_string() }
            }),
            default_model: flash.clone().unwrap_or_default(),
            timeout_secs: 60,
        };

        let provider: Arc<dyn LlmProvider> = if pc.id == "openai" {
            match GenericProvider::<OpenAIAdapter>::new(base_config, OpenAIAdapter::default()) {
                Ok(p) => Arc::new(p),
                Err(e) => { tracing::warn!(error = %e, "Failed to create OpenAI provider"); continue; }
            }
        } else if pc.id == "anthropic" {
            match GenericProvider::<AnthropicAdapter>::new(base_config, AnthropicAdapter::default()) {
                Ok(p) => Arc::new(p),
                Err(e) => { tracing::warn!(error = %e, "Failed to create Anthropic provider"); continue; }
            }
        } else {
            tracing::warn!(provider = %pc.id, "Unknown provider, skipping");
            continue;
        };

        router.register_provider(
            pc.id.clone(),
            provider,
            ProviderModels {
                model_flash: flash.clone(),
                model_pro: pc.model_pro.clone(),
                default_model: flash,
            },
        );
        has_any = true;
    }

    if has_any { Some(Arc::new(router)) } else { None }
}
```

移除 `AnthropicConfig`、`AnthropicProvider`、`OpenAIConfig`、`OpenAIProvider` 的 import。

- [ ] **Step 2: cargo check 验证编译**

```bash
cargo check
```

Expected: PASS

- [ ] **Step 3: 提交**

```bash
git add src/agent/mod.rs
git commit -m "refactor: agent build_llm_provider 使用 GenericProvider"
```

---

### Task 6: 清理 exports + clippy + 测试

**Files:**
- Modify: `backend/src/llm/mod.rs`
- Modify: `backend/src/llm/provider/mod.rs`

- [ ] **Step 1: 更新 llm/mod.rs exports**

移除 `AnthropicConfig`、`AnthropicProvider`、`OpenAIConfig`、`OpenAIProvider`。

新增：
```rust
pub use provider::{AnthropicAdapter, BaseConfig, GenericProvider, OpenAIAdapter, ProviderAdapter};
```

- [ ] **Step 2: cargo clippy -- -D warnings**

```bash
cargo clippy -- -D warnings
```

Expected: PASS（修复任何 clippy 警告）

- [ ] **Step 3: cargo test --lib**

```bash
cargo test --lib
```

Expected: 46 个测试全部通过

- [ ] **Step 4: cargo fmt**

```bash
cargo fmt
```

- [ ] **Step 5: 提交**

```bash
git add src/llm/mod.rs src/llm/provider/mod.rs
git commit -m "refactor: 清理 exports + clippy 通过"
```

---

## Self-Review

1. **Spec coverage:** BaseConfig (Task 1) + ProviderAdapter (Task 1) + GenericProvider (Task 1) + OpenAIAdapter (Task 2) + AnthropicAdapter (Task 3) + build_router 更新 (Task 4) + build_llm_provider 更新 (Task 5) + exports 清理 (Task 6) — 全部覆盖。
2. **Placeholder scan:** 无 TBD/TODO。所有代码块完整。
3. **Type consistency:** `BaseConfig` 字段名一致，`ProviderAdapter` trait 方法签名在 Task 1 定义，Task 2/3 实现完全匹配。`GenericProvider` 构造签名一致。
4. **Scope check:** 单一重构目标，范围清晰。
