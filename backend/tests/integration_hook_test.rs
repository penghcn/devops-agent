use devops_agent::harness::{Hook, HookPoint, MemoryHook, Orchestrator, Step, TokenHook};
use devops_agent::memory::ShortTermMemory;
use devops_agent::token::{TokenTracker, TokenUsage};
use std::sync::Arc;

/// 辅助：创建可共享的 TokenTracker
fn create_tracker(budget: u32) -> Arc<TokenTracker> {
    Arc::new(TokenTracker::new(budget))
}

/// Test 1: Orchestrator + TokenHook + MemoryHook，执行空步骤链后返回 Ok
#[tokio::test]
async fn test_hooks_execute_in_order() {
    let tracker = create_tracker(10000);
    let memory = ShortTermMemory::default();

    let token_hook = TokenHook::new(tracker);
    let memory_hook = MemoryHook::new(memory);

    let mut orchestrator = Orchestrator::new();
    orchestrator.add_hook(Arc::new(token_hook));
    orchestrator.add_hook(Arc::new(memory_hook));

    // 执行空步骤链
    let result = orchestrator.run(&[]).await;
    assert!(result.is_ok());
    assert!(result.unwrap().is_empty());
}

/// Test 2: Orchestrator + MemoryHook，执行后 Memory 有记录
#[tokio::test]
async fn test_memory_hook_records_session() {
    let memory = ShortTermMemory::default();
    let memory_hook = Arc::new(MemoryHook::new(memory));

    let mut orchestrator = Orchestrator::new();
    orchestrator.add_hook(memory_hook.clone());

    orchestrator.run(&[]).await.unwrap();

    // 空步骤链: SessionStart + SessionEnd = 2 次 hook 触发
    // MemoryHook 只在 SessionStart 记录
    let mem = memory_hook.memory().lock().unwrap();
    assert_eq!(mem.len(), 1);
    assert_eq!(mem.entries()[0].content, "session started");
}

/// Test 3: Token 超预算时 TokenHook 正确检测
#[tokio::test]
async fn test_token_hook_budget_check() {
    let tracker = create_tracker(100);

    // 记录 150 tokens，超过 100 的预算
    tracker.record(TokenUsage::new(80, 70, 150));
    assert!(tracker.is_exceeded());

    let token_hook = TokenHook::new(tracker);

    // TokenHook 处理 LlmResult 时应该返回 Ok（只是警告）
    let result = token_hook.on(HookPoint::LlmResult).await;
    assert!(result.is_ok());
}

/// Test 4: TokenHook + MemoryHook + 模拟 Step — 验证完整生命周期触发
#[tokio::test]
async fn test_full_integration() {
    let tracker = create_tracker(10000);
    let memory = ShortTermMemory::default();

    let token_hook = TokenHook::new(tracker);
    let memory_hook = Arc::new(MemoryHook::new(memory));

    // 创建模拟 Step
    struct MockStep {
        output: &'static str,
    }

    #[async_trait::async_trait]
    impl Step for MockStep {
        fn name(&self) -> &str {
            "mock"
        }
        async fn execute(&self) -> anyhow::Result<String> {
            Ok(self.output.to_string())
        }
    }

    let mut orchestrator = Orchestrator::new();
    orchestrator.add_hook(Arc::new(token_hook));
    orchestrator.add_hook(memory_hook.clone());

    let steps: Vec<Arc<dyn Step>> = vec![
        Arc::new(MockStep { output: "step1" }),
        Arc::new(MockStep { output: "step2" }),
    ];

    let results = orchestrator.run(&steps).await.unwrap();

    // 验证 Step 输出
    assert_eq!(results, vec!["step1", "step2"]);

    // 验证 MemoryHook 记录: SessionStart + StepStart + StepEnd + StepStart + StepEnd
    // = 5 条记录
    let mem = memory_hook.memory().lock().unwrap();
    assert_eq!(mem.len(), 5);
    assert_eq!(mem.entries()[0].content, "session started");
    assert_eq!(mem.entries()[1].content, "step started");
    assert_eq!(mem.entries()[2].content, "step ended");
    assert_eq!(mem.entries()[3].content, "step started");
    assert_eq!(mem.entries()[4].content, "step ended");
}
