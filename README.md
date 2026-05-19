
## 架构

### 后端模块

```
backend/src/
├── main.rs                    # Axum Web 服务入口
├── lib.rs                     # 库入口
├── config.rs                  # 配置管理
├── api.rs                     # HTTP 接口层（路由 + 请求/响应处理）
│
├── harness/                   # 编排框架
│   ├── mod.rs
│   ├── hook.rs                # Hook trait + 钩子点枚举
│   ├── orchestrator.rs        # 步骤链编排器
│   ├── session.rs             # 会话生命周期
│   ├── token_hook.rs          # Token 预算追踪 Hook
│   └── memory_hook.rs         # 记忆保存 Hook
│
├── memory/                    # 记忆系统
│   ├── mod.rs                 # MemoryEntry + MemoryType
│   ├── short_term.rs          # 环形缓冲区，200 条
│   ├── long_term.rs           # SQLite 持久化 + 关键词索引
│   └── store.rs               # SQLite 初始化 + 迁移
│
├── token/                     # Token 管理
│   ├── mod.rs
│   ├── tracker.rs             # 实时计数 + 预算 + 轮次计数
│   ├── window.rs              # 四层上下文（System/Compressed/Structured/Linear）
│   └── summarizer.rs          # 渐进式三阶段 LLM 压缩
│
├── security/                  # 权限控制
│   ├── mod.rs
│   ├── roles.rs               # 角色 + 工具请求模型
│   ├── policy.rs              # 策略引擎：ALLOW/DENY/PROMPT
│   └── audit.rs               # 操作审计日志
│
├── sandbox/                   # 沙箱隔离（多后端可切换架构）
│   ├── mod.rs
│   ├── trait_sandbox.rs       # Sandbox trait + ExecResult 统一接口
│   ├── factory.rs             # SandboxFactory：配置驱动降级 + 异步后端检测
│   │
│   ├── cubesandbox/           # CubeSandbox 后端（E2B 兼容，双平面架构）
│   │   ├── mod.rs
│   │   ├── config.rs          # CubeSandboxConfig：API 地址、模板、envd URL 模板
│   │   ├── client.rs          # ControlPlaneClient（REST 控制平面）+ EnvdClient（Connect RPC 数据平面）
│   │   └── backend.rs         # CubeSandboxBackend：懒初始化 + 持久化 + Sandbox trait 实现
│   │
│   ├── process_backend.rs     # ProcessBackend：本地进程沙箱（最终降级方案）
│   ├── microsandbox_backend.rs # MicrosandboxBackend：基于 microsandbox SDK（仅 Linux）
│   ├── process_sandbox.rs     # 进程限制 + 环境净化（底层实现）
│   ├── path_check.rs          # 路径穿越检测
│   ├── fs_isolation.rs        # 文件系统隔离 + 选择性挂载
│   └── network_whitelist.rs   # 网络白名单
│
├── tools/                     # 工具集
│   ├── mod.rs
│   ├── builtin/               # 内置工具
│   │   ├── mod.rs
│   │   ├── read.rs            # 安全文件读取
│   │   ├── write.rs           # 文件写入
│   │   ├── bash.rs            # 命令执行（白名单校验）
│   │   └── git.rs             # git 操作
│   ├── jenkins.rs             # Jenkins API 封装
│   ├── jenkins_cache.rs       # 构建缓存
│   └── gitlab.rs              # GitLab API 封装
│
├── llm/                       # LLM 提供商抽象
│   ├── mod.rs                 # LlmProvider trait + Message/ChatRequest 等类型
│   ├── router.rs              # ModelRouter：L1/L2 任务分类 + provider 路由
│   ├── structured_output.rs   # Schema 强约束输出
│   └── provider/              # Provider 实现（适配器模式）
│       ├── mod.rs
│       ├── base.rs            # BaseConfig + ProviderAdapter trait + GenericProvider<T>
│       ├── config.rs          # ProviderConfig + LlmConfigStore + build_model_router()
│       ├── client.rs          # 共享 HTTP 调用逻辑
│       ├── openai.rs          # OpenAIAdapter + OpenAIProvider 封装
│       └── anthropic.rs       # AnthropicAdapter + AnthropicProvider 封装
│
├── agent/                     # 意图识别 + 步骤编排
│   ├── mod.rs                 # Agent 入口
│   ├── intent.rs              # 意图数据结构
│   ├── router.rs              # 意图路由
│   ├── chain_mapping.rs       # 意图 → 步骤链映射
│   ├── step.rs                # Step trait + StepChain 执行器
│   ├── claude.rs              # Claude 交互 Step
│   └── steps/                 # 业务 Step
│       ├── mod.rs
│       ├── job_validate.rs    # Job 参数校验
│       ├── jenkins_trigger.rs # 触发构建
│       ├── jenkins_wait.rs    # 等待构建完成
│       ├── jenkins_status.rs  # 查询构建状态
│       ├── jenkins_log.rs     # 拉取构建日志
│       ├── claude_analyze.rs  # Claude 分析构建结果
│       └── claude_code.rs     # Claude 代码生成
│
└── frontend/                  # Vue 3.5 + TS + Vite 8 + Tailwind CSS 4 前端（SSE 流式推送）
```

