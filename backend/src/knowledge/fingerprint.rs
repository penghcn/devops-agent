//! 错误特征码提取。
//!
//! 从构建日志中提取错误堆栈签名，生成 SHA256 指纹。

use sha2::{Digest, Sha256};

/// 从构建日志提取错误指纹
pub fn extract_fingerprint(log: &str) -> String {
    let normalized = normalize_log(log);
    sha256_hex(&normalized)
}

/// 从错误文本提取分类
pub fn classify_error(error_text: &str) -> String {
    let lower = error_text.to_lowercase();

    if lower.contains("cannot find")
        || lower.contains("undefined reference")
        || lower.contains("use of undeclared")
        || lower.contains("expected")
        || lower.contains("syntax error")
        || lower.contains("no such file")
    {
        "compile".to_string()
    } else if lower.contains("test")
        || lower.contains("assertion")
        || lower.contains("failed assertion")
        || lower.contains("panic")
    {
        "test".to_string()
    } else if lower.contains("timeout")
        || lower.contains("connection refused")
        || lower.contains("network")
        || lower.contains("dns")
        || lower.contains("unable to resolve")
    {
        "network".to_string()
    } else if lower.contains("dependency")
        || lower.contains("resolution")
        || lower.contains("could not find")
        || lower.contains("no matching version")
        || lower.contains("nexus")
        || lower.contains("maven")
    {
        "dependency".to_string()
    } else if lower.contains("permission denied")
        || lower.contains("access denied")
        || lower.contains("forbidden")
    {
        "permission".to_string()
    } else if lower.contains("disk")
        || lower.contains("no space")
        || lower.contains("out of memory")
        || lower.contains("oom")
    {
        "resource".to_string()
    } else {
        "other".to_string()
    }
}

/// 规范化日志：去除行号、绝对路径、时间戳
fn normalize_log(log: &str) -> String {
    let mut lines: Vec<String> = Vec::new();

    for line in log.lines() {
        let mut normalized = line
            // 去除行号模式 :123 或 :123:
            .replace(|c: char| c.is_ascii_digit(), "")
            // 去除绝对路径
            .replace(|c: char| c == '/', "")
            .trim()
            .to_string();

        if !normalized.is_empty() {
            lines.push(normalized);
        }
    }

    // 取最后 50 行（错误通常在末尾）
    let tail = if lines.len() > 50 {
        lines.split_off(lines.len() - 50)
    } else {
        lines
    };

    tail.join("\n")
}

fn sha256_hex(input: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(input.as_bytes());
    let result = hasher.finalize();
    format!("{:x}", result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fingerprint_deterministic() {
        let log = "error[E0425]: cannot find value `x` in this scope\n  --> src/main.rs:12:5";
        let fp1 = extract_fingerprint(log);
        let fp2 = extract_fingerprint(log);
        assert_eq!(fp1, fp2);
    }

    #[test]
    fn test_classify_compile_error() {
        assert_eq!(classify_error("cannot find value `x`"), "compile");
    }

    #[test]
    fn test_classify_network_error() {
        assert_eq!(classify_error("connection refused"), "network");
    }

    #[test]
    fn test_classify_dependency_error() {
        assert_eq!(classify_error("could not find package"), "dependency");
    }
}
