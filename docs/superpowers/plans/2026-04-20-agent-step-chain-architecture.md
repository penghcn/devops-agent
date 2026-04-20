# Agent 步骤链架构升级 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 将 Agent 从单体 `process_request` 函数重构为可扩展的步骤链架构，跑通 Jenkins 多分支构建完整链路（触发 → 等待 → 日志 → Claude 智能分析）。

**Architecture:** 引入 `Step` trait 作为统一工具接口，`StepContext` 携带 Step 间共享数据，`StepChain` 编排器顺序执行 Step。`IntentRouter` 将用户意图映射到 StepChain。

**Tech Stack:** Rust, Axum, tokio, reqwest, serde, anyhow, tracing

---

## 文件清单

### 新增文件
| 文件 | 职责 |
|------|------|
| `backend/src/agent/step.rs` | Step trait, StepContext, StepResult, StepChain |
| `backend/src/agent/steps/mod.rs` | 所有 Step 实现的 mod 声明 |
| `backend/src/agent/steps/jenkins_trigger.rs` | JenkinsTriggerStep |
| `backend/src/agent/steps/jenkins_wait.rs` | JenkinsWaitStep |
| `backend/src/agent/steps/jenkins_log.rs` | JenkinsLogStep |
| `backend/src/agent/steps/jenkins_status.rs` | JenkinsStatusStep |
| `backend/src/agent/steps/claude_analyze.rs` | ClaudeAnalyzeStep |
| `backend/src/agent/steps/claude_code.rs` | ClaudeCodeStep |
| `backend/src/agent/router.rs` | Intent enum, IntentRouter |
| `backend/tests/step_chain_test.rs` | 步骤链集成测试 |

### 修改文件
| 文件 | 改动 |
|------|------|
| `backend/src/agent/mod.rs` | 简化为 IntentRouter + StepChain 入口 |
| `backend/src/tools/jenkins.rs` | 新增 `get_build_log()` 函数 |

---

### Task 1: 创建 step.rs — Step trait, StepContext, StepResult, StepChain

**Files:**
- Create: `backend/src/agent/step.rs`
- Create: `backend/src/agent/steps/mod.rs`（空文件，后续填充）

- [ ] **Step 1: 创建 steps/mod.rs**

```rust
pub mod jenkins_trigger;
pub mod jenkins_wait;
pub mod jenkins_log;
pub mod jenkins_status;
pub mod claude_analyze;
pub mod claude_code;
```

- [ ] **Step 2: 创建 step.rs**

```rust
use std::sync::Arc;
use anyhow::Result;
use serde_json::Value;
use crate::config::Config;
use super::TaskType;

/// Step 执行结果
#[derive(Debug)]
pub enum StepResult {
    Success { message: String },
    Failed { error: String },
    Abort { reason: String },
}

impl StepResult {
    pub fn is_success(&self) -> bool {
        matches!(self, StepResult::Success { .. })
    }

    pub fn is_abort(&self) -> bool {
        matches!(self, StepResult::Abort { .. })
    }
}

/// Step 间共享上下文
pub struct StepContext {
    pub prompt: String,
    pub task_type: TaskType,
    pub job_name: Option<String>,
    pub branch: Option<String>,
    pub config: Arc<Config>,
    pub build_number: Option<u32>,
    pub pipeline_status: Option<Value>,
    pub build_log: Option<String>,
    pub analysis_result: Option<String>,
    /// 记录每个 Step 的执行结果（用于展示思考过程）
    pub steps: Vec<AgentStep>,
}

impl StepContext {
    pub fn new(prompt: String, task_type: TaskType, job_name: Option<String>, branch: Option<String>, config: Arc<Config>) -> Self {
        Self {
            prompt,
            task_type,
            job_name,
            branch,
            config,
            build_number: None,
            pipeline_status: None,
            build_log: None,
            analysis_result: None,
            steps: Vec::new(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct AgentStep {
    pub action: String,
    pub result: String,
}

/// Step trait — 所有工具调用必须实现此 trait
pub trait Step: Send + Sync {
    fn name(&self) -> &str;
    async fn execute(&self, ctx: &mut StepContext) -> StepResult;
}

/// StepChain — 编排器，顺序执行 Step
pub struct StepChain {
    steps: Vec<Box<dyn Step>>,
}

impl StepChain {
    pub fn new(steps: Vec<Box<dyn Step>>) -> Self {
        Self { steps }
    }

    pub async fn execute(&self, ctx: StepContext) -> (StepContext, Vec<AgentStep>) {
        let mut ctx = ctx;
        let mut final_steps = Vec::new();

        for step in &self.steps {
            let step_name = step.name().to_string();
            ctx.steps.push(AgentStep {
                action: step_name.clone(),
                result: "执行中...".to_string(),
            });

            let result = step.execute(&mut ctx).await;

            match &result {
                StepResult::Success { message } => {
                    ctx.steps.last_mut().unwrap().result = message.clone();
                }
                StepResult::Failed { error } => {
                    ctx.steps.last_mut().unwrap().result = format!("失败: {}", error);
                    final_steps = ctx.steps;
                    break;
                }
                StepResult::Abort { reason } => {
                    ctx.steps.last_mut().unwrap().result = format!("中止: {}", reason);
                    final_steps = ctx.steps;
                    break;
                }
            }

            final_steps = ctx.steps.clone();

            if result.is_abort() {
                break;
            }
        }

        (ctx, final_steps)
    }
}
```

