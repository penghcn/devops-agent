# Agent 架构升级设计文档 — 步骤链模式

**日期**: 2026-04-20
**目标**: 将 Agent 从单体函数重构为可扩展的步骤链架构，优先跑通 Jenkins 多分支构建完整链路

## 1. 背景与目标

### 当前问题
- `process_request` 函数（160+ 行）耦合了意图识别、Jenkins 调用、Claude 调用
- 无法组合多步流程（如：触发构建 → 等待完成 → 获取日志 → 智能分析）
- 无法方便地添加新工具
- 测试困难，无法对单个工具调用做独立测试

### 设计目标
1. **跑通 Jenkins 多分支构建链路**: 触发 → 等待 → 日志获取 → Claude 智能分析
2. **Step 独立可测**: 每个 Step 是一个独立的、可单元测试的工具调用单元
3. **可扩展**: 添加新工具只需实现一个 `Step` trait
4. **最小改动**: 不改前端，不改现有 Jenkins API 封装，只重构 Agent 编排层

## 2. 核心架构

### 2.1 组件关系

```
AgentRequest
  └── IntentRouter → 识别意图，返回 StepChain
        └── StepChain → 有序执行 Step，传递 StepContext
              ├── JenkinsTriggerStep
              ├── JenkinsWaitStep
              ├── JenkinsLogStep
              ├── JenkinsStatusStep
              ├── ClaudeAnalyzeStep
              └── ClaudeCodeStep (通用)
```

### 2.2 核心 Trait

```rust
pub trait Step: Send + Sync {
    /// Step 名称（用于日志和展示）
    fn name(&self) -> &str;

    /// 执行 Step，接收 mutable context 返回结果
    async fn execute(&self, ctx: &mut StepContext) -> StepResult;
}
```

### 2.3 StepContext — Step 间共享数据

```rust
pub struct StepContext {
    // 输入
    pub prompt: String,
    pub task_type: TaskType,
    pub job_name: Option<String>,
    pub branch: Option<String>,
    pub config: Arc<Config>,

    // 中间状态（Step 间传递）
    pub build_number: Option<u32>,
    pub pipeline_status: Option<serde_json::Value>,
    pub build_log: Option<String>,
    pub analysis_result: Option<String>,
}
```

### 2.4 StepResult

```rust
pub enum StepResult {
    Success { message: String },
    Failed { error: String },
    // 某些 Step 可能需要中断后续执行
    Abort { reason: String },
}
```

### 2.5 StepChain — 编排器

```rust
pub struct StepChain {
    steps: Vec<Box<dyn Step>>,
}

impl StepChain {
    pub fn new(steps: Vec<Box<dyn Step>>) -> Self { ... }

    /// 顺序执行所有 Step，任一 Step 失败则中断
    pub async fn execute(&self, ctx: StepContext) -> AgentResponse { ... }
}
```

## 3. Step 实现

### 3.1 JenkinsTriggerStep
- **输入**: job_name, branch
- **调用**: `jenkins::trigger_pipeline()`
- **输出**: build_number → 存入 `ctx.build_number`
- **失败**: 返回 `StepResult::Failed`

### 3.2 JenkinsWaitStep
- **输入**: job_name, branch, build_number
- **调用**: `jenkins::wait_for_pipeline()`
- **输出**: pipeline_status → 存入 `ctx.pipeline_status`
- **超时**: 返回 `StepResult::Failed`

### 3.3 JenkinsLogStep
- **输入**: job_name, branch, build_number
- **调用**: 新增 `jenkins::get_build_log()`
- **输出**: build_log → 存入 `ctx.build_log`

### 3.4 JenkinsStatusStep
- **输入**: job_name, branch
- **调用**: `jenkins::get_pipeline_status()`
- **输出**: pipeline_status → 存入 `ctx.pipeline_status`

### 3.5 ClaudeAnalyzeStep
- **输入**: pipeline_status 或 build_log（从 ctx 读取）
- **调用**: `claude::call_claude_code()` 传入分析 prompt
- **输出**: analysis_result → 存入 `ctx.analysis_result`

### 3.6 ClaudeCodeStep（通用）
- **输入**: prompt, allowed_tools
- **调用**: `claude::call_claude_code()`
- **输出**: 直接返回结果

## 4. IntentRouter — 意图到 StepChain 的映射

```rust
pub enum Intent {
    DeployPipeline { job_name: String, branch: Option<String> },
    BuildPipeline { job_name: String, branch: Option<String> },
    QueryPipeline { job_name: String, branch: Option<String> },
    AnalyzeBuild { job_name: String, branch: Option<String> },
    General,
}

pub struct IntentRouter;

impl IntentRouter {
    /// 意图识别（调用 Claude）
    pub async fn identify(&self, prompt: &str) -> Intent { ... }

    /// Intent → StepChain 映射
    pub fn to_chain(&self, intent: &Intent) -> StepChain { ... }
}
```

### Intent → StepChain 映射表

| Intent | StepChain |
|--------|-----------|
| DeployPipeline | [JenkinsTriggerStep, JenkinsWaitStep, ClaudeAnalyzeStep] |
| BuildPipeline | [JenkinsTriggerStep, JenkinsWaitStep, ClaudeAnalyzeStep] |
| QueryPipeline | [JenkinsStatusStep] |
| AnalyzeBuild | [JenkinsLogStep, ClaudeAnalyzeStep] |
| General | [ClaudeCodeStep] |

## 5. 新文件与改动范围

### 新增文件
```
backend/src/agent/
├── mod.rs          # 现有（保留 process_request 入口，改为调用 StepChain）
├── claude.rs       # 现有（不改）
├── step.rs         # 新增: Step trait, StepContext, StepResult, StepChain
├── steps/
│   ├── mod.rs      # 新增: 所有 Step 的 mod 声明
│   ├── jenkins_trigger.rs
│   ├── jenkins_wait.rs
│   ├── jenkins_log.rs
│   ├── jenkins_status.rs
│   ├── claude_analyze.rs
│   └── claude_code.rs
└── router.rs       # 新增: Intent, IntentRouter
```

### 修改文件
```
backend/src/agent/mod.rs       # 简化: process_request 改为 IntentRouter + StepChain
backend/src/tools/jenkins.rs   # 新增: get_build_log() 函数
```

### 不改动的文件
- 前端（App.vue, types.ts, api/agent.ts）
- claude.rs（现有 Claude 调用逻辑不变）
- config.rs
- main.rs

## 6. 测试策略

每个 Step 独立单元测试：
- `jenkins_trigger_test`: 模拟 Jenkins API，验证触发逻辑
- `jenkins_wait_test`: 验证轮询逻辑和超时
- `jenkins_log_test`: 验证日志获取
- `jenkins_status_test`: 验证状态查询
- `claude_analyze_test`: 验证分析 prompt 构造
- `step_chain_test`: 验证 Step 顺序执行和中断逻辑
- `intent_router_test`: 验证意图识别和映射

## 7. 演进路线

### Phase 1（本次）
- 实现 Step trait + StepContext + StepChain
- 实现 6 个 Step
- 实现 IntentRouter
- 重构 `process_request` 调用新架构
- 新增 `jenkins::get_build_log()`
- 编写核心测试

### Phase 2（后续）
- 添加并行 Step 支持（多个 Step 同时执行）
- 添加条件 Step（根据前一步结果决定是否继续）
- 添加更多工具（GitLab、Slack 通知等）
- 前端增加构建日志展示和分析结果展示
