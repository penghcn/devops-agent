use serde::{Deserialize, Serialize};

/// 调用者角色
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum Role {
    Admin,
    Developer,
    Viewer,
}

/// 工具名称
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum ToolName {
    Read,
    Write,
    Bash,
    Git,
}

/// 策略决策结果
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum PolicyDecision {
    Allow,
    Deny,
    Prompt,
}

/// 工具调用请求
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolRequest {
    pub role: Role,
    pub tool_name: ToolName,
    pub target_path: Option<String>,
    pub arguments: Vec<String>,
}

impl ToolRequest {
    pub fn new(
        role: Role,
        tool_name: ToolName,
        target_path: Option<String>,
        arguments: Vec<String>,
    ) -> Self {
        Self {
            role,
            tool_name,
            target_path,
            arguments,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_role_has_three_variants() {
        let admin = Role::Admin;
        let dev = Role::Developer;
        let viewer = Role::Viewer;

        assert_ne!(admin, dev);
        assert_ne!(dev, viewer);
        assert_ne!(admin, viewer);
    }

    #[test]
    fn test_tool_name_has_four_variants() {
        let r = ToolName::Read;
        let w = ToolName::Write;
        let b = ToolName::Bash;
        let g = ToolName::Git;

        assert_ne!(r, w);
        assert_ne!(w, b);
        assert_ne!(b, g);
        assert_ne!(r, g);
    }

    #[test]
    fn test_policy_decision_has_three_variants() {
        let allow = PolicyDecision::Allow;
        let deny = PolicyDecision::Deny;
        let prompt = PolicyDecision::Prompt;

        assert_ne!(allow, deny);
        assert_ne!(deny, prompt);
        assert_ne!(allow, prompt);
    }

    #[test]
    fn test_tool_request_new_sets_all_fields() {
        let req = ToolRequest::new(
            Role::Admin,
            ToolName::Bash,
            Some("/tmp/test".to_string()),
            vec!["ls".to_string(), "-la".to_string()],
        );

        assert_eq!(req.role, Role::Admin);
        assert_eq!(req.tool_name, ToolName::Bash);
        assert_eq!(req.target_path, Some("/tmp/test".to_string()));
        assert_eq!(req.arguments.len(), 2);
        assert_eq!(req.arguments[0], "ls");
        assert_eq!(req.arguments[1], "-la");
    }

    #[test]
    fn test_tool_request_optional_path() {
        let req = ToolRequest::new(
            Role::Viewer,
            ToolName::Read,
            None,
            vec![],
        );

        assert_eq!(req.target_path, None);
        assert!(req.arguments.is_empty());
    }
}
