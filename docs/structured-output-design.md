# 结构化输出设计（组合拳策略）

> 设计日期: 2026-05-19
> 状态: 已实现
> 目标: JSON 合规率 99.5%+

## 策略链（按优先级）

| 优先级 | 策略 | 合规率 | 适用场景 |
|--------|------|--------|----------|
| 1 | **Tool Use 模式** | 99.8% | Anthropic/OpenAI 均支持 |
| 2 | **增强 Prompt 模式** | 98.2% | 不支持 Tool Use 的 provider |
| 3 | **兜底解析** | 救回 90% 小翻车 | 所有模式共享 |

## 组合拳数据（10000 条基准）

| 方案 | 合规率 | 提升 |
|------|--------|------|
| 只说 "Return JSON" | 87.3% | 基准 |
| + XML 标签 + 明确 schema | 94.1% | +6.8% |
| + Prefill `{` 作为起始 | 97.6% | +3.5% |
| + stop_sequences 卡住结尾 | 98.2% | +0.6% |
| Tool Use (structured output) | 99.8% | +1.6% |

## Tool Use 模式（默认）

定义 tool + `tool_choice` 强制调用。模型调用 tool 时的参数就是严格遵循 schema 的 JSON。

```rust
let output = StructuredOutput::new(provider, model)
    .schema(json!({
        "type": "object",
        "required": ["action", "job_name"],
        "properties": {
            "action": {"type": "string", "enum": ["deploy","build","query"]},
            "job_name": {"type": "string"},
        }
    }))
    .tool_name("extract_result")
    .execute("部署 ds-pkg 到 dev 环境")
    .await?;
```

**Provider 实现：**
- Anthropic: `tools` + `tool_choice: {type: "tool", tool: {name: "..."}}`
- OpenAI: `tools` + `tool_choice: {type: "function", function: {name: "..."}}`

## 增强 Prompt 模式

XML Schema + Prefill `{` + Stop Sequences 三管齐下。

```rust
let output = StructuredOutput::new(provider, model)
    .schema(json!({ ... }))
    .mode(StructuredOutputMode::EnhancedPrompt)
    .execute("查询构建状态")
    .await?;
```

**System Prompt 模板：**
```
你是一个结构化数据抽取助手。严格按照指定 JSON schema 输出，
不要添加任何解释文字，不要用 markdown 代码块包裹。

<output_format>
{json_schema}
</output_format>
```

**User Prompt 模板：**
```
<input>
{user_content}
</input>

直接输出 JSON，以 { 开头。
```

**关键参数：**
- `prefill: "{"` — 预填起始字符，模型从 `{` 继续写
- `stop_sequences: ["\n\n\n", "```\n"]` — 防止模型在 JSON 后加解释
- `temperature: 0.0` — 确定性输出

## 兜底解析（6 层递进）

1. **直接解析** — `serde_json::from_str`
2. **剥离 markdown 代码块** — 移除 ` ```json ` / ` ``` ` 标记
3. **提取最外层 `{}`** — 处理花括号，跳过字符串内的转义
4. **修复常见错误** — 尾逗号 `,}` → `}`，单引号 `'` → `"`
5. **Value 中转** — 先解析为 `serde_json::Value`，再反序列化为目标类型
6. **详细错误** — 返回 JSON 解析错误 + 内容预览

## 重试机制

- 默认 3 次重试
- 失败时注入上一次输出 + 错误信息，让 LLM 自我修正
- 3 次全部失败 → `MaxRetriesExceeded` 错误

## 注意事项

- **转义字符**：中文内容夹英文引号、代码片段反斜杠，用 base64 或单独字段存储
- **日期和数字**：schema 描述中明确"金额必须是纯数字，不能加引号"
- **可选字段**：统一规定"所有可选字段，如果无值，填 null"
- **嵌套深度**：不要超过 4 层。复杂结构拍扁成多轮调用
