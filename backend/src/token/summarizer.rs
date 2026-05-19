use anyhow;

/// 压缩阶段
#[derive(Debug, Clone, PartialEq)]
pub enum CompressionPhase {
    Linear,
    Summary,
    Structured,
}

/// 摘要压缩结果
#[derive(Debug, Clone)]
pub struct SummaryResult {
    pub summary: String,
    pub key_decisions: Vec<String>,
    pub action_items: Vec<String>,
    pub original_tokens: u32,
    pub compressed_tokens: u32,
}

/// 摘要压缩策略
#[derive(Debug, Clone, PartialEq)]
pub enum CompressionStrategy {
    PerMessage,
    Batch,
}

/// 压缩触发方式
#[derive(Debug, Clone, PartialEq)]
pub enum CompressionTrigger {
    /// 轮次到达阈值 → 本地快速压缩
    RoundThreshold,
    /// Token 接近上限 → LLM 深度压缩
    TokenBudget,
}

/// 摘要压缩器配置
#[derive(Debug, Clone)]
pub struct SummarizerConfig {
    /// 触发本地快速压缩的轮次阈值（默认 10）
    pub summary_threshold: u32,
    /// 触发结构化压缩的轮次阈值（默认 15）
    pub structure_threshold: u32,
    /// Linear 层保留轮次数（默认 5）
    pub linear_window: u32,
    /// 触发 LLM 深度压缩的 token 使用率阈值（默认 80%）
    pub token_budget_percent: f32,
    /// 本地摘要每条消息保留字符数（默认 200）
    pub local_summary_chars: usize,
}

impl Default for SummarizerConfig {
    fn default() -> Self {
        Self {
            summary_threshold: 10,
            structure_threshold: 15,
            linear_window: 5,
            token_budget_percent: 80.0,
            local_summary_chars: 200,
        }
    }
}

/// 渐进式三阶段摘要压缩器（混合触发）
#[derive(Debug, Clone, Default)]
pub struct Summarizer {
    pub config: SummarizerConfig,
}

impl Summarizer {
    /// 创建摘要压缩器
    pub fn new(config: SummarizerConfig) -> Self {
        Self { config }
    }

    /// 根据轮次判断当前压缩阶段
    pub fn current_phase(&self, round_count: u32) -> CompressionPhase {
        if round_count <= self.config.summary_threshold {
            CompressionPhase::Linear
        } else if round_count <= self.config.structure_threshold {
            CompressionPhase::Summary
        } else {
            CompressionPhase::Structured
        }
    }

    /// 判断应该用哪种方式触发压缩
    pub fn should_compress(
        &self,
        round_count: u32,
        token_usage_percent: f32,
    ) -> Option<CompressionTrigger> {
        // Token 预算优先（更紧急）
        if token_usage_percent >= self.config.token_budget_percent {
            return Some(CompressionTrigger::TokenBudget);
        }
        // 轮次阈值触发
        if round_count >= self.config.summary_threshold {
            return Some(CompressionTrigger::RoundThreshold);
        }
        None
    }

    /// 估算文本 Token 数（与 ContextWindow 一致的混合估算）
    fn estimate_tokens(s: &str) -> u32 {
        let mut cjk: f32 = 0.0;
        let mut ascii: f32 = 0.0;
        for ch in s.chars() {
            if ch.is_ascii() {
                ascii += 1.0;
            } else if ch.is_alphabetic() || ch.is_numeric() {
                cjk += 1.5;
            } else {
                cjk += 1.0;
            }
        }
        ((ascii / 4.0) + cjk) as u32
    }

