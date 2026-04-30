use std::fs;

use crate::sandbox::{FileSystemIsolator, PathValidator};

use super::{Tool, ToolInput, ToolOutput};

/// 安全文件读取工具
pub struct ReadTool {
    validator: PathValidator,
    isolator: FileSystemIsolator,
    /// 最大文件大小，默认 10MB
    pub max_file_bytes: usize,
}

impl ReadTool {
    pub fn new(validator: PathValidator, isolator: FileSystemIsolator) -> Self {
        Self {
            validator,
            isolator,
            max_file_bytes: 10 * 1024 * 1024, // 10MB
        }
    }

    pub fn with_max_bytes(
        validator: PathValidator,
        isolator: FileSystemIsolator,
        max_file_bytes: usize,
    ) -> Self {
        Self {
            validator,
            isolator,
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
        let path = match &input.path {
            Some(p) => p.clone(),
            None => return ToolOutput::fail("缺少文件路径".into()),
        };

        // 路径校验：拦截穿越和敏感文件
        let validation = self.validator.validate(&path);
        if validation != crate::sandbox::PathValidation::Ok {
            return ToolOutput::fail(format!("路径校验失败: {:?}", validation));
        }

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
