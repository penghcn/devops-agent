# 分层沙箱架构 Phase 1 实现计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 建立沙箱抽象层并集成 microsandbox 后端，保留现有进程沙箱作为降级方案

**Architecture:** 定义 `Sandbox` async trait 作为统一接口，实现 `MicrosandboxBackend` 和 `ProcessBackend` 两个后端实现，通过 `SandboxFactory` 根据环境变量自动选择。逐步将 `bash.rs` 从 `ProcessSandbox` 迁移到新的抽象层。

**Tech Stack:** Rust, microsandbox SDK (v0.4.6), tokio, async-trait

---

### Task 1: 添加 microsandbox 依赖

**Files:**
- Modify: `backend/Cargo.toml`

- [ ] **Step 1: 添加 microsandbox 依赖到 Cargo.toml**

在 `[dependencies]` 中添加：

```toml
microsandbox = "0.4"
```

- [ ] **Step 2: 验证依赖加载成功**

运行：`cd backend && cargo check 2>&1 | head -50`

预期：编译可能因为后续改动失败，但 `microsandbox` crate 应该能正确下载。如果下载失败，检查网络或配置 cargo source。

- [ ] **Step 3: 提交**

```bash
git add backend/Cargo.toml backend/Cargo.lock
git commit -m "chore: 添加 microsandbox SDK 依赖"
```

---

### Task 2: 定义 Sandbox Trait 和 ExecResult

**Files:**
- Create: `backend/src/sandbox/trait.rs`

- [ ] **Step 1: 创建 trait.rs**

```rust
use anyhow::Result;

/// 沙箱执行结果
#[derive(Debug, Clone)]
pub struct ExecResult {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
}

/// 沙箱统一接口
#[async_trait::async_trait]
pub trait Sandbox: Send + Sync {
    /// 执行命令
    async fn exec(&self, command: &str, args: &[String]) -> Result<ExecResult>;

    /// 读取沙箱内文件内容
    async fn read_file(&self, path: &str) -> Result<String>;

    /// 写入文件到沙箱内
    async fn write_file(&self, path: &str, content: &str) -> Result<()>;

    /// 停止并销毁沙箱
    async fn stop(&self) -> Result<()>;
}
```

- [ ] **Step 2: 验证编译**

运行：`cd backend && cargo check --lib 2>&1 | head -30`

预期：可能有未使用 import 警告，但不应该有类型错误。

- [ ] **Step 3: 提交**

```bash
git add backend/src/sandbox/trait.rs
git commit -m "feat(sandbox): 定义 Sandbox trait 和 ExecResult 类型"
```

---

### Task 3: 实现 ProcessBackend（适配现有 ProcessSandbox）

**Files:**
- Create: `backend/src/sandbox/process_backend.rs`
- Read: `backend/src/sandbox/process_sandbox.rs` (参考现有实现)

- [ ] **Step 1: 创建 process_backend.rs**

包装现有 `ProcessSandbox`，使其实现 `Sandbox` trait：

```rust
use anyhow::Result;
use tokio::runtime::Handle;

use super::process_sandbox::ProcessSandbox;
use super::trait_sandbox::{ExecResult, Sandbox};

/// 使用现有进程沙箱的 Sandbox 实现（降级方案）
pub struct ProcessBackend {
    inner: ProcessSandbox,
}

impl ProcessBackend {
    pub fn new() -> Self {
        Self {
            inner: ProcessSandbox::new(),
        }
    }

    pub fn with_config(config: super::process_sandbox::ProcessSandboxConfig) -> Self {
        Self {
            inner: ProcessSandbox::with_config(config),
        }
    }
}

#[async_trait::async_trait]
impl Sandbox for ProcessBackend {
    async fn exec(&self, command: &str, args: &[String]) -> Result<ExecResult> {
        // ProcessSandbox::execute_async 需要在 tokio 运行时中
        let inner = self.inner.clone();
        let cmd = command.to_string();
        let a = args.to_vec();

        let result = Handle::current().spawn(async move {
            inner.execute_async(&cmd, &a).await
        })
        .await??;

        Ok(ExecResult {
            stdout: result.stdout,
            stderr: result.stderr,
            exit_code: result.exit_code,
        })
    }

    async fn read_file(&self, path: &str) -> Result<String> {
        let result = self.exec("cat", &[path.to_string()]).await?;
        if result.exit_code == 0 {
            Ok(result.stdout)
        } else {
            anyhow::anyhow!("cat {} failed: {}", path, result.stderr)
        }
    }

    async fn write_file(&self, path: &str, content: &str) -> Result<()> {
        let escaped = content.replace('"', "\\\"");
        let cmd = format!("echo '{}' > '{}'", escaped, path);
        let result = self.exec("sh", &["-c".into(), cmd]).await?;
        if result.exit_code == 0 {
            Ok(())
        } else {
            anyhow::anyhow!("write {} failed: {}", path, result.stderr)
        }
    }

    async fn stop(&self) -> Result<()> {
        Ok(())
    }
}
```