    /// 本地摘要压缩（提取关键句子 + 截断）
    pub fn summarize_local(&self, messages: &[String]) -> SummaryResult {
        let original_tokens: u32 = messages.iter().map(|m| Self::estimate_tokens(m)).sum();

        let max_chars = self.config.local_summary_chars;
        let summaries: Vec<String> = messages
            .iter()
            .enumerate()
            .map(|(i, m)| {
                // 按字符数截断（安全处理多字节字符）
                let window: String = m.chars().take(max_chars).collect();

                // 尝试按句子边界截断
                let summary = if let Some(pos) = window.rfind('\n') {
                    &window[..pos]
                } else if let Some(pos) = window.rfind('.') {
                    &window[..=pos]
                } else if let Some(pos) = window.rfind('。') {
                    &window[..=pos]
                } else {
                    return format!("[{}] {}…", i + 1, window.trim());
                };
                format!("[{}] {}", i + 1, summary.trim())
            })
            .collect();

        let summary = summaries.join("\n");
        let compressed_tokens = Self::estimate_tokens(&summary);

        SummaryResult {
            summary,
            key_decisions: Vec::new(),
            action_items: Vec::new(),
            original_tokens,
            compressed_tokens,
        }
    }

    /// LLM 摘要压缩（深度压缩，尚未连接 LLM 层）
    pub fn summarize_with_llm(&self, _messages: &[String]) -> anyhow::Result<SummaryResult> {
        anyhow::bail!("LLM summarizer not yet connected");
    }

    /// 根据触发方式执行压缩
    pub fn compress(
        &self,
        messages: &[String],
        trigger: CompressionTrigger,
    ) -> anyhow::Result<SummaryResult> {
        match trigger {
            CompressionTrigger::RoundThreshold => Ok(self.summarize_local(messages)),
            CompressionTrigger::TokenBudget => self.summarize_with_llm(messages),
        }
    }

    /// 根据消息数量选择压缩策略
    pub fn strategy(&self, messages: &[String]) -> CompressionStrategy {
        if messages.len() <= 3 {
            CompressionStrategy::PerMessage
        } else {
            CompressionStrategy::Batch
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_phase_transitions() {
        let s = Summarizer::default();
        assert_eq!(s.current_phase(5), CompressionPhase::Linear);
        assert_eq!(s.current_phase(10), CompressionPhase::Linear);
        assert_eq!(s.current_phase(12), CompressionPhase::Summary);
        assert_eq!(s.current_phase(15), CompressionPhase::Summary);
        assert_eq!(s.current_phase(20), CompressionPhase::Structured);
    }

    #[test]
    fn test_should_compress_round_threshold() {
        let s = Summarizer::default();
        assert_eq!(s.should_compress(5, 50.0), None);
        assert_eq!(
            s.should_compress(10, 50.0),
            Some(CompressionTrigger::RoundThreshold)
        );
        assert_eq!(
            s.should_compress(10, 85.0),
            Some(CompressionTrigger::TokenBudget)
        );
    }

    #[test]
    fn test_should_compress_token_budget() {
        let s = Summarizer::default();
        // Token 预算优先于轮次
        assert_eq!(
            s.should_compress(5, 90.0),
            Some(CompressionTrigger::TokenBudget)
        );
    }

    #[test]
    fn test_summarize_local_sentence_boundary() {
        let s = Summarizer::default();
        let msgs = vec![
            "Line one.\nLine two.\nLine three.".to_string(),
            "Short message here".to_string(),
        ];
        let result = s.summarize_local(&msgs);
        assert!(result.summary.contains("Line one"));
        assert!(result.summary.contains("Line two"));
    }

    #[test]
    fn test_summarize_local_chinese_truncation() {
        let s = Summarizer::default();
        // 长中文文本会被截断
        let long_msg = "你好世界".repeat(60);
        let msgs = vec![long_msg.clone()];
        let result = s.summarize_local(&msgs);
        assert!(result.compressed_tokens < result.original_tokens);
        assert!(result.summary.ends_with('…'));
    }

    #[test]
    fn test_estimate_tokens_chinese() {
        // 中文 "你好世界" ≈ 4 * 1.5 = 6 tokens
        let tokens = Summarizer::estimate_tokens("你好世界");
        assert!((tokens as i32 - 6).abs() <= 1);
    }

    #[test]
    fn test_estimate_tokens_ascii() {
        // ASCII "Hello World" ≈ 11 / 4 ≈ 2.75 → 2
        let tokens = Summarizer::estimate_tokens("Hello World");
        assert!(tokens >= 2 && tokens <= 4);
    }
}
