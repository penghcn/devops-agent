use devops_agent::harness::{Hook, HookPoint, TokenHook};
use devops_agent::token::TokenTracker;
use std::sync::Arc;

/// Test 1: TokenHook::new(Arc<TokenTracker>) 创建成功
#[test]
fn test_token_hook_new() {
    let tracker = Arc::new(TokenTracker::new(10000));
    let hook = TokenHook::new(tracker);
    // 创建成功即可
    let _ = hook;
}

/// Test 2: TokenHook.on(LlmResult) 检查预算状态 — 未超预算时不报警
#[tokio::test]
async fn test_token_hook_on_llm_result_within_budget() {
    let tracker = Arc::new(TokenTracker::new(10000));
    let hook = TokenHook::new(tracker.clone());

    // 记录少量 token，未超预算
    tracker.record(devops_agent::token::TokenUsage::new(100, 50, 150));
    assert!(!tracker.is_exceeded());

    // on(LlmResult) 应该返回 Ok
    let result = hook.on(HookPoint::LlmResult).await;
    assert!(result.is_ok());
}

/// Test 3: TokenHook.on(LlmResult) 检查预算状态 — 超预算时仍返回 Ok 但记录警告
#[tokio::test]
async fn test_token_hook_on_llm_result_exceeded_budget() {
    let tracker = Arc::new(TokenTracker::new(100));
    let hook = TokenHook::new(tracker.clone());

    // 记录超过预算的 token
    tracker.record(devops_agent::token::TokenUsage::new(80, 40, 120));
    assert!(tracker.is_exceeded());

    // on(LlmResult) 仍然返回 Ok（只是警告）
    let result = hook.on(HookPoint::LlmResult).await;
    assert!(result.is_ok());
}

/// Test 4: TokenHook 对其他 HookPoint 无操作
#[tokio::test]
async fn test_token_hook_ignores_other_points() {
    let tracker = Arc::new(TokenTracker::new(10000));
    let hook = TokenHook::new(tracker);

    // 所有 HookPoint 都应该返回 Ok
    assert!(hook.on(HookPoint::SessionStart).await.is_ok());
    assert!(hook.on(HookPoint::StepStart).await.is_ok());
    assert!(hook.on(HookPoint::StepEnd).await.is_ok());
    assert!(hook.on(HookPoint::SessionEnd).await.is_ok());
}