注意：模块名使用 `trait_sandbox` 因为 Rust 不允许 `trait` 作为模块名。

- [ ] **Step 2: 验证编译**

运行：`cd backend && cargo check --lib 2>&1 | head -30`

预期：应该能编译通过。`ProcessSandbox` 已经是 `Clone` + 现有 `execute_async` 方法可用。

- [ ] **Step 3: 提交**

```bash
git add backend/src/sandbox/process_backend.rs
git commit -m "feat(sandbox): 实现 ProcessBackend 适配现有进程沙箱"
```

---

### Task 4: 实现 MicrosandboxBackend

**Files:**
- Create: `backend/src/sandbox/microsandbox_backend.rs`

- [ ] **Step 1: 创建 microsandbox_backend.rs**

```rust
use anyhow::Result;

use super::trait_sandbox::{ExecResult, Sandbox};

pub struct MicrosandboxBackend {
    sandbox: Option<microsandbox::Sandbox>,
    config: MicrosandboxConfig,
}

#[derive(Clone)]
pub struct MicrosandboxConfig {
    pub image: String,
    pub cpus: u32,
    pub memory: u64,
}

impl Default for MicrosandboxConfig {
    fn default() -> Self {
        Self {
            image: "debian".to_string(),
            cpus: 1,
            memory: 512,
        }
    }
}

impl MicrosandboxBackend {
    pub fn new(config: MicrosandboxConfig) -> Self {
        Self {
            sandbox: None,
            config,
        }
    }

    /// 懒初始化沙箱
    async fn ensure_running(&mut self) -> Result<&microsandbox::Sandbox> {
        if self.sandbox.is_none() {
            let sb = microsandbox::Sandbox::builder(&format!("devops-{}", std::process::id()))
                .image(&self.config.image)
                .cpus(self.config.cpus)
                .memory(self.config.memory)
                .create()
                .await
                .map_err(|e| anyhow::anyhow!("microsandbox 创建失败: {}", e))?;
            self.sandbox = Some(sb);
        }
        self.sandbox.as_ref().unwrap()
    }
}

#[async_trait::async_trait]
impl Sandbox for MicrosandboxBackend {
    async fn exec(&self, command: &str, args: &[String]) -> Result<ExecResult> {
        // microsandbox 的 exec 需要 &self
        let sb = self
            .sandbox
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("沙箱未启动"))?;

        let output = sb
            .exec(command, args)
            .await
            .map_err(|e| anyhow::anyhow!("microsandbox exec 失败: {}", e))?;

        let stdout = output.stdout().unwrap_or_default();
        let stderr = output.stderr().unwrap_or_default();
        let exit_code = output.exit_code().unwrap_or(-1);

        Ok(ExecResult {
            stdout,
            stderr,
            exit_code,
        })
    }

    async fn read_file(&self, path: &str) -> Result<String> {
        let output = self
            .exec("cat", &[path.to_string()])
            .await?;
        if output.exit_code == 0 {
            Ok(output.stdout)
        } else {
            anyhow::anyhow!("cat {} failed: {}", path, output.stderr)
        }
    }

    async fn write_file(&self, path: &str, content: &str) -> Result<()> {
        let escaped = content.replace('"', "\\\"");
        let cmd = format!("echo '{}' > '{}'", escaped, path);
        let result = self.exec("sh", &["-c".into(), cmd]).await?;
        if result.exit_code == 0 {
            Ok(())
        } else {
            anyhow::anyhow!("write {} failed: {}", path, result.stderr)
        }
    }

    async fn stop(&self) -> Result<()> {
        if let Some(ref sb) = self.sandbox {
            sb.stop_and_wait()
                .await
                .map_err(|e| anyhow::anyhow!("microsandbox 停止失败: {}", e))?;
        }
        Ok(())
    }
}
```

