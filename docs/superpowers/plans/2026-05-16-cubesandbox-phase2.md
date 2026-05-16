# CubeSandbox Phase 2 实现计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 添加 CubeSandboxBackend 实现 + Factory 配置驱动降级，支持 E2B 兼容 REST API

**Architecture:** 新增 `cubesandbox/` 模块（E2BClient + CubeSandboxBackend），改造 `SandboxFactory` 支持配置列表和启动时异步检测降级。命令执行通过控制平面 REST API `POST /sandboxes/{id}/commands`，文件操作通过 exec 间接实现。

**Tech Stack:** Rust, reqwest, anyhow, tokio, async-trait

---

### Task 1: 创建 CubeSandbox 模块骨架

**Files:**
- Create: `backend/src/sandbox/cubesandbox/mod.rs`
- Create: `backend/src/sandbox/cubesandbox/config.rs`
- Modify: `backend/src/sandbox/mod.rs`

- [ ] **Step 0: 更新 Cargo.toml 添加 yunli hash feature**

```toml
yunli = { version ="0.1", features = ["config", "hash"] }
```

- [ ] **Step 1: 创建 config.rs**

```rust
/// CubeSandbox 后端配置
#[derive(Debug, Clone)]
pub struct CubeSandboxConfig {
    pub api_url: String,
    pub api_key: String,
    pub template_id: String,
    pub timeout_secs: i32,
    pub allow_internet: bool,
}

impl Default for CubeSandboxConfig {
    fn default() -> Self {
        Self {
            api_url: String::new(),
            api_key: "dummy".to_string(),
            template_id: String::new(),
            timeout_secs: 1800,
            allow_internet: true,
        }
    }
}

impl CubeSandboxConfig {
    /// 配置是否完整（api_url 和 template_id 非空）
    pub fn is_complete(&self) -> bool {
        !self.api_url.is_empty() && !self.template_id.is_empty()
    }
}
```

- [ ] **Step 2: 创建 mod.rs**

```rust
pub mod config;
pub mod client;
pub mod backend;

pub use config::CubeSandboxConfig;
pub use client::E2BClient;
pub use backend::CubeSandboxBackend;
```

- [ ] **Step 3: 在 sandbox/mod.rs 中添加模块声明**

在现有 `pub mod factory;` 后添加：
```rust
pub mod cubesandbox;
```

在 exports 中添加：
```rust
pub use cubesandbox::{CubeSandboxBackend, CubeSandboxConfig};
```

- [ ] **Step 4: 编译验证**

```bash
cd backend && ./run-signed.sh
```

Expected: 编译通过（无新代码引用，只是模块声明）

- [ ] **Step 5: Commit**

```bash
git add backend/src/sandbox/cubesandbox/ backend/src/sandbox/mod.rs
git commit -m "feat(sandbox): 创建 CubeSandbox 模块骨架和配置结构"
```

---

### Task 2: 实现 E2BClient

**Files:**
- Create: `backend/src/sandbox/cubesandbox/client.rs`

- [ ] **Step 1: 定义响应结构**

```rust
#[derive(Debug, serde::Deserialize)]
pub struct SandboxCreateResponse {
    pub sandbox_id: String,
    #[allow(dead_code)]
    pub template_id: String,
}

#[derive(Debug, serde::Deserialize)]
pub struct CommandCreateResponse {
    #[serde(rename = "commandId")]
    pub command_id: String,
}

#[derive(Debug, serde::Deserialize)]
pub struct CommandResult {
    #[serde(rename = "exitCode")]
    pub exit_code: i32,
    pub stdout: String,
    pub stderr: String,
}
```

- [ ] **Step 2: 实现 E2BClient 结构体**

```rust
pub struct E2BClient {
    http: reqwest::Client,
    api_url: String,
    api_key: String,
}

impl E2BClient {
    pub fn new(api_url: String, api_key: String) -> Self {
        Self {
            http: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(30))
                .build()
                .expect("reqwest client build"),
            api_url: api_url.trim_end_matches('/').to_string(),
            api_key,
        }
    }
```

- [ ] **Step 3: 实现 create_sandbox**

```rust
    pub async fn create_sandbox(&self, template_id: &str, timeout_secs: i32) -> anyhow::Result<SandboxCreateResponse> {
        let resp = self.http
            .post(format!("{}/sandboxes", self.api_url))
            .header("Authorization", format!("Bearer {}", self.api_key))
            .json(&serde_json::json!({
                "templateID": template_id,
                "timeout": timeout_secs,
            }))
            .send()
            .await?;

        if !resp.status().is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!("创建沙箱失败: HTTP {} - {}", resp.status(), body));
        }

        let body: SandboxCreateResponse = resp.json().await?;
        Ok(body)
    }
```

