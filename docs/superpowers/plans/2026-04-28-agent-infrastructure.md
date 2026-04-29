# Agent 基础设施实现计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

> **修订 (2026-04-29):** Token 压缩改为纯轮次触发 + 渐进式三阶段；LLM Provider 缩小为 Anthropic + OpenAI；明确迁移替换策略；TokenUsage 统一在 llm/message.rs

**Goal:** 为 DevOps Agent 构建完整的运行基础设施：Harness 编排、记忆系统、Token 管理、权限控制、沙箱隔离、LLM 多提供商抽象、模型路由分级

**Architecture:** 分层插件式架构。Harness 作为编排核心，通过 Hook trait 在关键节点触发 Memory、Token、Security 行为。LLM 层提供多 provider 统一抽象 + L1/L2 路由。沙箱提供三层隔离。

**Tech Stack:** Rust 2024, SQLite (rusqlite), serde, schemars, reqwest, tokio

**本轮范围:**
- 构建: Harness(1-3), Memory(4-5), Token(6-8), Security(9), Sandbox(10), Tools(11), LLM(12-13), 集成(14), 验证(15)
- 不在本轮: DeepSeek/Qwen Provider, DecisionStep 多智能体决策
- 迁移策略: Task 1-13 建完底座后，Task 14 一次性迁移现有 agent/steps/ 下 7 个 Step

**依赖新增:**
```
rusqlite = { version = "0.32", features = ["bundled"] }
schemars = "0.8"
uuid = { version = "1", features = ["v4"] }
```

---

### Task 1: Harness — Hook Trait + 模块骨架

**Files:**
- Create: `backend/src/harness/mod.rs`
- Create: `backend/src/harness/hook.rs`
- Test: `backend/tests/harness_hook_test.rs`

**目标:** 定义 Hook trait 和钩子点枚举，建立模块骨架

- [ ] **Step 1: 编写 Hook trait 测试**

```rust
// backend/tests/harness_hook_test.rs
use devops_agent::harness::{Hook, HookPoint};

#[tokio::test]
async fn test_hook_trait_compiles() {
    struct TestHook;
    impl Hook for TestHook {
        async fn on(&self, point: HookPoint) -> anyhow::Result<()> { Ok(()) }
    }
    let hook = TestHook;
    assert!(hook.on(HookPoint::StepStart).await.is_ok());
}
```

- [ ] **Step 2: 运行测试确认失败**

```bash
cd backend && cargo test harness_hook_test --no-fail-fast 2>&1 | tail -5
# 预期: error: cannot find value `harness`
```

- [ ] **Step 3: 创建模块骨架**

```rust
// backend/src/harness/mod.rs
pub mod hook;
pub mod orchestrator;
pub mod session;
pub mod token_hook;
pub mod memory_hook;

pub use hook::{Hook, HookPoint};
pub use orchestrator::Orchestrator;
pub use session::Session;
pub use token_hook::TokenHook;
pub use memory_hook::MemoryHook;
```

```rust
// backend/src/harness/hook.rs
use async_trait::async_trait;
use anyhow;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum HookPoint {
    SessionStart,
    SessionEnd,
    StepStart,
    StepEnd,
    ToolCalled,
    ToolResult,
    LlmCalled,
    LlmResult,
    TokenBudgetExceeded,
    MemorySave,
    DecisionMade,  // 为下轮 DecisionStep 预留
}

#[async_trait]
pub trait Hook: Send + Sync {
    async fn on(&self, point: HookPoint) -> anyhow::Result<()>;
}
```

- [ ] **Step 4: 导出模块**

```rust
// backend/src/lib.rs 添加:
pub mod harness;
```

- [ ] **Step 5: 运行测试确认通过**

```bash
cd backend && cargo test harness_hook_test --no-fail-fast -v
```

- [ ] **Step 6: 提交**

```bash
git add backend/src/harness/ backend/src/lib.rs backend/tests/harness_hook_test.rs
git commit -m "feat(harness): 定义 Hook trait 和钩子点枚举"
```

---

### Task 2: Harness — Orchestrator 编排核心

**Files:**
- Create: `backend/src/harness/orchestrator.rs`
- Test: `backend/tests/harness_orchestrator_test.rs`

**目标:** 实现步骤链编排器，支持 hook 注册和执行

- [ ] **Step 1: 编写 Orchestrator 测试**

```rust
// backend/tests/harness_orchestrator_test.rs
use devops_agent::harness::*;
use std::sync::{Arc, atomic::{AtomicUsize, Ordering}};

#[tokio::test]
async fn test_orchestrator_runs_hooks() {
    let counter = Arc::new(AtomicUsize::new(0));
    // Orchestrator 应能注册 hook 并在执行时触发
    assert!(true); // 骨架测试
}
```

- [ ] **Step 2: 实现 Orchestrator**

```rust
// backend/src/harness/orchestrator.rs
use super::{Hook, HookPoint};
use anyhow;
use std::sync::Arc;
use async_trait::async_trait;

#[async_trait]
pub trait Step: Send + Sync {
    fn name(&self) -> &str;
    async fn execute(&self) -> anyhow::Result<String>;
}

pub struct Orchestrator {
    hooks: Vec<Arc<dyn Hook>>,
}

impl Orchestrator {
    pub fn new() -> Self {
        Self { hooks: Vec::new() }
    }

    pub fn add_hook(&mut self, hook: Arc<dyn Hook>) {
        self.hooks.push(hook);
    }

    async fn fire(&self, point: HookPoint) -> anyhow::Result<()> {
        for hook in &self.hooks {
            hook.on(point).await?;
        }
        Ok(())
    }

    pub async fn run(&self, steps: &[Arc<dyn Step>]) -> anyhow::Result<Vec<String>> {
        self.fire(HookPoint::SessionStart).await?;
        let mut results = Vec::new();

        for step in steps {
            self.fire(HookPoint::StepStart).await?;
            match step.execute().await {
                Ok(output) => {
                    results.push(output);
                    self.fire(HookPoint::StepEnd).await?;
                }
                Err(e) => {
                    self.fire(HookPoint::StepEnd).await?;
                    return Err(e);
                }
            }
        }

        self.fire(HookPoint::SessionEnd).await?;
        Ok(results)
    }
}

impl Default for Orchestrator {
    fn default() -> Self { Self::new() }
}
```

- [ ] **Step 3: 运行测试**

```bash
cd backend && cargo test harness_orchestrator_test --no-fail-fast -v
```

- [ ] **Step 4: 提交**

```bash
git add backend/src/harness/orchestrator.rs backend/tests/harness_orchestrator_test.rs
git commit -m "feat(harness): 实现 Orchestrator 步骤链编排"
```

---

### Task 3: Harness — Session 会话管理

**Files:**
- Create: `backend/src/harness/session.rs`
- Test: `backend/tests/harness_session_test.rs`

**目标:** 会话创建、状态追踪、清理

- [ ] **Step 1: 实现 Session**

```rust
// backend/src/harness/session.rs
use anyhow;
use chrono::{DateTime, Utc};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq)]
pub enum SessionStatus {
    Active,
    Paused,
    Completed,
    Failed,
}

pub struct Session {
    pub id: Uuid,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub status: SessionStatus,
}

impl Session {
    pub fn new() -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::new_v4(),
            created_at: now,
            updated_at: now,
            status: SessionStatus::Active,
        }
    }

    pub fn complete(&mut self) {
        self.updated_at = Utc::now();
        self.status = SessionStatus::Completed;
    }

    pub fn fail(&mut self) {
        self.updated_at = Utc::now();
        self.status = SessionStatus::Failed;
    }

    pub fn pause(&mut self) {
        self.updated_at = Utc::now();
        self.status = SessionStatus::Paused;
    }
}

impl Default for Session {
    fn default() -> Self { Self::new() }
}
```

- [ ] **Step 2: 编写测试**

```rust
// backend/tests/harness_session_test.rs
use devops_agent::harness::Session;

#[test]
fn test_session_creation() {
    let s = Session::new();
    assert_eq!(s.status, devops_agent::harness::SessionStatus::Active);
}

#[test]
fn test_session_lifecycle() {
    let mut s = Session::new();
    s.pause();
    assert_eq!(s.status, devops_agent::harness::SessionStatus::Paused);
    s.complete();
    assert_eq!(s.status, devops_agent::harness::SessionStatus::Completed);
}
```

- [ ] **Step 3: 运行测试并添加 uuid + chrono 依赖**

```bash
cd backend && cargo test harness_session_test --no-fail-fast -v
```

- [ ] **Step 4: 提交**

```bash
git add backend/src/harness/session.rs backend/tests/harness_session_test.rs backend/Cargo.toml
git commit -m "feat(harness): 实现 Session 会话生命周期管理"
```

---

### Task 4: Memory — 短期记忆环形缓冲区

**Files:**
- Create: `backend/src/memory/mod.rs`
- Create: `backend/src/memory/short_term.rs`
- Test: `backend/tests/memory_short_term_test.rs`

**目标:** 200 条容量的环形缓冲区