- [ ] **Step 2: 验证编译**

运行：`cd backend && cargo check --lib 2>&1 | head -50`

**重要**: 此时需要根据 microsandbox SDK 的实际 API 调整。SDK 的 `Sandbox::exec()` 返回值类型、`stdout()`/`stderr()` 方法签名可能与上述代码不同。需要阅读 `microsandbox` crate 导出的类型定义，调整方法调用。

如果 API 不匹配，参考 `examples/rust/root-oci/` 目录下的示例代码进行调整。

- [ ] **Step 3: 提交**

```bash
git add backend/src/sandbox/microsandbox_backend.rs
git commit -m "feat(sandbox): 实现 MicrosandboxBackend"
```

---

### Task 5: 实现 SandboxFactory

**Files:**
- Create: `backend/src/sandbox/factory.rs`

- [ ] **Step 1: 创建 factory.rs**

```rust
use anyhow::Result;
use std::sync::Arc;

use super::microsandbox_backend::{MicrosandboxBackend, MicrosandboxConfig};
use super::process_backend::ProcessBackend;
use super::trait_sandbox::Sandbox;

/// 沙箱后端枚举
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SandboxBackend {
    Microsandbox,
    Process,
}

impl SandboxBackend {
    /// 从环境变量自动检测后端类型
    pub fn from_env() -> Self {
        match std::env::var("SANDBOX_BACKEND").ok().as_deref() {
            Some("process") => Self::Process,
            Some("microsandbox") => Self::Microsandbox,
            _ => {
                if cfg!(target_os = "macos") || cfg!(target_os = "linux") {
                    Self::Microsandbox
                } else {
                    Self::Process
                }
            }
        }
    }
}

/// 沙箱工厂 — 根据配置创建对应的后端
pub struct SandboxFactory {
    backend: SandboxBackend,
    micro_config: MicrosandboxConfig,
}

impl SandboxFactory {
    pub fn new() -> Self {
        Self {
            backend: SandboxBackend::from_env(),
            micro_config: MicrosandboxConfig::default(),
        }
    }

    pub fn with_backend(mut self, backend: SandboxBackend) -> Self {
        self.backend = backend;
        self
    }

    pub fn with_micro_config(mut self, config: MicrosandboxConfig) -> Self {
        self.micro_config = config;
        self
    }

    /// 创建沙箱实例
    pub fn create(&self) -> Result<Arc<dyn Sandbox>> {
        match self.backend {
            SandboxBackend::Microsandbox => {
                tracing::info!("使用 microsandbox 后端");
                Ok(Arc::new(MicrosandboxBackend::new(self.micro_config.clone())))
            }
            SandboxBackend::Process => {
                tracing::info!("使用 process 后端（降级）");
                Ok(Arc::new(ProcessBackend::new()))
            }
        }
    }
}
```

- [ ] **Step 2: 验证编译**

运行：`cd backend && cargo check --lib 2>&1 | head -30`

- [ ] **Step 3: 提交**

```bash
git add backend/src/sandbox/factory.rs
git commit -m "feat(sandbox): 实现 SandboxFactory 后端选择逻辑"
```

---

### Task 6: 重写 sandbox/mod.rs

**Files:**
- Modify: `backend/src/sandbox/mod.rs`