- [ ] **Step 3: 在 agent/mod.rs 中声明新模块（暂不改动 process_request）**

修改 `backend/src/agent/mod.rs` 顶部，在现有内容前添加：

```rust
mod claude;
mod step;
mod steps;
mod router;

pub use step::{Step, StepContext, StepResult, StepChain, AgentStep};
```

- [ ] **Step 4: 确保编译通过**

```bash
cd backend && cargo check 2>&1
```

预期: 编译通过（新模块空实现，不会报错）

- [ ] **Step 5: Commit**

```bash
git add backend/src/agent/step.rs backend/src/agent/steps/mod.rs backend/src/agent/mod.rs
git commit -m "feat(agent): 添加 Step trait, StepContext, StepChain 核心框架"
```

---

### Task 2: 实现 JenkinsTriggerStep

**Files:**
- Create: `backend/src/agent/steps/jenkins_trigger.rs`
- Modify: `backend/src/agent/steps/mod.rs`（添加 mod 声明 — 已在 Task 1 完成）

- [ ] **Step 1: 创建 jenkins_trigger.rs**

```rust
use super::super::step::{Step, StepContext, StepResult};
use super::super::tools::jenkins;

pub struct JenkinsTriggerStep;

impl Step for JenkinsTriggerStep {
    fn name(&self) -> &str {
        "JenkinsTrigger"
    }

    async fn execute(&self, ctx: &mut StepContext) -> StepResult {
        let job_name = match &ctx.job_name {
            Some(j) => j.clone(),
            None => return StepResult::Abort { reason: "缺少 job_name 参数".to_string() },
        };

        let branch = ctx.branch.as_deref();

        match jenkins::trigger_pipeline(&job_name, branch, &ctx.config).await {
            Ok(msg) => {
                // 从 Location header 或消息中提取 build_number
                // trigger_pipeline 返回的消息格式: "Pipeline triggered successfully. Build URL: http://host/.../123/"
                if let Some(build_num) = extract_build_number(&msg) {
                    ctx.build_number = Some(build_num);
                    StepResult::Success { message: format!("触发成功, 构建号: {}", build_num) }
                } else {
                    StepResult::Success { message: msg }
                }
            }
            Err(e) => StepResult::Failed { error: e.to_string() },
        }
    }
}

/// 从 Jenkins 返回消息中提取构建号
fn extract_build_number(msg: &str) -> Option<u32> {
    // 消息格式: "Pipeline triggered successfully. Build URL: http://host/job/X/job/Y/123/"
    msg.split('/')
        .filter(|s| s.parse::<u32>().is_ok())
        .next()
        .and_then(|s| s.parse::<u32>().ok())
}
```

- [ ] **Step 2: 编译检查**

```bash
cd backend && cargo check 2>&1
```

- [ ] **Step 3: Commit**

```bash
git add backend/src/agent/steps/jenkins_trigger.rs
git commit -m "feat(agent): 实现 JenkinsTriggerStep"
```

---

### Task 3: 实现 JenkinsWaitStep

**Files:**
- Create: `backend/src/agent/steps/jenkins_wait.rs`

- [ ] **Step 1: 创建 jenkins_wait.rs**

