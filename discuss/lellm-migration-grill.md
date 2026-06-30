# lellm 迁移分析 — Grill 讨论

## 1. 现状分析

### 当前 LLM 层架构

```
backend/src/llm/
├── mod.rs                 # LlmProvider trait + 核心类型定义
├── router.rs              # ModelRouter, TaskLevel 路由
├── structured_output.rs   # 结构化输出
├── prompt_builder.rs      # Prompt 构建
└── provider/
    ├── mod.rs             # re-export
    ├── base.rs            # BaseConfig, ProviderAdapter trait, GenericProvider<T>
    ├── config.rs          # ProviderConfig, LlmConfigStore, build_model_router()
    ├── client.rs          # HTTP 调用封装
    ├── openai.rs          # OpenAIAdapter (~270行)
    ├── anthropic.rs       # AnthropicAdapter (~300行)
    ├── openai_compat.rs   # OpenAI 兼容 (DeepSeek/NVIDIA/VLLM/LLaMA)
    └── ...
```

### 自定义类型清单

| 类型 | 位置 | lellm-core 对应 |
|------|------|----------------|
| `LlmProvider` trait | `mod.rs:34` | `lellm_provider::LlmProvider` |
| `ChatRequest` | `mod.rs:147` | `lellm_core::ChatRequest` |
| `ChatResponse` | `mod.rs:224` | `lellm_core::ChatResponse` |
| `Message` | `mod.rs:103` | `lellm_core::Message` |
| `ContentBlock` | `mod.rs:63` | `lellm_core::ContentBlock` |
| `ToolCall` | `mod.rs:241` | `lellm_core::ToolCall` |
| `ToolDefinition` | `mod.rs:210` | `lellm_core::ToolDefinition` |
| `ToolChoice` | `mod.rs:200` | `lellm_core::ToolChoice` |
| `TokenUsage` | `mod.rs:249` | `lellm_core::TokenUsage` |
| `LlmError` | `mod.rs:259` | `lellm_core::LlmError` |
| `CacheControl` | `mod.rs:46` | `lellm_core::CacheControl` |
| `ImageSource` | `mod.rs:36` | `lellm_core::ImageSource` |
| `BaseConfig` | `provider/base.rs:16` | `lellm_provider::ProviderMeta` |
| `ProviderAdapter` trait | `provider/base.rs:35` | `lellm_provider::ProviderExtension` |
| `GenericProvider<T>` | `provider/base.rs:44` | `lellm_provider::CodecProvider<C>` |
| `ProviderConfig` | `provider/config.rs:18` | 项目自定义（保留） |
| `ModelRouter` | `router.rs` | `lellm_provider::ModelRouter` |

---

## 2. 关键差异分析

### 2.1 `Message::Assistant` — tool_calls 位置不同

**当前项目：**
```rust
Message::Assistant {
    content: Vec<ContentBlock>,
    tool_calls: Vec<ToolCall>,  // 直接携带
}
```

**lellm-core：**
```rust
Message::Assistant {
    content: Vec<ContentBlock>,  // tool_calls 嵌入 content 中
}
// tool_calls 通过 extract_tool_calls() 方法提取
```

**影响范围：** `tool_use_loop/mod.rs` 大量使用 `response.tool_calls`，`comparison.rs`，`prompt_builder.rs` 等。

**决策问题 Q1：** lellm 的 Assistant 消息将 tool_calls 嵌入 ContentBlock::ToolUse 中。这意味着：
- 优点：与 Anthropic API 原生格式一致，序列化更自然
- 缺点：需要修改所有 `msg.tool_calls` 为 `msg.extract_tool_calls()`，且 `extract_tool_calls()` 返回 `Vec<ToolCall>` 每次调用都会分配

**是否接受这个变化？** 这是最大的 breaking change。

---

### 2.2 `ChatResponse.content` — String vs Vec<ContentBlock>

**当前项目：**
```rust
pub struct ChatResponse {
    pub content: String,  // 简单字符串
    pub tool_calls: Vec<ToolCall>,
    ...
}
```

**lellm-core：**
```rust
pub struct ChatResponse {
    pub content: Vec<ContentBlock>,  // 结构化内容块
    // tool_calls 通过 tool_calls() 方法提取
    ...
}
```

**影响范围：** 几乎所有使用 `response.content` 的地方都假设是 String。如 `tool_use_loop/mod.rs:88` 的 `response.content.clone()`。

**决策问题 Q2：** `ChatResponse.content` 从 `String` 变为 `Vec<ContentBlock>`。这会导致：
- 优点：支持多模态响应、thinking blocks 等
- 缺点：所有 `response.content` 使用处都需要加 `.as_text()` 或类似转换

**是否接受这个变化？**

---

### 2.3 `LlmError` — 变体结构不同

**当前项目：**
```rust
LlmError::ApiError { status: u16, body: String }
LlmError::NotFound { model: String }
LlmError::MissingApiKey { provider: String }
```

