//! 降级交接 — 将任务交接给 Claude Code CLI

/// 降级原因
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FallbackReason {
    /// 信号投票触发升级
    SignalEscalation,
    /// 死循环检测 Level 3
    LoopDetected,
    /// 超过最大轮次
    MaxIterationsExceeded,
    /// 超时
    Timeout,
}

impl std::fmt::Display for FallbackReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FallbackReason::SignalEscalation => write!(f, "信号投票触发降级"),
            FallbackReason::LoopDetected => write!(f, "检测到死循环，触发降级"),
            FallbackReason::MaxIterationsExceeded => write!(f, "超过最大轮次，触发降级"),
            FallbackReason::Timeout => write!(f, "超时，触发降级"),
        }
    }
}

/// 降级执行结果
#[derive(Debug)]
pub struct FallbackResult {
    /// 是否成功
    pub success: bool,
    /// 输出内容
    pub output: String,
    /// 降级原因
    pub reason: Option<FallbackReason>,
    /// Token 消耗
    pub token_usage: u32,
}

impl FallbackResult {
    pub fn success(output: impl Into<String>, token_usage: u32) -> Self {
        Self {
            success: true,
            output: output.into(),
            reason: None,
            token_usage,
        }
    }

    pub fn failure(output: impl Into<String>, reason: FallbackReason) -> Self {
        Self {
            success: false,
            output: output.into(),
            reason: Some(reason),
            token_usage: 0,
        }
    }
}

/// 降级处理器 — 负责将任务交接给 Claude Code CLI
pub struct FallbackHandler {
    /// Claude Code CLI 路径
    claude_path: String,
    /// 超时时间（秒）
    timeout_secs: u64,
}

impl FallbackHandler {
    pub fn new(claude_path: impl Into<String>) -> Self {
        Self {
            claude_path: claude_path.into(),
            timeout_secs: 300, // 默认 5 分钟
        }
    }

    /// 设置超时时间
    pub fn set_timeout_secs(&mut self, secs: u64) -> &mut Self {
        self.timeout_secs = secs;
        self
    }

    /// 获取超时时间
    pub fn timeout_secs(&self) -> u64 {
        self.timeout_secs
    }

    /// 构建降级命令
    pub fn build_command(&self, task: &str, branch: &str) -> Vec<String> {
        vec![
            self.claude_path.clone(),
            "-p".to_string(),
            task.to_string(),
            "--branch".to_string(),
            branch.to_string(),
            "--timeout".to_string(),
            self.timeout_secs.to_string(),
        ]
    }
}
