use devops_agent::harness::*;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

/// Test 1: Orchestrator::new() 创建空编排器
#[test]
fn test_orchestrator_new() {
    let orchestrator = Orchestrator::new();
    // 验证可以通过 run() 执行空步骤链
    let rt = tokio::runtime::Runtime::new().unwrap();
    let result = rt.block_on(orchestrator.run(&[]));
    assert!(result.is_ok());
    assert!(result.unwrap().is_empty());
}

/// Test 2: add_hook() 注册 Hook 后，run() 执行时 Hook.on() 被调用
#[tokio::test]
async fn test_orchestrator_runs_hooks() {
    let counter = Arc::new(AtomicUsize::new(0));

    struct CountingHook {
        counter: Arc<AtomicUsize>,
    }

    #[async_trait::async_trait]
    impl Hook for CountingHook {
        async fn on(&self, _point: HookPoint) -> anyhow::Result<()> {
            self.counter.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
    }

    let mut orchestrator = Orchestrator::new();
    orchestrator.add_hook(Arc::new(CountingHook { counter: counter.clone() }));

    // 空步骤链: SessionStart + SessionEnd = 2 次
    orchestrator.run(&[]).await.unwrap();
    assert_eq!(counter.load(Ordering::SeqCst), 2);
}

/// Test 3: run() 执行空步骤链时，触发 SessionStart → SessionEnd 顺序
#[tokio::test]
async fn test_orchestrator_empty_chain_order() {
    let order = Arc::new(std::sync::Mutex::new(Vec::new()));

    struct OrderHook {
        order: Arc<std::sync::Mutex<Vec<String>>>,
    }

    #[async_trait::async_trait]
    impl Hook for OrderHook {
        async fn on(&self, point: HookPoint) -> anyhow::Result<()> {
            self.order.lock().unwrap().push(format!("{:?}", point));
            Ok(())
        }
    }

    let mut orchestrator = Orchestrator::new();
    orchestrator.add_hook(Arc::new(OrderHook {
        order: order.clone(),
    }));

    orchestrator.run(&[]).await.unwrap();

    let order = order.lock().unwrap();
    assert_eq!(order[0], "SessionStart");
    assert_eq!(order[1], "SessionEnd");
}

/// Test 4: run() 执行含 Step 的步骤链时，触发正确 Hook 顺序
#[tokio::test]
async fn test_orchestrator_step_chain_order() {
    let order = Arc::new(std::sync::Mutex::new(Vec::new()));

    struct OrderHook {
        order: Arc<std::sync::Mutex<Vec<String>>>,
    }

    #[async_trait::async_trait]
    impl Hook for OrderHook {
        async fn on(&self, point: HookPoint) -> anyhow::Result<()> {
            self.order.lock().unwrap().push(format!("{:?}", point));
            Ok(())
        }
    }

    struct TestStep {
        name: &'static str,
        output: &'static str,
    }

    #[async_trait::async_trait]
    impl Step for TestStep {
        fn name(&self) -> &str {
            self.name
        }
        async fn execute(&self) -> anyhow::Result<String> {
            Ok(self.output.to_string())
        }
    }

    let mut orchestrator = Orchestrator::new();
    orchestrator.add_hook(Arc::new(OrderHook {
        order: order.clone(),
    }));

    let steps: Vec<Arc<dyn Step>> = vec![
        Arc::new(TestStep {
            name: "step1",
            output: "result1",
        }),
        Arc::new(TestStep {
            name: "step2",
            output: "result2",
        }),
    ];

    let results = orchestrator.run(&steps).await.unwrap();

    // 验证 Hook 触发顺序: SessionStart → StepStart → StepEnd → StepStart → StepEnd → SessionEnd
    let order = order.lock().unwrap();
    assert_eq!(
        *order,
        vec![
            "SessionStart",
            "StepStart",
            "StepEnd",
            "StepStart",
            "StepEnd",
            "SessionEnd",
        ]
    );

    // 验证 Step 输出
    assert_eq!(results, vec!["result1", "result2"]);
}

/// Test 5: Step 执行失败时，触发 StepEnd 后返回 Err
#[tokio::test]
async fn test_orchestrator_step_failure() {
    let order = Arc::new(std::sync::Mutex::new(Vec::new()));

    struct OrderHook {
        order: Arc<std::sync::Mutex<Vec<String>>>,
    }

    #[async_trait::async_trait]
    impl Hook for OrderHook {
        async fn on(&self, point: HookPoint) -> anyhow::Result<()> {
            self.order.lock().unwrap().push(format!("{:?}", point));
            Ok(())
        }
    }

    struct FailingStep;

    #[async_trait::async_trait]
    impl Step for FailingStep {
        fn name(&self) -> &str {
            "failing"
        }
        async fn execute(&self) -> anyhow::Result<String> {
            anyhow::bail!("step failed")
        }
    }

    let mut orchestrator = Orchestrator::new();
    orchestrator.add_hook(Arc::new(OrderHook {
        order: order.clone(),
    }));

    let steps: Vec<Arc<dyn Step>> = vec![Arc::new(FailingStep)];

    let result = orchestrator.run(&steps).await;
    assert!(result.is_err());

    // 验证: SessionStart → StepStart → StepEnd (失败仍然触发 StepEnd)
    // SessionEnd 不应该被触发
    let order = order.lock().unwrap();
    assert_eq!(
        *order,
        vec!["SessionStart", "StepStart", "StepEnd"]
    );
}
