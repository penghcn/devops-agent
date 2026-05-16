# CubeSandbox 后端集成设计

> **日期:** 2026-05-16
> **状态:** 待评审
> **关联:** [[2026-05-15-sandbox-design]] (分层沙箱架构设计), [[2026-05-15-sandbox-phase1]] (Phase 1 实现计划)

## 概述

在现有 `Sandbox` trait 抽象层上添加 `CubeSandboxBackend` 实现，支持 CubeSandbox 自托管服务（E2B 兼容 REST API）。同时改造 `SandboxFactory`，支持配置驱动的后端优先级列表和启动时自动降级检测。

## 需求

- **CubeSandbox 后端支持**: 通过 E2B 兼容 REST API 与自托管 CubeSandbox 服务通信
- **配置驱动降级**: 按 toml 配置的优先级列表依次尝试后端，自动选择第一个可用的
- **轻量可用性检测**: 启动时快速检测各后端可用性，不阻塞正常流程
- **向后兼容**: 现有 `from_factory()` 同步接口不变，调用方无感知

## 架构

### 后端选择流程

```
config.toml: sandbox.backend = ["microsandbox", "cubesandbox"]
                    ↓
          SandboxFactory::init()  [启动时异步执行一次]
                    ↓
          ┌─────────┴─────────────────────┐
   按配置顺序检测可用性                     │
   ┌──────┤                                │
   │      ↓                                │
   │  microsandbox?                        │
   │  (cfg(linux) && /dev/kvm 存在)        │
   │      │否                              │
   │      ↓                                │
   │  cubesandbox?                         │
   │  (配置完整 && HTTP 健康检查 3s 超时)   │
   │      │否                              │
   │      ↓                                │
   └────→ process (隐式兜底，始终可用)      │
                                            │
   配置首选项不完整 ──────────→ panic 中断启动 │
```

### 模块结构

```
backend/src/sandbox/
├── cubesandbox/              # 新增
│   ├── mod.rs                # 导出 CubeSandboxBackend, CubeSandboxConfig
│   ├── backend.rs            # CubeSandboxBackend: Sandbox trait 实现
│   └── client.rs             # E2BClient: REST API 封装
├── factory.rs                # 修改: 降级逻辑 + CubeSandbox 支持
├── mod.rs                    # 修改: 导出 cubesandbox 模块
└── (现有文件保持不变)
```

## CubeSandboxBackend 实现

### E2B 兼容 API 端点映射

| Sandbox trait 方法 | E2B API 端点 | 说明 |
|---|---|---|
| `exec(cmd, args)` | envd gRPC → `commands.run()` | 通过 CubeProxy 路由到沙箱 envd 执行命令 |
| `read_file(path)` | envd gRPC → `files.read()` | 通过 CubeProxy 路由到沙箱 envd |
| `write_file(path, content)` | envd gRPC → `files.write()` | 通过 CubeProxy 路由到沙箱 envd |
| `stop()` | `DELETE /sandboxes/{id}` | 控制平面 REST API 销毁沙箱 |

### 沙箱生命周期

与 `MicrosandboxBackend` 一致的懒创建 + 持久化模式：

1. **懒创建**: 首次 `exec()`/`read_file()`/`write_file()` 时创建远程沙箱
2. **持久化**: 同一 `CubeSandboxBackend` 实例复用沙箱
3. **自动过期**: 创建时设置 TTL（默认 30 分钟），超时后服务端自动 kill
4. **显式销毁**: `stop()` 调用 `DELETE /sandboxes/{id}` 立即清理
5. **安全兜底**: 即使进程崩溃未调用 `stop()`，TTL 到期后服务端自动回收

### 内部状态

```rust
pub struct CubeSandboxBackend {
    client: E2BClient,
    config: CubeSandboxConfig,
    /// 懒初始化: 首次使用时创建沙箱
    sandbox_id: Arc<parking_lot::RwLock<Option<String>>>,
}
```

### E2BClient

纯 `reqwest` HTTP 客户端，无额外 crate 依赖。

```rust
pub struct E2BClient {
    http: reqwest::Client,
    api_url: Url,
    api_key: String,
}

impl E2BClient {
    /// 创建沙箱
    async fn create_sandbox(&self, template_id: &str, timeout_secs: i32) -> Result<SandboxCreateResponse>;

    /// 执行命令
    async fn exec_command(&self, sandbox_id: &str, command: &str, args: &[String]) -> Result<CommandResult>;

    /// 读取文件（通过 envd filesystem gRPC）
    async fn read_file(&self, sandbox_id: &str, path: &str) -> Result<String>;

    /// 写入文件（通过 envd filesystem gRPC）
    async fn write_file(&self, sandbox_id: &str, path: &str, content: &[u8]) -> Result<()>;

    /// 销毁沙箱
    async fn kill_sandbox(&self, sandbox_id: &str) -> Result<()>;

    /// 健康检查（用于可用性检测）
    async fn health_check(&self) -> Result<()>;
}
```