**lellm-core：**
```rust
LlmError::Provider { provider: String, status: Option<u16>, code: Option<String>, message: String }
LlmError::Network { detail: String }
// 无 NotFound、MissingApiKey 变体
```

**影响范围：** 错误处理代码、`config.rs` 中的 `MissingApiKey` 检查、`router.rs` 中的 `DummyProvider`。

**决策问题 Q3：** lellm 的 `LlmError` 更通用但丢失了一些项目特有的语义（如 `MissingApiKey`）。需要决定：
- 方案 A：用 `lellm_core::LlmError`，在应用层用额外字段区分
- 方案 B：保留自定义 `LlmError`，做 `From` 转换

---

### 2.4 `ToolDefinition` — 字段名差异

**当前项目：** `parameters: serde_json::Value`
**lellm-core：** `input_schema: serde_json::Value`

**影响：** 仅命名差异，可用 `#[serde(alias = "parameters")]` 兼容。

---

### 2.5 `LlmProvider` trait — 方法签名不同

**当前项目：**
```rust
async fn llm_call(&self, request: &ChatRequest) -> Result<ChatResponse, LlmError>;
fn provider_id(&self) -> &str;
```

**lellm-provider：**
```rust
fn call(&self, request: &ChatRequest) -> Pin<Box<dyn Future<Output = Result<ChatResponse, LlmError>> + Send>>;
fn stream(&self, request: &ChatRequest) -> Pin<Box<dyn Future<Output = Result<ProviderStream, LlmError>> + Send>>;
fn provider_id(&self) -> &str;
```

**影响：** 所有 `provider.llm_call(&req)` 调用需要改为 `provider.call(&req)`。且 lellm 的 trait 要求实现 `stream()` 方法。

**决策问题 Q4：** lellm 的 `LlmProvider` trait 强制要求实现 `stream()` 方法（返回 `ProviderStream`）。当前项目不支持流式输出。如何处理？
- 方案 A：`stream()` 返回 `Err(LlmError::UnsupportedFeature)` 或 `unimplemented!()`
- 方案 B：后续再支持流式，先 stub

---

### 2.6 `ProviderAdapter` vs `ProviderExtension`

**当前项目：** 自定义 `ProviderAdapter` trait + `GenericProvider<T>`
**lellm-provider：** `ProviderExtension: ChatCodec + ModelCapabilities + ProviderMeta` + `CodecProvider<C>`

**lellm 已有适配器：**
- `OpenAICompatCodec` — 覆盖 OpenAI, NVIDIA, DeepSeek, VLLM, LLaMA, SGLang, Ollama, MiMo, 智谱, DashScope
- `AnthropicCodec` — Anthropic 协议

**影响：** 可以完全删除 `provider/base.rs`、`provider/openai.rs`、`provider/anthropic.rs`、`provider/openai_compat.rs` 中的 `ProviderAdapter` 实现，改用 lellm 的 Codec。

---

### 2.7 `ModelRouter` — 路由实现

**当前项目：** 自定义 `ModelRouter`，支持 `TaskLevel::L1/L2` 路由
**lellm-provider：** `ModelRouter` + `ProviderRegistry`

**决策问题 Q5：** lellm 有自己的 `ModelRouter` 实现。当前项目的 `ModelRouter` 支持 `TaskLevel` 路由（L1 简单任务用 flash，L2 复杂任务用 pro）。lellm 的 `ModelRouter` 是否支持类似功能？还是需要保留自定义路由？

---

## 3. 可替换范围总结

### 3.1 完全可替换（高收益）

| 模块 | 替换为 | 预计删除代码行数 |
|------|--------|----------------|
| `mod.rs` 核心类型定义 | `lellm_core::*` | ~200 行 |
| `provider/base.rs` | `lellm_provider::CodecProvider` | ~100 行 |
| `provider/openai.rs` | `lellm_provider::OpenAICompatCodec` | ~270 行 |
| `provider/anthropic.rs` | `lellm_provider::AnthropicCodec` | ~300 行 |
| `provider/client.rs` | lellm 内置 HTTP 客户端 | ~80 行 |
| **合计** | | **~950 行** |

### 3.2 需要适配（中等收益）

| 模块 | 处理方式 |
|------|----------|
| `router.rs` | 评估 lellm ModelRouter 是否满足需求，否则保留 |
| `config.rs` | 保留项目特有的 `ProviderConfig`，改用 lellm 构建 provider |
| `tool_use_loop/` | 适配 `Message` 和 `ChatResponse` 的 API 变化 |
| `prompt_builder.rs` | 适配 `Message` 和 `ContentBlock` 的 API 变化 |

### 3.3 不可替换（保留）

