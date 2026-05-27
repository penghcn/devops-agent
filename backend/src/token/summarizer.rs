use anyhow;
use std::sync::Arc;

use crate::llm::{ChatRequest, LlmProvider, Message, ToolChoice, ToolDefinition, text_block};

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
pub struct Summarizer {
    pub config: SummarizerConfig,
    /// 专用压缩模型 provider（如 Haiku），用于 LLM 深度压缩
    compression_provider: Option<Arc<dyn LlmProvider>>,
}

impl Default for Summarizer {
    fn default() -> Self {
        Self {
            config: SummarizerConfig::default(),
            compression_provider: None,
        }
    }
}

impl Summarizer {
    /// 创建摘要压缩器
    pub fn new(config: SummarizerConfig) -> Self {
        Self {
            config,
            compression_provider: None,
        }
    }

    /// 设置专用压缩模型 provider
    pub fn set_compression_provider(&mut self, provider: Arc<dyn LlmProvider>) {
        self.compression_provider = Some(provider);
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

    /// 估算文本 Token 数（委托到 llm::estimate_tokens）
    fn estimate_tokens(s: &str) -> u32 {
        crate::llm::estimate_tokens(s)
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

    /// LLM 摘要压缩（深度压缩，使用专用压缩模型）
    pub async fn summarize_with_llm(&self, messages: &[String]) -> anyhow::Result<SummaryResult> {
        let Some(provider) = &self.compression_provider else {
            anyhow::bail!("LLM summarizer not yet connected");
        };

        let concatenated = messages.join("\n---\n");
        let prompt = format!(
            "请将以下对话历史压缩为结构化摘要。只输出 JSON，不要包含其他文字。\n\n{}",
            concatenated
        );

        let schema = serde_json::json!({
            "type": "object",
            "required": ["summary", "key_decisions", "action_items"],
            "properties": {
                "summary": {
                    "type": "string",
                    "description": "对话的简洁摘要，包含主要讨论内容和结论"
                },
                "key_decisions": {
                    "type": "array",
                    "items": {"type": "string"},
                    "description": "关键决策列表（如部署目标、配置变更等）"
                },
                "action_items": {
                    "type": "array",
                    "items": {"type": "string"},
                    "description": "待办事项列表（如需要执行的构建、需要验证的结果等）"
                }
            }
        });

        let tool = ToolDefinition {
            name: "compression_summary".to_string(),
            description: "输出压缩后的结构化摘要".to_string(),
            parameters: schema,
            cache_control: None,
        };

        let request = ChatRequest {
            model: String::new(),
            messages: vec![
                Message::System {
                    content: text_block(
                        "你是一个专业的对话摘要助手。你的任务是将对话压缩为简洁的结构化摘要。"
                            .to_string(),
                    ),
                },
                Message::User {
                    content: text_block(prompt),
                },
            ],
            tools: Some(vec![tool]),
            temperature: Some(0.5),
            tool_choice: Some(ToolChoice::Tool {
                name: "compression_summary".to_string(),
            }),
            stop_sequences: None,
            prefill: None,
        };

        let response = provider.llm_call(&request).await?;

        // 从 tool_calls 或 content 中提取 JSON 数据
        let data: serde_json::Value = if let Some(tc) = response.tool_calls.first() {
            tc.arguments.clone()
        } else {
            serde_json::from_str(&response.content)
                .map_err(|e| anyhow::anyhow!("Failed to parse LLM summary response: {}", e))?
        };

        Ok(self._parse_summary_from_json(&data, messages))
    }

    /// 从 JSON 数据解析摘要结果
    fn _parse_summary_from_json(
        &self,
        data: &serde_json::Value,
        messages: &[String],
    ) -> SummaryResult {
        let summary = data
            .get("summary")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let key_decisions: Vec<String> = data
            .get("key_decisions")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect()
            })
            .unwrap_or_default();
        let action_items: Vec<String> = data
            .get("action_items")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect()
            })
            .unwrap_or_default();

        let original_tokens: u32 = messages.iter().map(|m| Self::estimate_tokens(m)).sum();
        let compressed_tokens = Self::estimate_tokens(&summary);

        SummaryResult {
            summary,
            key_decisions,
            action_items,
            original_tokens,
            compressed_tokens,
        }
    }

    /// 根据触发方式执行压缩（异步版本）
    pub async fn compress(
        &self,
        messages: &[String],
        trigger: CompressionTrigger,
    ) -> anyhow::Result<SummaryResult> {
        match trigger {
            CompressionTrigger::RoundThreshold => Ok(self.summarize_local(messages)),
            CompressionTrigger::TokenBudget => self.summarize_with_llm(messages).await,
        }
    }

    /// 同步版本的压缩（仅本地压缩，不触发 LLM）
    pub fn compress_local(&self, messages: &[String]) -> SummaryResult {
        self.summarize_local(messages)
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