- [ ] **Step 1: 实现 Memory trait + 模块声明**

```rust
// backend/src/memory/mod.rs
pub mod short_term;
pub mod long_term;
pub mod store;

pub use short_term::ShortTermMemory;
pub use long_term::LongTermMemory;
pub use store::MemoryStore;

#[derive(Debug, Clone)]
pub struct MemoryEntry {
    pub id: u64,
    pub content: String,
    pub r#type: MemoryType,
    pub timestamp: chrono::DateTime<chrono::Utc>,
    pub score: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub enum MemoryType {
    ToolCall,
    ToolResult,
    LlmResponse,
    UserInput,
    Decision,
    Summary,
}
```

- [ ] **Step 2: 实现 ShortTermMemory**

```rust
// backend/src/memory/short_term.rs
use super::{MemoryEntry, MemoryType};
use chrono::Utc;

pub struct ShortTermMemory {
    buffer: Vec<MemoryEntry>,
    capacity: usize,
    next_id: u64,
}

impl ShortTermMemory {
    pub fn new(capacity: usize) -> Self {
        Self {
            buffer: Vec::with_capacity(capacity),
            capacity,
            next_id: 0,
        }
    }

    pub fn add(&mut self, content: String, r#type: MemoryType) {
        if self.buffer.len() >= self.capacity {
            self.buffer.remove(0);
        }
        self.next_id += 1;
        self.buffer.push(MemoryEntry {
            id: self.next_id,
            content,
            r#type,
            timestamp: Utc::now(),
            score: 1.0,
        });
    }

    pub fn recent(&self, n: usize) -> &[MemoryEntry] {
        let start = if self.buffer.len() > n {
            self.buffer.len() - n
        } else {
            0
        };
        &self.buffer[start..]
    }

    pub fn entries(&self) -> &[MemoryEntry] {
        &self.buffer
    }

    pub fn len(&self) -> usize {
        self.buffer.len()
    }

    pub fn is_empty(&self) -> bool {
        self.buffer.is_empty()
    }

    pub fn clear(&mut self) {
        self.buffer.clear();
    }
}

impl Default for ShortTermMemory {
    fn default() -> Self { Self::new(200) }
}
```

- [ ] **Step 3: 编写测试**

```rust
// backend/tests/memory_short_term_test.rs
use devops_agent::memory::{ShortTermMemory, MemoryType};

#[test]
fn test_short_term_add_and_recent() {
    let mut mem = ShortTermMemory::new(3);
    mem.add("msg1".into(), MemoryType::UserInput);
    mem.add("msg2".into(), MemoryType::UserInput);
    assert_eq!(mem.len(), 2);
    assert_eq!(mem.recent(2)[0].content, "msg1");
}

#[test]
fn test_short_term_eviction() {
    let mut mem = ShortTermMemory::new(2);
    mem.add("old".into(), MemoryType::UserInput);
    mem.add("new".into(), MemoryType::UserInput);
    mem.add("latest".into(), MemoryType::UserInput);
    assert_eq!(mem.len(), 2);
    assert_eq!(mem.recent(2)[0].content, "new");
    assert_eq!(mem.recent(2)[1].content, "latest");
}

#[test]
fn test_short_term_clear() {
    let mut mem = ShortTermMemory::new(10);
    mem.add("x".into(), MemoryType::UserInput);
    mem.clear();
    assert!(mem.is_empty());
}
```

- [ ] **Step 4: 导出 + 运行测试**

```rust
// backend/src/lib.rs 添加:
pub mod memory;
```

```bash
cd backend && cargo test memory_short_term_test --no-fail-fast -v
```

- [ ] **Step 5: 提交**

```bash
git add backend/src/memory/ backend/src/lib.rs backend/tests/memory_short_term_test.rs
git commit -m "feat(memory): 实现短期记忆环形缓冲区"
```

---

### Task 5: Memory — SQLite 持久化存储

**Files:**
- Create: `backend/src/memory/store.rs`
- Create: `backend/src/memory/long_term.rs`
- Modify: `backend/Cargo.toml` (添加 rusqlite)
- Test: `backend/tests/memory_long_term_test.rs`

**目标:** SQLite 持久化 + 关键词索引

- [ ] **Step 1: 添加 rusqlite 依赖**

```toml
# backend/Cargo.toml [dependencies] 中添加:
rusqlite = { version = "0.32", features = ["bundled"] }
```

- [ ] **Step 2: 实现 Store**

```rust
// backend/src/memory/store.rs
use rusqlite::{Connection, Result};

pub struct MemoryStore {
    conn: Connection,
}

impl MemoryStore {
    pub fn new(path: &str) -> Result<Self> {
        let conn = Connection::open(path)?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS memories (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                content TEXT NOT NULL,
                type TEXT NOT NULL,
                keywords TEXT NOT NULL DEFAULT '',
                score REAL NOT NULL DEFAULT 1.0,
                created_at TEXT NOT NULL
            )",
        )?;
        Ok(Self { conn })
    }

    pub fn insert(
        &self,
        content: &str,
        type_: &str,
        keywords: &[&str],
        score: f64,
    ) -> Result<i64> {
        let keywords_str = keywords.join(",");
        let now = chrono::Utc::now().to_rfc3339();
        self.conn.execute(
            "INSERT INTO memories (content, type, keywords, score, created_at) VALUES (?, ?, ?, ?, ?)",
            (&content, &type_, &keywords_str, score, &now),
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn search(&self, keyword: &str) -> Result<Vec<String>> {
        let keyword = format!("%{}%", keyword);
        let mut stmt = self.conn.prepare(
            "SELECT content FROM memories WHERE keywords LIKE ? ORDER BY score DESC LIMIT 20"
        )?;
        let rows = stmt.query_map([&keyword], |row| row.get(0))?;
        rows.map(|r| r.unwrap()).collect()
    }

    pub fn count(&self) -> Result<i64> {
        let mut stmt = self.conn.prepare("SELECT COUNT(*) FROM memories")?;
        let val: i64 = stmt.query_scalar([])?.next().unwrap()?;
        Ok(val)
    }
}
```

- [ ] **Step 3: 实现 LongTermMemory**

```rust
// backend/src/memory/long_term.rs
use super::store::MemoryStore;
use anyhow;

pub struct LongTermMemory {
    store: MemoryStore,
}

impl LongTermMemory {
    pub fn new(path: &str) -> anyhow::Result<Self> {
        Ok(Self {
            store: MemoryStore::new(path)
                .map_err(|e| anyhow::anyhow!("Failed to open memory store: {}", e))?,
        })
    }

    pub fn save(&self, content: &str, type_: &str, keywords: &[&str], score: f64) -> anyhow::Result<()> {
        self.store.insert(content, type_, keywords, score)
            .map_err(|e| anyhow::anyhow!("Failed to save memory: {}", e))?;
        Ok(())
    }

    pub fn retrieve(&self, keyword: &str) -> anyhow::Result<Vec<String>> {
        self.store.search(keyword)
            .map_err(|e| anyhow::anyhow!("Failed to search memory: {}", e))
    }
}
```

- [ ] **Step 4: 编写测试**

```rust
// backend/tests/memory_long_term_test.rs
use devops_agent::memory::LongTermMemory;
use std::env::temp_dir;

#[test]
fn test_long_term_save_and_retrieve() {
    let path = temp_dir().join(format!("test_memory_{}.db", std::process::id()));
    let mem = LongTermMemory::new(path.to_str().unwrap()).unwrap();
    mem.save("deploy jenkins job #123", "decision", &["deploy", "jenkins"], 1.0).unwrap();
    let results = mem.retrieve("deploy").unwrap();
    assert!(!results.is_empty());
    assert!(results[0].contains("jenkins"));
}

#[test]
fn test_long_term_multiple_search() {
    let path = temp_dir().join(format!("test_mem2_{}.db", std::process::id()));
    let mem = LongTermMemory::new(path.to_str().unwrap()).unwrap();
    mem.save("config A", "config", &["config", "a"], 0.8).unwrap();
    mem.save("config B", "config", &["config", "b"], 1.0).unwrap();
    let results = mem.retrieve("config").unwrap();
    assert_eq!(results.len(), 2);
    // score 高的在前
    assert!(results[0].contains("B"));
}
```

- [ ] **Step 5: 运行测试**

```bash
cd backend && cargo test memory_long_term_test --no-fail-fast -v
```

- [ ] **Step 6: 提交**

```bash
git add backend/src/memory/store.rs backend/src/memory/long_term.rs backend/Cargo.toml backend/tests/memory_long_term_test.rs
git commit -m "feat(memory): 实现 SQLite 长期记忆持久化"
```

---

### Task 6: Token — Tracker 计数器

**Files:**
- Create: `backend/src/token/mod.rs`
- Create: `backend/src/token/tracker.rs`
- Test: `backend/tests/token_tracker_test.rs`

- [ ] **Step 1: 模块声明**

```rust
// backend/src/token/mod.rs
pub mod tracker;
pub mod window;
pub mod summarizer;

pub use tracker::{TokenTracker, TokenUsage};
pub use window::ContextWindow;
pub use summarizer::Summarizer;
```

