# lellm 迁移实施计划

## 决策确认

| 问题 | 决策 |
|------|------|
| Q1: tool_calls 变化 | A. 完全接受 lellm 格式，改为 `msg.extract_tool_calls()` |
| Q2: content 类型变化 | A. 接受 lellm 格式，加 `.as_text()` 转换 |
| Q3: LlmError 语义 | A. 用 lellm LlmError，在 Provider 变体中区分 |
| Q4: stream() 要求 | A. stub 返回 `Err(UnsupportedFeature)` |
| Q5: ModelRouter | A. 用 lellm ModelRouter |
| Q6: 迁移策略 | A. 一次性大迁移 |

---

## 迁移范围

### 删除的文件/模块

| 文件 | 操作 |
|------|------|
| `backend/src/llm/mod.rs` | **重写**：删除类型定义，改为 re-export `lellm_core::*` |
| `backend/src/llm/provider/base.rs` | **删除** |
| `backend/src/llm/provider/openai.rs` | **删除** |
| `backend/src/llm/provider/anthropic.rs` | **删除** |
| `backend/src/llm/provider/client.rs` | **删除** |
| `backend/src/llm/provider/config.rs` | **重写**：用 lellm CodecProvider 构建 provider |

### 修改的文件

| 文件 | 变更 |
|------|------|
| `backend/Cargo.toml` | 添加 `lellm = "0.4"` 依赖 |
| `backend/src/llm/router.rs` | 适配 lellm ModelRouter + LlmProvider trait |
| `backend/src/llm/tool_use_loop/mod.rs` | `response.tool_calls` → `response.tool_calls()` |
| `backend/src/llm/tool_use_loop/executor.rs` | 适配 ToolCall 类型 |
| `backend/src/llm/prompt_builder.rs` | 适配 Message/ContentBlock API |
| `backend/src/llm/structured_output.rs` | 适配 ChatRequest API |
| `backend/src/agent/steps/*.rs` | 适配 LlmProvider/ChatRequest/ChatResponse |
| `backend/src/token/tracker.rs` | 适配 TokenUsage |
| `backend/tests/*.rs` | 适配测试 |

### API 变化对照

| 旧 API | 新 API (lellm) |
|--------|----------------|
| `msg.tool_calls` | `msg.extract_tool_calls()` |
| `response.content` (String) | `response.content` (Vec<ContentBlock>) → `.content.iter().filter_map(\|b\| b.as_text())` 或用扩展 trait |
| `response.tool_calls` (Vec) | `response.tool_calls()` (Iterator) |
| `ToolDefinition { parameters }` | `ToolDefinition { input_schema }` |
| `provider.llm_call(&req)` | `provider.call(&req)` |
| `LlmError::ApiError { status, body }` | `LlmError::Provider { status: Some(status), message, .. }` |
| `LlmError::MissingApiKey { provider }` | `LlmError::InvalidRequest { message }` 或自定义 |
| `LlmError::NotFound { model }` | `LlmError::Provider { code: Some("model_not_found"), .. }` |

---

## 实施步骤

### Step 1: Cargo.toml 添加依赖

```toml
lellm = "0.4"
```

### Step 2: 重写 llm/mod.rs

将 `mod.rs` 改为 re-export lellm_core 类型 + 保留项目特有的扩展。

核心改动：
- 删除所有类型定义（Message, ContentBlock, ChatRequest, ChatResponse, ToolCall, ToolDefinition, ToolChoice, TokenUsage, LlmError, CacheControl, ImageSource）
- 改为 `pub use lellm_core::*`
- 保留 `LlmProvider` trait（用于项目内的 Router），方法签名改为 `call()` + `stream()`
- 添加 `text_content()` 扩展 trait 适配旧代码

### Step 3: 删除 provider 层

删除以下文件：
- `provider/base.rs`（ProviderAdapter, GenericProvider）
- `provider/openai.rs`（OpenAIAdapter）
- `provider/anthropic.rs`（AnthropicAdapter）
- `provider/client.rs`（http_call）

### Step 4: 重写 provider/config.rs

用 lellm CodecProvider 构建 provider：
```rust
use lellm::provider::providers::openai_compat::OpenAICompatCodec;
use lellm::provider::providers::anthropic::AnthropicCodec;
use lellm::provider::providers::base::CodecProvider;
use lellm::provider::providers::codec::ProviderExtension;
```

每个 provider 用 `CodecProvider::new(OpenAICompatCodec, config)` 构建。

### Step 5: 适配 router.rs

用 lellm 的 ModelRouter 或保留自定义路由逻辑。需要实现 lellm 的 `LlmProvider` trait。

### Step 6: 适配 tool_use_loop

主要改动：
- `response.tool_calls` → `response.tool_calls().cloned().collect()`
- `msg.tool_calls` → `msg.extract_tool_calls()`
- `ChatResponse { content: String, .. }` → `ChatResponse::new(content_blocks, usage, raw)`

### Step 7: 适配 prompt_builder

主要改动：
- `Message::System { content }` → `Message::system(blocks)`
- `ContentBlock::Text { text, cache_control }` → lellm 的 ContentBlock 变体
- `msg.tool_calls` → `msg.extract_tool_calls()`

### Step 8: 适配 agent/steps

所有使用 `provider.llm_call()` 的地方改为 `provider.call()`。

### Step 9: 更新测试

适配所有测试文件的类型变化。

---

## 风险点

1. **lellm 的 `ChatResponse` 无 `tool_calls` 字段** — 需要通过 `tool_calls()` 方法获取，且返回的是借用，需要 `.cloned().collect()`
2. **lellm 的 `Message::Assistant` 无 `tool_calls` 字段** — 需要通过 `extract_tool_calls()` 获取
3. **lellm 的 `ContentBlock` 变体名称可能不同** — 需要验证 lellm 的 `ContentBlock::Text` 是否与项目一致
4. **lellm 的 `ToolDefinition.input_schema`** — 需要验证字段名变化是否影响序列化
5. **lellm 的 `LlmError` 无 `MissingApiKey`** — 需要在应用层处理 API key 校验

---

## 验证清单

- [ ] `cargo check` 编译通过
- [ ] `cargo test` 所有测试通过
- [ ] `cargo clippy` 无警告
- [ ] tool_use_loop 正常工作（LLM → tool call → result → LLM 循环）
- [ ] prompt_builder 正常构建 prompt
- [ ] ModelRouter 正常路由 L1/L2 任务
- [ ] 配置加载正常（从 config.toml 加载 provider 配置）