### API 请求/响应模型

**创建沙箱:**
```
POST {api_url}/sandboxes
Headers: Authorization: Bearer {api_key}
Body: { "templateID": "{template_id}", "timeout": {timeout_secs} }
Response: { "sandboxID": "...", "templateID": "...", ... }
```

**执行命令:**
```
E2B SDK 的 commands.run() 通过 envd gRPC 执行，经 CubeProxy 路由。
控制平面 (CubeAPI) 不直接执行命令。

简化方案：因为 envd gRPC 需要 CubeProxy DNS 路由（{port}-{sandbox_id}.cube.app），
自托管环境下配置复杂。Phase 2 先用控制平面 REST API 执行命令：
- POST /sandboxes/{id}/commands (如果 CubeAPI 支持)
- 或者通过 exec 间接调用

若 CubeAPI 不支持命令执行 REST 端点，则通过创建沙箱后使用
sh -lc "command" 方式执行。详见实现计划。
```

**读取/写入文件:**
```
envd filesystem gRPC 端点，经 CubeProxy 路由。
Phase 2 简化：通过 exec("cat"/"tee") 间接实现。
```

**销毁沙箱:**
```
DELETE {api_url}/sandboxes/{sandbox_id}
```

## 配置

### config.toml 扩展

```toml
# 后端优先级列表，按顺序尝试
sandbox.backend = ["microsandbox", "cubesandbox"]
sandbox.timeout_secs = 30

# microsandbox 配置（已有，保持不变）
sandbox.microsandbox.image = "debian"
sandbox.microsandbox.cpus = 1
sandbox.microsandbox.memory = 512

# CubeSandbox 配置（新增）
sandbox.cubesandbox.api_url = "http://192.168.1.100:3000"
sandbox.cubesandbox.api_key = "dummy"
sandbox.cubesandbox.template_id = ""
sandbox.cubesandbox.timeout = 1800
sandbox.cubesandbox.allow_internet = true
```

### 配置字段说明

| 字段 | 类型 | 默认值 | 说明 |
|---|---|---|---|
| `sandbox.backend` | `[]string` | `["microsandbox"]` | 后端优先级列表 |
| `sandbox.cubesandbox.api_url` | `string` | `""` | CubeSandbox 服务地址 |
| `sandbox.cubesandbox.api_key` | `string` | `"dummy"` | API 密钥（自托管可用 dummy） |
| `sandbox.cubesandbox.template_id` | `string` | `""` | 沙箱模板 ID（必填） |
| `sandbox.cubesandbox.timeout` | `int` | `1800` | 沙箱 TTL（秒） |
| `sandbox.cubesandbox.allow_internet` | `bool` | `true` | 是否允许沙箱出网 |

### 配置校验规则

1. **首选项配置不完整 → panic**: 如果 `sandbox.backend[0]` 是 `cubesandbox`，则 `api_url` 和 `template_id` 必须非空
2. **非首选项配置不完整 → warn 跳过**: 列表后续项配置缺失时记录 warn 日志，检测阶段跳过
3. **`api_url` 为空** → 视为配置不完整
4. **`template_id` 为空** → 视为配置不完整

## Factory 改造

### 启动时异步初始化

```rust
pub struct SandboxFactory {
    /// 配置的后端优先级列表
    backends: Vec<SandboxBackend>,
    /// 启动时检测后选中的后端
    selected: OnceLock<SandboxBackend>,
    micro_config: MicrosandboxConfig,
    cube_config: CubeSandboxConfig,
}

impl SandboxFactory {
    /// 从配置构建工厂（同步，不检测）
    pub fn from_config(
        backends: Vec<SandboxBackend>,
        micro_config: MicrosandboxConfig,
        cube_config: CubeSandboxConfig,
    ) -> Self { ... }

    /// 异步初始化：按优先级检测后端可用性，缓存选择结果
    /// 应在应用启动时调用一次
    pub async fn init(&self) {
        for backend in &self.backends {
            if Self::check(backend, &self.micro_config, &self.cube_config).await {
                tracing::info!("选中沙箱后端: {:?}", backend);
                self.selected.set(*backend).ok();
                return;
            }
        }
        tracing::warn!("配置的后端都不可用，降级为 process");
        self.selected.set(SandboxBackend::Process).ok();
    }

    /// 创建沙箱实例（同步，使用已选中的后端）
    pub fn create(&self) -> Result<Arc<dyn Sandbox>> {
        let backend = self.selected.get().unwrap_or(&SandboxBackend::Process);
        match backend {
            SandboxBackend::Microsandbox => { ... }
            SandboxBackend::CubeSandbox => { ... }
            SandboxBackend::Process => Ok(Arc::new(ProcessBackend::new())),
        }
    }

    /// 轻量可用性检测
    async fn check(backend: &SandboxBackend, ...) -> bool { ... }
}
```