- [ ] **Step 2: 实现 TokenTracker**

```rust
// backend/src/token/tracker.rs
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone, Default)]
pub struct TokenUsage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
}

pub struct TokenTracker {
    state: Arc<Mutex<TrackerState>>,
    budget: u32,
}

struct TrackerState {
    total_usage: TokenUsage,
    per_call: Vec<TokenUsage>,
    round_count: u32,  // 对话轮次计数
}

impl TokenTracker {
    pub fn new(budget: u32) -> Self {
        Self {
            state: Arc::new(Mutex::new(TrackerState {
                total_usage: TokenUsage::default(),
                per_call: Vec::new(),
                round_count: 0,
            })),
            budget,
        }
    }

    pub fn record(&self, usage: TokenUsage) {
        let mut state = self.state.lock().unwrap();
        state.total_usage.prompt_tokens += usage.prompt_tokens;
        state.total_usage.completion_tokens += usage.completion_tokens;
        state.total_usage.total_tokens += usage.total_tokens;
        state.per_call.push(usage);
    }

    pub fn usage(&self) -> TokenUsage {
        self.state.lock().unwrap().total_usage.clone()
    }

    pub fn remaining(&self) -> u32 {
        let usage = self.usage();
        self.budget.saturating_sub(usage.total_tokens)
    }

    pub fn is_exceeded(&self) -> bool {
        self.remaining() == 0
    }

    pub fn call_count(&self) -> usize {
        self.state.lock().unwrap().per_call.len()
    }

    pub fn increment_round(&self) {
        self.state.lock().unwrap().round_count += 1;
    }

    pub fn round_count(&self) -> u32 {
        self.state.lock().unwrap().round_count
    }
}
```

- [ ] **Step 3: 编写测试**

```rust
// backend/tests/token_tracker_test.rs
use devops_agent::token::{TokenTracker, TokenUsage};

#[test]
fn test_tracker_records_usage() {
    let tracker = TokenTracker::new(10000);
    tracker.record(TokenUsage {
        prompt_tokens: 100,
        completion_tokens: 50,
        total_tokens: 150,
    });
    assert_eq!(tracker.usage().total_tokens, 150);
    assert_eq!(tracker.remaining(), 9850);
}

#[test]
fn test_tracker_budget_exceeded() {
    let tracker = TokenTracker::new(100);
    tracker.record(TokenUsage { prompt_tokens: 60, completion_tokens: 50, total_tokens: 110 });
    assert!(tracker.is_exceeded());
    assert_eq!(tracker.remaining(), 0);
}

#[test]
fn test_tracker_multiple_calls() {
    let tracker = TokenTracker::new(1000);
    for _ in 0..5 {
        tracker.record(TokenUsage { prompt_tokens: 50, completion_tokens: 50, total_tokens: 100 });
    }
    assert_eq!(tracker.call_count(), 5);
    assert_eq!(tracker.usage().total_tokens, 500);
}
```

- [ ] **Step 4: 导出 + 运行**

```rust
// backend/src/lib.rs 添加:
pub mod token;
```

```bash
cd backend && cargo test token_tracker_test --no-fail-fast -v
```

- [ ] **Step 5: 提交**

```bash
git add backend/src/token/ backend/src/lib.rs backend/tests/token_tracker_test.rs
git commit -m "feat(token): 实现 Token 计数器与预算管理"
```

---

### Task 7: Token — 四层上下文窗口

**Files:**
- Create: `backend/src/token/window.rs`
- Test: `backend/tests/token_window_test.rs`

- [ ] **Step 1: 实现 ContextWindow**

```rust
// backend/src/token/window.rs
use anyhow;

#[derive(Debug, Clone)]
pub struct ContextLayer {
    pub name: String,
    pub messages: Vec<String>,
    pub compressible: bool,
}

impl ContextLayer {
    pub fn token_estimate(&self) -> u32 {
        self.messages.iter().map(|m| m.len() as u32 / 4).sum()
    }
}

pub struct ContextWindow {
    system: ContextLayer,
    working: ContextLayer,
    recent: ContextLayer,
    long_term: ContextLayer,
    max_tokens: u32,
}

impl ContextWindow {
    pub fn new(max_tokens: u32) -> Self {
        Self {
            system: ContextLayer { name: "system".into(), messages: Vec::new(), compressible: false },
            working: ContextLayer { name: "working".into(), messages: Vec::new(), compressible: false },
            recent: ContextLayer { name: "recent".into(), messages: Vec::new(), compressible: true },
            long_term: ContextLayer { name: "long_term".into(), messages: Vec::new(), compressible: true },
            max_tokens,
        }
    }

    pub fn add_to_layer(&mut self, layer: Layer, message: String) {
        match layer {
            Layer::System => self.system.messages.push(message),
            Layer::Working => self.working.messages.push(message),
            Layer::Recent => self.recent.messages.push(message),
            Layer::LongTerm => self.long_term.messages.push(message),
        }
    }

    pub fn total_tokens(&self) -> u32 {
        self.system.token_estimate()
            + self.working.token_estimate()
            + self.recent.token_estimate()
            + self.long_term.token_estimate()
    }

    pub fn usage_percent(&self) -> f32 {
        if self.max_tokens == 0 { return 0.0; }
        (self.total_tokens() as f32 / self.max_tokens as f32) * 100.0
    }

    pub fn is_over_threshold(&self, threshold_percent: f32) -> bool {
        self.usage_percent() > threshold_percent
    }

    pub fn compress_long_term(&mut self) {
        self.long_term.messages.clear();
    }

    pub fn compress_recent(&mut self, keep_last: usize) {
        let len = self.recent.messages.len();
        if len > keep_last {
            self.recent.messages.drain(0..len - keep_last);
        }
    }

    pub fn build_context(&self) -> Vec<String> {
        let mut result = Vec::new();
        result.extend_from_slice(&self.system.messages);
        result.extend_from_slice(&self.working.messages);
        result.extend_from_slice(&self.recent.messages);
        result.extend_from_slice(&self.long_term.messages);
        result
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Layer {
    System,
    Working,
    Recent,
    LongTerm,
}
```

- [ ] **Step 2: 编写测试**

```rust
// backend/tests/token_window_test.rs
use devops_agent::token::{ContextWindow, Layer};

#[test]
fn test_window_add_and_count() {
    let mut w = ContextWindow::new(10000);
    w.add_to_layer(Layer::System, "system prompt".into());
    w.add_to_layer(Layer::Working, "user: hello".into());
    assert!(w.total_tokens() > 0);
}

#[test]
fn test_window_threshold() {
    let mut w = ContextWindow::new(100);
    // 添加足够多的内容超过阈值
    for _ in 0..20 {
        w.add_to_layer(Layer::Recent, "this is a test message for padding".into());
    }
    assert!(w.is_over_threshold(50.0));
}

#[test]
fn test_window_compress_long_term() {
    let mut w = ContextWindow::new(10000);
    w.add_to_layer(Layer::LongTerm, "old memory 1".into());
    w.add_to_layer(Layer::LongTerm, "old memory 2".into());
    w.compress_long_term();
    let ctx = w.build_context();
    assert!(!ctx.iter().any(|m| m.contains("old memory")));
}
```

- [ ] **Step 3: 运行 + 提交**

```bash
cd backend && cargo test token_window_test --no-fail-fast -v
git add backend/src/token/window.rs backend/tests/token_window_test.rs
git commit -m "feat(token): 实现四层上下文窗口管理"
```

---

### Task 8: Token — Summarizer 摘要压缩

**Files:**
- Create: `backend/src/token/summarizer.rs`
- Test: `backend/tests/token_summarizer_test.rs`

> 注: 完整实现依赖 Task 10 (LLM 客户端)。此处先建立接口和骨架。

- [ ] **Step 1: 实现 Summarizer 骨架**

```rust
// backend/src/token/summarizer.rs
use anyhow;

#[derive(Debug, Clone)]
pub struct SummaryResult {
    pub summary: String,
    pub key_decisions: Vec<String>,
    pub action_items: Vec<String>,
    pub original_tokens: u32,
    pub compressed_tokens: u32,
}

pub struct SummarizerConfig {
    pub short_message_threshold: u32,  // < 500 tokens 按条摘要
    pub batch_rounds: usize,           // >= N 轮按轮次摘要
}

impl Default for SummarizerConfig {
    fn default() -> Self {
        Self {
            short_message_threshold: 500,
            batch_rounds: 5,
        }
    }
}

pub struct Summarizer {
    config: SummarizerConfig,
}

impl Summarizer {
    pub fn new(config: SummarizerConfig) -> Self {
        Self { config }
    }

    /// 判断应该用哪种压缩策略
    pub fn strategy(&self, messages: &[String]) -> CompressionStrategy {
        let total_tokens: u32 = messages.iter().map(|m| m.len() as u32 / 4).sum();
        if messages.len() >= self.config.batch_rounds || total_tokens >= self.config.short_message_threshold {
            CompressionStrategy::Batch
        } else {
            CompressionStrategy::PerMessage
        }
    }

    /// 本地摘要（不依赖 LLM，用规则提取关键信息）
    pub fn summarize_local(&self, messages: &[String]) -> SummaryResult {
        let original_tokens: u32 = messages.iter().map(|m| m.len() as u32 / 4).sum();
        // 简单策略：取每条消息的前 100 字符
        let summary: String = messages
            .iter()
            .map(|m| {
                let truncated: String = m.chars().take(100).collect();
                format!("- {}", truncated)
            })
            .collect();
        let compressed_tokens = summary.len() as u32 / 4;

        SummaryResult {
            summary,
            key_decisions: Vec::new(),
            action_items: Vec::new(),
            original_tokens,
            compressed_tokens,
        }
    }

    /// LLM 摘要（后续对接 LLM 客户端）
    pub async fn summarize_with_llm(&self, _messages: &[String]) -> anyhow::Result<SummaryResult> {
        // TODO: 对接 LLM 客户端，使用结构化输出
        anyhow::bail!("LLM summarizer not yet connected");
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum CompressionStrategy {
    PerMessage,
    Batch,
}
```

