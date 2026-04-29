use std::sync::{Arc, Mutex};

/// 单次 LLM 调用的 Token 使用量
#[derive(Debug, Clone, Default)]
pub struct TokenUsage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
}

impl TokenUsage {
    /// 创建新的 Token 使用量记录
    pub fn new(prompt_tokens: u32, completion_tokens: u32, total_tokens: u32) -> Self {
        Self {
            prompt_tokens,
            completion_tokens,
            total_tokens,
        }
    }
}

/// Token 计数器 + 预算追踪 + 轮次计数
pub struct TokenTracker {
    state: Arc<Mutex<TrackerState>>,
    budget: u32,
}

/// 内部状态
#[derive(Debug)]
struct TrackerState {
    total_usage: TokenUsage,
    per_call: Vec<TokenUsage>,
    round_count: u32,
}

impl TokenTracker {
    /// 创建指定预算的 Token 追踪器
    pub fn new(budget: u32) -> Self {
        Self {
            state: Arc::new(Mutex::new(TrackerState {
                total_usage: TokenUsage::default(),
                per_call: Vec::new(),
                round_count: 0,
            })),
            budget,
        }
    }

    /// 记录一次 LLM 调用的 Token 使用量
    pub fn record(&self, usage: TokenUsage) {
        let mut state = self.state.lock().unwrap();
        state.total_usage.prompt_tokens += usage.prompt_tokens;
        state.total_usage.completion_tokens += usage.completion_tokens;
        state.total_usage.total_tokens += usage.total_tokens;
        state.per_call.push(usage);
    }

    /// 返回累计使用量
    pub fn usage(&self) -> TokenUsage {
        let state = self.state.lock().unwrap();
        state.total_usage.clone()
    }

    /// 返回剩余预算
    pub fn remaining(&self) -> u32 {
        self.budget.saturating_sub(self.usage().total_tokens)
    }

    /// 判断是否已超预算
    pub fn is_exceeded(&self) -> bool {
        self.remaining() == 0
    }

    /// 返回调用次数
    pub fn call_count(&self) -> usize {
        let state = self.state.lock().unwrap();
        state.per_call.len()
    }

    /// 增加轮次计数
    pub fn increment_round(&self) {
        let mut state = self.state.lock().unwrap();
        state.round_count += 1;
    }

    /// 返回当前轮次
    pub fn round_count(&self) -> u32 {
        let state = self.state.lock().unwrap();
        state.round_count
    }
}
