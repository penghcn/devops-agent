//! 错误特征码提取。
//!
//! 从构建日志中提取错误堆栈签名，生成 SHA256 指纹。
//! 中粒度归一化：保留错误代码和关键字，去除行号和绝对路径。

use regex::Regex;
use sha2::{Digest, Sha256};
use std::sync::LazyLock;

/// 匹配 :行号:列号 模式（如 :12:5）或 :行号（如 :99）
static RE_LINE_NUM: LazyLock<Regex> = LazyLock::new(|| Regex::new(r":\d+(?::\d+)?").unwrap());

/// 匹配绝对路径（如 /home/user/project/src/main.rs）
static RE_ABS_PATH: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?:/[\w.-]+)+").unwrap());

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

/// 规范化日志：去除行号、绝对路径，保留错误代码和关键字。
fn normalize_log(log: &str) -> String {
    let mut lines: Vec<String> = log
        .lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            // 跳过空行和过短行（纯行号等噪音）
            if trimmed.is_empty() || trimmed.len() < 4 {
                return None;
            }
            // 去除 :行号:列号 模式
            let without_line_num = RE_LINE_NUM.replace_all(trimmed, ":");
            // 替换绝对路径为 [PATH]
            let normalized = RE_ABS_PATH.replace_all(&without_line_num, "[PATH]");
            Some(normalized.into_owned())
        })
        .collect();

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
    fn normalize_keeps_error_code() {
        let log = "error[E0425]: cannot find value `x`";
        let normalized = normalize_log(log);
        assert!(normalized.contains("E0425"), "应保留错误代码");
    }

    #[test]
    fn normalize_strips_line_numbers() {
        let log = "--> src/main.rs:12:5";
        let normalized = normalize_log(log);
        assert!(!normalized.contains("12"), "应去除行号 12");
        assert!(!normalized.contains("5") || normalized.contains(":5"), "应去除列号引用");
    }

    #[test]
    fn normalize_replaces_absolute_paths() {
        let log = "/home/user/project/src/main.rs:12:5 error";
        let normalized = normalize_log(log);
        assert!(normalized.contains("[PATH]"), "应替换绝对路径");
        assert!(!normalized.contains("/home"), "不应保留原始路径");
    }

    #[test]
    fn fingerprint_same_error_different_lines() {
        // 同一错误不同行号应产生相同指纹
        let log1 = "error[E0425]: cannot find `x`\n  --> src/main.rs:12:5";
        let log2 = "error[E0425]: cannot find `x`\n  --> src/main.rs:99:3";
        assert_eq!(
            extract_fingerprint(log1),
            extract_fingerprint(log2),
            "同一错误不同行号应产生相同指纹"
        );
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
