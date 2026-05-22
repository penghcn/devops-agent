//! 信号投票器 — 收集负面信号，决定是否升级

use std::collections::HashSet;

/// 负面信号类型
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum NegativeSignal {
    /// 重复工具调用
    RepeatedToolCall,
    /// 工具执行失败
    ToolExecutionFailed,
    /// Token 超出预算
    TokenOverBudget,
    /// 内容异常
    ContentAnomaly,
    /// 响应超时
    ResponseTimeout,
    /// 轮次超过阈值
    RoundExceeded,
}

impl NegativeSignal {
    /// 是否为强信号（单独即可引起重视）
    pub fn is_strong(&self) -> bool {
        matches!(
            self,
            NegativeSignal::RepeatedToolCall | NegativeSignal::ToolExecutionFailed
        )
    }
}

/// 信号投票器 — 收集负面信号，决定是否升级
pub struct SignalVoter {
    signals: HashSet<NegativeSignal>,
}

impl SignalVoter {
    pub fn new() -> Self {
        Self {
            signals: HashSet::new(),
        }
    }

    /// 添加负面信号（自动去重）
    pub fn add(&mut self, signal: NegativeSignal) {
        self.signals.insert(signal);
    }

    /// 当前信号数量
    pub fn signal_count(&self) -> usize {
        self.signals.len()
    }

    /// 判断是否需要升级：≥3 个信号且至少一个强信号
    pub fn should_escalate(&self) -> bool {
        self.signals.len() >= 3 && self.signals.iter().any(|s| s.is_strong())
    }
}
