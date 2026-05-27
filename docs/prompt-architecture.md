# LLM Prompt 架构设计

> 设计日期: 2026-05-19
> 状态: 已确认，待实现

## 核心设计原则

**前缀缓存最大化**：不变的內容放在最前面，变化的內容放在最后面，最大化 Anthropic prompt caching 命中率。

**能变成工具的就变成工具**：时间戳、环境变量、配置查询等动态信息，通过 tool call 获取，而不是注入 prompt。

## 分层结构（从前到后，稳定性递减）

| 层 | 名称 | 内容 | 缓存性 | 压缩性 |
|---|------|------|--------|--------|
| 1 | **静态 System 核心** | 角色定位、行为准则、安全红线、输出格式 | 100% | 不可压缩 |
| 2 | **静态工具定义** | 核心工具集 schema + 辅助工具(get_time/get_env/get_config) | 100% | 不可压缩 |
| 3 | **半静态规则** | 项目 CLAUDE.md、通用规则（安全/代码规范） | 高 | 不可压缩 |
| 4 | **动态记忆** | 高分记忆摘要 (score > threshold) | 部分 | 不可压缩 |
| 5 | **Session 上下文** | 结构化槽位: 目标(1) + 最近步骤(3-5) + 活跃错误(≤3) | 低 | 可压缩 |
| 6 | **动态工具** | 场景工具（Jenkins/GitLab 等）按需追加尾部 | 低 | 不可压缩 |
| 7 | **对话 Messages** | User/Assistant 交替 | 0% | 可压缩 |

## 压缩策略

- **混合触发**：轮次到达 → 本地快速压缩；token 快满 → LLM 深度压缩
- **产物处理**：被压缩的 Linear 消息删除，摘要追加到 Compressed 层尾部
- **Token 估算**：中文 ~1.5 token/字，ASCII 4 char/token，混合计算
- **对话裁剪**：需求/约束类消息提升到 Compressed 层永不裁剪；普通问答按时间裁剪

### Tool-Use 消息统一管理

Tool-Use 流程产生的消息按类型分发到四层，与对话消息共享压缩策略：

```
Layer::System       ← 层 1~3 静态前缀（不可压缩，100% 缓存）
Layer::Compressed   ← 压缩产物 + 用户原始请求（不可压缩，永不裁剪）
Layer::Structured   ← Session 结构化槽位（可压缩，轮次>15 时压缩）
Layer::Linear       ← tool_calls + tool_results + assistant 文本（可压缩）
```

**Tool-Use 消息分层规则：**

| 消息类型 | 归属层 | 压缩策略 |
|---------|--------|---------|
| 用户原始请求 | Compressed | 永不裁剪，追加到 Compressed 层头部 |
| tool_call (LLM 发起的工具调用) | Linear | 跟随 Linear 层压缩 |
| tool_result (工具执行结果) | Linear | 跟随 Linear 层压缩 |
| assistant 纯文本回复 | Linear | 跟随 Linear 层压缩 |
| 死循环干预提示 | Linear | 跟随 Linear 层压缩 |
| Session 槽位（目标/步骤/错误） | Structured | 轮次>15 时整体压缩 |

**Linear 层压缩三阶段（与对话统一）：**

| 阶段 | 轮次范围 | 策略 | Linear 保留 |
|------|---------|------|------------|
| Phase 1 | 1~10 | 零开销，全部保留 | 全部 |
| Phase 2 | 11~15 | 本地快速压缩（句子边界截断 200 字符） | 最近 5 轮 |
| Phase 3 | 16+ | 结构化文档 + LLM 深度压缩 | 最近 5 轮 |

**压缩产物格式（Tool-Use 专用摘要）：**

```
[压缩摘要 轮次 1-10]
- 执行工具: read×3, get_env×1, jenkins_status×2, bash×1
- 关键发现: 构建 #42 状态 RUNNING, 环境变量 DEPLOY_TARGET=prod
- 决策: 用户确认部署到 prod-us-east 区域
- 错误: bash 超时 1 次，已重试成功
```

