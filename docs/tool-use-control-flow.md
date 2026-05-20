# Tool-Use 控制流程设计

> 设计日期: 2026-05-20
> 状态: 已确认，待实现

## 架构总览

混合路线：垂直场景优先自建 Agent Loop，降级 Claude Code CLI，最终目标完全取代。

```
用户请求
  ↓
┌──────────────────────────────────────────────────────┐
│ Phase 1: 两阶段批处理 (最多 5 轮)                      │
│  ┌────────────────────────────────────────────────┐  │
│  │ LLM → tool_calls → 分组执行                     │  │
│  │   Safe 组: 并行 (semaphore ≤ 5)                │  │
│  │   Exclusive 组: 串行                            │  │
│  │ → 结果注入 → LLM 再判断                         │  │
│  │ → tool_search 动态注入场景工具                   │  │
│  └────────────────────────────────────────────────┘  │
│                  ↓ 信号≥3 且含强信号                   │
│                                              │       │
│  Phase 2: DAG 编排 (最多 3 轮)                     │       │
│  ┌────────────────────────────────────────────────┐  │
│  │ 拓扑排序 → 层级并行 → 节点级重试 (2 次)          │  │
│  └────────────────────────────────────────────────┘  │
│                  ↓ 仍失败                              │
│                                              │       │
│  降级: Claude Code CLI (超时 5 分钟，可配置)          │       │
└──────────────────────────────────────────────────────┘
```

## 工具注入策略（分层）

```
层 2 [100% 缓存] 静态工具定义:
  - read(path)
  - write(path, content)
  - bash(command, timeout)
  - git(operation, args)
  - get_time()          TTL 1 分钟，会话级缓存
  - get_env(key)        TTL 10 分钟，白名单校验
  - get_config(key)     TTL 10 分钟，白名单校验
  - tool_search(query)  元工具，搜索动态工具

层 6 [低缓存] 动态场景工具（tool_search 按需追加）:
  - jenkins_trigger / jenkins_status / jenkins_log
  - gitlab_merge_request / gitlab_pipeline
  - ... 会话级持久，一旦注入不再移除
```

### tool_search 设计

**匹配策略（组合）：**
1. 正则精确匹配 — 工具名、参数名精确匹配
2. 同义词匹配 — 硬编码映射表（单独文件）
3. 分类匹配 — 按分类（jenkins/gitlab/docker/k8s）批量召回
4. 向量/LLM 轻量调用 — 语义匹配（兜底）

**来源注册表：**
- **Tools** — 直接转为 ToolDefinition
- **Skills** — 简化参数后转为 ToolDefinition（如 `gsd-code-review → {target, strictness}`）
- **MCP** — 代理包装为高层语义工具（如 `github_search` 而非暴露所有 MCP 方法）

**同义词映射示例：**
```
"Jenkins" → ["流水线", "CI", "构建", "pipeline", "job", "部署"]
"GitLab"  → ["MR", "merge request", "合并请求", "代码审查"]
"Docker"  → ["容器", "镜像", "container", "image"]
```

## Agent Loop 主循环

```
loop {
    // 1. 构建请求
    req = ChatRequest {
        model,
        messages,
        tools: static_tools + dynamic_tools,
        temperature: 0.2,
    }

    // 2. 调用 LLM
    resp = llm.chat(req)

    // 3. 无工具调用 → 返回结果
    if !resp.has_tool_calls() {
        return resp.content
    }

    // 4. 死循环检测
    loop_detector.record(resp.tool_calls)
    if let Some(loop) = loop_detector.is_looping() {
        messages.push(loop.to_injection_message())
        signals.add(LoopSignal)
        continue
    }

    // 5. 分组执行
    (safe, exclusive) = partition_by_safety(resp.tool_calls)

    // 5A. Safe 组并行（semaphore ≤ 5）
    safe_results = parallel_with_semaphore(safe, max: 5)

    // 5B. Exclusive 组串行
    exclusive_results = serialize(exclusive)

    // 5C. tool_search 特殊处理
    for result in all_results {
        if result.tool == "tool_search" {
            for tool in result.matched_tools {
                dynamic_tools.add(tool)
            }
        }
    }

    // 6. 结果注入
    for (call, result) in zip(tool_calls, all_results) {
        messages.push(ToolResult { call_id: call.id, content: result })
    }

    // 7. 继续下一轮
}
```

## 并发安全分级

```rust
enum ParallelSafety {
    Safe,                // 可任意并行（读操作、查询）
    CategoryExclusive,   // 同类互斥，不同类可并行（写文件）
    Exclusive,           // 必须串行（bash、修改系统状态）
}

// 分类
Safe:              read, get_time, get_env, get_config, jenkins_status, git_log
CategoryExclusive: write, git_commit
Exclusive:         bash, git_push, tool_search

// 执行顺序: Safe 组先（读快照） → Exclusive 组后（写修改）
// 组间有阶段屏障，不需要文件锁
// 并行上限: semaphore = 5
```

## 重试策略

### 工具级立即重试