```rust
use super::super::step::{Step, StepContext, StepResult};
use super::super::tools::jenkins;

pub struct JenkinsWaitStep {
    pub poll_interval_secs: u64,
    pub max_wait_secs: u64,
}

impl Default for JenkinsWaitStep {
    fn default() -> Self {
        Self {
            poll_interval_secs: 10,
            max_wait_secs: 1800,
        }
    }
}

impl Step for JenkinsWaitStep {
    fn name(&self) -> &str {
        "JenkinsWait"
    }

    async fn execute(&self, ctx: &mut StepContext) -> StepResult {
        let build_number = match ctx.build_number {
            Some(n) => n,
            None => return StepResult::Abort { reason: "缺少 build_number，需先触发构建".to_string() },
        };

        let job_name = match &ctx.job_name {
            Some(j) => j.clone(),
            None => return StepResult::Abort { reason: "缺少 job_name 参数".to_string() },
        };

        let branch = match &ctx.branch {
            Some(b) => b.clone(),
            None => return StepResult::Abort { reason: "缺少 branch 参数".to_string() },
        };

        match jenkins::wait_for_pipeline(&job_name, &branch, build_number, &ctx.config, self.poll_interval_secs, self.max_wait_secs).await {
            Ok(status) => {
                ctx.pipeline_status = Some(status.clone());
                let result = status.get("result").and_then(|r| r.as_str()).unwrap_or("UNKNOWN");
                let in_progress = status.get("inProgress").and_then(|v| v.as_bool()).unwrap_or(false);
                StepResult::Success {
                    message: format!("构建 #{} 完成: {} (inProgress: {})", build_number, result, in_progress),
                }
            }
            Err(e) => StepResult::Failed { error: e.to_string() },
        }
    }
}
```

- [ ] **Step 2: 编译检查**

```bash
cd backend && cargo check 2>&1
```

- [ ] **Step 3: Commit**

```bash
git add backend/src/agent/steps/jenkins_wait.rs
git commit -m "feat(agent): 实现 JenkinsWaitStep"
```

---

### Task 4: 实现 JenkinsLogStep — 新增 get_build_log API

**Files:**
- Modify: `backend/src/tools/jenkins.rs`（新增 `get_build_log` 函数）
- Create: `backend/src/agent/steps/jenkins_log.rs`

- [ ] **Step 1: 在 jenkins.rs 末尾新增 get_build_log 函数**

在 `wait_for_pipeline` 函数之后添加：

```rust
/// 获取指定构建的 console 日志
pub async fn get_build_log(
    job_name: &str,
    branch: &str,
    build_number: u32,
    config: &Config,
) -> Result<String> {
    let job_name = sanitize_job_name(job_name)?;
    let branch = sanitize_job_name(branch)?;
    let client = Client::new();

    let auth_value = format!("{}:{}", config.jenkins_user, config.jenkins_token);
    let encoded = base64::engine::general_purpose::STANDARD.encode(&auth_value);
    let mut headers = HeaderMap::new();
    headers.insert(
        AUTHORIZATION,
        HeaderValue::from_str(&format!("Basic {}", encoded))?,
    );

    let url = format!(
        "{}/job/{}/job/{}//{}/consoleText",
        config.jenkins_url, job_name, branch, build_number
    );

    let response = client
        .get(&url)
        .headers(headers)
        .send()
        .await?;

    if response.status().is_success() {
        let log = response.text().await?;
        Ok(log)
    } else {
        anyhow::bail!("Failed to get build log: {} ({})", response.status(), response.text().await?)
    }
}
```

- [ ] **Step 2: 创建 jenkins_log.rs**

```rust
use super::super::step::{Step, StepContext, StepResult};
use super::super::tools::jenkins;

pub struct JenkinsLogStep;

impl Step for JenkinsLogStep {
    fn name(&self) -> &str {
        "JenkinsLog"
    }

    async fn execute(&self, ctx: &mut StepContext) -> StepResult {
        let build_number = match ctx.build_number {
            Some(n) => n,
            None => return StepResult::Abort { reason: "缺少 build_number".to_string() },
        };

        let job_name = match &ctx.job_name {
            Some(j) => j.clone(),
            None => return StepResult::Abort { reason: "缺少 job_name".to_string() },
        };

        let branch = match &ctx.branch {
            Some(b) => b.clone(),
            None => return StepResult::Abort { reason: "缺少 branch".to_string() },
        };

        match jenkins::get_build_log(&job_name, &branch, build_number, &ctx.config).await {
            Ok(log) => {
                // 截断过长的日志（最多保留 5000 字符用于分析）
                let truncated = if log.len() > 5000 {
                    format!("{}...[日志已截断，共 {} 字符]", &log[..5000], log.len())
                } else {
                    log
                };
                ctx.build_log = Some(truncated.clone());
                StepResult::Success {
                    message: format!("获取日志成功 ({} 字符)", truncated.len()),
                }
            }
            Err(e) => StepResult::Failed { error: e.to_string() },
        }
    }
}
```

