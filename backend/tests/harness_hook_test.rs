use devops_agent::harness::{Hook, HookPoint};

/// Test 1: 实现 Hook trait 的结构体可以调用 on(HookPoint) 并返回 Ok(())
#[tokio::test]
async fn test_hook_trait_compiles_and_runs() {
    struct TestHook;
    #[async_trait::async_trait]
    impl Hook for TestHook {
        async fn on(&self, _point: HookPoint) -> anyhow::Result<()> {
            Ok(())
        }
    }
    let hook = TestHook;
    assert!(hook.on(HookPoint::SessionStart).await.is_ok());
    assert!(hook.on(HookPoint::StepEnd).await.is_ok());
}

/// Test 2: HookPoint 枚举包含所有 11 个钩子点
#[test]
fn test_hookpoint_has_all_variants() {
    // 通过模式匹配穷举来验证所有变体存在
    fn check_all_variants(point: HookPoint) -> bool {
        matches!(
            point,
            HookPoint::SessionStart
                | HookPoint::SessionEnd
                | HookPoint::StepStart
                | HookPoint::StepEnd
                | HookPoint::ToolCalled
                | HookPoint::ToolResult
                | HookPoint::LlmCalled
                | HookPoint::LlmResult
                | HookPoint::TokenBudgetExceeded
                | HookPoint::MemorySave
                | HookPoint::DecisionMade
        )
    }
    assert!(check_all_variants(HookPoint::SessionStart));
    assert!(check_all_variants(HookPoint::DecisionMade));
}

/// Test 3: HookPoint 支持 Debug, Clone, Copy, PartialEq
#[test]
fn test_hookpoint_traits() {
    let p1 = HookPoint::SessionStart;
    let p2 = p1.clone(); // Clone
    let p3 = p1; // Copy
    assert_eq!(p1, p2); // PartialEq
    assert_eq!(p1, p3);
    let _debug = format!("{:?}", p1); // Debug
}

/// Test 4: 11 个钩子点互不相同
#[test]
fn test_hookpoint_equality() {
    assert_ne!(HookPoint::SessionStart, HookPoint::SessionEnd);
    assert_ne!(HookPoint::StepStart, HookPoint::StepEnd);
    assert_ne!(HookPoint::ToolCalled, HookPoint::ToolResult);
    assert_ne!(HookPoint::LlmCalled, HookPoint::LlmResult);
    assert_ne!(HookPoint::TokenBudgetExceeded, HookPoint::MemorySave);
    assert_ne!(HookPoint::MemorySave, HookPoint::DecisionMade);
}
