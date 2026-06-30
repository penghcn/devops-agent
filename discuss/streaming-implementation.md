# 流式输出实施计划

## 目标
使用 lellm-provider 的 `stream()` 方法实现 LLM token 级别流式输出，通过 SSE 推送到前端。

## 当前状态
- lellm-provider 已实现 `LlmProvider::stream()` 返回 `ProviderStream`
- 项目已有 SSE 基础设施（api.rs 中的 `handle_agent_stream`）
- 当前只支持 Step 级别事件，不支持 token 级别流式

## 实施步骤

### Phase 1: LlmProvider trait 扩展
在 `llm/mod.rs` 中添加 `stream` 方法到 `LlmProvider` trait：
```rust
pub trait LlmProvider: Send + Sync {
    async fn llm_call(&self, request: &ChatRequest) -> Result<ChatResponse, LlmError>;
    async fn stream(&self, request: &ChatRequest) -> Result<ProviderStream, LlmError>;
    fn provider_id(&self) -> &str;
}
```

### Phase 2: LlmProviderAdapter 适配
在 `provider/config.rs` 中实现 `LlmProviderAdapter` 的 `stream` 方法：
```rust
impl<T: lellm_provider::LlmProvider + Send + Sync + 'static> LlmProvider for LlmProviderAdapter<T> {
    async fn stream(&self, request: &ChatRequest) -> Result<ProviderStream, LlmError> {
        let lellm_request = convert_request(request);
        self.0.stream(&lellm_request).await
    }
}
```

### Phase 3: StreamEvent 扩展
在 `agent/mod.rs` 中添加 LLM 流式事件：
```rust
pub enum StreamEvent {
    // 现有事件...
    LlmTokenDelta { token: String },
    LlmThinkingDelta { thinking: String },
    LlmToolCallDelta { index: usize, id: Option<String>, name: Option<String>, arguments_delta: Option<String> },
}
```

### Phase 4: ToolUseLoop 流式支持
在 `tool_use_loop/mod.rs` 中添加流式执行方法：
```rust
pub async fn execute_streaming(self, event_tx: Arc<mpsc::Sender<StreamEvent>>) -> Result<ToolUseResult, LlmError>
```

### Phase 5: API 端点
创建新的 SSE 端点 `/api/v1/agent/stream_llm`，支持 token 级别流式。

## 文件变更

| 文件 | 变更 |
|------|------|
| `llm/mod.rs` | 添加 `stream` 方法到 LlmProvider trait |
| `provider/config.rs` | 实现 LlmProviderAdapter 的 stream |
| `agent/mod.rs` | 扩展 StreamEvent 枚举 |
| `tool_use_loop/mod.rs` | 添加流式执行方法 |
| `api.rs` | 添加流式端点 |

## 依赖
- lellm-provider (已添加)
- tokio-stream (已添加)
- futures (已添加)
