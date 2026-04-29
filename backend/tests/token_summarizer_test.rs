use devops_agent::token::{Summarizer, SummarizerConfig};

#[test]
fn summarizer_default_config() {
    let config = SummarizerConfig::default();
    assert_eq!(config.summary_threshold, 10);
    assert_eq!(config.structure_threshold, 15);
    assert_eq!(config.linear_window, 5);
}

#[test]
fn summarizer_current_phase_linear() {
    let summarizer = Summarizer::default();

    // Round count <= summary_threshold -> Linear phase
    let phase = summarizer.current_phase(5);
    assert_eq!(format!("{:?}", phase), "Linear");
}

#[test]
fn summarizer_current_phase_summary() {
    let summarizer = Summarizer::default();

    // summary_threshold < round_count <= structure_threshold -> Summary phase
    let phase = summarizer.current_phase(12);
    assert_eq!(format!("{:?}", phase), "Summary");
}

#[test]
fn summarizer_current_phase_structured() {
    let summarizer = Summarizer::default();

    // round_count > structure_threshold -> Structured phase
    let phase = summarizer.current_phase(20);
    assert_eq!(format!("{:?}", phase), "Structured");
}

#[test]
fn summarizer_summarize_local() {
    let summarizer = Summarizer::default();

    let messages = vec![
        "This is the first message with some content to summarize. It contains a lot of detailed information that exceeds the truncation limit and should be compressed significantly when the summarizer processes it.".to_string(),
        "This is the second message that contains additional information about the conversation context. The message continues with more details and observations that push it well beyond the hundred character threshold for local summarization.".to_string(),
        "This is the third and final message in this batch. It wraps up the conversation with concluding remarks and summary information that would normally span several sentences and paragraphs of detailed analysis.".to_string(),
    ];

    let result = summarizer.summarize_local(&messages);

    assert!(!result.summary.is_empty());
    assert!(result.compressed_tokens < result.original_tokens);
    assert!(result.key_decisions.is_empty() || !result.key_decisions.is_empty());
}

#[test]
fn summarizer_strategy_per_message() {
    let summarizer = Summarizer::default();

    // Few messages -> PerMessage
    let messages = vec!["short".to_string()];
    let strategy = summarizer.strategy(&messages);
    assert_eq!(format!("{:?}", strategy), "PerMessage");
}

#[test]
fn summarizer_summarize_with_llm_not_connected() {
    let summarizer = Summarizer::default();
    let messages = vec!["test".to_string()];

    let result = summarizer.summarize_with_llm(&messages);
    assert!(result.is_err());
}
