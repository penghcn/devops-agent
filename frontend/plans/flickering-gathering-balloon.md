# 计划：Jenkins 校验 + Claude JSON 输出 + 前端适配

## Context

当前问题：
1. **未校验 Jenkins Job 是否存在**：Intent 解析出 job_name/branch 后，直接执行步骤链，如果 job 不存在会报模糊错误
2. **Claude 输出是纯文本**：`AgentResponse.output` 是字符串，前端用 `{{ msg.agent }}` 渲染，太乱
3. **前端 UI 简陋**：所有信息挤在一个 `<div>` 里，没有结构化展示

## 改动方案

### 1. 后端：新增 Job 校验 Step + 结构化输出

#### 1.1 新增 `tools/jenkins.rs` 函数 `check_job_exists`
- 调用 `GET /job/{name}/api/json?fields=_class,name`
- 返回 `Ok((exists: bool, job_type: String, name: String))`
- `job_type` 根据 `_class` 判断：`WorkflowMultiBranchProject` → `pipeline`，`WorkflowJob` → `job`，其他 → 根据

#### 1.2 新增 `steps/job_validate.rs`
- 在步骤链最前面插入 `JobValidateStep`
- 校验 job_name/branch 是否存在、类型是否匹配
- 将校验结果存入 `ctx.pipeline_status`（复用已有字段）

#### 1.3 修改 `router.rs` 的 `to_chain_with_prompt`
- 所有 Pipeline 相关意图的步骤链，在开头插入 `JobValidateStep`
- 顺序：`[JobValidateStep, JenkinsTriggerStep, JenkinsWaitStep, JenkinsLogStep, ClaudeAnalyzeStep]`

#### 1.4 修改 `AgentResponse` 结构
- 新增 `structured_output: Option<serde_json::Value>` 字段
- Claude 分析步骤（`claude_analyze.rs`）输出 JSON 而非纯文本
- 修改 prompt 要求 Claude 输出结构化 JSON

#### 1.5 修改 `claude_analyze.rs` 的 prompt
- 成功场景：要求输出 `{"deploy_status": "success/fail", "servers": [...], "summary": "..."}`
- 失败场景：要求输出 `{"build_status": "failed", "error": "...", "suggestion": "..."}`
- 用 ````json` 包裹 JSON 输出，解析时提取 JSON 块

### 2. 前端：结构化渲染

#### 2.1 修改 `types.ts`
- `AgentResponse` 新增 `structured_output?: Record<string, any>`
- 新增 `AnalyzeResult` 接口（deploy_status, servers, summary 等）

#### 2.2 修改 `App.vue` 的渲染逻辑
- 如果有 `structured_output`，渲染结构化卡片（服务器列表、部署状态等）
- 如果没有，回退到纯文本渲染
- 优化整体布局：用户消息右对齐蓝色，Agent 消息左对齐绿色
- 思考过程折叠面板优化样式

#### 2.3 新增 `components/StructuredResponse.vue`
- 根据 `structured_output` 的类型渲染不同视图
- 部署结果：服务器列表 + 状态标签
- 构建分析：错误信息 + 修复建议

## 涉及文件

| 文件 | 改动 |
|------|------|
| `backend/src/tools/jenkins.rs` | 新增 `check_job_exists` |
| `backend/src/agent/steps/mod.rs` | 新增 `job_validate` 模块 |
| `backend/src/agent/steps/job_validate.rs` | 新建 - Job 校验 Step |
| `backend/src/agent/router.rs` | 步骤链插入校验 + 结构化输出 |
| `backend/src/agent/mod.rs` | `AgentResponse` 新增字段 |
| `backend/src/agent/steps/claude_analyze.rs` | JSON 结构化输出 |
| `frontend/src/types.ts` | 新增类型 |
| `frontend/src/App.vue` | 结构化渲染 |
| `frontend/src/components/StructuredResponse.vue` | 新建 - 结构化响应组件 |
| `backend/tests/step_chain_test.rs` | 新增 Job 校验测试 |

## 验证

1. `cargo test` — 所有单元测试通过
2. `cargo test --test log_analysis_test` — 集成测试通过
3. 前端访问 http://localhost:5173，发送 "部署 ds-pkg 到 staging"，验证：
   - Job 不存在时返回明确错误
   - Claude 分析结果以结构化卡片展示
   - 思考过程折叠面板样式正常