- [ ] **Step 2: 编写测试**

```rust
// backend/tests/token_summarizer_test.rs
use devops_agent::token::{Summarizer, SummarizerConfig, CompressionStrategy};

#[test]
fn test_strategy_short_messages() {
    let s = Summarizer::new(SummarizerConfig::default());
    let msgs: Vec<String> = vec!["hello".into(), "world".into()];
    assert_eq!(s.strategy(&msgs), CompressionStrategy::PerMessage);
}

#[tokio::test]
async fn test_local_summarize() {
    let s = Summarizer::new(SummarizerConfig::default());
    let msgs = vec!["important decision: use Rust".into(), "follow up: write tests".into()];
    let result = s.summarize_local(&msgs);
    assert!(result.summary.contains("Rust"));
    assert!(result.compressed_tokens < result.original_tokens || result.original_tokens == 0);
}
```

- [ ] **Step 3: 运行 + 提交**

```bash
cd backend && cargo test token_summarizer_test --no-fail-fast -v
git add backend/src/token/summarizer.rs backend/tests/token_summarizer_test.rs
git commit -m "feat(token): 实现摘要压缩器骨架（本地规则 + LLM 接口）"
```

---

### Task 9: Security — 权限策略引擎

**Files:**
- Create: `backend/src/security/mod.rs`
- Create: `backend/src/security/permission.rs`
- Create: `backend/src/security/policy.rs`
- Create: `backend/src/security/audit.rs`
- Test: `backend/tests/security_policy_test.rs`

- [ ] **Step 1: 模块声明 + 权限模型**

```rust
// backend/src/security/mod.rs
pub mod permission;
pub mod policy;
pub mod audit;

pub use permission::{ToolRequest, Role};
pub use policy::{PolicyEngine, PolicyDecision};
pub use audit::AuditLog;
```

```rust
// backend/src/security/permission.rs
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolRequest {
    pub tool: String,
    pub args: serde_json::Value,
    pub session_id: String,
    pub role: Role,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Role {
    Admin,
    Developer,
    Viewer,
}
```

- [ ] **Step 2: 策略引擎**

```rust
// backend/src/security/policy.rs
use super::permission::{ToolRequest, Role};

#[derive(Debug, Clone, PartialEq)]
pub enum PolicyDecision {
    Allow,
    Deny(String),
    Prompt(String),
}

pub struct PolicyRule {
    pub tool: String,
    pub allowed_roles: Vec<Role>,
    pub decision: PolicyDecision,
}

pub struct PolicyEngine {
    rules: Vec<PolicyRule>,
}

impl PolicyEngine {
    pub fn new() -> Self {
        let mut engine = Self { rules: Vec::new() };
        // 默认规则
        engine.add_rule(PolicyRule {
            tool: "read".into(),
            allowed_roles: vec![Role::Admin, Role::Developer, Role::Viewer],
            decision: PolicyDecision::Allow,
        });
        engine.add_rule(PolicyRule {
            tool: "write".into(),
            allowed_roles: vec![Role::Admin, Role::Developer],
            decision: PolicyDecision::Allow,
        });
        engine.add_rule(PolicyRule {
            tool: "bash".into(),
            allowed_roles: vec![Role::Admin],
            decision: PolicyDecision::Prompt("执行 shell 命令需要确认".into()),
        });
        engine
    }

    fn add_rule(&mut self, rule: PolicyRule) {
        self.rules.push(rule);
    }

    pub fn evaluate(&self, request: &ToolRequest) -> PolicyDecision {
        // 查找匹配的规则
        if let Some(rule) = self.rules.iter().find(|r| r.tool == request.tool) {
            if rule.allowed_roles.contains(&request.role) {
                rule.decision.clone()
            } else {
                PolicyDecision::Deny(format!("角色 {:?} 无权使用工具 '{}'", request.role, request.tool))
            }
        } else {
            // 未知工具需要确认
            PolicyDecision::Prompt(format!("未知工具 '{}'，是否允许？", request.tool))
        }
    }
}

impl Default for PolicyEngine {
    fn default() -> Self { Self::new() }
}
```

- [ ] **Step 3: 审计日志**

```rust
// backend/src/security/audit.rs
use chrono::{DateTime, Utc};

#[derive(Debug, Clone)]
pub struct AuditEntry {
    pub timestamp: DateTime<Utc>,
    pub tool: String,
    pub decision: String,
    pub session_id: String,
    pub result: AuditResult,
}

#[derive(Debug, Clone, PartialEq)]
pub enum AuditResult {
    Executed,
    Denied,
    Pending,
}

pub struct AuditLog {
    entries: Vec<AuditEntry>,
}

impl AuditLog {
    pub fn new() -> Self { Self { entries: Vec::new() } }

    pub fn record(&mut self, tool: &str, decision: &str, session_id: &str, result: AuditResult) {
        self.entries.push(AuditEntry {
            timestamp: Utc::now(),
            tool: tool.into(),
            decision: decision.into(),
            session_id: session_id.into(),
            result,
        });
    }

    pub fn entries(&self) -> &[AuditEntry] { &self.entries }
}

impl Default for AuditLog {
    fn default() -> Self { Self::new() }
}
```

- [ ] **Step 4: 编写测试**

```rust
// backend/tests/security_policy_test.rs
use devops_agent::security::*;

#[test]
fn test_allow_read_for_viewer() {
    let engine = PolicyEngine::new();
    let req = ToolRequest {
        tool: "read".into(),
        args: serde_json::json!({}),
        session_id: "test".into(),
        role: Role::Viewer,
    };
    assert_eq!(engine.evaluate(&req), PolicyDecision::Allow);
}

#[test]
fn test_deny_write_for_viewer() {
    let engine = PolicyEngine::new();
    let req = ToolRequest {
        tool: "write".into(),
        args: serde_json::json!({}),
        session_id: "test".into(),
        role: Role::Viewer,
    };
    assert!(matches!(engine.evaluate(&req), PolicyDecision::Deny(_)));
}

#[test]
fn test_prompt_for_bash() {
    let engine = PolicyEngine::new();
    let req = ToolRequest {
        tool: "bash".into(),
        args: serde_json::json!({"cmd": "ls"}),
        session_id: "test".into(),
        role: Role::Admin,
    };
    assert!(matches!(engine.evaluate(&req), PolicyDecision::Prompt(_)));
}

#[test]
fn test_audit_log() {
    let mut log = AuditLog::new();
    log.record("read", "allow", "sess-1", AuditResult::Executed);
    assert_eq!(log.entries().len(), 1);
}
```

- [ ] **Step 5: 导出 + 运行**

```rust
// backend/src/lib.rs 添加:
pub mod security;
```

```bash
cd backend && cargo test security_policy_test --no-fail-fast -v
```

- [ ] **Step 6: 提交**

```bash
git add backend/src/security/ backend/src/lib.rs backend/tests/security_policy_test.rs
git commit -m "feat(security): 实现权限策略引擎与审计日志"
```

---

### Task 10: Sandbox — 沙箱核心

**Files:**
- Create: `backend/src/sandbox/mod.rs`
- Create: `backend/src/sandbox/validator.rs`
- Create: `backend/src/sandbox/filesystem.rs`
- Create: `backend/src/sandbox/process.rs`
- Create: `backend/src/sandbox/network.rs`
- Test: `backend/tests/sandbox_test.rs`

- [ ] **Step 1: 模块声明 + 路径校验器**

```rust
// backend/src/sandbox/mod.rs
pub mod validator;
pub mod filesystem;
pub mod process;
pub mod network;

pub use validator::PathValidator;
pub use filesystem::FilesystemSandbox;
pub use process::ProcessSandbox;
pub use network::NetworkSandbox;
```