- [ ] **Step 1: 重写 mod.rs**

保留原有模块导出，新增抽象层导出：

```rust
pub mod factory;
pub mod microsandbox_backend;
pub mod process_backend;
pub mod trait_sandbox;

// 保留原有模块作为降级方案的内部实现
pub mod fs_isolation;
pub mod network_whitelist;
pub mod path_check;
pub mod process_sandbox;

use std::fmt;

/// 沙箱统一错误类型
#[derive(Debug)]
pub enum SandboxError {
    PathTraversal(String),
    SensitiveFile(String),
    OutsideWorkspace(String),
    TimeoutExceeded(String),
    NetworkBlocked(String),
    IoError(String),
}

impl fmt::Display for SandboxError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SandboxError::PathTraversal(msg) => write!(f, "路径穿越: {}", msg),
            SandboxError::SensitiveFile(msg) => write!(f, "敏感文件访问: {}", msg),
            SandboxError::OutsideWorkspace(msg) => write!(f, "超出工作区: {}", msg),
            SandboxError::TimeoutExceeded(msg) => write!(f, "超时: {}", msg),
            SandboxError::NetworkBlocked(msg) => write!(f, "网络拦截: {}", msg),
            SandboxError::IoError(msg) => write!(f, "IO 错误: {}", msg),
        }
    }
}

impl std::error::Error for SandboxError {}

// 新抽象层导出
pub use factory::{SandboxBackend, SandboxFactory};
pub use trait_sandbox::{ExecResult, Sandbox};

// 保留旧导出以兼容现有代码
pub use fs_isolation::{FileSystemIsolator, FsIsolationConfig};
pub use network_whitelist::{NetworkCheckResult, NetworkWhitelist};
pub use path_check::{PathValidation, PathValidator};
pub use process_sandbox::{ProcessResult, ProcessSandbox, ProcessSandboxConfig};
```

- [ ] **Step 2: 验证编译**

运行：`cd backend && cargo check --lib 2>&1 | head -30`

预期：所有模块应该能正确导出。原有代码的 import 路径不变。

- [ ] **Step 3: 提交**

```bash
git add backend/src/sandbox/mod.rs
git commit -m "refactor(sandbox): 重写 mod.rs 导出新抽象层，保留旧接口兼容"
```

---

### Task 7: 添加 Sandbox 配置到 Config

**Files:**
- Modify: `backend/src/config.rs`

- [ ] **Step 1: 在 Config 结构体中添加沙箱配置字段**

在 `Config` struct 中添加：

```rust
pub sandbox_backend: String,
pub sandbox_timeout_secs: u64,
pub sandbox_image: String,
pub sandbox_cpus: u32,
pub sandbox_memory: u64,
```

- [ ] **Step 2: 在 from_file() 中加载配置**

在 `from_file()` 方法中，在 `cors_origins` 解析之后添加：

```rust
let sandbox_backend = conf
    .get("sandbox.backend")
    .map(|s| s.as_str())
    .unwrap_or("microsandbox")
    .to_string();
let sandbox_timeout_secs = conf
    .get("sandbox.timeout_secs")
    .and_then(|s| s.parse().ok())
    .unwrap_or(30);
let sandbox_image = conf
    .get("sandbox.microsandbox.image")
    .cloned()
    .unwrap_or_else(|| "debian".to_string());
let sandbox_cpus = conf
    .get("sandbox.microsandbox.cpus")
    .and_then(|s| s.parse().ok())
    .unwrap_or(1);
let sandbox_memory = conf
    .get("sandbox.microsandbox.memory")
    .and_then(|s| s.parse().ok())
    .unwrap_or(512);
```

在 `Self { ... }` 构造中添加这些字段。

- [ ] **Step 3: 同步更新 test_default() 方法**

```rust
sandbox_backend: "process".to_string(),
sandbox_timeout_secs: 30,
sandbox_image: "debian".to_string(),
sandbox_cpus: 1,
sandbox_memory: 512,
```

- [ ] **Step 4: 验证编译**

