# DevOps Agent 基础设施设计

> **日期:** 2026-04-28
> **状态:** 已审批（2026-04-29 修订）
> **范围:** Harness 编排、记忆系统、Token 管理、权限控制、沙箱隔离、LLM 多提供商抽象、模型路由分级

---

## 修订记录

| 日期 | 变更 |
|------|------|
| 2026-04-28 | 初始设计 |
| 2026-04-29 | 修订：Token 压缩策略改为纯轮次触发 + 渐进式三阶段；LLM Provider 缩小为 Anthropic + OpenAI；明确迁移替换策略；TokenUsage 统一来源 |

---

## 零、本轮范围与边界

### 构建内容

| 模块 | Task | 说明 |
|------|------|------|
| Harness | 1-3 | Hook trait、Orchestrator、Session |
| Memory | 4-5 | 短期记忆（环形缓冲）、长期记忆（SQLite） |
| Token | 6-8 | 计数器、四层上下文、渐进式压缩 |
| Security | 9 | 权限策略引擎 + 审计日志 |
| Sandbox | 10 | 三层沙箱隔离 |
| Tools | 11 | 内置工具（Read/Write/Bash/Git） |
| LLM | 12-13 | Anthropic + OpenAI 统一客户端，路由 + 结构化输出 |
| 集成 | 14 | Token Hook + Memory Hook + 迁移现有 7 个 Step |
| 验证 | 15 | 全量构建 + clippy + 测试 |

### 不在本轮

- DeepSeek、Qwen Provider（下轮扩展）
- DecisionStep 多智能体决策点注入（下轮，依赖本轮底座）

### 关键决策

1. **迁移替换**: 新 `harness/` 模块取代现有 `agent/` 编排逻辑，现有 Step 逐步迁移
2. **先建底座再迁移**: Task 1-13 完成所有新模块，Task 14 一次性迁移现有 Step
3. **Provider 范围**: 仅 Anthropic + OpenAI，留扩展接口
4. **TokenUsage 统一**: 在 `llm/message.rs` 单一来源定义，token 模块引用

---

## 一、整体架构

### 模块关系

```
Harness Orchestrator (编排核心)
├── Hook: Memory (记忆系统)
├── Hook: Token (Token 管理)
├── Hook: Security (权限控制)
├── Sandbox (沙箱隔离)
├── Tools (工具集)
├── LLM Router (模型路由)
└── Structured Output (结构化输出)
```

### 新增文件结构

```
backend/src/
├── harness/            # 编排框架
│   ├── mod.rs
│   ├── orchestrator.rs # 步骤链编排 + hook 生命周期
│   ├── hook.rs         # Hook trait + 钩子点定义
│   ├── session.rs      # 会话创建/恢复/销毁
│   ├── token_hook.rs   # Token 预算追踪 Hook
│   └── memory_hook.rs  # 记忆保存 Hook
│
├── memory/             # 记忆系统
│   ├── mod.rs          # MemoryEntry + MemoryType
│   ├── short_term.rs   # 环形缓冲区，容量 200 条
│   ├── long_term.rs    # SQLite 持久化 + 关键词索引
│   └── store.rs        # SQLite 初始化 + 迁移
│
├── token/              # Token 管理
│   ├── mod.rs
│   ├── tracker.rs      # 实时计数 + 预算 + 告警 + 轮次计数
│   ├── window.rs       # 四层上下文管理
│   └── summarizer.rs   # LLM 摘要：渐进式三阶段压缩
│
├── security/           # 权限控制
│   ├── mod.rs
│   ├── permission.rs   # 权限模型：角色 + 策略
│   ├── policy.rs       # 策略引擎：ALLOW / DENY / PROMPT
│   └── audit.rs        # 操作审计日志
│
├── sandbox/            # 沙箱隔离
│   ├── mod.rs
│   ├── filesystem.rs   # 文件系统隔离 + 选择性挂载
│   ├── process.rs      # 进程限制 + 环境净化
│   ├── network.rs      # 网络白名单
│   └── validator.rs    # 路径穿越检测
│
├── tools/              # 工具集（扩展）
│   ├── builtin/
│   │   ├── mod.rs
│   │   ├── read.rs     # 安全文件读取（路径白名单）
│   │   ├── write.rs    # 文件写入（路径白名单 + 大小限制）
│   │   ├── bash.rs     # 命令执行（黑名单 + 超时 + 沙箱）
│   │   └── git.rs      # git 操作封装
│   └── external/       # 已有 jenkins.rs, gitlab.rs
│
├── llm/                # LLM 提供商抽象
│   ├── mod.rs          # LlmProvider trait + Provider 枚举
│   ├── client.rs       # ChatRequest / ChatResponse
│   ├── router.rs       # L1/L2 模型路由决策引擎
│   ├── structured.rs   # Schema 强约束输出
│   ├── message.rs      # 统一消息格式 + TokenUsage（单一来源）
│   ├── config.rs       # 多 provider 配置管理
│   └── providers/
│       ├── mod.rs
│       ├── openai.rs
│       └── anthropic.rs
```

---