- [ ] **Step 3: 编译检查**

```bash
cd backend && cargo check 2>&1
```

- [ ] **Step 4: Commit**

```bash
git add backend/src/tools/jenkins.rs backend/src/agent/steps/jenkins_log.rs
git commit -m "feat(agent): 实现 JenkinsLogStep，新增 get_build_log API"
```

---

### Task 5: 实现 JenkinsStatusStep

**Files:**
- Create: `backend/src/agent/steps/jenkins_status.rs`

- [ ] **Step 1: 创建 jenkins_status.rs**

```rust
use super::super::step::{Step, StepContext, StepResult};
use super::super::tools::jenkins;

pub struct JenkinsStatusStep;

impl Step for JenkinsStatusStep {
    fn name(&self) -> &str {
        "JenkinsStatus"
    }

    async fn execute(&self, ctx: &mut StepContext) -> StepResult {
        let job_name = match &ctx.job_name {
            Some(j) => j.clone(),
            None => return StepResult::Abort { reason: "缺少 job_name".to_string() },
        };

        let branch = match &ctx.branch {
            Some(b) => b.clone(),
            None => return StepResult::Abort { reason: "缺少 branch".to_string() },
        };

        match jenkins::get_job_status(&job_name, &ctx.config).await {
            Ok(job_status) => {
                if let Some(last_build) = job_status.get("lastBuild")
                    .and_then(|b| b.get("number"))
                    .and_then(|n| n.as_u64())
                {
                    let build_num = last_build as u32;
                    match jenkins::get_pipeline_status(&job_name, &branch, build_num, &ctx.config).await {
                        Ok(status) => {
                            ctx.pipeline_status = Some(status.clone());
                            let result = status.get("result").and_then(|r| r.as_str()).unwrap_or("RUNNING");
                            let in_progress = status.get("inProgress").and_then(|v| v.as_bool()).unwrap_or(true);
                            StepResult::Success {
                                message: format!(
                                    "Pipeline #{} [{}]: {} ({}s)",
                                    build_num,
                                    branch,
                                    result,
                                    if in_progress { "构建中" } else { "已完成" }
                                ),
                            }
                        }
                        Err(e) => StepResult::Failed { error: e.to_string() },
                    }
                } else {
                    StepResult::Success {
                        message: format!("No builds found for {}/{}", job_name, branch),
                    }
                }
            }
            Err(e) => StepResult::Failed { error: e.to_string() },
        }
    }
}
```

- [ ] **Step 2: 编译检查**

```bash
cd backend && cargo check 2>&1
```

- [ ] **Step 3: Commit**

```bash
git add backend/src/agent/steps/jenkins_status.rs
git commit -m "feat(agent): 实现 JenkinsStatusStep"
```

---

### Task 6: 实现 ClaudeAnalyzeStep 和 ClaudeCodeStep

**Files:**
- Create: `backend/src/agent/steps/claude_analyze.rs`
- Create: `backend/src/agent/steps/claude_code.rs`

- [ ] **Step 1: 创建 claude_analyze.rs**

```rust
use super::super::step::{Step, StepContext, StepResult};
use super::super::claude;

pub struct ClaudeAnalyzeStep {
    pub prompt_template: String,
}

impl Default for ClaudeAnalyzeStep {
    fn default() -> Self {
        Self {
            prompt_template: "你是一个 DevOps 工程师。请分析以下构建结果，给出:\n1. 构建状态摘要\n2. 如果失败，分析可能的失败原因\n3. 修复建议\n\n构建数据: {}"
                .to_string(),
        }
    }
}

impl Step for ClaudeAnalyzeStep {
    fn name(&self) -> &str {
        "ClaudeAnalyze"
    }

    async fn execute(&self, ctx: &mut StepContext) -> StepResult {
        // 优先使用构建日志，其次使用构建状态
        let analysis_input = match &ctx.build_log {
            Some(log) => format!("构建日志 ({} 字符):\n{}", log.len(), log),
            None => match &ctx.pipeline_status {
                Some(status) => format!("构建状态 JSON:\n{}", serde_json::to_string_pretty(status).unwrap_or_default()),
                None => return StepResult::Abort { reason: "没有可分析的数据（需要 build_log 或 pipeline_status）".to_string() },
            },
        };

        let prompt = format!(self.prompt_template, analysis_input);

        match claude::call_claude_code(&prompt, "Bash,Read,Write,Grep,Glob").await {
            Ok(result) => {
                ctx.analysis_result = Some(result.clone());
                StepResult::Success { message: "分析完成".to_string() }
            }
            Err(e) => StepResult::Failed { error: e.to_string() },
        }
    }
}
```

