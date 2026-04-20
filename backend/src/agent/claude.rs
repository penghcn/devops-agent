use std::process::Command;
use anyhow::{Result, Context};

/// 调用 Claude Code 执行任务
/// 
/// 设计决策：使用 subprocess 调用 Claude Code CLI，
/// 而非直接调用 Anthropic API。原因：
/// 1. 复用 Claude Code 已有的 Agent 编排能力（Subagent、Skill）
/// 2. 自动获得文件读写、Shell 执行等工具能力
/// 3. 支持 --allowedTools 限制权限，安全可控
pub async fn call_claude_code(prompt: &str, allowed_tools: &str) -> Result<String> {
    tracing::info!("Calling Claude Code with prompt: {}", prompt);
    
    // 异步执行命令（tokio::task::spawn_blocking 避免阻塞）
    let prompt_owned = prompt.to_string();
    let tools_owned = allowed_tools.to_string();
    
    let output = tokio::task::spawn_blocking(move || {
        let mut cmd = Command::new("claude");
        cmd.arg("--print").arg(&prompt_owned);
        if !tools_owned.is_empty() {
            cmd.arg("--allowedTools").arg(&tools_owned);
        }
        cmd.output()
    })
    .await
    .context("Failed to spawn Claude Code process")?
    .context("Failed to execute Claude Code")?;
    
    if output.status.success() {
        let result = String::from_utf8(output.stdout)
            .context("Invalid UTF-8 in Claude output")?;
        Ok(result)
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("Claude Code failed: {}", stderr)
    }
}

/// 使用特定 Skill 调用 Claude Code
/// 
/// 设计决策：Skill 是预定义的 Prompt 模板，放在 .claude/skills/ 目录
/// 这样 Claude 会按照 Skill 定义的行为执行，比每次写完整 Prompt 更稳定
pub async fn call_with_skill(skill_name: &str, input: &str) -> Result<String> {
    let prompt = format!(
        "请使用 {} skill 处理以下任务：\n{}",
        skill_name, input
    );
    call_claude_code(&prompt, "Bash,Read,Write").await
}