## 二、记忆系统

### 短期记忆

- **结构:** 环形缓冲区，容量 200 条
- **内容:** 工具调用、LLM 响应、用户输入、中间结果
- **生命周期:** 会话内有效，会话结束清除

### 长期记忆

- **存储:** SQLite（轻量，无外部依赖）
- **索引:** 关键词 + 评分
- **内容:** 项目配置、用户偏好、历史构建记录、决策记录
- **持久化:** 会话结束时自动保存，支持跨会话恢复

---

## 三、Token 管理

### 渐进式三阶段压缩

```
阶段 1 - 线性 (轮次 1 ~ summary_threshold):
  R1  R2  R3  R4  R5  R6  R7  R8  R9  R10
  ─────────────────────────────────────────
  零开销，全部保留

阶段 2 - 摘要 (轮次 > summary_threshold):
  触发: 轮次 > summary_threshold (默认 10)

  [摘要(R1-R5)] [R6] [R7] [R8] [R9] [R10] [R11]
  ───────────── ─────────────────────────────────
    压缩区        线性区 (保留最近 linear_window 轮)

  LLM 摘要 rounds 1 ~ N-linear_window，保留最近 linear_window 轮

阶段 3 - 结构化 (轮次 > structure_threshold):
  触发: 轮次 > structure_threshold (默认 15)

  [摘要(R1-R5)] [结构化文档] [R11] [R12] [R13] [R14] [R15]
  ───────────── ────────────── ──────────────────────────────
    压缩区         结构化区        线性区 (保留最近 5 轮)

  每轮新增 → 最旧线性轮滑入结构化文档 → L1 模型增量更新
```

### 可配置参数

```rust
pub struct SummarizerConfig {
    pub summary_threshold: u32,   // 触发首次摘要的轮次，默认 10
    pub structure_threshold: u32, // 切换到结构化模式的轮次，默认 15
    pub linear_window: u32,       // 线性区保留轮数，默认 5
}
```

### 结构化文档格式

```json
{
  "confirmed": ["用户确认使用 Rust + Axum", "构建超时阈值 30s"],
  "conflicts": ["部署方案未定: Docker vs 直接部署"],
  "pending": ["需要验证数据库迁移脚本", "前端 API 接口待确认"]
}
```

- `confirmed`: 已确认的结论、决策、事实
- `conflicts`: 存在分歧或矛盾的点
- `pending`: 需要补充信息或后续行动项

### 结构化文档更新流程

```
新轮次完成 (LlmResult Hook)
    ↓
检查: 轮次 > structure_threshold 且线性区 > linear_window ?
    ↓ 是
取线性区最旧一轮
    ↓
调用 L1 模型: "基于当前结构化文档，融入这轮对话，增量更新"
    ↓
替换结构化文档
    ↓
线性区收缩一轮，保持 linear_window 轮
```

- **独立 LLM 调用**: 使用 L1 模型，不污染主 Agent 上下文
- **滑动窗口**: 每轮将最旧一轮滑入结构化文档，增量更新
- **无大小限制**: 结构化文档不设 Token 上限

### 四层上下文

| 层级 | 名称 | 内容 | 保留策略 |
|------|------|------|---------|
| Layer 0 | System | 系统指令、技能定义 | 永不压缩 |
| Layer 1 | Compressed | LLM 生成的摘要块 | 多条摘要，按时间排列 |
| Layer 2 | Structured | 结构化文档 (confirmed/conflicts/pending) | 每轮增量更新 |
| Layer 3 | Linear | 最近 N 轮线性对话 | 完整保留 |

### Token 计数器

- 实时追踪 `prompt_tokens` / `completion_tokens` / `total_tokens`
- 预算上限检查 + 告警
- 对话轮次计数 `round_count`

---

## 四、权限控制

### 权限模型

```
ToolRequest
├── tool: String          # 工具名
├── args: Value           # 参数
├── session_id: UUID      # 会话标识
└── user_role: Role       # 用户角色

Policy Decision:
├── ALLOW   → 直接执行
├── DENY    → 拒绝，返回错误
└── PROMPT  → 暂停，等待用户确认
```

### 安全策略

| 工具 | 安全机制 |
|------|---------|
| Read | 路径白名单，禁止 `.env`、`*.key`、`*.pem` |
| Write | 路径白名单 + 10MB 大小限制 |
| Bash | 命令黑名单（rm -rf, dd, curl 外部 URL）+ 30s 超时 |
| Jenkins | 已有 token 隔离 + 操作日志 |

---

## 五、沙箱隔离

### 三层隔离

| 层级 | 隔离类型 | 状态 | 机制 |
|------|---------|------|------|
| 第 1 层 | 文件系统 | 始终开启 | 隔离工作区 + 选择性只读挂载 |
| 第 2 层 | 进程 | 始终开启 | 超时 + 内存限制 + 环境净化 |
| 第 3 层 | 网络 | 可选开启 | 默认阻断，白名单放行 |

### 文件系统沙箱

