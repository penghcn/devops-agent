//! 工具重试策略 — 错误分类、退避、提示注入

use super::ToolCallResult;

/// 工具错误类型分类
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolErrorKind {
    /// 超时 — 可重试 5 次，指数退避
    Timeout,
    /// 权限拒绝 — 不可重试
    PermissionDenied,
    /// 资源不存在 — 不可重试
    NotFound,
    /// 网络异常 — 可重试 3 次，固定 3s
    NetworkError,
    /// 解析失败 — 不可重试
    ParseError,
    /// 未知错误 — 可重试 3 次，固定 3s
    Unknown,
}

impl ToolErrorKind {
    /// 是否允许重试
    pub fn is_retriable(self) -> bool {
        matches!(self, Self::Timeout | Self::NetworkError | Self::Unknown)
    }

    /// 最大重试次数（0 = 不可重试）
    pub fn max_attempts(self) -> u32 {
        match self {
            Self::Timeout => 5,
            Self::NetworkError => 3,
            Self::Unknown => 3,
            _ => 0,
        }
    }

    /// 第 N 次重试的退避时间（毫秒）
    pub fn backoff_ms(self, attempt: u32) -> u64 {
        match self {
            Self::Timeout => {
                // 指数退避: 2, 4, 8, 16 秒
                (2_u64).saturating_pow(attempt + 1) * 1000
            }
            Self::NetworkError | Self::Unknown => 3000,
            _ => 0,
        }
    }

    /// 生成提示消息（注入回 LLM）
    pub fn hint(self) -> &'static str {
        match self {
            Self::Timeout => "该操作超时，请检查参数或尝试更轻量的替代工具",
            Self::PermissionDenied => "权限不足，请确认当前角色是否允许此操作",
            Self::NotFound => "资源未找到，请检查参数拼写",
            Self::NetworkError => "网络异常，请重试或考虑降级方案",
            Self::ParseError => "输出格式不匹配，请严格遵循 JSON Schema",
            Self::Unknown => "操作失败，请分析错误信息并调整策略",
        }
    }
}

/// 重试策略（根据错误类型查询重试参数）
pub struct RetryPolicy;

impl RetryPolicy {
    /// 根据错误类型执行带重试的工具调用
    pub async fn execute_with_retry<F>(kind: ToolErrorKind, f: F) -> ToolCallResult
    where
        F: Fn() -> ToolCallResult,
    {
        let max = kind.max_attempts();
        if max == 0 {
            return f();
        }

        for attempt in 0..max {
            let result = f();
            match result {
                ToolCallResult::Ok(_) => return result,
                ToolCallResult::Err(msg) if attempt == max - 1 => {
                    return ToolCallResult::Err(format!(
                        "{} (重试 {} 次后仍失败, 提示: {})",
                        msg,
                        max,
                        kind.hint()
                    ));
                }
                ToolCallResult::Err(_) => {
                    let delay = kind.backoff_ms(attempt);
                    if delay > 0 {
                        tokio::time::sleep(std::time::Duration::from_millis(delay)).await;
                    }
                }
            }
        }
        f()
    }
}
