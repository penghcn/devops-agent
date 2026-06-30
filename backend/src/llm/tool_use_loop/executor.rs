//! 工具执行器 — 注册、分派、批量执行、并行安全分级

use std::collections::HashMap;
use std::pin::Pin;
use std::sync::Arc;

use super::{ToolCallResult, ToolErrorKind};
use crate::llm::{Message, ToolCall};

/// 异步工具函数类型
type ToolFn = Arc<
    dyn Fn(&serde_json::Value) -> Pin<Box<dyn std::future::Future<Output = ToolCallResult> + Send>>
        + Send
        + Sync,
>;

/// 工具安全分级
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParallelSafety {
    Safe,
    CategoryExclusive(&'static str),
    Exclusive,
}

/// 工具注册信息
pub struct ToolRegistration {
    safety: ParallelSafety,
    func: ToolFn,
}

impl ToolRegistration {
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

/// 工具注册表
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

    pub fn register(&mut self, name: &str, reg: ToolRegistration) {
        self.safety.insert(name.to_string(), reg.safety.clone());
        self.tools.insert(name.to_string(), reg.func);
    }

    pub fn safety_for(&self, name: &str) -> ParallelSafety {
        self.safety
            .get(name)
            .cloned()
            .unwrap_or(ParallelSafety::Exclusive)
    }

    pub async fn execute(&self, call: &ToolCall) -> ToolCallResult {
        match self.tools.get(&call.name) {
            Some(tool_fn) => tool_fn(&call.arguments).await,
            None => ToolCallResult::Err(format!("未知工具: {}", call.name)),
        }
    }

    pub async fn execute_batch(&self, calls: &[ToolCall]) -> Vec<Message> {
        let mut results = Vec::new();
        for call in calls {
            let result = self.execute(call).await;
            let msg = match result {
                ToolCallResult::Ok(s) => Message::tool_result_ok(call.id.clone(), s),
                ToolCallResult::Err(e) => {
                    Message::tool_error(call.id.clone(), format!("工具执行错误: {}", e))
                }
            };
            results.push(msg);
        }
        results
    }

    pub async fn execute_with_retry(&self, call: &ToolCall) -> ToolCallResult {
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

    pub fn partition_calls(
        &self,
        calls: &[ToolCall],
    ) -> (Vec<ToolCall>, Vec<ToolCall>) {
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
