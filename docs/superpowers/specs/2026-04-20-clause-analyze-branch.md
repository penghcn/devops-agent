# ClaudeAnalyzeStep 分支优化设计

## 背景

当前 `ClaudeAnalyzeStep` 无论构建成功还是失败，都使用同一个通用 prompt 让 Claude 分析构建结果。这导致：
- 构建成功时也调用通用分析 prompt，浪费 API 成本
- 成功时没有针对性地分析部署结果

## 目标

`ClaudeAnalyzeStep` 根据 `pipeline_status.result` 判断走不同分析路径：
- **成功 (SUCCESS)**: 分析 SSH deploy 阶段日志，确认部署状态和结果
- **失败 (FAILURE/ABORTED)**: 分析错误原因 + 修复建议（保持不变）

## 设计方案

### 修改文件

`backend/src/agent/steps/claude_analyze.rs`

### 核心逻辑

```rust
async fn execute(&self, ctx: &mut StepContext) -> StepResult {
    // 1. 获取 pipeline_status
    let status = match &ctx.pipeline_status {
        Some(s) => s,
        None => return Abort { ... }
    };

    // 2. 判断结果
    let result = status.get("result").and_then(|r| r.as_str()).unwrap_or("");

    // 3. 根据结果选择 prompt
    let prompt = match result {
        "SUCCESS" => build_deploy_check_prompt(&ctx.build_log),
        _ => build_failure_analysis_prompt(&ctx.build_log, status),
    };

    // 4. 调用 Claude
    match claude::call_claude_code(&prompt, "Bash,Read,Write,Grep,Glob").await {
        Ok(result) => {
            ctx.analysis_result = Some(result.clone());
            StepResult::Success { message: "分析完成".to_string() }
        }
        Err(e) => StepResult::Failed { error: e.to_string() },
    }
}
```

### 成功时 prompt (部署检查)

```
你是一个 DevOps 工程师。请分析以下 Jenkins 构建日志中的 SSH deploy 阶段，给出:
1. 部署状态（成功/失败）
2. 部署到了哪些目标服务器/环境
3. 部署关键日志摘要
4. 是否需要进一步操作（如验证服务健康状态等）

构建日志: {}
```

### 失败时 prompt (错误分析，保持不变)

```
你是一个 DevOps 工程师。请分析以下构建结果，给出:
1. 构建状态摘要
2. 如果失败，分析可能的失败原因
3. 修复建议

构建数据: {}
```

## 成功标准

- [ ] 构建成功时，Claude 输出部署状态、目标、关键日志、后续建议
- [ ] 构建失败时，Claude 输出错误原因和修复建议（与现有行为一致）
- [ ] 构建日志为空时，正确 Abort
- [ ] 现有测试通过

## 影响范围

- 仅修改 `claude_analyze.rs`，不影响其他 Step
- StepChain 结构不变（仍然是 Trigger → Wait → Log → ClaudeAnalyze）
- API 响应格式不变
