use devops_agent::harness::{Session, SessionStatus};

/// Test 1: Session::new() 创建时 status == Active，id 为有效 Uuid
#[test]
fn test_session_creation() {
    let session = Session::new();
    assert_eq!(session.status, SessionStatus::Active);
    // UUID v4 应该可以格式化
    let id_str = session.id.to_string();
    assert!(!id_str.is_empty());
    assert_eq!(id_str.len(), 36); // UUID v4 标准格式: xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx
}

/// Test 2: Session.pause() 后 status == Paused
#[test]
fn test_session_pause() {
    let mut session = Session::new();
    session.pause();
    assert_eq!(session.status, SessionStatus::Paused);
}

/// Test 3: Session.complete() 后 status == Completed
#[test]
fn test_session_complete() {
    let mut session = Session::new();
    session.complete();
    assert_eq!(session.status, SessionStatus::Completed);
}

/// Test 4: Session.fail() 后 status == Failed
#[test]
fn test_session_fail() {
    let mut session = Session::new();
    session.fail();
    assert_eq!(session.status, SessionStatus::Failed);
}

/// Test 5: 状态转换后 updated_at 变化
#[test]
fn test_session_updated_at_changes() {
    let mut session = Session::new();
    let created_at = session.created_at;
    let initial_updated = session.updated_at;

    // 验证初始状态
    assert_eq!(created_at, initial_updated);

    // 短暂延迟后改变状态
    std::thread::sleep(std::time::Duration::from_millis(10));
    session.complete();

    // updated_at 应该变化
    assert!(session.updated_at > initial_updated);
    // created_at 不应该变化
    assert_eq!(session.created_at, created_at);
}

/// Test 6: Session Default 实现
#[test]
fn test_session_default() {
    let session = Session::default();
    assert_eq!(session.status, SessionStatus::Active);
}

/// Test 7: 多次状态转换
#[test]
fn test_session_multiple_transitions() {
    let mut session = Session::new();
    assert_eq!(session.status, SessionStatus::Active);

    session.pause();
    assert_eq!(session.status, SessionStatus::Paused);

    session.complete();
    assert_eq!(session.status, SessionStatus::Completed);

    session.fail();
    assert_eq!(session.status, SessionStatus::Failed);
}

/// Test 8: 不同 Session 有不同 ID
#[test]
fn test_session_unique_ids() {
    let session1 = Session::new();
    let session2 = Session::new();
    assert_ne!(session1.id, session2.id);
}