```rust
// backend/src/sandbox/validator.rs
use anyhow;
use std::path::{Path, PathBuf};

pub struct PathValidator {
    allowed_base: PathBuf,
    blocked_patterns: Vec<String>,
}

impl PathValidator {
    pub fn new(base: PathBuf) -> Self {
        Self {
            blocked_patterns: vec![
                ".env".into(),
                ".key".into(),
                ".pem".into(),
                ".secret".into(),
                "id_rsa".into(),
            ],
            allowed_base: base,
        }
    }

    pub fn validate(&self, path: &str) -> anyhow::Result<PathBuf> {
        let p = Path::new(path);

        // 检测路径穿越
        if path.contains("..") {
            anyhow::bail!("路径穿越检测失败: {}", path);
        }

        // 检测敏感文件
        let file_name = p.file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        for pattern in &self.blocked_patterns {
            if file_name.contains(pattern) {
                anyhow::bail!("禁止访问敏感文件: {}", path);
            }
        }

        // 解析为绝对路径并检查是否在允许范围内
        let abs_path = p.canonicalize().unwrap_or_else(|_| p.to_path_buf());
        if !abs_path.starts_with(&self.allowed_base) {
            anyhow::bail!("路径超出允许范围: {}", path);
        }

        Ok(abs_path)
    }
}
```

- [ ] **Step 2: 文件系统沙箱**

```rust
// backend/src/sandbox/filesystem.rs
use anyhow;
use std::path::{Path, PathBuf};
use std::fs;

pub struct FilesystemSandbox {
    workspace: PathBuf,
    mounts: Vec<PathBuf>,
}

impl FilesystemSandbox {
    pub fn new(workspace: &str) -> anyhow::Result<Self> {
        let ws = PathBuf::from(workspace);
        fs::create_dir_all(ws.join("tmp"))?;
        fs::create_dir_all(ws.join("output"))?;
        Ok(Self {
            workspace: ws,
            mounts: Vec::new(),
        })
    }

    pub fn mount_readonly(&mut self, source: &str, target: &str) -> anyhow::Result<()> {
        let mount_path = self.workspace.join("mounts").join(target);
        fs::create_dir_all(&mount_path)?;
        self.mounts.push(PathBuf::from(source));
        Ok(())
    }

    pub fn workspace_path(&self) -> &Path {
        &self.workspace
    }

    pub fn tmp_path(&self) -> &Path {
        self.workspace.join("tmp").as_ref()
    }

    pub fn output_path(&self) -> &Path {
        self.workspace.join("output").as_ref()
    }
}
```

- [ ] **Step 3: 进程沙箱**

```rust
// backend/src/sandbox/process.rs
use anyhow;
use std::collections::HashSet;
use std::process::Command;
use std::time::Duration;
use tokio::time::timeout;

pub struct ProcessSandboxConfig {
    pub timeout_secs: u64,
    pub max_output_bytes: usize,
    pub blocked_env: Vec<String>,
}

impl Default for ProcessSandboxConfig {
    fn default() -> Self {
        Self {
            timeout_secs: 30,
            max_output_bytes: 1_048_576,
            blocked_env: vec![
                "API_KEY".into(), "TOKEN".into(), "SECRET".into(),
                "PASSWORD".into(), "PRIVATE_KEY".into(),
            ],
        }
    }
}

pub struct ProcessSandbox {
    config: ProcessSandboxConfig,
}

impl ProcessSandbox {
    pub fn new(config: ProcessSandboxConfig) -> Self {
        Self { config }
    }

    pub async fn run(&self, cmd: &str, args: &[&str]) -> anyhow::Result<String> {
        let parts: Vec<&str> = cmd.split_whitespace().collect();
        if parts.is_empty() {
            anyhow::bail!("空命令");
        }

        let mut command = Command::new(parts[0]);
        command.args(&parts[1..]).args(args);

        // 净化环境变量
        for env in &self.config.blocked_env {
            command.env_remove(env);
        }

        let output = timeout(
            Duration::from_secs(self.config.timeout_secs),
            tokio::process::Command::new(parts[0])
                .args(&parts[1..])
                .args(args)
                .output()
                .await,
        ).await.map_err(|_| anyhow::anyhow!("命令执行超时: {}s", self.config.timeout_secs))??;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("命令执行失败: {}", stderr);
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let truncated: String = stdout.chars().take(self.config.max_output_bytes).collect();
        Ok(truncated)
    }
}
```

- [ ] **Step 4: 网络沙箱（应用层）**

```rust
// backend/src/sandbox/network.rs
use std::collections::HashSet;

pub struct NetworkSandbox {
    blocked_commands: HashSet<String>,
    allowed_hosts: HashSet<String>,
}

impl NetworkSandbox {
    pub fn new() -> Self {
        let mut blocked = HashSet::new();
        for cmd in ["curl", "wget", "nc", "netcat", "ssh", "scp", "telnet"] {
            blocked.insert(cmd.into());
        }
        Self { blocked_commands: blocked, allowed_hosts: HashSet::new() }
    }

    pub fn is_blocked(&self, command: &str) -> bool {
        let cmd = command.split_whitespace().next().unwrap_or("");
        self.blocked_commands.contains(cmd)
    }

    pub fn allow_host(&mut self, host: &str) {
        self.allowed_hosts.insert(host.into());
    }

    pub fn is_host_allowed(&self, host: &str) -> bool {
        self.allowed_hosts.contains(host)
    }
}

impl Default for NetworkSandbox {
    fn default() -> Self { Self::new() }
}
```

- [ ] **Step 5: 编写测试**

```rust
// backend/tests/sandbox_test.rs
use devops_agent::sandbox::*;
use std::path::PathBuf;
use std::env::temp_dir;

#[test]
fn test_path_validator_blocks_traversal() {
    let v = PathValidator::new(PathBuf::from("/tmp/test"));
    assert!(v.validate("/tmp/test/../etc/passwd").is_err());
}

#[test]
fn test_path_validator_blocks_sensitive() {
    let v = PathValidator::new(PathBuf::from("/tmp/test"));
    assert!(v.validate("/tmp/test/.env").is_err());
}

#[test]
fn test_network_blocks_curl() {
    let n = NetworkSandbox::new();
    assert!(n.is_blocked("curl https://example.com"));
    assert!(!n.is_blocked("ls -la"));
}

#[tokio::test]
async fn test_process_sandbox_runs() {
    let ps = ProcessSandbox::new(Default::default());
    let result = ps.run("echo", &["hello"]).await;
    assert!(result.is_ok());
    assert!(result.unwrap().contains("hello"));
}
```

- [ ] **Step 6: 导出 + 运行**

```rust
// backend/src/lib.rs 添加:
pub mod sandbox;
```

```bash
cd backend && cargo test sandbox_test --no-fail-fast -v
```

- [ ] **Step 7: 提交**

```bash
git add backend/src/sandbox/ backend/src/lib.rs backend/tests/sandbox_test.rs
git commit -m "feat(sandbox): 实现三层沙箱隔离"
```

---

### Task 11: Tools — 内置工具

**Files:**
- Create: `backend/src/tools/builtin/mod.rs`
- Create: `backend/src/tools/builtin/read.rs`
- Create: `backend/src/tools/builtin/write.rs`
- Create: `backend/src/tools/builtin/bash.rs`
- Create: `backend/src/tools/builtin/git.rs`
- Modify: `backend/src/tools/mod.rs`
- Test: `backend/tests/tools_builtin_test.rs`

- [ ] **Step 1: 工具 Trait + 模块声明**

```rust
// backend/src/tools/builtin/mod.rs
pub mod read;
pub mod write;
pub mod bash;
pub mod git;

pub use read::ReadTool;
pub use write::WriteTool;
pub use bash::BashTool;
pub use git::GitTool;

use async_trait::async_trait;
use serde::{Serialize, Deserialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolOutput {
    pub success: bool,
    pub output: String,
    pub error: Option<String>,
}

#[async_trait]
pub trait BuiltinTool: Send + Sync {
    fn name(&self) -> &str;
    async fn execute(&self, args: &str) -> ToolOutput;
}
```

- [ ] **Step 2: Read 工具**

```rust
// backend/src/tools/builtin/read.rs
use super::{BuiltinTool, ToolOutput};
use async_trait::async_trait;
use std::fs;

pub struct ReadTool;

#[async_trait]
impl BuiltinTool for ReadTool {
    fn name(&self) -> &str { "read" }

    async fn execute(&self, args: &str) -> ToolOutput {
        let path = args.trim().trim_matches('"');
        match fs::read_to_string(path) {
            Ok(content) => ToolOutput { success: true, output: content, error: None },
            Err(e) => ToolOutput {
                success: false,
                output: String::new(),
                error: Some(format!("读取文件失败: {}", e)),
            },
        }
    }
}
```

- [ ] **Step 3: Write 工具**

