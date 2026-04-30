use std::fs;
use std::path::Path;

use crate::sandbox::{FileSystemIsolator, PathValidator};
use crate::security::policy::PolicyEngine;
use crate::security::roles::{PolicyDecision, ToolName, ToolRequest};

use super::{Tool, ToolInput, ToolOutput};

/// 安全文件读取工具
pub struct ReadTool {
    validator: PathValidator,
    isolator: FileSystemIsolator,
    policy_engine: PolicyEngine,
    /// 最大文件大小，默认 10MB
    pub max_file_bytes: usize,
}

impl ReadTool {
    pub fn new(
        validator: PathValidator,
        isolator: FileSystemIsolator,
        policy_engine: PolicyEngine,
    ) -> Self {
        Self {
            validator,
            isolator,
            policy_engine,
            max_file_bytes: 10 * 1024 * 1024, // 10MB
        }
    }

    pub fn with_max_bytes(
        validator: PathValidator,
        isolator: FileSystemIsolator,
        policy_engine: PolicyEngine,
        max_file_bytes: usize,
    ) -> Self {
        Self {
            validator,
            isolator,
            policy_engine,
            max_file_bytes,
        }
    }
}

#[async_trait::async_trait]
impl Tool for ReadTool {
    fn name(&self) -> &str {
        "Read"
    }

    async fn execute(&self, input: &ToolInput) -> ToolOutput {
        let raw_path = match &input.path {
            Some(p) => p.clone(),
            None => return ToolOutput::fail("缺少文件路径".into()),
        };

        // 策略检查
        let request = ToolRequest::new(
            input.user_role,
            ToolName::Read,
            Some(raw_path.clone()),
            Vec::new(),
        );
        let decision = self.policy_engine.check(&request);
        if decision != PolicyDecision::Allow {
            return ToolOutput::fail(format!("策略拒绝: {:?} - 无权执行 Read", decision));
        }

        // 路径校验：拦截穿越和敏感文件
        let validation = self.validator.validate(&raw_path);
        if validation != crate::sandbox::PathValidation::Ok {
            return ToolOutput::fail(format!("路径校验失败: {:?}", validation));
        }

        // 解析为绝对路径：相对路径基于 workspace_root
        let path = if Path::new(&raw_path).is_absolute() {
            raw_path.clone()
        } else {
            format!("{}/{}", self.validator.workspace_root(), raw_path)
        };

        // 文件系统隔离：检查是否在 workspace 内
        if !self.isolator.can_read(&path) {
            return ToolOutput::fail("文件不在工作区内".into());
        }

        // 检查文件大小
        let metadata = match fs::metadata(&path) {
            Ok(m) => m,
            Err(e) => return ToolOutput::fail(format!("无法访问文件: {}", e)),
        };

        if metadata.len() as usize > self.max_file_bytes {
            return ToolOutput::fail(format!(
                "文件过大: {} bytes (限制 {} bytes)",
                metadata.len(),
                self.max_file_bytes
            ));
        }

        // 读取文件内容
        match fs::read_to_string(&path) {
            Ok(content) => ToolOutput::success(content),
            Err(e) => ToolOutput::fail(format!("读取文件失败: {}", e)),
        }
    }
}