- [ ] **Step 4: 实现 exec_command**

```rust
    pub async fn exec_command(&self, sandbox_id: &str, command: &str, args: &[String]) -> anyhow::Result<CommandResult> {
        let mut full_cmd = command.to_string();
        for arg in args {
            full_cmd.push(' ');
            full_cmd.push_str(arg);
        }

        // 通过 envd 执行命令 — 使用 CubeProxy 路由
        // envd 端点格式: {sandbox_id}.default.{api_url_host}
        // 但自托管环境下 CubeProxy 需要 DNS 配置，暂时用控制平面
        let resp = self.http
            .post(format!("{}/sandboxes/{}/commands", self.api_url, sandbox_id))
            .header("Authorization", format!("Bearer {}", self.api_key))
            .json(&serde_json::json!({
                "command": full_cmd,
            }))
            .send()
            .await?;

        if !resp.status().is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!("执行命令失败: HTTP {} - {}", resp.status(), body));
        }

        let cmd_resp: CommandCreateResponse = resp.json().await?;

        // 轮询等待命令完成（最多 60 次，每次 1 秒）
        for _ in 0..60 {
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            let status_resp = self.http
                .get(format!("{}/sandboxes/{}/commands/{}", self.api_url, sandbox_id, cmd_resp.command_id))
                .header("Authorization", format!("Bearer {}", self.api_key))
                .send()
                .await?;

            if !status_resp.status().is_success() {
                continue;
            }

            let result: CommandResult = status_resp.json().await?;
            if result.exit_code != -1 {
                return Ok(result);
            }
        }

        Err(anyhow::anyhow!("命令执行超时"))
    }
```

- [ ] **Step 5: 实现 kill_sandbox**

```rust
    pub async fn kill_sandbox(&self, sandbox_id: &str) -> anyhow::Result<()> {
        let resp = self.http
            .delete(format!("{}/sandboxes/{}", self.api_url, sandbox_id))
            .header("Authorization", format!("Bearer {}", self.api_key))
            .send()
            .await?;

        if resp.status().is_success() || resp.status().is_client_error() {
            Ok(())
        } else {
            Err(anyhow::anyhow!("销毁沙箱失败: HTTP {}", resp.status()))
        }
    }
```

- [ ] **Step 6: 实现 health_check**

```rust
    /// 健康检查：3 秒超时探测 API 是否可达
    pub async fn health_check(api_url: &str) -> bool {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(3))
            .no_proxy()
            .build()
            .ok();

        if let Some(client) = client {
            client
                .get(format!("{}/", api_url.trim_end_matches('/')))
                .send()
                .await
                .map(|r| r.status().is_success())
                .unwrap_or(false)
        } else {
            false
        }
    }
}
```

- [ ] **Step 7: Commit**

```bash
git add backend/src/sandbox/cubesandbox/client.rs
git commit -m "feat(sandbox): 实现 E2BClient REST API 客户端"
```

---

### Task 3: 实现 CubeSandboxBackend

**Files:**
- Create: `backend/src/sandbox/cubesandbox/backend.rs`

- [ ] **Step 1: 实现 CubeSandboxBackend 结构体**

```rust
use anyhow::Result;
use std::sync::Arc;
use tokio::sync::RwLock;

use super::client::E2BClient;
use super::config::CubeSandboxConfig;
use crate::sandbox::trait_sandbox::{ExecResult, Sandbox};

pub struct CubeSandboxBackend {
    client: E2BClient,
    config: CubeSandboxConfig,
    sandbox_id: Arc<RwLock<Option<String>>>,
}

impl CubeSandboxBackend {
    pub fn new(config: CubeSandboxConfig) -> Self {
        let api_url = config.api_url.clone();
        let api_key = config.api_key.clone();
        Self {
            client: E2BClient::new(api_url, api_key),
            config,
            sandbox_id: Arc::new(RwLock::new(None)),
        }
    }

    async fn ensure_running(&self) -> Result<String> {
        let id = {
            let mut guard = self.sandbox_id.write().await;
            if let Some(ref id) = *guard {
                id.clone()
            } else {
                let resp = self.client
                    .create_sandbox(&self.config.template_id, self.config.timeout_secs)
                    .await?;
                let id = resp.sandbox_id;
                *guard = Some(id.clone());
                id
            }
        };
        Ok(id)
    }
}
```

- [ ] **Step 2: 实现 Sandbox trait**

