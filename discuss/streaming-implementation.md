# 流式输出实施计划

## 目标
使用 lellm-provider 的 `stream()` 方法实现 LLM token 级别流式输出，通过 SSE 推送到前端。

## 已完成

### Phase 1: LlmProvider trait 扩展 ✅
- 添加 `StreamEvent` 枚举（TextDelta, ThinkingDelta, ToolCallDelta, Usage, Done）
- 添加 `ProviderStream` 类型别名
- 添加 `stream()` 方法到 `LlmProvider` trait

### Phase 2: LlmProviderAdapter 适配 ✅
- 实现 `LlmProviderAdapter` 的 `stream` 方法
- 转换 lellm ProviderEvent 到项目 StreamEvent

### Phase 3: StreamEvent 扩展 ✅
- 在 `llm/mod.rs` 中定义 StreamEvent 枚举
- 支持文本、思考、工具调用增量事件
- 添加 Serialize/Deserialize 用于 SSE 传输

### Phase 4: ToolUseLoop 流式支持 ✅
- 添加 `execute_streaming()` 方法
- 实时转发 token 到 event channel
- 支持流式模式下的工具调用执行

### Phase 5: API 端点 ✅
- 添加 `/api/agent/stream_llm` SSE 端点
- 实现 `handle_agent_stream_llm` 处理函数
- 实现 `process_request_stream_llm` 流式处理逻辑

## 文件变更

| 文件 | 变更 |
|------|------|
| `llm/mod.rs` | ✅ 添加 StreamEvent, ProviderStream, stream() 方法 |
| `llm/router.rs` | ✅ 实现 ModelRouter 的 stream() |
| `llm/provider/config.rs` | ✅ 实现 LlmProviderAdapter 的 stream |
| `llm/tool_use_loop/mod.rs` | ✅ 添加 execute_streaming() 方法 |
| `api.rs` | ✅ 添加流式端点 |

## 依赖
- lellm-provider (已添加)
- tokio-stream (已添加)
- futures (已添加)

## API 使用

### 请求
```http
POST /api/agent/stream_llm
Content-Type: application/json

{
  "prompt": "分析构建日志",
  "task_type": "Auto"
}
```

### SSE 响应
```
data: {"TextDelta":"根据"}
data: {"TextDelta":"日志"}
data: {"TextDelta":"分析"}
data: {"Usage":{"prompt_tokens":100,"completion_tokens":50,"total_tokens":150}}
data: "Done"
```
