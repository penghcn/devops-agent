//! 死循环检测 — 基于滑动窗口检测重复工具调用

use std::collections::{HashSet, VecDeque};

use crate::llm::ToolCall;

/// 循环干预级别
#[derive(Debug, Clone)]
pub enum LoopIntervention {
    /// Level 2: 警告注入，提示 LLM 改变策略
    Level2 { message: String },
    /// Level 3: 强制中断，建议降级
    Level3 { message: String },
}

impl LoopIntervention {
    /// 生成注入到 LLM 上下文的消息
    pub fn to_injection_message(&self) -> String {
        match self {
            LoopIntervention::Level2 { message } => {
                format!("[警告] 检测到重复调用模式: {}", message)
            }
            LoopIntervention::Level3 { message } => format!("[强制中断] 检测到死循环: {}", message),
        }
    }
}

/// 死循环检测器 — 基于滑动窗口检测重复工具调用
pub struct LoopDetector {
    window: usize,
    history: VecDeque<Vec<(String, serde_json::Value)>>,
    escalation_count: u32,
}

impl LoopDetector {
    pub fn new(window: usize) -> Self {
        Self {
            window,
            history: VecDeque::new(),
            escalation_count: 0,
        }
    }

    /// 记录一轮工具调用
    pub fn record(&mut self, calls: &[ToolCall]) {
        let signature = calls
            .iter()
            .map(|c| (c.name.clone(), c.arguments.clone()))
            .collect();
        self.history.push_back(signature);
        if self.history.len() > self.window {
            self.history.pop_front();
        }
    }

    /// 检查是否检测到循环。返回干预级别。
    pub fn is_looping(&mut self) -> Option<LoopIntervention> {
        if self.history.len() < 2 {
            return None;
        }

        let last = self.history.back()?.clone();

        // 精确重复检测：检查窗口内是否有完全相同的调用
        let mut repeat_count = 0;
        for entry in self.history.iter() {
            if Self::signatures_match(entry, &last) {
                repeat_count += 1;
            }
        }

        // 也检查工具名频率（参数不同但工具名相同）
        let tool_names: HashSet<&str> = last.iter().map(|(n, _)| n.as_str()).collect();
        let mut name_repeat_count = 0;
        for entry in self.history.iter() {
            let entry_names: HashSet<&str> = entry.iter().map(|(n, _)| n.as_str()).collect();
            if entry_names == tool_names && entry.len() == last.len() {
                name_repeat_count += 1;
            }
        }

        let max_repeat = repeat_count.max(name_repeat_count);

        if max_repeat >= 2 {
            self.escalation_count += 1;

            let desc = Self::describe_pattern(&last);

            if self.escalation_count >= 2 {
                return Some(LoopIntervention::Level3 {
                    message: format!("同一工具调用模式重复 {} 次，建议降级或终止", max_repeat),
                });
            }

            return Some(LoopIntervention::Level2 {
                message: format!("检测到重复工具调用模式: {} (重复 {} 次)", desc, max_repeat),
            });
        }

        None
    }

    fn signatures_match(
        a: &[(String, serde_json::Value)],
        b: &[(String, serde_json::Value)],
    ) -> bool {
        if a.len() != b.len() {
            return false;
        }
        a.iter()
            .zip(b.iter())
            .all(|((n1, v1), (n2, v2))| n1 == n2 && v1.to_string() == v2.to_string())
    }

    fn describe_pattern(calls: &[(String, serde_json::Value)]) -> String {
        calls
            .iter()
            .map(|(n, _)| n.as_str())
            .collect::<Vec<_>>()
            .join(" → ")
    }
}