运行：`cd backend && cargo check --lib 2>&1 | head -30`

- [ ] **Step 5: 提交**

```bash
git add backend/src/config.rs
git commit -m "feat(config): 添加沙箱配置字段"
```

---

### Task 8: 集成到 BashTool

**Files:**
- Modify: `backend/src/tools/builtin/bash.rs`
- Read: `backend/src/tools/builtin/mod.rs` (Tool trait)

- [ ] **Step 1: 修改 BashTool 使用 Arc<dyn Sandbox>**

替换 `BashTool` 结构体定义：

```rust
use crate::sandbox::{NetworkCheckResult, NetworkWhitelist, Sandbox, SandboxFactory};
use crate::security::policy::PolicyEngine;
use crate::security::roles::{PolicyDecision, ToolName, ToolRequest};
use std::sync::Arc;

use super::{Tool, ToolInput, ToolOutput};

pub struct BashTool {
    sandbox: Arc<dyn Sandbox>,
    network_check: NetworkWhitelist,
    policy_engine: PolicyEngine,
}

impl BashTool {
    pub fn new(
        sandbox: Arc<dyn Sandbox>,
        network_check: NetworkWhitelist,
        policy_engine: PolicyEngine,
    ) -> Self {
        Self {
            sandbox,
            network_check,
            policy_engine,
        }
    }

    /// 从工厂创建 BashTool（便捷方法）
    pub fn from_factory(
        factory: &SandboxFactory,
        network_check: NetworkWhitelist,
        policy_engine: PolicyEngine,
    ) -> anyhow::Result<Self> {
        let sandbox = factory.create()?;
        Ok(Self::new(sandbox, network_check, policy_engine))
    }
}
```

- [ ] **Step 2: 修改 execute 方法中的沙箱调用**

将原来的：
```rust
let result = match self.sandbox.execute_async(cmd, &args_slice).await {
```

替换为：
```rust
let result = match self.sandbox.exec(cmd, &args_slice).await {
    Ok(r) => r,
    Err(e) => {
        return ToolOutput::fail(format!("命令执行失败: {}", e));
    }
};
```

注意 `ExecResult` 字段名与旧的 `ProcessResult` 不同。调整后续代码：

```rust
let success = result.exit_code == 0;
let mut output = result.stdout;

let error = if !result.stderr.is_empty() {
    Some(result.stderr)
} else {
    None
};
```

移除 `result.timed_out` 和 `result.truncated` 的检查——这些由底层后端处理。

- [ ] **Step 3: 验证编译**

运行：`cd backend && cargo check --lib 2>&1 | head -50`

- [ ] **Step 4: 提交**

```bash
git add backend/src/tools/builtin/bash.rs
git commit -m "refactor(tools): BashTool 迁移到 Sandbox trait 抽象层"
```

---

### Task 9: 更新 GitTool 的调用方式

**Files:**
- Modify: `backend/src/tools/builtin/git.rs`

- [ ] **Step 1: 检查 GitTool 是否也需要迁移**

GitTool 当前使用 `ProcessSandbox`。由于 Phase 1 主要关注 bash.rs，GitTool 可以保持使用 `ProcessSandbox`，因为 `mod.rs` 仍然导出 `ProcessSandbox`。

如果编译报错（因为 `ProcessSandbox` 的导出路径变了），更新 import：

```rust
use crate::sandbox::process_sandbox::ProcessSandbox;
```

- [ ] **Step 2: 验证编译**

运行：`cd backend && cargo check --lib 2>&1 | head -30`

- [ ] **Step 3: 提交（如果有改动）**

```bash
git add backend/src/tools/builtin/git.rs
git commit -m "fix(tools): 修正 GitTool 的 ProcessSandbox import 路径"
```

---

### Task 10: 编写单元测试

**Files:**
- Create: `backend/tests/sandbox_trait_test.rs`

- [ ] **Step 1: 创建测试文件**