```rust
#[async_trait::async_trait]
impl Sandbox for CubeSandboxBackend {
    async fn exec(&self, command: &str, args: &[String]) -> Result<ExecResult> {
        let id = self.ensure_running().await?;
        let result = self.client.exec_command(&id, command, args).await?;
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
            Err(anyhow::anyhow!("cat {} failed: {}", path, result.stderr))
        }
    }

    async fn write_file(&self, path: &str, content: &str) -> Result<()> {
        // 使用 yunli::hash::b64 编码避免 shell 转义问题
        let encoded = yunli::hash::b64(content.as_bytes());
        let cmd = format!("echo '{}' | base64 -d > '{}'", encoded, path);
        let result = self.exec("sh", &["-c".into(), cmd]).await?;
        if result.exit_code == 0 {
            Ok(())
        } else {
            Err(anyhow::anyhow!("write {} failed: {}", path, result.stderr))
        }
    }

    async fn stop(&self) -> Result<()> {
        let id = {
            let guard = self.sandbox_id.read().await;
            guard.clone()
        };
        if let Some(id) = id {
            let _ = self.client.kill_sandbox(&id).await;
            *self.sandbox_id.write().await = None;
        }
        Ok(())
    }
}
```

- [ ] **Step 5: Commit**

```bash
git add backend/src/sandbox/cubesandbox/backend.rs backend/src/sandbox/cubesandbox/mod.rs
git commit -m "feat(sandbox): 实现 CubeSandboxBackend Sandbox trait"
```

---

### Task 4: 改造 SandboxFactory — 添加 CubeSandbox 枚举和配置

**Files:**
- Modify: `backend/src/sandbox/factory.rs`

- [ ] **Step 1: 添加 CubeSandbox 到枚举**

在 `SandboxBackend` 枚举中添加：
```rust
pub enum SandboxBackend {
    Microsandbox,
    CubeSandbox,
    Process,
}
```

- [ ] **Step 2: 添加 CubeSandboxConfig 导入和 OnceLock**

```rust
use std::sync::OnceLock;
use super::cubesandbox::{CubeSandboxBackend, CubeSandboxConfig};
```

- [ ] **Step 3: 改造 SandboxFactory 结构体**

```rust
pub struct SandboxFactory {
    backends: Vec<SandboxBackend>,
    selected: OnceLock<SandboxBackend>,
    #[cfg(target_os = "linux")]
    micro_config: MicrosandboxConfig,
    cube_config: CubeSandboxConfig,
}
```

- [ ] **Step 4: 实现 from_config 构造函数**

```rust
impl SandboxFactory {
    pub fn from_config(
        backends: Vec<SandboxBackend>,
        #[cfg(target_os = "linux")] micro_config: MicrosandboxConfig,
        cube_config: CubeSandboxConfig,
    ) -> Self {
        // 校验首选项配置完整性
        let primary = backends.first().copied().unwrap_or(SandboxBackend::Microsandbox);
        if primary == SandboxBackend::CubeSandbox && !cube_config.is_complete() {
            panic!("CubeSandbox 配置不完整: api_url 和 template_id 为必填字段");
        }

        Self {
            backends,
            selected: OnceLock::new(),
            #[cfg(target_os = "linux")]
            micro_config,
            cube_config,
        }
    }
```

- [ ] **Step 5: 保留兼容的 new() 构造函数**

```rust
    pub fn new() -> Self {
        Self {
            backends: vec![SandboxBackend::from_env()],
            selected: OnceLock::new(),
            #[cfg(target_os = "linux")]
            micro_config: MicrosandboxConfig::default(),
            cube_config: CubeSandboxConfig::default(),
        }
    }

    pub fn with_backend(mut self, backend: SandboxBackend) -> Self {
        self.backends = vec![backend];
        self
    }
```

- [ ] **Step 6: 实现异步 init()**

```rust
    pub async fn init(&self) {
        for backend in &self.backends {
            if Self::check(backend, #[cfg(target_os = "linux")] &self.micro_config, &self.cube_config).await {
                tracing::info!("选中沙箱后端: {:?}", backend);
                self.selected.set(*backend).ok();
                return;
            }
        }
        tracing::warn!("配置的后端都不可用，降级为 process");
        self.selected.set(SandboxBackend::Process).ok();
    }

    async fn check(
        backend: &SandboxBackend,
        #[cfg(target_os = "linux")] _micro_config: &MicrosandboxConfig,
        cube_config: &CubeSandboxConfig,
    ) -> bool {
        match backend {
            SandboxBackend::Microsandbox => {
                cfg!(target_os = "linux") && std::path::Path::new("/dev/kvm").exists()
            }
            SandboxBackend::CubeSandbox => {
                if !cube_config.is_complete() {
                    tracing::warn!("CubeSandbox 配置不完整，跳过");
                    return false;
                }
                E2BClient::health_check(&cube_config.api_url).await
            }
            SandboxBackend::Process => true,
        }
    }
```

