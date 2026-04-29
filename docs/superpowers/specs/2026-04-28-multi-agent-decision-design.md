# 多智能体决策点注入设计

**日期:** 2026-04-28
**状态:** 设计方案
**驱动需求:** 在现有 Step 链的确定性之上，注入 Agent 自主决策能力，为未来演进做准备

## 核心决策

- **选型**: 决策点注入（在 Step 链中插入 DecisionStep），而非替换 Step 链或双轨模型
- **理由**: 运维场景稳定压倒一切；决策点只在"需要判断"的地方引入不确定性，其余流程保持确定
- **约束**: 可逆操作自主执行（重试、回滚、降级），不可逆操作需人工确认（发布、删除）

## 架构

```
现有：Step1 → Step2 → Step3 → Step4 (线性)
新增：Step1 → Step2 → [DecisionStep] → 根据决策走分支
                                        ├→ 重试 Step2
                                        ├→ 执行回滚 Step
                                        └→ 继续 Step3
```

DecisionStep 是 Step trait 的一个实现，Orchestrator 无需感知。

## 核心组件

### AgentRole

| 角色 | 职责 | 典型场景 |
|------|------|----------|
| Architect | 分析根因、制定方案 | 构建失败后分析日志 |
| Coder | 执行修复、生成代码 | 根据分析结果编写修复脚本 |
| Tester | 验证结果、回归测试 | 修复后验证功能 |
| Reviewer | 质量审查、风险评估 | 部署前安全审查 |

### Reversibility

| 类型 | 操作示例 | 处理方式 |
|------|----------|----------|
| Reversible | 重试、回滚、降级 | Agent 自主执行 |
| Irreversible | 发布到生产、删除数据 | 人工确认后执行 |

### DecisionStep

```rust
pub struct DecisionStep {
    role: AgentRole,
    reversibility: Reversibility,
    prompt_template: String,
    allowed_actions: Vec<Action>,
    context_builder: Box<dyn ContextBuilder>,
}
```

### Action

```rust
pub enum Action {
    Retry { count: u32 },
    Rollback { to_step: usize },
    Continue,
    Escalate { reason: String },
    Custom { name: String },
}
```

### DecisionResult

```rust
pub struct DecisionResult {
    pub action: Action,
    pub reasoning: String,      // Agent 思考过程
    pub confidence: f32,        // 置信度 0.0-1.0
    pub requires_approval: bool,
}
```

### DecisionContext

```rust
pub struct DecisionContext {
    pub previous_outputs: Vec<String>,
    pub error_logs: Vec<String>,
    pub environment: HashMap<String, String>,
    pub timestamp: DateTime<Utc>,
}
```

### ContextBuilder

```rust
#[async_trait]
pub trait ContextBuilder: Send + Sync {
    async fn build(&self, step_results: &[String]) -> DecisionContext;
}
```

### HumanConfirmationGate

不可逆操作的决策结果通过此门控，支持 webhook / CLI prompt / API 等确认通道。

## 数据流

```
Step1(预检) → Step2(触发构建) → [DecisionStep: Architect]
                                              ↓
                                    分析构建日志，判断根因
                                              ↓
                              ┌───────────────┴───────────────┐
                              ↓                               ↓
                        可逆操作（自主执行）          不可逆操作（需确认）
                              ↓                               ↓
                        重试构建 / 回滚              发布到生产？
                              ↓                       → 发送审批请求
                        Step3(等待)                  → 等待人工确认
                              ↓                         ↓
                        Step4(验证)              批准→执行 / 拒绝→回滚
```

## 错误处理

| 场景 | 处理方式 |
|------|----------|
| LLM 不可用 | 返回 Escalate，升级给人 |
| 决策置信度 < 0.3 | 自动 Escalate |
| 人工拒绝 | 返回 Err，Orchestrator 按失败流程处理 |
| 可逆操作失败 | 记录到 Hook，不阻断流程 |

## 与现有架构的关系

- **零侵入**：Step trait 签名不变，Orchestrator 无需修改
- **复用 Hook**：决策结果通过 HookPoint 记录（新增 `DecisionMade` 钩子点）
- **复用 Security**：不可逆操作通过 PolicyEngine 校验
- **复用 Token**：DecisionStep 内部调用 LLM，受 TokenHook 跟踪

## 测试策略

- **Unit**: DecisionStep 各角色的 prompt 渲染、Action 执行
- **Integration**: DecisionStep 注册到 Orchestrator，验证决策分支
- **Mock**: StubLlmProvider 返回预设决策，验证可逆/不可逆分支

## 演进路径

1. **Phase 5**: 实现 DecisionStep 基础框架 + Architect 角色 + Reversible 决策
2. **Phase 6**: 实现 HumanConfirmationGate + Irreversible 决策
3. **Phase 7**: 实现 Coder/Tester/Reviewer 角色
4. **Phase 8**: 多 DecisionStep 串联（Architect 分析 → Coder 修复 → Tester 验证）