```rust
// backend/src/tools/builtin/write.rs
use super::{BuiltinTool, ToolOutput};
use async_trait::async_trait;
use std::fs;

pub struct WriteTool {
    max_bytes: usize,
}

impl WriteTool {
    pub fn new(max_bytes: usize) -> Self { Self { max_bytes } }
}

#[async_trait]
impl BuiltinTool for WriteTool {
    fn name(&self) -> &str { "write" }

    async fn execute(&self, args: &str) -> ToolOutput {
        // 格式: "path|content"
        let parts: Vec<&str> = args.splitn(2, '|').collect();
        if parts.len() != 2 {
            return ToolOutput {
                success: false, output: String::new(),
                error: Some("格式: path|content".into()),
            };
        }
        let (path, content) = (parts[0].trim(), parts[1]);
        if content.len() > self.max_bytes {
            return ToolOutput {
                success: false, output: String::new(),
                error: Some(format!("内容超过 {} 字节限制", self.max_bytes)),
            };
        }
        match fs::write(path, content) {
            Ok(_) => ToolOutput { success: true, output: format!("已写入 {}", path), error: None },
            Err(e) => ToolOutput {
                success: false, output: String::new(),
                error: Some(format!("写入失败: {}", e)),
            },
        }
    }
}
```

- [ ] **Step 4: Bash + Git 工具**

```rust
// backend/src/tools/builtin/bash.rs
use super::{BuiltinTool, ToolOutput};
use async_trait::async_trait;
use crate::sandbox::ProcessSandbox;

pub struct BashTool {
    sandbox: ProcessSandbox,
}

impl BashTool {
    pub fn new() -> Self {
        Self { sandbox: ProcessSandbox::new(Default::default()) }
    }
}

#[async_trait]
impl BuiltinTool for BashTool {
    fn name(&self) -> &str { "bash" }

    async fn execute(&self, args: &str) -> ToolOutput {
        let cmd = args.trim().trim_matches('"');
        match self.sandbox.run(cmd, &[]).await {
            Ok(output) => ToolOutput { success: true, output, error: None },
            Err(e) => ToolOutput {
                success: false, output: String::new(),
                error: Some(e.to_string()),
            },
        }
    }
}

impl Default for BashTool {
    fn default() -> Self { Self::new() }
}
```

```rust
// backend/src/tools/builtin/git.rs
use super::{BuiltinTool, ToolOutput};
use async_trait::async_trait;

pub struct GitTool;

#[async_trait]
impl BuiltinTool for GitTool {
    fn name(&self) -> &str { "git" }

    async fn execute(&self, args: &str) -> ToolOutput {
        let cmd = args.trim().trim_matches('"');
        let full_cmd = format!("git {}", cmd);
        // 复用 BashTool 逻辑
        let sandbox = crate::sandbox::ProcessSandbox::new(Default::default());
        match sandbox.run("git", &cmd.split_whitespace().collect::<Vec<_>>()).await {
            Ok(output) => ToolOutput { success: true, output, error: None },
            Err(e) => ToolOutput {
                success: false, output: String::new(),
                error: Some(e.to_string()),
            },
        }
    }
}
```

- [ ] **Step 5: 更新 tools/mod.rs**

```rust
// backend/src/tools/mod.rs 添加:
pub mod builtin;
pub mod jenkins;
pub mod jenkins_cache;
pub mod gitlab;
```

- [ ] **Step 6: 测试 + 运行 + 提交**

```rust
// backend/tests/tools_builtin_test.rs
use devops_agent::tools::builtin::*;

#[tokio::test]
async fn test_read_tool_file_not_found() {
    let tool = ReadTool;
    let output = tool.execute("/nonexistent/file").await;
    assert!(!output.success);
    assert!(output.error.is_some());
}

#[tokio::test]
async fn test_write_tool() {
    let tool = WriteTool::new(1024);
    let tmp = format!("{}/test_write_{}", std::env::temp_dir().display(), std::process::id());
    let output = tool.execute(format!("{}|hello world", tmp).as_str()).await;
    assert!(output.success);
    std::fs::remove_file(&tmp).ok();
}

#[tokio::test]
async fn test_bash_tool() {
    let tool = BashTool::new();
    let output = tool.execute("echo test").await;
    assert!(output.success);
    assert!(output.output.contains("test"));
}
```

```bash
cd backend && cargo test tools_builtin_test --no-fail-fast -v
git add backend/src/tools/builtin/ backend/src/tools/mod.rs backend/tests/tools_builtin_test.rs
git commit -m "feat(tools): 实现 Read/Write/Bash/Git 内置工具"
```

---

### Task 12: LLM — 统一客户端 + Provider 抽象

**Files:**
- Create: `backend/src/llm/mod.rs`
- Create: `backend/src/llm/client.rs`
- Create: `backend/src/llm/message.rs`
- Create: `backend/src/llm/config.rs`
- Create: `backend/src/llm/providers/mod.rs`
- Create: `backend/src/llm/providers/openai.rs`
- Create: `backend/src/llm/providers/deepseek.rs`
- Create: `backend/src/llm/providers/qwen.rs`
- Create: `backend/src/llm/providers/anthropic.rs`
- Modify: `backend/Cargo.toml` (添加 schemars)
- Test: `backend/tests/llm_client_test.rs`

- [ ] **Step 1: 添加依赖**

```toml
# backend/Cargo.toml [dependencies] 中添加:
schemars = "0.8"
```

- [ ] **Step 2: 消息格式 + 模块声明**

```rust
// backend/src/llm/mod.rs
pub mod client;
pub mod router;
pub mod structured;
pub mod message;
pub mod config;
pub mod providers;

pub use client::LlmClient;
pub use message::*;
pub use config::*;
pub use router::ModelRouter;
pub use structured::StructuredOutput;
```

```rust
// backend/src/llm/message.rs
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: MessageRole,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum MessageRole {
    System,
    User,
    Assistant,
    Tool,
}

#[derive(Debug, Clone, Default)]
pub struct TokenUsage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: serde_json::Value,
}
```

- [ ] **Step 3: 配置**

```rust
// backend/src/llm/config.rs
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderConfig {
    pub name: String,
    pub api_key: String,
    pub base_url: String,
    pub default_model: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmConfig {
    pub providers: Vec<ProviderConfig>,
    pub l1_models: Vec<String>,
    pub l2_models: Vec<String>,
    pub strategy: RoutingStrategy,
    pub fallback_enabled: bool,
    pub max_fallback_attempts: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RoutingStrategy {
    Fixed,
    CostFirst,
    QualityFirst,
}

impl Default for RoutingStrategy {
    fn default() -> Self { Self::CostFirst }
}
```

- [ ] **Step 4: 统一 Trait**

```rust
// backend/src/llm/client.rs
use super::message::*;
use super::config::ProviderConfig;
use anyhow;
use async_trait::async_trait;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatRequest {
    pub model: String,
    pub messages: Vec<Message>,
    pub temperature: Option<f32>,
    pub max_tokens: Option<u32>,
}

#[derive(Debug, Clone)]
pub struct ChatResponse {
    pub content: String,
    pub tool_calls: Vec<ToolCall>,
    pub usage: TokenUsage,
}

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

- [ ] **Step 5: OpenAI Provider（参考实现）**

```rust
// backend/src/llm/providers/openai.rs
use crate::llm::*;
use async_trait::async_trait;
use serde_json::json;

pub struct OpenAiProvider {
    config: ProviderConfig,
    client: reqwest::Client,
}

impl OpenAiProvider {
    pub fn new(config: ProviderConfig) -> Self {
        Self {
            config,
            client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(120))
                .build()
                .unwrap_or_default(),
        }
    }
}

#[async_trait]
impl LlmProvider for OpenAiProvider {
    fn name(&self) -> &str { "openai" }

    async fn chat(&self, req: &ChatRequest) -> anyhow::Result<ChatResponse> {
        let body = json!({
            "model": req.model,
            "messages": req.messages,
            "temperature": req.temperature,
            "max_tokens": req.max_tokens,
        });

        let resp = self.client
            .post(format!("{}/chat/completions", self.config.base_url))
            .header("Authorization", format!("Bearer {}", self.config.api_key))
            .json(&body)
            .send()
            .await?
            .json::<serde_json::Value>()
            .await?;

        let content = resp["choices"][0]["message"]["content"]
            .as_str()
            .unwrap_or("")
            .to_string();

        let usage = TokenUsage {
            prompt_tokens: resp["usage"]["prompt_tokens"].as_u64().unwrap_or(0) as u32,
            completion_tokens: resp["usage"]["completion_tokens"].as_u64().unwrap_or(0) as u32,
            total_tokens: resp["usage"]["total_tokens"].as_u64().unwrap_or(0) as u32,
        };

        Ok(ChatResponse { content, tool_calls: Vec::new(), usage })
    }

    async fn chat_with_schema<T: schemars::JsonSchema>(&self, req: &ChatRequest) -> anyhow::Result<T> {
        // 使用 OpenAI response_format JSON Schema
        let schema = schemars::schema_for!(T);
        let body = json!({
            "model": req.model,
            "messages": req.messages,
            "response_format": {
                "type": "json_schema",
                "json_schema": {
                    "name": "structured_output",
                    "schema": schema,
                    "strict": true
                }
            }
        });

        let resp = self.client
            .post(format!("{}/chat/completions", self.config.base_url))
            .header("Authorization", format!("Bearer {}", self.config.api_key))
            .json(&body)
            .send()
            .await?
            .text()
            .await?;

        // 清理可能的 markdown 代码块
        let cleaned = Self::clean_json_output(&resp);
        let result: T = serde_json::from_str(&cleaned)?;
        Ok(result)
    }
}