### 模块关系

```
用户请求
  └── api.rs (HTTP 路由层)
        └── process_request_with_store()
              ├── Provider Config → build_model_router() → ModelRouter
              │     ├── BaseConfig + ProviderAdapter trait + GenericProvider<T>
              │     ├── OpenAIProvider (GenericProvider<OpenAIAdapter>)
              │     └── AnthropicProvider (GenericProvider<AnthropicAdapter>)
              │
              ├── IntentRouter → 意图识别
              │     ├── 正则匹配（精确指令）
              │     ├── LLM 结构化输出（自然语言）
              │     ├── Jenkins 缓存（Job/分支模糊匹配 + Levenshtein 修正）
              │     └── Correction 记录（job/branch 修正提示，前端展示）
              │
              ├── StepChain → 步骤编排执行（支持 SSE 流式推送）
              │     ├── JobValidate → JenkinsTrigger → JenkinsWait
              │     └── JenkinsLog → ClaudeAnalyze / ClaudeCode
              │
              └── Harness Orchestrator → 编排核心
                    ├── Hook: Token (Token 预算追踪)
                    ├── Hook: Memory (记忆保存)
                    ├── SandboxFactory → 配置驱动后端选择
                    │     ├── CubeSandbox（REST 控制平面 + Connect RPC 数据平面，懒初始化）
                    │     ├── Microsandbox（microsandbox SDK，仅 Linux KVM）
                    │     └── Process（本地进程沙箱，最终降级）
                    └── Tools (工具集)
```

### Token 渐进式压缩

```
阶段 1 (轮次 1~10):  线性保留，零开销
阶段 2 (轮次 11~15): LLM 摘要旧数据，保留最近 5 轮线性
阶段 3 (轮次 16+):   结构化文档 (confirmed/conflicts/pending) + 最近 5 轮线性
                     每轮滑动窗口，L1 模型增量更新
```
## Agent Loop
```
Agent 执行流程（精简版）：

1. 接收用户输入 → 构建 Prompt → 调用 LLM
2. 解析 LLM 返回：
   - 工具调用 → 权限检查 → 执行工具 → 结果注入上下文 → 回到步骤 1
   - 纯文本 → 返回给用户，结束本轮
3. 安全约束：
   - 授权：PolicyEngine 校验工具调用权限
   - 重试：最大重试次数限制（防止死循环）
   - 降级：LLM 不可用时返回降级响应

参考实现：
- backend/tests/agent_simple_test.rs — run_openai_agent() / run_claude_agent()
- Phase 4 集成计划：TokenHook + MemoryHook 串联完整流程
```
## 部署、测试
```
# 重启前后端
# 端口默认 后端8080 前端3000，可在 config.toml 配置
./scripts/run.sh

# 1. 启动 Rust 后端
cd backend
./run-signed.sh

cargo test 需忽略 ignore

# 2. 启动前端（另一个终端）
cd frontend
bun install
bun run dev

# 3. 访问 http://localhost:3000

```


## 效果图
![预检失败](./images/jda1.png)
![构建成功](./images/jda2.png)