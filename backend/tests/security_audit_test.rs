use devops_agent::security::{AuditEntry, AuditLog, PolicyDecision, Role, ToolName};

#[test]
fn test_audit_entry_fields() {
    let entry = AuditEntry {
        id: 1,
        timestamp: chrono::Local::now(),
        role: Role::Admin,
        tool_name: ToolName::Bash,
        decision: PolicyDecision::Allow,
        message: "admin executed bash".to_string(),
    };

    assert_eq!(entry.id, 1);
    assert_eq!(entry.role, Role::Admin);
    assert_eq!(entry.tool_name, ToolName::Bash);
    assert_eq!(entry.decision, PolicyDecision::Allow);
    assert_eq!(entry.message, "admin executed bash");
}

#[test]
fn test_audit_log_new_creates_empty_log() {
    let log = AuditLog::new();
    assert!(log.entries().is_empty());
}

#[test]
fn test_audit_log_record_adds_entry() {
    let log = AuditLog::new();

    log.record(AuditEntry {
        id: 0,
        timestamp: chrono::Local::now(),
        role: Role::Developer,
        tool_name: ToolName::Write,
        decision: PolicyDecision::Allow,
        message: "developer wrote file".to_string(),
    });

    let entries = log.entries();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].id, 1);
    assert_eq!(entries[0].role, Role::Developer);
    assert!(!entries[0].message.is_empty());
}

#[test]
fn test_audit_log_entries_returns_all() {
    let log = AuditLog::new();

    log.record(AuditEntry {
        id: 0,
        timestamp: chrono::Local::now(),
        role: Role::Admin,
        tool_name: ToolName::Read,
        decision: PolicyDecision::Allow,
        message: "entry 1".to_string(),
    });

    log.record(AuditEntry {
        id: 0,
        timestamp: chrono::Local::now(),
        role: Role::Viewer,
        tool_name: ToolName::Write,
        decision: PolicyDecision::Deny,
        message: "entry 2".to_string(),
    });

    let entries = log.entries();
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0].id, 1);
    assert_eq!(entries[1].id, 2);
}

#[test]
fn test_audit_log_thread_safe() {
    let log = AuditLog::new();
    let mut handles = vec![];

    for i in 0..3 {
        let log_clone = log.clone();
        let handle = std::thread::spawn(move || {
            log_clone.record(AuditEntry {
                id: 0,
                timestamp: chrono::Local::now(),
                role: Role::Admin,
                tool_name: ToolName::Bash,
                decision: PolicyDecision::Allow,
                message: format!("thread {}", i),
            });
        });
        handles.push(handle);
    }

    for handle in handles {
        handle.join().unwrap();
    }

    let entries = log.entries();
    assert_eq!(entries.len(), 3);
    // IDs should be auto-incremented uniquely
    assert_eq!(entries[0].id, 1);
    assert_eq!(entries[1].id, 2);
    assert_eq!(entries[2].id, 3);
}