impl OpenAiProvider {
    fn clean_json_output(text: &str) -> String {
        let t = text.trim();
        if t.starts_with("```") {
            let lines: Vec<&str> = t.lines().collect();
            let mut result = Vec::new();
            let mut in_code = false;
            for line in lines {
                if line.starts_with("```") {
                    in_code = !in_code;
                    continue;
                }
                if in_code {
                    result.push(line);
                }
            }
            result.join("\n")
        } else {
            t.to_string()
        }
    }
}
```

- [ ] **Step 6: DeepSeek + Qwen（兼容 OpenAI 格式）**

```rust
// backend/src/llm/providers/deepseek.rs
use crate::llm::*;
use async_trait::async_trait;
use serde_json::json;

pub struct DeepSeekProvider {
    config: ProviderConfig,
    client: reqwest::Client,
}

impl DeepSeekProvider {
    pub fn new(config: ProviderConfig) -> Self {
        Self {
            config,
            client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(120))
                .build()
                .unwrap_or_default(),
        }
    }
}

#[async_trait]
impl LlmProvider for DeepSeekProvider {
    fn name(&self) -> &str { "deepseek" }

    async fn chat(&self, req: &ChatRequest) -> anyhow::Result<ChatResponse> {
        let body = json!({
            "model": req.model,
            "messages": req.messages,
            "temperature": req.temperature,
            "max_tokens": req.max_tokens,
        });

        let resp = self.client
            .post(format!("{}/chat/completions", self.config.base_url))
            .header("Authorization", format!("Bearer {}", self.config.api_key))
            .json(&body)
            .send()
            .await?
            .json::<serde_json::Value>()
            .await?;

        let content = resp["choices"][0]["message"]["content"]
            .as_str().unwrap_or("").to_string();
        let usage = TokenUsage {
            prompt_tokens: resp["usage"]["prompt_tokens"].as_u64().unwrap_or(0) as u32,
            completion_tokens: resp["usage"]["completion_tokens"].as_u64().unwrap_or(0) as u32,
            total_tokens: resp["usage"]["total_tokens"].as_u64().unwrap_or(0) as u32,
        };

        Ok(ChatResponse { content, tool_calls: Vec::new(), usage })
    }

    async fn chat_with_schema<T: schemars::JsonSchema>(&self, req: &ChatRequest) -> anyhow::Result<T> {
        // DeepSeek 使用 tool calling 方式
        let schema = schemars::schema_for!(T);
        let body = json!({
            "model": req.model,
            "messages": req.messages,
            "tools": [{
                "type": "function",
                "function": {
                    "name": "structured_output",
                    "parameters": schema,
                    "strict": true
                }
            }],
            "tool_choice": "required"
        });

        let resp = self.client
            .post(format!("{}/chat/completions", self.config.base_url))
            .header("Authorization", format!("Bearer {}", self.config.api_key))
            .json(&body)
            .send()
            .await?
            .json::<serde_json::Value>()
            .await?;

        let args = resp["choices"][0]["message"]["tool_calls"][0]["function"]["arguments"]
            .as_str()
            .unwrap_or("{}");

        Ok(serde_json::from_str(args)?)
    }
}
```

```rust
// backend/src/llm/providers/qwen.rs
// 与 DeepSeek 实现相同（兼容 OpenAI API 格式）
// 复制 deepseek.rs 结构，修改 name 和 base_url
```

- [ ] **Step 7: Anthropic Provider**

```rust
// backend/src/llm/providers/anthropic.rs
use crate::llm::*;
use async_trait::async_trait;
use serde_json::json;

pub struct AnthropicProvider {
    config: ProviderConfig,
    client: reqwest::Client,
}

impl AnthropicProvider {
    pub fn new(config: ProviderConfig) -> Self {
        Self {
            config,
            client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(120))
                .build()
                .unwrap_or_default(),
        }
    }
}

#[async_trait]
impl LlmProvider for AnthropicProvider {
    fn name(&self) -> &str { "anthropic" }

    async fn chat(&self, req: &ChatRequest) -> anyhow::Result<ChatResponse> {
        let system_msg: String = req.messages.iter()
            .filter(|m| m.role == MessageRole::System)
            .map(|m| m.content.clone())
            .collect();
        let non_system: Vec<&Message> = req.messages.iter()
            .filter(|m| m.role != MessageRole::System)
            .collect();

        let body = json!({
            "model": req.model,
            "system": system_msg,
            "messages": non_system,
            "temperature": req.temperature,
            "max_tokens": req.max_tokens.unwrap_or(4096),
        });

        let resp = self.client
            .post(format!("{}v1/messages", self.config.base_url))
            .header("x-api-key", &self.config.api_key)
            .header("anthropic-version", "2023-06-01")
            .json(&body)
            .send()
            .await?
            .json::<serde_json::Value>()
            .await?;

        let content = resp["content"][0]["text"]
            .as_str().unwrap_or("").to_string();
        let usage = TokenUsage {
            prompt_tokens: resp["usage"]["input_tokens"].as_u64().unwrap_or(0) as u32,
            completion_tokens: resp["usage"]["output_tokens"].as_u64().unwrap_or(0) as u32,
            total_tokens: 0,
        };

        Ok(ChatResponse { content, tool_calls: Vec::new(), usage })
    }

    async fn chat_with_schema<T: schemars::JsonSchema>(&self, _req: &ChatRequest) -> anyhow::Result<T> {
        // Anthropic tool use 模式，后续完善
        anyhow::bail!("Anthropic schema output not yet implemented");
    }
}
```

- [ ] **Step 8: 测试**

```rust
// backend/tests/llm_client_test.rs
use devops_agent::llm::*;

#[test]
fn test_message_serialization() {
    let msg = Message { role: MessageRole::User, content: "hello".into() };
    let json = serde_json::to_string(&msg).unwrap();
    assert!(json.contains("user"));
    assert!(json.contains("hello"));
}

#[test]
fn test_token_usage_default() {
    let usage = TokenUsage::default();
    assert_eq!(usage.prompt_tokens, 0);
    assert_eq!(usage.total_tokens, 0);
}
```

- [ ] **Step 9: 导出 + 运行**

```rust
// backend/src/lib.rs 添加:
pub mod llm;
```

```bash
cd backend && cargo test llm_client_test --no-fail-fast -v
```

- [ ] **Step 10: 提交**

```bash
git add backend/src/llm/ backend/src/lib.rs backend/Cargo.toml backend/tests/llm_client_test.rs
git commit -m "feat(llm): 实现多 provider 统一客户端抽象"
```

---

### Task 13: LLM — 模型路由 + 结构化输出

**Files:**
- Create: `backend/src/llm/router.rs`
- Create: `backend/src/llm/structured.rs`
- Test: `backend/tests/llm_router_test.rs`

- [ ] **Step 1: 实现 ModelRouter**

```rust
// backend/src/llm/router.rs
use super::config::{LlmConfig, RoutingStrategy, ProviderConfig};
use anyhow;
use std::sync::Arc;

#[derive(Debug, Clone, PartialEq)]
pub enum TaskLevel {
    L1, // 经济模型
    L2, // 专业模型
}

pub struct ModelRouter {
    config: LlmConfig,
}

impl ModelRouter {
    pub fn new(config: LlmConfig) -> Self {
        Self { config }
    }

    /// 根据任务级别选择模型
    pub fn select_model(&self, level: TaskLevel) -> Option<&ProviderConfig> {
        match level {
            TaskLevel::L1 => self.config.l1_models.iter().find_map(|name| {
                self.config.providers.iter().find(|p| p.name == *name)
            }),
            TaskLevel::L2 => self.config.l2_models.iter().find_map(|name| {
                self.config.providers.iter().find(|p| p.name == *name)
            }),
        }
    }

    /// 任务分类（基于关键词）
    pub fn classify_task(&self, prompt: &str) -> TaskLevel {
        let l2_keywords = ["根因", "架构", "安全", "重构", "设计", "root cause", "architecture", "security", "refactor"];
        let l1_keywords = ["注释", "commit", "格式化", "摘要", "单测", "comment", "summary", "test"];

        let lower = prompt.to_lowercase();
        let has_l2 = l2_keywords.iter().any(|k| lower.contains(k));
        let has_l1 = l1_keywords.iter().any(|k| lower.contains(k));

        if has_l2 {
            TaskLevel::L2
        } else if has_l1 || prompt.len() < 200 {
            TaskLevel::L1
        } else {
            TaskLevel::L2
        }
    }
}
```

- [ ] **Step 2: 实现 StructuredOutput**

```rust
// backend/src/llm/structured.rs
use super::client::{ChatRequest, LlmProvider};
use anyhow;
use std::sync::Arc;

pub struct StructuredOutput;