```
sandbox/
├── workspace/            # 隔离工作区
│   ├── tmp/              # 临时文件
│   └── output/           # 执行结果
├── mounts/               # 只读挂载
│   ├── project-code/     → 项目源码
│   └── config/           → 配置文件
└── lock.txt              # 沙箱状态锁
```

### 进程沙箱

- **超时:** 30s 默认
- **内存:** 256MB 上限
- **输出:** 1MB 截断
- **环境净化:** 清除 `API_KEY`、`TOKEN` 等敏感变量

### 跨平台

- **macOS:** 文件系统 + 进程隔离（应用层网络控制）
- **Linux:** 增强模式（cgroups 网络 + 进程隔离）

---

## 六、LLM 提供商抽象

### 核心 Trait

```rust
#[async_trait]
pub trait LlmProvider: Send + Sync {
    fn name(&self) -> &str;
    async fn chat(&self, req: &ChatRequest) -> anyhow::Result<ChatResponse>;
    async fn chat_with_schema<T: schemars::JsonSchema>(
        &self,
        req: &ChatRequest,
    ) -> anyhow::Result<T>;
}
```

### 支持 Provider（本轮）

- **OpenAI** — GPT-4o, GPT-4o-mini
- **Anthropic** — Claude Sonnet, Claude Haiku

### 支持 Provider（后续扩展）

- **DeepSeek** — DeepSeek-V3, DeepSeek-R1
- **Qwen** — Qwen-Plus, Qwen-Max

### 与现有 Claude Code CLI 的关系

- `agent/claude.rs` — 本地 CLI 进程调用（保持不变）
- `llm/` — HTTP API 调用（摘要压缩、意图解析、轻量判断）

---

## 七、结构化输出约束

### 机制

利用各 provider 的 Tool Use / Function Calling / JSON Schema 能力，强制 LLM 返回符合 schema 的结构化数据。
返回数据需要 json 解析并校验，默认{}

### Provider 映射

| Provider | 约束机制 |
|----------|---------|
| OpenAI | `response_format: JSONSchema` |
| Anthropic | `tools: tool use` |

### 自动重试

反序列化失败 → 错误信息附加到 prompt → 重新调用（最多 2 次）→ 仍失败返回原始文本

---

## 八、模型路由分级

### L1 经济模型

- **任务:** 代码注释生成、Commit message 格式化、单测生成、PR 摘要、**结构化文档增量更新**
- **模型:** GPT-4o-mini / Claude Haiku
- **预算:** 5000 tokens/次

### L2 专业模型

- **任务:** Bug 根因分析、架构建议、复杂代码生成、安全审查
- **模型:** GPT-4o / Claude Sonnet
- **预算:** 20000 tokens/次

### 路由策略

- **Fixed:** 按任务级别直接选择
- **CostFirst:** L1 优先，失败升级 L2
- **QualityFirst:** L2 优先，简单任务降级 L1

### 降级链

```
L1 失败: GPT-4o-mini → Claude Haiku → 升级 L2
L2 失败: Claude Sonnet → GPT-4o
```

---

## 九、错误处理

| 错误类型 | 处理策略 |
|---------|---------|
| 沙箱创建失败 | 拒绝执行，返回明确错误 |
| Token 超预算 | 触发压缩，压缩后重试一次 |
| 压缩后仍超预算 | 截断最低优先级层，保留 Linear 层 |
| 权限拒绝 | 返回 DENY 错误 + 审计日志 |
| 工具执行超时 | 终止进程，返回超时错误 |
| LLM 调用失败 | 指数退避重试，最多 3 次 |
| 反序列化失败 | 自动重试 2 次，仍失败返回原始文本 |

---

## 十、测试策略

| 测试类型 | 覆盖范围 |
|---------|---------|
| 单元测试 | Memory CRUD, Token 计数, 路径穿越检测, 权限策略, 模型路由 |
| 集成测试 | 沙箱隔离验证, Hook 触发顺序, 上下文压缩正确性 |
| E2E 测试 | 完整请求流程: 意图 → 权限 → 执行 → 记忆 → 返回 |

---

## 配置示例

```env
# LLM 路由
LLM_ROUTER_STRATEGY=cost_first
LLM_L1_MODELS=gpt-4o-mini,claude-haiku
LLM_L1_DEFAULT=gpt-4o-mini
LLM_L2_MODELS=claude-sonnet-4-6,gpt-4o
LLM_L2_DEFAULT=claude-sonnet-4-6
LLM_FALLBACK_ENABLED=true
LLM_MAX_FALLBACK_ATTEMPTS=2

# Provider API Keys
LLM_OPENAI_API_KEY=sk-xxx
LLM_ANTHROPIC_API_KEY=sk-ant-xxx

# 沙箱配置
SANDBOX_TIMEOUT=30
SANDBOX_MAX_MEMORY=268435456
SANDBOX_MAX_OUTPUT=1048576

# Token 预算
TOKEN_L1_BUDGET=5000
TOKEN_L2_BUDGET=20000

# Token 压缩配置
TOKEN_SUMMARY_THRESHOLD=10
TOKEN_STRUCTURE_THRESHOLD=15
TOKEN_LINEAR_WINDOW=5
```