### 可用性检测实现

```rust
async fn check(backend: &SandboxBackend, micro_cfg: &MicrosandboxConfig, cube_cfg: &CubeSandboxConfig) -> bool {
    match backend {
        SandboxBackend::Microsandbox => {
            cfg!(target_os = "linux") && std::path::Path::new("/dev/kvm").exists()
        }
        SandboxBackend::CubeSandbox => {
            // 配置完整性检查
            if cube_cfg.api_url.is_empty() || cube_cfg.template_id.is_empty() {
                tracing::warn!("CubeSandbox 配置不完整，跳过");
                return false;
            }
            // HTTP 健康检查，3 秒超时
            E2BClient::health_check_timeout(&cube_cfg.api_url, Duration::from_secs(3)).await
                .map(|r| r.status().is_success())
                .unwrap_or(false)
        }
        SandboxBackend::Process => true,
    }
}
```

### 配置首选项校验

在 `SandboxFactory::from_config()` 中：

```rust
// 校验首选项配置完整性
let primary = backends.first().copied().unwrap_or(SandboxBackend::Microsandbox);
match primary {
    SandboxBackend::CubeSandbox => {
        if cube_config.api_url.is_empty() || cube_config.template_id.is_empty() {
            panic!("CubeSandbox 配置不完整: api_url 和 template_id 为必填字段");
        }
    }
    _ => {} // microsandbox 和 process 无需额外配置
}
```

## 错误处理

| 场景 | 行为 |
|---|---|
| 启动时首选项配置不完整 | panic，中断启动 |
| 启动时非首选项配置不完整 | warn 日志，跳过检测 |
| CubeSandbox HTTP 健康检查超时 (3s) | 视为不可用，尝试下一个 |
| CubeSandbox 创建沙箱失败 | 返回 anyhow::Error，由调用方处理 |
| 沙箱执行命令超时 | 返回错误，不重试 |
| 网络断开（已创建沙箱） | 命令执行返回错误，不自动重连 |

## 测试策略

### 单元测试

1. **CubeSandboxBackend 配置校验**: 测试配置完整/不完整的各种组合
2. **E2BClient 请求构建**: 测试 API URL 拼接、Header 设置（mock reqwest）
3. **Factory 降级逻辑**: 测试各种后端组合的降级路径
4. **可用性检测**: 测试 microsandbox KVM 检测、CubeSandbox 配置检查

### 集成测试

1. **CubeSandbox E2E**: 连接真实 CubeSandbox 实例，测试创建 → 执行 → 销毁全流程（需 CI 环境）
2. **Factory 启动选择**: 验证配置列表优先级和降级行为

## 变更文件清单

| 文件 | 操作 | 说明 |
|---|---|---|
| `backend/src/sandbox/cubesandbox/mod.rs` | 新增 | 模块导出 |
| `backend/src/sandbox/cubesandbox/backend.rs` | 新增 | CubeSandboxBackend 实现 |
| `backend/src/sandbox/cubesandbox/client.rs` | 新增 | E2B REST API 客户端 |
| `backend/src/sandbox/factory.rs` | 修改 | 降级检测 + CubeSandbox 支持 |
| `backend/src/sandbox/mod.rs` | 修改 | 导出 cubesandbox 模块 |
| `backend/src/config.rs` | 修改 | 新增 cubesandbox 配置字段 |
| `backend/tests/sandbox_trait_test.rs` | 修改 | 新增降级测试 |

## 与 Phase 1 的关系

- **复用**: `Sandbox` trait、`ProcessBackend`、`SandboxFactory` 框架
- **扩展**: `SandboxBackend` 枚举新增 `CubeSandbox`；Factory 增加异步 `init()`
- **兼容**: `from_factory()` 同步接口不变；`create()` 仍返回 `Arc<dyn Sandbox>`

## 风险

- **CubeSandbox 部署复杂度**: 需要独立的 Linux 服务器 + KVM + Docker。用户需先部署服务才能使用
- **E2B API 兼容性**: CubeSandbox 声称 E2B 兼容，但命令执行通过 envd gRPC（CubeProxy 路由）而非控制平面 REST。Phase 2 需确认 CubeAPI 是否有命令执行 REST 端点，否则用 exec 间接方案
- **网络依赖**: CubeSandbox 是远程服务，网络延迟和故障会影响命令执行。3 秒超时 + 降级机制缓解
