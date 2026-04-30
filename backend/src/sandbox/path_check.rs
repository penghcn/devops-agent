use std::path::Path;

/// 路径校验结果
#[derive(Debug, Clone, PartialEq)]
pub enum PathValidation {
    /// 路径安全
    Ok,
    /// 检测到路径穿越
    TraversalDetected,
    /// 尝试访问敏感文件
    SensitiveFile,
    /// 路径超出工作区
    OutsideWorkspace,
}

/// 路径校验器，负责检测路径穿越、敏感文件访问和工作区边界
#[derive(Clone)]
pub struct PathValidator {
    workspace_root: String,
    sensitive_patterns: Vec<String>,
}

impl PathValidator {
    /// 创建新的路径校验器，使用默认敏感文件模式
    pub fn new(workspace_root: &str) -> Self {
        Self {
            workspace_root: workspace_root.to_string(),
            sensitive_patterns: default_sensitive_patterns(),
        }
    }

    /// 路径校验主入口
    pub fn validate(&self, path: &str) -> PathValidation {
        // 1. 解码 URL 编码
        let decoded = url_decode(path);

        // 2. 检测 ".." 组件（路径穿越）
        if decoded.split('/').any(|c| c == "..") {
            return PathValidation::TraversalDetected;
        }

        // 3. 检查敏感文件模式
        let lower = decoded.to_lowercase();
        for pattern in &self.sensitive_patterns {
            if lower.contains(pattern) {
                return PathValidation::SensitiveFile;
            }
        }

        // 4. 解析为绝对路径，检查是否在 workspace_root 下
        let full_path = if Path::new(path).is_absolute() {
            path.to_string()
        } else {
            format!("{}/{}", self.workspace_root, path)
        };

        if full_path.starts_with(&self.workspace_root) {
            PathValidation::Ok
        } else {
            PathValidation::OutsideWorkspace
        }
    }
}

/// 默认敏感文件模式列表
fn default_sensitive_patterns() -> Vec<String> {
    vec![
        "/etc/passwd".into(),
        "/etc/shadow".into(),
        "/etc/sudoers".into(),
        ".ssh/".into(),
        ".ssh/id_rsa".into(),
        ".ssh/id_dsa".into(),
        ".ssh/authorized_keys".into(),
        ".env".into(),
        ".env.local".into(),
        ".env.production".into(),
        ".aws/credentials".into(),
        ".gnupg/".into(),
        ".npmrc".into(),
    ]
}

/// 简单 URL 解码：处理 %XX 编码
fn url_decode(input: &str) -> String {
    let mut result = String::with_capacity(input.len());
    let mut chars = input.chars();

    while let Some(c) = chars.next() {
        if c == '%' {
            let hex: String = chars.by_ref().take(2).collect();
            if hex.len() == 2
                && let Ok(byte) = u8::from_str_radix(&hex, 16)
            {
                result.push(byte as char);
                continue;
            }
            result.push('%');
            result.push_str(&hex);
        } else if c == '\\' {
            result.push('/');
        } else {
            result.push(c);
        }
    }

    result
}