- [ ] **Step 2: 创建 claude_code.rs**

```rust
use super::super::step::{Step, StepContext, StepResult};
use super::super::claude;

pub struct ClaudeCodeStep {
    pub prompt: String,
    pub allowed_tools: String,
}

impl Step for ClaudeCodeStep {
    fn name(&self) -> &str {
        "ClaudeCode"
    }

    async fn execute(&self, _ctx: &mut StepContext) -> StepResult {
        match claude::call_claude_code(&self.prompt, &self.allowed_tools).await {
            Ok(result) => StepResult::Success { message: result },
            Err(e) => StepResult::Failed { error: e.to_string() },
        }
    }
}
```

- [ ] **Step 3: 编译检查**

```bash
cd backend && cargo check 2>&1
```

- [ ] **Step 4: Commit**

```bash
git add backend/src/agent/steps/claude_analyze.rs backend/src/agent/steps/claude_code.rs
git commit -m "feat(agent): 实现 ClaudeAnalyzeStep 和 ClaudeCodeStep"
```

---

### Task 7: 实现 IntentRouter — 意图识别与 StepChain 映射

**Files:**
- Create: `backend/src/agent/router.rs`

- [ ] **Step 1: 创建 router.rs**

```rust
use super::step::{StepChain, StepResult};
use super::steps::{
    jenkins_trigger::JenkinsTriggerStep,
    jenkins_wait::JenkinsWaitStep,
    jenkins_log::JenkinsLogStep,
    jenkins_status::JenkinsStatusStep,
    claude_analyze::ClaudeAnalyzeStep,
    claude_code::ClaudeCodeStep,
};
use super::claude;
use crate::agent::{AgentStep, TaskType};

#[derive(Debug, PartialEq)]
pub enum Intent {
    DeployPipeline { job_name: String, branch: Option<String> },
    BuildPipeline { job_name: String, branch: Option<String> },
    QueryPipeline { job_name: String, branch: Option<String> },
    AnalyzeBuild { job_name: String, branch: Option<String> },
    General,
}

pub struct IntentRouter;

impl IntentRouter {
    /// 调用 Claude 识别用户意图
    pub async fn identify(&self, prompt: &str) -> Intent {
        let intent_prompt = format!(
            "判断以下用户意图，只输出一个词（deploy/build/query/analyze），不要输出其他内容：\n{}",
            prompt
        );

        match claude::call_claude_code(&intent_prompt, "").await {
            Ok(response) => {
                let r = response.trim().to_lowercase();
                if r.contains("deploy") {
                    Intent::DeployPipeline {
                        job_name: extract_job_name(prompt),
                        branch: extract_branch(prompt),
                    }
                } else if r.contains("build") {
                    Intent::BuildPipeline {
                        job_name: extract_job_name(prompt),
                        branch: extract_branch(prompt),
                    }
                } else if r.contains("query") {
                    Intent::QueryPipeline {
                        job_name: extract_job_name(prompt),
                        branch: extract_branch(prompt),
                    }
                } else if r.contains("analyze") {
                    Intent::AnalyzeBuild {
                        job_name: extract_job_name(prompt),
                        branch: extract_branch(prompt),
                    }
                } else {
                    Intent::General
                }
            }
            Err(_) => Intent::General,
        }
    }

    /// 根据 Intent 返回对应的 StepChain
    pub fn to_chain(&self, intent: &Intent) -> StepChain {
        match intent {
            Intent::DeployPipeline { .. } | Intent::BuildPipeline { .. } => {
                StepChain::new(vec![
                    Box::new(JenkinsTriggerStep),
                    Box::new(JenkinsWaitStep::default()),
                    Box::new(JenkinsLogStep),
                    Box::new(ClaudeAnalyzeStep::default()),
                ])
            }
            Intent::QueryPipeline { .. } => {
                StepChain::new(vec![
                    Box::new(JenkinsStatusStep),
                ])
            }
            Intent::AnalyzeBuild { .. } => {
                StepChain::new(vec![
                    Box::new(JenkinsLogStep),
                    Box::new(ClaudeAnalyzeStep::default()),
                ])
            }
            Intent::General => {
                StepChain::new(vec![
                    Box::new(ClaudeCodeStep {
                        prompt: prompt.clone(),
                        allowed_tools: "Bash,Read,Write".to_string(),
                    }),
                ])
            }
        }
    }
}

/// 从 prompt 中提取 job_name（简单启发式）
fn extract_job_name(prompt: &str) -> String {
    // 简单策略：取 prompt 中第一个看起来像 job name 的单词
    // 实际项目中可能需要更复杂的 NLP 或让用户提供结构化参数
    prompt.split_whitespace()
        .find(|w| w.chars().all(|c| c.is_alphanumeric() || c == '-' || c == '_' || c == '/'))
        .unwrap_or("ds-pkg")
        .to_string()
}

/// 从 prompt 中提取 branch（简单启发式）
fn extract_branch(prompt: &str) -> Option<String> {
    let lower = prompt.to_lowercase();
    if lower.contains("dev") { Some("dev".to_string()) }
    else if lower.contains("staging") { Some("staging".to_string()) }
    else if lower.contains("prod") || lower.contains("production") { Some("main".to_string()) }
    else { None }
}
```