这样 Tool-Use 和对话消息共享同一套压缩机制，无需独立实现。

## 工具化策略

### 静态辅助工具（层 2，schema 固定）

| 工具 | 功能 | 参数 | TTL |
|------|------|------|-----|
| `get_time` | 返回当前时间戳/日期 | 无 | 1 分钟 |
| `get_env` | 读取环境变量 | `key: string` | 10 分钟 |
| `get_config` | 读取项目配置 | `path: string` | 10 分钟 |

### 工具集合策略

- **固定核心**：Read / Write / Bash / Git + 辅助工具，永远存在，保证 tools 前缀稳定
- **动态扩展**：Jenkins / GitLab 等场景工具按需追加在尾部

### 工具执行策略

- Agent 层直接执行，零延迟零开销
- 访问控制：环境变量白名单、配置路径白名单
- 会话级缓存 + TTL：默认 10 分钟，`get_time` 特殊 1 分钟

## 记忆注入策略

- **分层记忆**：高分记忆注入动态 System 层（层 4），低分记忆留在 SQLite 不注入
- 记忆类型：ToolCall / ToolResult / LlmResponse / UserInput / Decision / Summary

## Session 上下文管理

- **结构化槽位**：
  - 当前目标：1 条
  - 最近步骤结果：3-5 个
  - 活跃错误：最多 3 个
- 每个槽位限制最大字符数，超出则本地摘要压缩

## 性能策略

- **预编译缓存**：层 1~3 在启动时预编译为缓存字符串
- 运行时只需拼接层 4~7，构建延迟 <1ms

## 代码组织

```
llm/
  ├── mod.rs
  ├── router.rs
  ├── structured_output.rs
  ├── prompt_builder.rs        # 新增：Prompt 构建器（独立模块）
  ├── token/
  │   ├── window.rs            # 四层上下文窗口
  │   ├── summarizer.rs        # 渐进式压缩器
  │   └── tracker.rs           # Token 追踪器
  └── provider/
      ├── mod.rs
      ├── anthropic.rs
      ├── openai.rs
      ├── config.rs
      └── http_client.rs

tools/
  ├── builtin/
  │   ├── mod.rs
  │   ├── bash.rs
  │   ├── git.rs
  │   ├── read.rs
  │   ├── write.rs
  │   └── helpers.rs           # 新增：get_time / get_env / get_config
  └── jenkins.rs / gitlab.rs   # 场景工具
```

## 设计决策记录

| # | 决策 | 选择 | 原因 |
|---|------|------|------|
| 1 | Prompt 组装策略 | 混合方案 | System 静态化利用缓存，工具/记忆动态拼接 |
| 2 | 工具定义缓存 | 双轨制 | tools 字段完整 schema + system prompt 工具行为指南 |
| 3 | 前缀缓存边界 | 激进缓存 | 静态核心 + 静态工具 + 半静态规则全部缓存（~70-80%） |
| 4 | Memory 注入 | 分层记忆注入 | 高分记忆进 prompt，低分留数据库 |
| 5 | 压缩触发 | 混合触发 | 轮次到本地快压，token 快满 LLM 深压 |
| 6 | 压缩产物 | 替换 + 尾部追加 | Linear 删除，摘要追加 Compressed 尾部 |
| 7 | Token 估算 | 混合估算 | 中文 1.5/字，ASCII 4char/token |
| 8 | 工具集合 | 固定核心 + 动态扩展 | 核心工具稳定缓存，场景工具按需追加 |
| 9 | 规则注入 | 分层注入 | 通用规则静态层，项目规则半静态层，任务规则动态层 |
| 10 | Session 边界 | 结构化槽位 | 固定槽位 + 字符限制 + 超出压缩 |
| 11 | 对话裁剪 | 智能分层裁剪 | 需求/约束提升不裁剪，普通问答按时间裁剪 |
| 12 | 构建性能 | 预编译缓存 | 层 1~3 启动预编译，<1ms 延迟 |
| 13 | 动态内容 | 全部工具化 | 时间/环境/配置等改为工具，最大化前缀稳定 |
| 14 | 代码组织 | 独立模块 | llm/prompt_builder.rs 职责单一，可单独测试 |

