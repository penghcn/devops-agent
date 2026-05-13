# Provider 抽象设计

## 目标

消除 `OpenAIConfig`/`AnthropicConfig` 和 `OpenAIProvider`/`AnthropicProvider` 之间的重复代码，提取公共逻辑。

## 现状

| 重复项 | OpenAI | Anthropic |
|--------|--------|-----------|
| Config (api_key, base_url, default_model, timeout_secs) | 有 | 有 |
| Provider struct (config + client) | 有 | 有 |
| new() (api_key 检查 + 建 client) | 有 | 有 |
| llm_call() 流程 (build → http_call → parse) | 有 | 有 |

差异仅在于 **API 格式适配**：URL path、headers、请求/响应 JSON 格式。

## 设计

### 1. BaseConfig — 公共配置

```rust
pub struct BaseConfig {
    pub api_key: String,
    pub base_url: String,
    pub default_model: String,
    pub timeout_secs: u64,
}
```

移除 `OpenAIConfig`、`AnthropicConfig`。

### 2. ProviderAdapter trait — 每个 Provider 独有的适配逻辑

```rust
pub trait ProviderAdapter: Send + Sync {
    fn id(&self) -> &str;
    fn endpoint(&self, base: &str) -> String;
    fn headers(&self, api_key: &str, builder: reqwest::RequestBuilder) -> reqwest::RequestBuilder;
    fn build_request(&self, request: &ChatRequest, default_model: &str) -> serde_json::Value;
    fn parse_response(&self, raw: &serde_json::Value) -> Result<ChatResponse, LlmError>;
}
```

### 3. GenericProvider<T> — 通用实现

```rust
pub struct GenericProvider<T: ProviderAdapter> {
    config: BaseConfig,
    client: reqwest::Client,
    adapter: T,
}
```

实现 `LlmProvider` trait，内部委托给 adapter。

### 4. OpenAIAdapter / AnthropicAdapter

各自只需实现 `ProviderAdapter` trait，将现有 `build_request`、`parse_response`、`llm_call` 中的适配逻辑迁移过去。

### 5. 对外暴露

```rust
pub type OpenAIProvider = GenericProvider<OpenAIAdapter>;
pub type AnthropicProvider = GenericProvider<AnthropicAdapter>;
```

保持对外 API 不变。

## 影响范围

| 文件 | 变更 |
|------|------|
| `provider/mod.rs` | 新增 `base.rs`(BaseConfig + GenericProvider + ProviderAdapter) |
| `provider/openai.rs` | 删除 OpenAIConfig/OpenAIProvider，新增 OpenAIAdapter |
| `provider/anthropic.rs` | 删除 AnthropicConfig/AnthropicProvider，新增 AnthropicAdapter |
| `provider/config.rs` | `build_router()` 中改 `BaseConfig` + `GenericProvider` |
| `agent/mod.rs` | `build_llm_provider()` 中改 `BaseConfig` + `GenericProvider` |
| `llm/mod.rs` | exports 调整 |

## 不变性

- `LlmProvider` trait 不变
- `ChatRequest`/`ChatResponse`/`LlmError` 不变
- 对外 `OpenAIProvider`/`AnthropicProvider` 类型名不变（type alias）