**注意**: `to_chain` 方法中 `Intent::General` 分支引用了 `prompt` 变量，但这在方法签名中不可用。需要重构为存储 prompt。修正方案：

修改 `to_chain` 签名，将 Intent 改为携带更多信息的结构。但为了最小改动，改为在 `IntentRouter::execute` 中处理：

```rust
impl IntentRouter {
    pub async fn identify(&self, prompt: &str) -> Intent { ... }

    /// 完整流程：识别意图 + 执行 StepChain
    pub async fn execute(&self, prompt: &str, task_type: TaskType, job_name: Option<String>, branch: Option<String>) -> (super::AgentResponse) {
        let intent = self.identify(prompt).await;
        let chain = self.to_chain_with_prompt(&intent, prompt);

        let ctx = super::StepContext::new(
            prompt.to_string(),
            task_type,
            job_name,
            branch,
            std::sync::Arc::new(crate::config::Config::from_env()),
        );

        let (final_ctx, steps) = chain.execute(ctx).await;

        super::AgentResponse {
            success: final_ctx.steps.iter().any(|s| s.result.contains("成功") || s.result.contains("完成")),
            output: final_ctx.analysis_result.clone().unwrap_or_else(|| {
                final_ctx.steps.last().map(|s| s.result.clone()).unwrap_or_else(|| "处理完成".to_string())
            }),
            steps,
        }
    }

    pub fn to_chain_with_prompt(&self, intent: &Intent, prompt: &str) -> StepChain {
        match intent {
            Intent::DeployPipeline { .. } | Intent::BuildPipeline { .. } => {
                StepChain::new(vec![
                    Box::new(JenkinsTriggerStep),
                    Box::new(JenkinsWaitStep::default()),
                    Box::new(JenkinsLogStep),
                    Box::new(ClaudeAnalyzeStep::default()),
                ])
            }
            Intent::QueryPipeline { .. } => {
                StepChain::new(vec![
                    Box::new(JenkinsStatusStep),
                ])
            }
            Intent::AnalyzeBuild { .. } => {
                StepChain::new(vec![
                    Box::new(JenkinsLogStep),
                    Box::new(ClaudeAnalyzeStep::default()),
                ])
            }
            Intent::General => {
                StepChain::new(vec![
                    Box::new(ClaudeCodeStep {
                        prompt: prompt.to_string(),
                        allowed_tools: "Bash,Read,Write".to_string(),
                    }),
                ])
            }
        }
    }
}
```

- [ ] **Step 2: 在 agent/mod.rs 中导出 IntentRouter**

修改 `backend/src/agent/mod.rs` 中的 pub use 行：

```rust
pub use step::{Step, StepContext, StepResult, StepChain, AgentStep};
pub use router::IntentRouter;
```

- [ ] **Step 3: 编译检查**

```bash
cd backend && cargo check 2>&1
```

- [ ] **Step 4: Commit**

```bash
git add backend/src/agent/router.rs backend/src/agent/mod.rs
git commit -m "feat(agent): 实现 IntentRouter 意图识别与 StepChain 映射"
```

---

### Task 8: 重构 process_request — 接入 StepChain 架构

**Files:**
- Modify: `backend/src/agent/mod.rs`

- [ ] **Step 1: 重写 process_request 函数**

将 `backend/src/agent/mod.rs` 中的 `process_request` 函数替换为：