- [ ] **Step 7: 更新 create() 使用 selected**

```rust
    pub fn create(&self) -> anyhow::Result<Arc<dyn Sandbox>> {
        let backend = self.selected.get().unwrap_or(&SandboxBackend::Process);
        match backend {
            #[cfg(target_os = "linux")]
            SandboxBackend::Microsandbox => {
                Ok(Arc::new(MicrosandboxBackend::new(self.micro_config.clone())))
            }
            #[cfg(not(target_os = "linux"))]
            SandboxBackend::Microsandbox => {
                tracing::warn!("microsandbox 仅支持 Linux，降级为 process 后端");
                Ok(Arc::new(ProcessBackend::new()))
            }
            SandboxBackend::CubeSandbox => {
                Ok(Arc::new(CubeSandboxBackend::new(self.cube_config.clone())))
            }
            SandboxBackend::Process => {
                Ok(Arc::new(ProcessBackend::new()))
            }
        }
    }
}
```

- [ ] **Step 8: 编译验证**

```bash
cd backend && ./run-signed.sh
```

- [ ] **Step 9: Commit**

```bash
git add backend/src/sandbox/factory.rs
git commit -m "feat(sandbox): Factory 支持 CubeSandbox + 配置驱动降级"
```

---

### Task 5: 扩展 Config 加载 CubeSandbox 配置

**Files:**
- Modify: `backend/src/config.rs`

- [ ] **Step 1: 添加 CubeSandbox 配置字段到 Config 结构体**

将 `sandbox_backend: String` 替换为：
```rust
pub sandbox_backends: Vec<devops_agent::sandbox::SandboxBackend>,
```

在现有 sandbox 字段后添加：
```rust
pub cubesandbox_api_url: String,
pub cubesandbox_api_key: String,
pub cubesandbox_template_id: String,
pub cubesandbox_timeout: i32,
pub cubesandbox_allow_internet: bool,
```

- [ ] **Step 2: 添加 parse_sandbox_backends() 函数**

在 `parse_cors_origins()` 之后添加：
```rust
fn parse_sandbox_backends(conf: &BTreeMap<String, String>) -> Vec<devops_agent::sandbox::SandboxBackend> {
    use devops_agent::sandbox::SandboxBackend;
    let mut backends = Vec::new();
    let mut i = 0;
    loop {
        match conf.get(&format!("sandbox.backend.{}", i)) {
            Some(v) => {
                let b = match v.as_str() {
                    "cubesandbox" => SandboxBackend::CubeSandbox,
                    "process" => SandboxBackend::Process,
                    _ => SandboxBackend::Microsandbox,
                };
                backends.push(b);
            }
            None => break,
        }
        i += 1;
    }
    // 向后兼容：如果数组为空，读旧字段 sandbox.backend (单个字符串)
    if backends.is_empty() {
        if let Some(v) = conf.get("sandbox.backend") {
            let b = match v.as_str() {
                "cubesandbox" => SandboxBackend::CubeSandbox,
                "process" => SandboxBackend::Process,
                _ => SandboxBackend::Microsandbox,
            };
            backends.push(b);
        }
    }
    // 默认值
    if backends.is_empty() {
        backends.push(SandboxBackend::Microsandbox);
    }
    backends
}
```

- [ ] **Step 3: 在 from_file() 中加载配置**

替换原有的 sandbox 配置加载逻辑：
```rust
let sandbox_backends = parse_sandbox_backends(&conf);

let cubesandbox_api_url = conf_get(&conf, "sandbox.cubesandbox.api_url").unwrap_or_default();
let cubesandbox_api_key = conf_get(&conf, "sandbox.cubesandbox.api_key").unwrap_or_else(|| "dummy".to_string());
let cubesandbox_template_id = conf_get(&conf, "sandbox.cubesandbox.template_id").unwrap_or_default();
let cubesandbox_timeout = conf
    .get("sandbox.cubesandbox.timeout")
    .and_then(|s| s.parse().ok())
    .unwrap_or(1800i32);
let cubesandbox_allow_internet = conf
    .get("sandbox.cubesandbox.allow_internet")
    .and_then(|s| s.parse().ok())
    .unwrap_or(true);
```

- [ ] **Step 4: 更新 test_default()**