---

## 前缀缓存实现决策（2026-05-26 grill-me 确认）

| # | 决策 | 选择 | 原因 |
|---|------|------|------|
| D1 | 缓存标记控制 | PromptBuilder 直接操控 `cache_control` | 精确控制每层缓存策略，provider 层做会丢失粒度 |
| D2 | 缓存断点位置 | 层 3 末尾 + 层 5 末尾各一个 | 两层断点覆盖 90%+ 缓存收益，不超限 |
| D3 | 缓存断点实现 | 内容块内嵌 `cache_control` | 单条 system 消息内即可控制，无需拆消息 |
| D4 | 跨 provider 抽象 | `Vec<ContentBlock>` + `CacheControl` 枚举 | 最灵活，未来支持多模态和 caching contours |
| D5 | Message 重构范围 | 全部变体改为 `Vec<ContentBlock>` | 接口一致性，未来多模态准备，From\<String\> 缓解迁移 |
| D6 | ContentBlock 类型 | 最小集 Text + Image | 当前够用，未来扩展 forward-compatible |
| D7 | 静态工具位置 | 仅 `tools` 字段，不重复进 system | `tools` 本身可缓存，避免 LLM 困惑 |
| D8 | provider 序列化 | 各 provider 各自实现 | Anthropic/OpenAI 格式差异大，无复用空间 |
| D9 | 静态工具注册 | PromptBuilder 构造时注入 | 项目级配置，build() 签名更简洁 |
| D10 | tools 缓存断点 | 静态工具后插断点 | 跨会话缓存静态工具，动态工具每次重传 |
| D11 | 工具分组 | static + workflow + request 三组 | 三层缓存粒度，workflow 同工作流内可缓存 |
| D12 | 记忆评分 | 类型加权 | Decision(0.9) > UserInput(0.8) > Summary(0.75) > LlmResponse(0.5) > ToolResult(0.4) > ToolCall(0.2) |
| D13 | LLM 深度压缩 | 专用压缩模型（Haiku） | 避免自引用死锁，成本低，Summarizer 持独立 provider |
| D14 | ToolUseLoop 集成 | ToolUseLoop 内置 PromptBuilder | 职责清晰，PromptBuilder 可独立测试和复用 |
| D15 | SessionSlots 更新 | ToolUseLoop 自动更新 | 信息足够，零延迟，无需跨层通信 |
| D16 | 压缩协作 | ToolUseLoop 内置 Summarizer，暂停压缩 | 避免竞态，本地压缩 <1ms，LLM 压缩 ~2s 可感知 |
| D17 | 压缩摘要 | structured_output 结构化输出 | 格式一致，可解析，复用已有模块 |
| D18 | 静态前缀来源 | 混合：system_core 硬编码，其余文件加载 | 核心保证可用，工具/规则可维护 |

### 实现阶段

**Phase 1 — 基础设施**
1. `ContentBlock` + `CacheControl` 类型定义
2. `Message` 类型重构（`String` → `Vec<ContentBlock>`）
3. provider 层序列化适配（Anthropic + OpenAI）

**Phase 2 — Prompt 构建**
4. `PromptBuilder` 接入内容块 + 缓存断点
5. 静态工具注册（构造时注入）
6. 工具分组合并（static + workflow + request）带缓存标记
7. `StaticPrefix` 内容加载（混合方案）

**Phase 3 — 压缩与记忆**
8. 记忆类型加权评分
9. `Summarizer` 接入专用压缩模型
10. 结构化压缩摘要
11. ToolUseLoop 集成 PromptBuilder + Summarizer + SessionSlots