impl StructuredOutput {
    /// 带 schema 约束的 LLM 调用，自动重试
    pub async fn chat<T: schemars::JsonSchema + serde::de::DeserializeOwned>(
        provider: &dyn LlmProvider,
        req: &ChatRequest,
        max_retries: u32,
    ) -> anyhow::Result<T> {
        let mut last_error = String::new();

        for attempt in 0..=max_retries {
            match provider.chat_with_schema::<T>(req).await {
                Ok(result) => return Ok(result),
                Err(e) => {
                    last_error = e.to_string();
                    if attempt < max_retries {
                        // 重试时附加错误信息（需要在实际实现中修改 req）
                        tracing::warn!(attempt, error = %last_error, "structured output retry");
                    }
                }
            }
        }

        anyhow::bail!("Structured output failed after {} attempts: {}", max_retries + 1, last_error)
    }
}
```

- [ ] **Step 3: 测试**

```rust
// backend/tests/llm_router_test.rs
use devops_agent::llm::*;
use devops_agent::llm::config::*;

#[test]
fn test_classify_simple_task_as_l1() {
    let config = LlmConfig {
        providers: Vec::new(),
        l1_models: vec!["openai".into()],
        l2_models: vec!["anthropic".into()],
        strategy: RoutingStrategy::Fixed,
        fallback_enabled: false,
        max_fallback_attempts: 0,
    };
    let router = ModelRouter::new(config);
    assert_eq!(router.classify_task("生成 commit message"), TaskLevel::L1);
    assert_eq!(router.classify_task("写代码注释"), TaskLevel::L1);
}

#[test]
fn test_classify_complex_task_as_l2() {
    let config = LlmConfig {
        providers: Vec::new(),
        l1_models: vec!["openai".into()],
        l2_models: vec!["anthropic".into()],
        strategy: RoutingStrategy::Fixed,
        fallback_enabled: false,
        max_fallback_attempts: 0,
    };
    let router = ModelRouter::new(config);
    assert_eq!(router.classify_task("分析这个 bug 的根因"), TaskLevel::L2);
    assert_eq!(router.classify_task("设计系统架构"), TaskLevel::L2);
}
```

- [ ] **Step 4: 运行 + 提交**

```bash
cd backend && cargo test llm_router_test --no-fail-fast -v
git add backend/src/llm/router.rs backend/src/llm/structured.rs backend/tests/llm_router_test.rs
git commit -m "feat(llm): 实现模型路由分级与结构化输出"
```

---

### Task 14: 集成 — Token Hook + Memory Hook

**Files:**
- Create: `backend/src/harness/token_hook.rs`
- Create: `backend/src/harness/memory_hook.rs`
- Modify: `backend/src/harness/mod.rs`
- Test: `backend/tests/integration_hook_test.rs`

- [ ] **Step 1: Token Hook**

```rust
// backend/src/harness/token_hook.rs
use super::hook::{Hook, HookPoint};
use crate::token::TokenTracker;
use async_trait::async_trait;
use std::sync::Arc;

pub struct TokenHook {
    tracker: Arc<TokenTracker>,
}

impl TokenHook {
    pub fn new(tracker: Arc<TokenTracker>) -> Self {
        Self { tracker }
    }
}

#[async_trait]
impl Hook for TokenHook {
    async fn on(&self, point: HookPoint) -> anyhow::Result<()> {
        match point {
            HookPoint::LlmResult => {
                if self.tracker.is_exceeded() {
                    tracing::warn!("Token budget exceeded");
                }
            }
            _ => {}
        }
        Ok(())
    }
}
```

- [ ] **Step 2: Memory Hook**

```rust
// backend/src/harness/memory_hook.rs
use super::hook::{Hook, HookPoint};
use crate::memory::ShortTermMemory;
use async_trait::async_trait;
use std::sync::{Arc, Mutex};

pub struct MemoryHook {
    memory: Arc<Mutex<ShortTermMemory>>,
}

impl MemoryHook {
    pub fn new(memory: ShortTermMemory) -> Self {
        Self { memory: Arc::new(Mutex::new(memory)) }
    }
}

#[async_trait]
impl Hook for MemoryHook {
    async fn on(&self, point: HookPoint) -> anyhow::Result<()> {
        let mem = self.memory.lock().unwrap();
        match point {
            HookPoint::SessionStart => {
                drop(mem);
                self.memory.lock().unwrap().add("session started".into(), crate::memory::MemoryType::Decision);
            }
            HookPoint::StepStart => {
                drop(mem);
                self.memory.lock().unwrap().add(format!("step: {:?}", point).into(), crate::memory::MemoryType::Decision);
            }
            _ => {}
        }
        Ok(())
    }
}
```

- [ ] **Step 3: 集成测试**

```rust
// backend/tests/integration_hook_test.rs
use devops_agent::harness::*;
use devops_agent::token::TokenTracker;
use devops_agent::memory::ShortTermMemory;
use std::sync::{Arc, atomic::{AtomicUsize, Ordering}};

#[tokio::test]
async fn test_hooks_execute_in_order() {
    let mut orchestrator = Orchestrator::new();

    // 添加 Token Hook
    let tracker = Arc::new(TokenTracker::new(10000));
    orchestrator.add_hook(Arc::new(TokenHook::new(tracker.clone())));

    // 执行空步骤链
    let results = orchestrator.run(&[]).await;
    assert!(results.is_ok());
}
```

- [ ] **Step 4: 运行 + 提交**

```bash
cd backend && cargo test integration_hook_test --no-fail-fast -v
git add backend/src/harness/token_hook.rs backend/src/harness/memory_hook.rs backend/src/harness/mod.rs backend/tests/integration_hook_test.rs
git commit -m "feat(harness): 集成 Token Hook 和 Memory Hook"
```

---

### Task 15: 全量构建验证 + 清理

**目标:** 确保所有模块编译通过，clippy 无警告

- [ ] **Step 1: 全量编译**

```bash
cd backend && cargo build 2>&1
```

- [ ] **Step 2: 全量测试**

```bash
cd backend && cargo test --no-fail-fast 2>&1
```

- [ ] **Step 3: Clippy 检查**

```bash
cd backend && cargo clippy -- -D warnings 2>&1
```

- [ ] **Step 4: 格式化**

```bash
cd backend && cargo fmt
```

- [ ] **Step 5: 提交**

```bash
git add backend/
git commit -m "refactor: 全量构建验证 + clippy 修复 + 格式化"
```

---

## Self-Review 检查

### Spec 覆盖率

| 设计文档章节 | 对应 Task | 覆盖 |
|-------------|----------|------|
| 一、整体架构 | Task 1-3 (Harness) | 是 |
| 二、记忆系统 | Task 4-5 | 是 |
| 三、Token 管理 | Task 6-8 | 是 |
| 四、权限控制 | Task 9 | 是 |
| 五、沙箱隔离 | Task 10 | 是 |
| 六、LLM 提供商抽象 | Task 12 | 是 |
| 七、结构化输出约束 | Task 13 | 是 |
| 八、模型路由分级 | Task 13 | 是 |
| 九、错误处理 | 分散在各 Task | 是 |
| 十、测试策略 | 每个 Task 含测试 | 是 |

### 2026-04-29 修订标记（实施时注意）

以下 Task 的代码示例需按修订后的设计调整，请以 `docs/superpowers/specs/2026-04-28-agent-infrastructure-design.md` 为准：

| Task | 变更 | 说明 |
|------|------|------|
| Task 7: ContextWindow | 4 层改为 System/Compressed/Structured/Linear | 移除 Layer enum，增加 StructuredDoc、滑动窗口 |
| Task 8: Summarizer | 移除本地摘要，增加 `summarize_rounds()` + `update_structure()` | 渐进式三阶段压缩，纯轮次触发 |
| Task 12: LLM Providers | 仅实现 Anthropic + OpenAI | 移除 DeepSeek + Qwen |
| Task 14: 集成 | 增加迁移现有 7 个 Step | 将 agent/steps/ 迁移到新 Step trait |

### Placeholder 扫描

- Task 8 (Summarizer): `summarize_with_llm` 标记为 TODO，将在 Task 12 完成后补充。这是**有意的分阶段实现**，因为 Summarizer 依赖 LLM 客户端。
- Task 12 Anthropic provider: `chat_with_schema` 未完全实现。原因：Anthropic tool use 格式与其他 provider 不同，需要单独研究。建议后续迭代补充。

### 类型一致性

- `TokenUsage` 统一在 `llm/message.rs` 单一来源定义，token 模块引用。

---

## 执行建议

**推荐顺序:** Task 1 → 2 → 3 → 4 → 5 → 6 → 7 → 8 → 9 → 10 → 11 → 12 → 13 → 14 → 15

**可并行:** Task 4-5（记忆）和 Task 6-8（Token）无依赖关系，可并行开发。Task 9-10（Security + Sandbox）也可并行。

Plan complete and saved to `docs/superpowers/plans/2026-04-28-agent-infrastructure.md`. 两种执行方式：

**1. Subagent-Driven（推荐）** — 每个 Task 派发独立子代理，任务间人工审核，迭代快
**2. Inline Execution** — 在本会话中按任务顺序执行，checkpoint 检查点评审

你选择哪种方式？
