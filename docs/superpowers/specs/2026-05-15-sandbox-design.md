# 分层沙箱架构设计

> **日期:** 2026-05-15
> **状态:** 待评审
> **关联:** [[2026-05-03-provider-abstraction-design]] (类似的抽象模式)

## 概述

为 devops-agent 引入硬件级隔离沙箱，替代现有进程级安全措施。采用分层架构，通过统一 Trait 抽象支持多种后端，根据运行环境自动选择。

## 需求

- **代码执行隔离**: Claude Agent 生成的代码需要在隔离环境中执行
- **命令执行隔离**: Bash 工具执行的命令需要硬件级隔离
- **跨平台**: 支持 Linux (KVM) 和 macOS (Apple Silicon)
- **分层部署**: 开发环境嵌入式（零运维），生产环境可切换为集群服务

## 方案对比

| 维度 | microsandbox | CubeSandbox | 现有进程沙箱 |
|---|---|---|---|
| 隔离级别 | 极高 (microVM 独立内核) | 极高 (KVM microVM + eBPF) | 低 (进程级) |
| 跨平台 | Linux + macOS | 仅 Linux x86_64 | 跨平台 |
| 部署形态 | 嵌入式 SDK，无守护进程 | 集群服务栈 | 内嵌 |
| 启动速度 | <100ms | <60ms | 即时 |
| 集成难度 | 低 (`cargo add`) | 高 (部署服务栈) | 无 |
| 成熟度 | Beta (v0.4.6) | 生产级 | 已实现 |

## 架构

```
┌─────────────────────────────────┐
│      devops-agent backend       │
│                                 │
│  ┌─────────────────────────┐    │
│  │    SandboxTrait         │    │
│  │  exec / upload /        │    │
│  │  download / read /      │    │
│  │  write / stop           │    │
│  └─────────┬───────────────┘    │
│            │                    │
│  ┌─────────┴──────────┐        │
│  │  SandboxFactory     │        │
│  │  (env-based select) │        │
│  └──────┬──────┬──────┘        │
│         │      │                │
│  ┌──────┴──┐  ┌┴──────────┐    │
│  │MicroVM  │  │CubeSandbox │    │
│  │Backend  │  │Backend     │    │
│  │(本地)   │  │(E2B API)   │    │
│  └─────────┘  └────────────┘    │
│                                 │
│  tools/builtin/bash.rs ─────────┘
│  agent/claude_code.rs
│  agent/claude_analyze.rs
└─────────────────────────────────┘
```

## 接口设计

```rust
#[async_trait]
pub trait Sandbox: Send + Sync {
    async fn exec(&self, command: &str, args: &[String]) -> Result<ExecResult>;
    async fn upload(&self, local_path: &str, remote_path: &str) -> Result<()>;
    async fn download(&self, remote_path: &str, local_path: &str) -> Result<()>;
    async fn read_file(&self, path: &str) -> Result<String>;
    async fn write_file(&self, path: &str, content: &str) -> Result<()>;
    async fn stop(&self) -> Result<()>;
}

pub struct ExecResult {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
}
```

## 后端选择

环境变量 `SANDBOX_BACKEND` 控制：
- `microsandbox` — 本地 microVM（开发/macOS 默认）
- `cubesandbox` — CubeSandbox 服务（Linux 生产）
- `process` — 降级到进程级沙箱

自动检测：存在 `E2B_API_URL` → CubeSandbox；macOS/Linux → microsandbox；其他 → process

## 文件结构

```
backend/src/sandbox/
├── mod.rs                    # 重写：导出 Trait + Factory
├── trait.rs                  # 新增：SandboxTrait + ExecResult
├── factory.rs                # 新增：SandboxBackend 枚举 + 工厂
├── microsandbox/
│   ├── mod.rs
│   └── backend.rs            # MicrosandboxBackend
├── cubesandbox/
│   ├── mod.rs
│   └── backend.rs            # CubeSandboxBackend
├── path_check.rs             # 保留
├── fs_isolation.rs           # 保留
├── process_sandbox.rs        # 保留（降级）
└── network_whitelist.rs      # 保留
```

## 配置

```toml
[sandbox]
backend = "microsandbox"
timeout_secs = 30
max_output_bytes = 1048576

[sandbox.microsandbox]
image = "debian"
cpus = 1
memory = 512

[sandbox.cubesandbox]
api_url = "http://127.0.0.1:3000"
api_key = ""
template_id = ""
```

## 实施阶段

### Phase 1 — 抽象层 + microsandbox
1. 定义 `SandboxTrait` 接口
2. 实现 `MicrosandboxBackend`
3. 实现 `SandboxFactory`
4. 重写 `mod.rs`
5. 集成到 `bash.rs`

### Phase 2 — CubeSandbox
1. 实现 `CubeSandboxBackend`
2. 完善工厂切换逻辑

### Phase 3 — 集成打磨
1. 集成到 Claude Agent 步骤
2. 端到端测试
3. 性能调优

## 风险

- **microsandbox Beta 阶段**: 可能存在 breaking change，通过 `Cargo.toml` 版本锁定缓解
- **KVM 依赖**: Linux 需要 KVM 支持，macOS 需要 Apple Silicon。不支持时降级到进程沙箱
- **性能开销**: microVM 启动有 ~100ms 延迟，对高频调用场景可能需要沙箱复用
