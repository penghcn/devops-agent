use std::fs;

use crate::sandbox::{FileSystemIsolator, PathValidator};

use super::{Tool, ToolInput, ToolOutput};

/// 带大小限制的文件写入工具
pub struct WriteTool {
    validator: PathValidator,
    isolator: FileSystemIsolator,
    /// 最大写入大小，默认 5MB
    pub max_content_bytes: usize,
}

impl WriteTool {
    pub fn new(validator: PathValidator, isolator: FileSystemIsolator) -> Self {
        Self {
            validator,
            isolator,
            max_content_bytes: 5 * 1024 * 1024, // 5MB
        }
    }

    pub fn with_max_bytes(
        validator: PathValidator,
        isolator: FileSystemIsolator,
        max_content_bytes: usize,
    ) -> Self {
        Self {
            validator,
            isolator,
            max_content_bytes,
        }
    }
}

#[async_trait::async_trait]
impl Tool for WriteTool {
    fn name(&self) -> &str {
        "Write"
    }

    async fn execute(&self, input: &ToolInput) -> ToolOutput {
        let path = match &input.path {
            Some(p) => p.clone(),
            None => return ToolOutput::fail("缺少文件路径".into()),
        };

        let content = match &input.content {
            Some(c) => c.clone(),
            None => return ToolOutput::fail("缺少写入内容".into()),
        };

        // 路径校验：拦截穿越和敏感文件（防御纵深）
        let validation = self.validator.validate(&path);
        if validation != crate::sandbox::PathValidation::Ok {
            return ToolOutput::fail(format!("路径校验失败: {:?}", validation));
        }

        // 文件系统隔离：检查是否在 output 目录
        if !self.isolator.can_write(&path) {
            return ToolOutput::fail("写入路径不在 output 目录内".into());
        }

        // 检查内容大小
        if content.len() > self.max_content_bytes {
            return ToolOutput::fail(format!(
                "内容过大: {} bytes (限制 {} bytes)",
                content.len(),
                self.max_content_bytes
            ));
        }

        // 写入文件
        match fs::write(&path, &content) {
            Ok(_) => ToolOutput::success("写入成功".into()),
            Err(e) => ToolOutput::fail(format!("写入文件失败: {}", e)),
        }
    }
}
