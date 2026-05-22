//! 工具执行器 — 注册、分派、批量执行、并行安全分级

use std::collections::HashMap;
use std::pin::Pin;
use std::sync::Arc;

use super::{ToolCallResult, ToolErrorKind};

/// 异步工具函数类型
type ToolFn = Arc<
    dyn Fn(&serde_json::Value) -> Pin<Box<dyn std::future::Future<Output = ToolCallResult> + Send>>
        + Send
        + Sync,
>;

/// 工具安全分级
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParallelSafety {
    /// 可安全并行执行
    Safe,
    /// 同类工具互斥（参数为分类名称）
    CategoryExclusive(&'static str),
    /// 完全互斥，必须串行
    Exclusive,
}

/// 工具注册信息（包含安全分级 + 执行函数）
pub struct ToolRegistration {
    safety: ParallelSafety,
    func: ToolFn,
}

impl ToolRegistration {
    /// 注册为安全工具
    pub fn safe<F, Fut>(f: F) -> Self
    where
        F: Fn(&serde_json::Value) -> Fut + Send + Sync + 'static,
        Fut: std::future::Future<Output = ToolCallResult> + Send + 'static,
    {
        Self {
            safety: ParallelSafety::Safe,
            func: Arc::new(move |args: &serde_json::Value| Box::pin(f(args))),
        }
    }

    /// 注册为同类互斥工具
    pub fn category_exclusive<F, Fut>(category: &'static str, f: F) -> Self
    where
        F: Fn(&serde_json::Value) -> Fut + Send + Sync + 'static,
        Fut: std::future::Future<Output = ToolCallResult> + Send + 'static,
    {
        Self {
            safety: ParallelSafety::CategoryExclusive(category),
            func: Arc::new(move |args: &serde_json::Value| Box::pin(f(args))),
        }
    }

    /// 注册为完全互斥工具
    pub fn exclusive<F, Fut>(f: F) -> Self
    where
        F: Fn(&serde_json::Value) -> Fut + Send + Sync + 'static,
        Fut: std::future::Future<Output = ToolCallResult> + Send + 'static,
    {
        Self {
            safety: ParallelSafety::Exclusive,
            func: Arc::new(move |args: &serde_json::Value| Box::pin(f(args))),
        }
    }
}

/// 工具注册表 — 按名称分派 ToolCall 到实际工具函数
#[derive(Default)]
pub struct ToolExecutor {
    tools: HashMap<String, ToolFn>,
    safety: HashMap<String, ParallelSafety>,
}

impl ToolExecutor {
    pub fn new() -> Self {
        Self {
            tools: HashMap::new(),
            safety: HashMap::new(),
        }
    }

    /// 注册工具（含安全分级）
    pub fn register(&mut self, name: &str, reg: ToolRegistration) {
        self.safety.insert(name.to_string(), reg.safety.clone());
        self.tools.insert(name.to_string(), reg.func);
    }

    /// 查询工具的安全等级
    pub fn safety_for(&self, name: &str) -> ParallelSafety {
        self.safety
            .get(name)
            .cloned()
            .unwrap_or(ParallelSafety::Exclusive)
    }

    /// 执行一个 ToolCall，返回工具结果。
    pub async fn execute(&self, call: &crate::llm::ToolCall) -> ToolCallResult {
        match self.tools.get(&call.name) {
            Some(tool_fn) => tool_fn(&call.arguments).await,
            None => ToolCallResult::Err(format!("未知工具: {}", call.name)),
        }
    }

    /// 批量执行一轮所有 tool_calls，返回 Message::ToolResult 列表。
    pub async fn execute_batch(&self, calls: &[crate::llm::ToolCall]) -> Vec<crate::llm::Message> {
        let mut results = Vec::new();
        for call in calls {
            let result = self.execute(call).await;
            let content = match result {
                ToolCallResult::Ok(s) => s,
                ToolCallResult::Err(e) => format!("工具执行错误: {}", e),
            };
            results.push(crate::llm::Message::ToolResult {
                tool_call_id: call.id.clone(),
                content,
            });
        }
        results
    }

    /// 执行工具调用并自动重试（默认按 Unknown 策略重试 3 次）
    pub async fn execute_with_retry(&self, call: &crate::llm::ToolCall) -> ToolCallResult {
        let kind = ToolErrorKind::Unknown;
        let max = kind.max_attempts();
        for attempt in 0..max {
            let result = self.execute(call).await;
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
        self.execute(call).await
    }

    /// 按安全分级将 tool_calls 分为可并行和需串行两组
    pub fn partition_calls(
        &self,
        calls: &[crate::llm::ToolCall],
    ) -> (Vec<crate::llm::ToolCall>, Vec<crate::llm::ToolCall>) {
        let mut safe = Vec::new();
        let mut exclusive = Vec::new();
        for call in calls {
            let safety = self.safety_for(&call.name);
            match safety {
                ParallelSafety::Safe => safe.push(call.clone()),
                ParallelSafety::CategoryExclusive(_) | ParallelSafety::Exclusive => {
                    exclusive.push(call.clone());
                }
            }
        }
        (safe, exclusive)
    }
}