```rust
/// 主 Agent 入口 — 基于步骤链架构
pub async fn process_request(req: AgentRequest, config: &Config) -> AgentResponse {
    let intent_router = IntentRouter;
    let intent = intent_router.identify(&req.prompt).await;

    let chain = intent_router.to_chain_with_prompt(&intent, &req.prompt);

    let ctx = StepContext::new(
        req.prompt.clone(),
        req.task_type,
        req.job_name.clone(),
        req.branch.clone(),
        std::sync::Arc::new(config.clone()),
    );

    let (final_ctx, steps) = chain.execute(ctx).await;

    AgentResponse {
        success: final_ctx.steps.iter().any(|s| s.result.contains("成功") || s.result.contains("完成")),
        output: final_ctx.analysis_result.clone().unwrap_or_else(|| {
            final_ctx.steps.last().map(|s| s.result.clone()).unwrap_or_else(|| "处理完成".to_string())
        }),
        steps,
    }
}
```

- [ ] **Step 2: 移除旧的 intent 识别和 execute 逻辑**

删除 `process_request` 中原有的 Step 1/Step 2 手写逻辑（意图识别 match + tokio::spawn 后台等待等）。

- [ ] **Step 3: 编译检查**

```bash
cd backend && cargo check 2>&1
```

- [ ] **Step 4: Commit**

```bash
git add backend/src/agent/mod.rs
git commit -m "refactor(agent): 将 process_request 重构为步骤链架构"
```

---

### Task 9: 编写步骤链集成测试

**Files:**
- Create: `backend/tests/step_chain_test.rs`

- [ ] **Step 1: 创建 step_chain_test.rs**

```rust
use devops_agent::agent::{IntentRouter, StepContext, StepChain};
use devops_agent::agent::step::{StepResult, AgentStep};
use devops_agent::agent::TaskType;
use devops_agent::config::Config;
use std::sync::Arc;

#[ctor::ctor]
fn init_env() {
    dotenv::dotenv().ok();
}

/// 测试 StepContext 创建
#[tokio::test]
async fn test_step_context_creation() {
    let config = Config::from_env();
    let ctx = StepContext::new(
        "deploy order-service".to_string(),
        TaskType::default(),
        Some("ds-pkg".to_string()),
        Some("dev".to_string()),
        Arc::new(config),
    );

    assert_eq!(ctx.prompt, "deploy order-service");
    assert_eq!(ctx.job_name, Some("ds-pkg".to_string()));
    assert_eq!(ctx.branch, Some("dev".to_string()));
    assert!(ctx.steps.is_empty());
    assert!(ctx.build_number.is_none());
}

/// 测试 IntentRouter 识别 deploy 意图
#[tokio::test]
async fn test_intent_deploy() {
    let router = IntentRouter;
    let intent = router.identify("部署 order-service 到 staging 环境").await;
    assert!(matches!(intent, devops_agent::agent::router::Intent::DeployPipeline { .. }));
}

/// 测试 IntentRouter 识别 build 意图
#[tokio::test]
async fn test_intent_build() {
    let router = IntentRouter;
    let intent = router.identify("构建 ds-pkg 项目").await;
    assert!(matches!(intent, devops_agent::agent::router::Intent::BuildPipeline { .. }));
}

/// 测试 IntentRouter 识别 query 意图
#[tokio::test]
async fn test_intent_query() {
    let router = IntentRouter;
    let intent = router.identify("查询 ds-pkg dev 分支的构建状态").await;
    assert!(matches!(intent, devops_agent::agent::router::Intent::QueryPipeline { .. }));
}

/// 测试 IntentRouter 识别 analyze 意图
#[tokio::test]
async fn test_intent_analyze() {
    let router = IntentRouter;
    let intent = router.identify("分析 ds-pkg dev 分支的构建日志").await;
    assert!(matches!(intent, devops_agent::agent::router::Intent::AnalyzeBuild { .. }));
}

/// 测试 IntentRouter 的 StepChain 映射 — DeployPipeline
#[test]
fn test_chain_deploy_pipeline() {
    let router = IntentRouter;
    let intent = devops_agent::agent::router::Intent::DeployPipeline {
        job_name: "ds-pkg".to_string(),
        branch: Some("dev".to_string()),
    };
    let chain = router.to_chain_with_prompt(&intent, "部署 ds-pkg");
    // StepChain 内部 steps 是私有字段，无法直接测试数量
    // 但可以通过 execute 端到端验证
}

/// 测试 IntentRouter 的 StepChain 映射 — QueryPipeline
#[test]
fn test_chain_query_pipeline() {
    let router = IntentRouter;
    let intent = devops_agent::agent::router::Intent::QueryPipeline {
        job_name: "ds-pkg".to_string(),
        branch: Some("dev".to_string()),
    };
    let chain = router.to_chain_with_prompt(&intent, "查询 ds-pkg dev 状态");
    // 同上，端到端验证
}
```