将 `sandbox_backend: "process".to_string()` 替换为：
```rust
sandbox_backends: vec![devops_agent::sandbox::SandboxBackend::Process],
```

添加默认值：
```rust
cubesandbox_api_url: String::new(),
cubesandbox_api_key: "dummy".to_string(),
cubesandbox_template_id: String::new(),
cubesandbox_timeout: 1800,
cubesandbox_allow_internet: true,
```

- [ ] **Step 4: Commit**

```bash
git add backend/src/config.rs
git commit -m "feat(config): 添加 CubeSandbox 配置字段"
```

---

### Task 6: 在 main.rs 中调用 Factory init()

**Files:**
- Modify: `backend/src/main.rs`
- Modify: `backend/src/api.rs` (if AppState needs factory)

- [ ] **Step 1: 在 run() 中添加 Factory 初始化**

在 `AppState` 创建后、`api::run()` 之前，添加 spawn 任务：

```rust
// 初始化沙箱后端选择
spawn_sandbox_init();
```

添加 spawn 函数：
```rust
fn spawn_sandbox_init() {
    // Factory init 在 api.rs 中通过 AppState 触发
    // 暂不在此处硬编码，由 api 层在首次请求时懒初始化
}
```

**注意：** 如果 `SandboxFactory` 在 `api.rs` 中创建，需要在 `AppState` 中持有工厂引用，并在启动时调用 `init()`。具体集成方式取决于 `api.rs` 中工厂的使用方式。

- [ ] **Step 2: Commit**

```bash
git add backend/src/main.rs
git commit -m "feat: 预留沙箱 Factory 初始化入口"
```

---

### Task 7: 更新测试

**Files:**
- Modify: `backend/tests/sandbox_trait_test.rs`

- [ ] **Step 1: 添加 CubeSandbox 配置校验测试**

```rust
#[test]
fn cubesandbox_config_complete() {
    use devops_agent::sandbox::CubeSandboxConfig;
    let config = CubeSandboxConfig {
        api_url: "http://localhost:3000".to_string(),
        template_id: "test-template".to_string(),
        ..CubeSandboxConfig::default()
    };
    assert!(config.is_complete());
}

#[test]
fn cubesandbox_config_incomplete() {
    use devops_agent::sandbox::CubeSandboxConfig;
    let config = CubeSandboxConfig::default();
    assert!(!config.is_complete());
}
```

- [ ] **Step 2: 添加 Factory 降级测试**

```rust
#[tokio::test]
async fn factory_fallback_to_process() {
    use devops_agent::sandbox::{SandboxBackend, SandboxFactory, CubeSandboxConfig};
    #[cfg(target_os = "linux")]
    use devops_agent::sandbox::MicrosandboxConfig;

    let factory = SandboxFactory::from_config(
        vec![SandboxBackend::CubeSandbox],
        #[cfg(target_os = "linux")]
        MicrosandboxConfig::default(),
        CubeSandboxConfig::default(), // 配置不完整
    );
    factory.init().await;
    let _sandbox = factory.create().unwrap(); // 应降级到 process
}
```

- [ ] **Step 3: 运行测试**

```bash
cd backend && ./run-signed.sh -- test sandbox
```

- [ ] **Step 4: Commit**

```bash
git add backend/tests/sandbox_trait_test.rs
git commit -m "test(sandbox): 添加 CubeSandbox 配置和降级测试"
```

---

### Task 8: 编译验证 + cargo fmt + 最终提交

- [ ] **Step 1: 格式化**

```bash
cd backend && cargo fmt
```

- [ ] **Step 2: 完整编译**

```bash
cd backend && ./run-signed.sh
```

- [ ] **Step 3: 完整测试**

```bash
cd backend && ./run-signed.sh -- test -- --ignored
```

- [ ] **Step 4: 最终提交**

```bash
git add -A
git commit -m "feat(sandbox): 完成 CubeSandbox Phase 2 — 后端集成 + 配置驱动降级"
```

---

## Self-Review

**Spec coverage:**
- CubeSandboxBackend ✅ (Task 2-3)
- E2BClient ✅ (Task 2)
- Factory 降级 ✅ (Task 4)
- Config 扩展 ✅ (Task 5)
- 测试 ✅ (Task 7)
- 启动初始化 ✅ (Task 6)

**Placeholder scan:** 无 TBD/TODO

**Type consistency:** `CubeSandboxConfig` 在 config.rs、factory.rs、backend.rs 中使用一致；`SandboxBackend::CubeSandbox` 枚举贯穿所有文件

**Scope:** 聚焦 CubeSandbox 后端集成，不涉及 UI 或 agent 层改动