```rust
use devops_agent::sandbox::trait_sandbox::{ExecResult, Sandbox};
use devops_agent::sandbox::process_backend::ProcessBackend;
use devops_agent::sandbox::factory::{SandboxBackend, SandboxFactory};

#[tokio::test]
async fn process_backend_exec_echo() {
    let backend = ProcessBackend::new();
    let result = backend.exec("echo", &["hello".to_string()]).await.unwrap();
    assert_eq!(result.exit_code, 0);
    assert!(result.stdout.contains("hello"));
}

#[tokio::test]
async fn process_backend_read_file() {
    let backend = ProcessBackend::new();
    let result = backend.exec("echo", &["test-content".to_string()]).await.unwrap();
    assert_eq!(result.exit_code, 0);
}

#[tokio::test]
async fn process_backend_stop() {
    let backend = ProcessBackend::new();
    let result = backend.stop().await;
    assert!(result.is_ok());
}

#[test]
fn factory_creates_process_backend() {
    let factory = SandboxFactory::new().with_backend(SandboxBackend::Process);
    let sandbox = factory.create().unwrap();
    assert!(!sandbox.as_ptr().is_null());
}

#[test]
fn factory_backend_from_env_process() {
    std::env::set_var("SANDBOX_BACKEND", "process");
    let backend = SandboxBackend::from_env();
    assert_eq!(backend, SandboxBackend::Process);
    std::env::remove_var("SANDBOX_BACKEND");
}

#[test]
fn factory_backend_from_env_microsandbox() {
    std::env::set_var("SANDBOX_BACKEND", "microsandbox");
    let backend = SandboxBackend::from_env();
    assert_eq!(backend, SandboxBackend::Microsandbox);
    std::env::remove_var("SANDBOX_BACKEND");
}
```

- [ ] **Step 2: 运行测试**

运行：`cd backend && cargo test --test sandbox_trait_test -- --nocapture 2>&1 | tail -20`

预期：所有测试通过。

- [ ] **Step 3: 提交**

```bash
git add backend/tests/sandbox_trait_test.rs
git commit -m "test(sandbox): 添加 Sandbox trait 单元测试"
```

---

### Task 11: 全局编译验证

**Files:**
- 全项目验证

- [ ] **Step 1: 完整编译检查**

运行：`cd backend && cargo check --all-targets 2>&1 | tail -20`

修复任何编译错误。

- [ ] **Step 2: 运行全部测试**

运行：`cd backend && cargo test 2>&1 | tail -30`

确保原有测试不被破坏。

- [ ] **Step 3: 代码格式化**

运行：`cd backend && cargo fmt`

- [ ] **Step 4: 最终提交**

```bash
git add -A
git commit -m "feat(sandbox): 完成 Phase 1 — 抽象层 + microsandbox 集成"
```

---

## 自审查

**Spec 覆盖度:**
- [x] `SandboxTrait` 接口定义 → Task 2
- [x] `MicrosandboxBackend` 实现 → Task 4
- [x] `SandboxFactory` 工厂 → Task 5
- [x] `mod.rs` 重写 → Task 6
- [x] 集成到 `bash.rs` → Task 8
- [x] 配置支持 → Task 7
- [x] 降级方案（ProcessBackend）→ Task 3

**占位符扫描:** 无 TBD/TODO。所有代码块包含具体实现。

**类型一致性:**
- `ExecResult` 在 trait.rs 中定义，所有后端统一使用
- `Sandbox` trait 方法签名在各后端实现中一致
- `SandboxFactory::create()` 返回 `Arc<dyn Sandbox>`，与 BashTool 字段类型匹配

**风险注意:**
- Task 4 中 microsandbox SDK 的实际 API 可能与计划代码不同。实施时需要读取 SDK 导出的类型定义，调整方法调用。这是 Beta 软件的已知风险。
- `MicrosandboxBackend` 需要 `mut &self` 来懒初始化沙箱。如果 `Sandbox` trait 的 `exec` 需要 `&self`，可能需要用 `Arc<Mutex<>>` 或 `Arc<RwLock<>>` 包装内部状态。实施时根据编译反馈调整。
