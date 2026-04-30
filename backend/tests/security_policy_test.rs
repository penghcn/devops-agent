use devops_agent::security::{PolicyDecision, PolicyEngine, PolicyRule, Role, ToolName, ToolRequest};

#[test]
fn test_admin_allows_all_tools() {
    let engine = PolicyEngine::new();
    let tools = [ToolName::Read, ToolName::Write, ToolName::Bash, ToolName::Git];

    for tool in tools {
        let req = ToolRequest::new(Role::Admin, tool, None, vec![]);
        assert_eq!(engine.check(&req), PolicyDecision::Allow);
    }
}

#[test]
fn test_viewer_allows_read_only() {
    let engine = PolicyEngine::new();

    let read_req = ToolRequest::new(Role::Viewer, ToolName::Read, None, vec![]);
    assert_eq!(engine.check(&read_req), PolicyDecision::Allow);

    let write_req = ToolRequest::new(Role::Viewer, ToolName::Write, None, vec![]);
    assert_eq!(engine.check(&write_req), PolicyDecision::Deny);

    let bash_req = ToolRequest::new(Role::Viewer, ToolName::Bash, None, vec![]);
    assert_eq!(engine.check(&bash_req), PolicyDecision::Deny);

    let git_req = ToolRequest::new(Role::Viewer, ToolName::Git, None, vec![]);
    assert_eq!(engine.check(&git_req), PolicyDecision::Deny);
}

#[test]
fn test_developer_read_write_git_allow_bash_prompt() {
    let engine = PolicyEngine::new();

    let read_req = ToolRequest::new(Role::Developer, ToolName::Read, None, vec![]);
    assert_eq!(engine.check(&read_req), PolicyDecision::Allow);

    let write_req = ToolRequest::new(Role::Developer, ToolName::Write, None, vec![]);
    assert_eq!(engine.check(&write_req), PolicyDecision::Allow);

    let git_req = ToolRequest::new(Role::Developer, ToolName::Git, None, vec![]);
    assert_eq!(engine.check(&git_req), PolicyDecision::Allow);

    let bash_req = ToolRequest::new(Role::Developer, ToolName::Bash, None, vec![]);
    assert_eq!(engine.check(&bash_req), PolicyDecision::Prompt);
}

#[test]
fn test_policy_engine_new_creates_default_rules() {
    let engine = PolicyEngine::new();
    // Default rules should be loaded — engine should not panic on check
    let req = ToolRequest::new(Role::Admin, ToolName::Read, None, vec![]);
    let _ = engine.check(&req);
}

#[test]
fn test_add_rule_overrides_default() {
    let mut engine = PolicyEngine::new();

    // By default, Viewer + Read = Allow
    let req = ToolRequest::new(Role::Viewer, ToolName::Read, None, vec![]);
    assert_eq!(engine.check(&req), PolicyDecision::Allow);

    // Override: Viewer + Read = Deny
    engine.add_rule(PolicyRule {
        role: Role::Viewer,
        tool_name: ToolName::Read,
        decision: PolicyDecision::Deny,
    });

    assert_eq!(engine.check(&req), PolicyDecision::Deny);
}

#[test]
fn test_custom_rule_priority() {
    let mut engine = PolicyEngine::new();

    // Add rule: Developer + Bash = Allow (override default Prompt)
    engine.add_rule(PolicyRule {
        role: Role::Developer,
        tool_name: ToolName::Bash,
        decision: PolicyDecision::Allow,
    });

    let req = ToolRequest::new(Role::Developer, ToolName::Bash, None, vec![]);
    assert_eq!(engine.check(&req), PolicyDecision::Allow);
}
