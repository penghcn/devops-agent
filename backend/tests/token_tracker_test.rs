use devops_agent::token::{TokenTracker, TokenUsage};

#[test]
fn token_tracker_record_usage() {
    let tracker = TokenTracker::new(10000);
    let usage = TokenUsage::new(100, 50, 150);

    tracker.record(usage);
    let total = tracker.usage();
    assert_eq!(total.prompt_tokens, 100);
    assert_eq!(total.completion_tokens, 50);
    assert_eq!(total.total_tokens, 150);
}

#[test]
fn token_tracker_budget_exceeded() {
    let tracker = TokenTracker::new(100);

    // First record within budget
    tracker.record(TokenUsage::new(30, 20, 50));
    assert!(!tracker.is_exceeded());
    assert_eq!(tracker.remaining(), 50);

    // Second record exceeds budget
    tracker.record(TokenUsage::new(25, 30, 60));
    assert!(tracker.is_exceeded());
    assert_eq!(tracker.remaining(), 0);
}

#[test]
fn token_tracker_call_count() {
    let tracker = TokenTracker::new(10000);

    tracker.record(TokenUsage::new(10, 10, 20));
    assert_eq!(tracker.call_count(), 1);

    tracker.record(TokenUsage::new(10, 10, 20));
    assert_eq!(tracker.call_count(), 2);

    tracker.record(TokenUsage::new(10, 10, 20));
    assert_eq!(tracker.call_count(), 3);
}

#[test]
fn token_tracker_round_count() {
    let tracker = TokenTracker::new(10000);

    assert_eq!(tracker.round_count(), 0);
    tracker.increment_round();
    assert_eq!(tracker.round_count(), 1);
    tracker.increment_round();
    assert_eq!(tracker.round_count(), 2);
}

#[test]
fn token_tracker_multiple_records_accumulate() {
    let tracker = TokenTracker::new(10000);

    tracker.record(TokenUsage::new(100, 50, 150));
    tracker.record(TokenUsage::new(200, 100, 300));

    let total = tracker.usage();
    assert_eq!(total.prompt_tokens, 300);
    assert_eq!(total.completion_tokens, 150);
    assert_eq!(total.total_tokens, 450);
}