| 模块 | 原因 |
|------|------|
| `config.rs` ProviderConfig | 项目特有的配置结构 |
| `router.rs` TaskLevel 路由 | 项目特有的路由逻辑 |
| `tool_use_loop/` | 项目特有的工具循环逻辑 |
| `structured_output.rs` | 项目特有的结构化输出 |
| `prompt_builder.rs` | 项目特有的 prompt 构建 |
| `token/tracker.rs` | 项目特有的 token 追踪 |

---

## 4. Grill 问题清单

### Q1: Message::Assistant tool_calls 变化
`Message::Assistant` 从 `{ content, tool_calls }` 变为只有 `{ content }`，tool_calls 嵌入 ContentBlock::ToolUse 中。

**影响：** tool_use_loop、comparison、prompt_builder 等约 23 处引用 `msg.tool_calls` 或 `response.tool_calls`。

**选项：**
- A. 完全接受 lellm 格式，所有调用处改为 `msg.extract_tool_calls()`
- B. 写一个 adapter wrapper，保持内部使用 `tool_calls` 字段，序列化时转为 lellm 格式
- C. 分阶段迁移，先替换类型定义，再逐步适配调用处

### Q2: ChatResponse.content 类型变化
从 `String` 变为 `Vec<ContentBlock>`。

**影响：** 几乎所有 `response.content` 使用处。

**选项：**
- A. 完全接受 lellm 格式，加 `.as_text()` 转换
- B. 写一个扩展 trait `ChatResponseExt`，提供 `.text_content() -> String` 便捷方法
- C. 保留自定义 ChatResponse，做 From/Into 转换

### Q3: LlmError 语义丢失
lellm 的 `LlmError` 无 `MissingApiKey` 和 `NotFound` 变体。

**选项：**
- A. 用 lellm 的 `LlmError`，在 `Provider` 变体中通过 `code` 字段区分
- B. 保留自定义 `LlmError`，实现 `From<lellm_core::LlmError>`
- C. 用 lellm 的 `LlmError` + 项目自定义的 `AppError` 包装

### Q4: LlmProvider::stream() 强制要求
lellm 的 `LlmProvider` trait 要求实现 `stream()` 方法。

**选项：**
- A. stub 实现，返回 `Err(LlmError::UnsupportedFeature)`
- B. 先不实现流式，后续再补
- C. 直接用 `lellm_provider::CodecProvider`，不自定义 LlmProvider

### Q5: ModelRouter 保留还是替换
当前 `ModelRouter` 支持 `TaskLevel::L1/L2` 路由。

**选项：**
- A. 用 lellm 的 `ModelRouter`，扩展 `TaskLevel` 支持
- B. 保留自定义 `ModelRouter`，只替换底层 provider
- C. 用 lellm 的 `ProviderRegistry` + 自定义路由逻辑

### Q6: 迁移策略
**选项：**
- A. 一次性大迁移（一次性替换所有类型 + provider 实现）
- B. 分层迁移（先替换类型定义，再替换 provider 实现）
- C. 渐进式迁移（保留两套，逐步切换）

---

## 5. 推荐方案

### 收益估算

- **可删除代码：** ~950 行（provider 层的适配器实现）
- **新增依赖：** `lellm = "0.4"` （含 core + provider）
- **需要修改的调用处：** ~30-40 处（主要是 tool_calls 和 content 的访问方式变化）

### 推荐策略

**Phase 1: 类型替换**
- 用 `lellm_core::*` 替换 `mod.rs` 中的类型定义
- 保留 `LlmProvider` trait 和 `ModelRouter`（项目特有）
- 适配 `tool_use_loop` 和 `prompt_builder` 的调用

**Phase 2: Provider 替换**
- 用 `lellm_provider::CodecProvider<OpenAICompatCodec>` 替换 OpenAI 适配器
- 用 `lellm_provider::CodecProvider<AnthropicCodec>` 替换 Anthropic 适配器
- 删除 `provider/base.rs`、`provider/openai.rs`、`provider/anthropic.rs`、`provider/client.rs`

**Phase 3: 集成测试**
- 确保所有现有测试通过
- 验证 tool_use_loop 正常工作

---

## 6. 待确认事项

1. **lellm 的 `ModelRouter` 是否支持 `TaskLevel` 路由？** 如果支持，可以进一步减少自定义代码。
2. **lellm 的 `ChatResponse.tool_calls()` 返回的是借用还是拥有？** 影响是否需要额外分配。
3. **lellm 是否支持 `cache_control` 字段？** 当前项目使用缓存控制标记。
4. **lellm 的 `ProviderExtension` trait 的 `ChatCodec` 是否支持自定义 endpoint（如自建代理）？** 当前项目支持自定义 `base_url`。

---

## 7. 行动项

- [ ] 确认 Q1-Q6 的决策
- [ ] 验证 lellm ModelRouter 的 TaskLevel 支持
- [ ] 验证 lellm 的 cache_control 支持
- [ ] 验证 lellm 的自定义 base_url 支持
- [ ] 编写迁移计划
