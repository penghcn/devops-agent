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

/// 摘要压缩器配置
#[derive(Debug, Clone)]
pub struct SummarizerConfig {
    /// 触发摘要压缩的轮次阈值（默认 10）
    pub summary_threshold: u32,
    /// 触发结构化压缩的轮次阈值（默认 15）
    pub structure_threshold: u32,
    /// Linear 层保留轮次数（默认 5）
    pub linear_window: u32,
}

impl Default for SummarizerConfig {
    fn default() -> Self {
        Self {
            summary_threshold: 10,
            structure_threshold: 15,
            linear_window: 5,
        }
    }
}

/// 渐进式三阶段摘要压缩器
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

    /// 本地摘要压缩（简单策略：取每条消息前 100 字符）
    pub fn summarize_local(&self, messages: &[String]) -> SummaryResult {
        let original_tokens: u32 = messages.iter().map(|m| m.len() as u32 / 4).sum();

        let summaries: Vec<String> = messages
            .iter()
            .map(|m| {
                let truncated: Vec<char> = m.chars().take(100).collect();
                format!("- {}", truncated.into_iter().collect::<String>())
            })
            .collect();

        let summary = summaries.join("\n");
        let compressed_tokens = summary.len() as u32 / 4;

        SummaryResult {
            summary,
            key_decisions: Vec::new(),
            action_items: Vec::new(),
            original_tokens,
            compressed_tokens,
        }
    }

    /// LLM 摘要压缩（尚未连接 LLM 层）
    pub fn summarize_with_llm(&self, _messages: &[String]) -> anyhow::Result<SummaryResult> {
        anyhow::bail!("LLM summarizer not yet connected");
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
