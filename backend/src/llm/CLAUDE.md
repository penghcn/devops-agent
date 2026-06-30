## 架构
```
llm/
  ├── mod.rs                 # LlmProvider trait + lellm-core 类型 re-export + 扩展 trait
  ├── router.rs              # ModelRouter：L1/L2 任务分类 + provider 路由
  ├── structured_output.rs   # Schema 强约束输出
  ├── prompt_builder.rs      # 七层 Prompt 构建器
  └── provider/
      ├── mod.rs
      └── config.rs          # ProviderConfig + LlmConfigStore + CodecProvider 适配
```

## 依赖
- **lellm-core**: 协议类型（Message, ChatRequest, ChatResponse, ToolCall 等）
- **lellm-provider**: Provider 适配器（OpenAICompatCodec, AnthropicCodec, CodecProvider）

## 类型 re-export
```rust
pub use lellm_core::{CacheControl, ChatRequest, ChatResponse, ContentBlock, ...};
pub use lellm_core::LlmError;
```

## 扩展 trait
- `ChatResponseExt`: 提供 `.text_content()` 方法从 Vec<ContentBlock> 提取文本
- `ToolDefinitionExt`: 提供 `.clone_with_cache()` 和 `::cache_breakpoint()` 方法

## Provider 架构
使用 lellm-provider 的 CodecProvider：
```rust
CodecProvider::builder(OpenAICompatCodec::openai())
    .base_url(base_url)
    .api_key(api_key)
    .build()?
```

## Router
```
Router 通过 ProviderModels.select(TaskLevel) 决定用哪个 model，再传给 provider 的 ChatRequest

使用时：
let resp = router.llm_call(&request).await.unwrap();
```