| 错误类型 | 可重试 | 次数 | 退避策略 |
|---------|--------|------|---------|
| 网络超时 | ✅ | 5 | 指数退避 2→4→8→16s |
| 网络连接拒绝 | ✅ | 3 | 固定 3s |
| HTTP 429 限流 | ✅ | 3 | Retry-After 头 |
| HTTP 5xx | ✅ | 3 | 指数退避 2→4→8→16s |
| HTTP 4xx | ❌ | 0 | — |
| 权限拒绝 | ❌ | 0 | — |
| 资源不存在 | ❌ | 0 | — |
| 业务超时 | ✅ | 2 | 固定 3s |
| 解析失败 | ❌ | 0 | — |
| 未知错误 | ✅ | 3 | 固定 3s |

### Agent Loop 轮次重试

```
Phase 1 两阶段:  最大 5 轮 LLM 交互
Phase 2 DAG:     最大 3 轮
Claude Code CLI: 超时 5 分钟（可 TOML 配置）
```

### 失败反馈注入（方案 A）

错误 + 提示直接作为 `tool_result` 回传，不碰 system 层。

```rust
enum ToolErrorKind {
    Timeout,          // "该操作超时，请检查参数或尝试更轻量的替代工具"
    PermissionDenied, // "权限不足，请确认当前角色是否允许此操作"
    NotFound,         // "资源未找到，请检查参数拼写"
    NetworkError,     // "网络异常，请重试或考虑降级方案"
    ParseError,       // "输出格式不匹配，请严格遵循 JSON Schema"
    Unknown,          // "操作失败，请分析错误信息并调整策略"
}
```

## 成功判定（信号投票）

### 负面信号（每个 +1 分）

| # | 信号 | 检测方式 |
|---|------|----------|
| ① | 重复工具调用 | 精确去重 + 工具频率，窗口 5 轮 |
| ② | 硬约束不满足 | 结构化输出 Schema + 业务规则校验失败 |
| ③ | 工具执行失败 | 不可重试错误 |
| ④ | LLM 自评分低 | 开放任务评分 < 阈值 |
| ⑤ | 轮次超限 | 超过 max_rounds 未收敛 |
| ⑥ | Token 超标 | 已用 >70% 预算未收敛 |
| ⑦ | 响应超时 | 单轮 >60 秒 |
| ⑧ | 内容异常 | 返回过短/过长，偏离主题 |

### 触发条件

```
≥ 3 个信号同时出现 且 包含 ①或③（强信号）
```

### 死循环检测

```
窗口: 5 轮（约等于 Phase 1 总预算）
精确重复阈值: 2（同一调用出现 3 次）
工具频率阈值: 5（同一工具被调用 5+ 次）

渐进式干预 3 级:
  Level 1: 提醒 + 上次结果 + 建议换方向（不加分）
  Level 2: 明确循环 + 确认已知事实 + 建议 tool_search（信号① +1）
  Level 3: 强制终止 + 触发升级（信号① +1 + 信号⑤ +1）
```

## 降级交接

### 交接上下文（方案 B，裁剪完整历史）

```rust
struct HandoffContext {
    original_user_input: String,
    completed_steps: Vec<CompletedStep>,  // 脱敏后
    failure_summary: String,              // 失败信号摘要
    last_error: String,
}
```

转为 Claude Code prompt：
```
用户请求：{original_user_input}

已完成步骤（由编排引擎执行）：
- read(src/main.rs) → 成功 (2340 字符)
- get_env(DEPLOY_TARGET) → prod-us-east
- jenkins_status(ds-pkg, #42) → RUNNING

失败原因：重复调用 jenkins_status 5 次未收敛
最后错误：构建状态持续 RUNNING，无法获取最终结果

请基于以上上下文继续完成用户的请求。
```

### Claude Code CLI 控制

```
硬超时: 5 分钟（TOML 可配置）
输出监控: 实时读取 stdout/stderr，检测关键错误提前终止
```

## 消息历史管理（统一压缩）

与 Prompt 压缩共享同一套四层上下文窗口机制，不独立实现。

详见 [prompt-architecture.md](./prompt-architecture.md) 的 Tool-Use 消息统一管理章节。

核心规则：
- 用户原始请求 → Compressed 层（永不裁剪）
- tool_calls / tool_results / assistant 文本 → Linear 层（跟随三阶段压缩）
- Session 槽位 → Structured 层（轮次>15 压缩）
- 压缩产物追加到 Compressed 层尾部

## 结构化输出

详见 [structured-output-design.md](./structured-output-design.md)

策略链：Tool Use 模式 (99.8%) → 增强 Prompt (98.2%) → 兜底解析 (6 层)

## 验证策略

### 垂直场景硬约束

JSON Schema + 后置校验函数。通用 Validator 先行，插件机制后续。

```rust
// 通用校验规则
#[validate(required)]              // 必填
#[validate(min = 0, max = 100)]   // 数值范围
#[validate(gt = 0, lte = 3600)]   // 开区间/闭区间
#[validate(enum = "A,B,C")]       // 枚举值
#[validate(pattern = "^[a-z]+$")] // 正则
#[validate(min_length = 1)]       // 字符串长度
```

### A/B 测试

后续加入，对比自建 vs Claude Code CLI 的完成率、耗时、Token 消耗。
