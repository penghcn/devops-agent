use std::sync::Arc;

use super::audit::AuditLog;
use super::roles::{PolicyDecision, Role, ToolName, ToolRequest};

/// 策略规则：指定某个角色对某个工具的决策结果
#[derive(Debug, Clone)]
pub struct PolicyRule {
    pub role: Role,
    pub tool_name: ToolName,
    pub decision: PolicyDecision,
}

/// 策略引擎：维护默认策略表 + 自定义规则，评估工具调用请求
pub struct PolicyEngine {
    rules: Vec<PolicyRule>,
    audit_log: Arc<AuditLog>,
}

impl std::fmt::Debug for PolicyEngine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PolicyEngine")
            .field("rules", &self.rules)
            .finish_non_exhaustive()
    }
}

impl Clone for PolicyEngine {
    fn clone(&self) -> Self {
        Self {
            rules: self.rules.clone(),
            audit_log: self.audit_log.clone(),
        }
    }
}

impl PolicyEngine {
    /// 创建策略引擎，加载默认策略表
    pub fn new(audit_log: Arc<AuditLog>) -> Self {
        let rules = vec![
            // Admin: all tools allowed
            PolicyRule {
                role: Role::Admin,
                tool_name: ToolName::Read,
                decision: PolicyDecision::Allow,
            },
            PolicyRule {
                role: Role::Admin,
                tool_name: ToolName::Write,
                decision: PolicyDecision::Allow,
            },
            PolicyRule {
                role: Role::Admin,
                tool_name: ToolName::Bash,
                decision: PolicyDecision::Allow,
            },
            PolicyRule {
                role: Role::Admin,
                tool_name: ToolName::Git,
                decision: PolicyDecision::Allow,
            },
            // Developer: read/write/git allowed, bash prompts
            PolicyRule {
                role: Role::Developer,
                tool_name: ToolName::Read,
                decision: PolicyDecision::Allow,
            },
            PolicyRule {
                role: Role::Developer,
                tool_name: ToolName::Write,
                decision: PolicyDecision::Allow,
            },
            PolicyRule {
                role: Role::Developer,
                tool_name: ToolName::Bash,
                decision: PolicyDecision::Prompt,
            },
            PolicyRule {
                role: Role::Developer,
                tool_name: ToolName::Git,
                decision: PolicyDecision::Allow,
            },
            // Viewer: read only
            PolicyRule {
                role: Role::Viewer,
                tool_name: ToolName::Read,
                decision: PolicyDecision::Allow,
            },
            PolicyRule {
                role: Role::Viewer,
                tool_name: ToolName::Write,
                decision: PolicyDecision::Deny,
            },
            PolicyRule {
                role: Role::Viewer,
                tool_name: ToolName::Bash,
                decision: PolicyDecision::Deny,
            },
            PolicyRule {
                role: Role::Viewer,
                tool_name: ToolName::Git,
                decision: PolicyDecision::Deny,
            },
        ];

        Self { rules, audit_log }
    }

    /// 评估工具调用请求，返回策略决策
    ///
    /// 自定义规则优先（后添加的优先），无匹配时返回默认策略表结果。
    /// 每次决策自动记录审计日志。
    pub fn check(&self, request: &ToolRequest) -> PolicyDecision {
        // Reverse iterate: last matching rule wins (custom rules override defaults)
        let decision = self
            .rules
            .iter()
            .rev()
            .find(|rule| rule.role == request.role && rule.tool_name == request.tool_name)
            .map(|rule| rule.decision)
            .unwrap_or(PolicyDecision::Deny);

        self.audit_log.record(super::audit::AuditEntry {
            id: 0,
            timestamp: chrono::Local::now(),
            role: request.role,
            tool_name: request.tool_name,
            decision,
            message: format!("{:?} -> {:?}", request.tool_name, decision),
        });

        decision
    }

    /// 添加自定义规则，覆盖默认策略
    ///
    /// 后添加的规则优先于先添加的（包括默认规则）。
    pub fn add_rule(&mut self, rule: PolicyRule) {
        self.rules.push(rule);
    }
}