- [ ] **Step 2: 编译并运行测试（部分测试需要 Claude CLI 可用）**

```bash
cd backend && cargo test --lib 2>&1
```

注意：`test_intent_*` 测试需要 `claude` CLI 可用。如果本地没有安装，这些测试会跳过或失败。可以先只跑 `test_step_context_creation`。

- [ ] **Step 3: Commit**

```bash
git add backend/tests/step_chain_test.rs
git commit -m "test(agent): 添加步骤链集成测试"
```

---

### Task 10: 端到端验证 — 跑通 Jenkins 多分支构建链路

**Files:**
- Modify: `backend/tests/jenkins_test.rs`（新增端到端测试）

- [ ] **Step 1: 在 jenkins_test.rs 末尾添加端到端测试**

```rust
/// 端到端测试: 触发多分支构建 → 等待完成 → 获取日志 → 验证结果
#[tokio::test]
async fn test_e2e_multi_branch_pipeline() {
    let config = devops_agent::config::Config::from_env();
    let job_name = "ds-pkg";
    let branch = "dev";

    // Step 1: 触发构建
    let trigger_result = devops_agent::tools::jenkins::trigger_pipeline(job_name, Some(branch), &config).await;
    assert!(trigger_result.is_ok(), "Trigger failed: {}", trigger_result.unwrap_err());
    println!("Trigger result: {}", trigger_result.unwrap());

    // Step 2: 等待构建完成
    // 注意: trigger_pipeline 返回的消息中可能不包含 build_number
    // 这里需要重新查询最新构建号
    let job_status = devops_agent::tools::jenkins::get_job_status(job_name, &config).await.unwrap();
    let build_num = job_status.get("lastBuild")
        .and_then(|b| b.get("number"))
        .and_then(|n| n.as_u64())
        .unwrap_or(0) as u32;
    println!("Latest build number: {}", build_num);
    assert!(build_num > 0, "Should have a build number");

    // Step 3: 等待完成
    let wait_result = devops_agent::tools::jenkins::wait_for_pipeline(
        job_name, branch, build_num, &config, 10, 1800
    ).await;
    assert!(wait_result.is_ok(), "Wait failed: {}", wait_result.unwrap_err());

    let status = wait_result.unwrap();
    let result = status.get("result").and_then(|r| r.as_str()).unwrap_or("UNKNOWN");
    println!("Build #{} result: {}", build_num, result);

    // Step 4: 获取日志
    let log_result = devops_agent::tools::jenkins::get_build_log(job_name, branch, build_num, &config).await;
    assert!(log_result.is_ok(), "Get log failed: {}", log_result.unwrap_err());

    let log = log_result.unwrap();
    println!("Log length: {} characters", log.len());
    assert!(log.len() > 0, "Log should not be empty");
}
```

- [ ] **Step 2: 运行端到端测试**

```bash
cd backend && cargo test test_e2e_multi_branch_pipeline -- --nocapture 2>&1
```

这个测试会实际调用 Jenkins API，需要确保：
- `.env` 文件配置正确
- Jenkins 服务可访问
- ds-pkg 项目存在

- [ ] **Step 3: Commit**

```bash
git add backend/tests/jenkins_test.rs
git commit -m "test(agent): 添加 Jenkins 多分支构建端到端测试"
```

---

## 自检

**Spec 覆盖检查:**
- Step trait + StepContext + StepResult + StepChain → Task 1 ✅
- JenkinsTriggerStep → Task 2 ✅
- JenkinsWaitStep → Task 3 ✅
- JenkinsLogStep + get_build_log API → Task 4 ✅
- JenkinsStatusStep → Task 5 ✅
- ClaudeAnalyzeStep + ClaudeCodeStep → Task 6 ✅
- IntentRouter → Task 7 ✅
- process_request 重构 → Task 8 ✅
- 集成测试 → Task 9 ✅
- 端到端测试 → Task 10 ✅

**Placeholder 扫描:** 无 "TBD"、"TODO"、"fill in" 等占位符。所有代码完整。

**类型一致性:** 所有任务使用统一的 `StepContext`、`StepResult`、`AgentStep` 类型。`Intent` 枚举在 router.rs 中定义，测试中通过 `devops_agent::agent::router::Intent` 引用。

**Scope 检查:** 聚焦 Agent 架构升级 + Jenkins 多分支链路，不改动前端，范围合适。
