use devops_agent::harness::{Hook, HookPoint, MemoryHook};
use devops_agent::memory::ShortTermMemory;

/// Test 1: MemoryHook::new(ShortTermMemory) 创建成功
#[test]
fn test_memory_hook_new() {
    let memory = ShortTermMemory::default();
    let hook = MemoryHook::new(memory);
    // 创建成功即可
    let _ = hook;
}

/// Test 2: MemoryHook.on(SessionStart) 在记忆中添加记录
#[tokio::test]
async fn test_memory_hook_on_session_start() {
    let memory = ShortTermMemory::default();
    let hook = MemoryHook::new(memory);

    // 执行前为空
    let mem_before = hook.memory().lock().unwrap();
    assert_eq!(mem_before.len(), 0);
    drop(mem_before);

    hook.on(HookPoint::SessionStart).await.unwrap();

    // 执行后有 1 条记录
    let mem_after = hook.memory().lock().unwrap();
    assert_eq!(mem_after.len(), 1);
    assert_eq!(mem_after.entries()[0].content, "session started");
}

/// Test 3: MemoryHook.on(StepStart) 添加 ToolCall 类型记录
#[tokio::test]
async fn test_memory_hook_on_step_start() {
    let memory = ShortTermMemory::default();
    let hook = MemoryHook::new(memory);

    hook.on(HookPoint::StepStart).await.unwrap();

    let mem = hook.memory().lock().unwrap();
    assert_eq!(mem.len(), 1);
    assert_eq!(mem.entries()[0].content, "step started");
}

/// Test 4: MemoryHook.on(StepEnd) 添加 ToolResult 类型记录
#[tokio::test]
async fn test_memory_hook_on_step_end() {
    let memory = ShortTermMemory::default();
    let hook = MemoryHook::new(memory);

    hook.on(HookPoint::StepEnd).await.unwrap();

    let mem = hook.memory().lock().unwrap();
    assert_eq!(mem.len(), 1);
    assert_eq!(mem.entries()[0].content, "step ended");
}

/// Test 5: MemoryHook 对所有 HookPoint 返回 Ok
#[tokio::test]
async fn test_memory_hook_all_points_ok() {
    let memory = ShortTermMemory::default();
    let hook = MemoryHook::new(memory);

    assert!(hook.on(HookPoint::SessionStart).await.is_ok());
    assert!(hook.on(HookPoint::SessionEnd).await.is_ok());
    assert!(hook.on(HookPoint::StepStart).await.is_ok());
    assert!(hook.on(HookPoint::StepEnd).await.is_ok());
    assert!(hook.on(HookPoint::ToolCalled).await.is_ok());
    assert!(hook.on(HookPoint::LlmResult).await.is_ok());
}
