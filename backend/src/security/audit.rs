use std::sync::{Arc, Mutex};

use chrono::DateTime;
use serde::{Deserialize, Serialize};

use super::roles::{PolicyDecision, Role, ToolName};

/// 审计日志条目
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEntry {
    pub id: u64,
    pub timestamp: DateTime<chrono::Local>,
    pub role: Role,
    pub tool_name: ToolName,
    pub decision: PolicyDecision,
    pub message: String,
}

/// 线程安全的审计日志记录器
#[derive(Debug, Clone)]
pub struct AuditLog {
    entries: Arc<Mutex<Vec<AuditEntry>>>,
    next_id: Arc<Mutex<u64>>,
}

impl Default for AuditLog {
    fn default() -> Self {
        Self::new()
    }
}

impl AuditLog {
    /// 创建空的审计日志
    pub fn new() -> Self {
        Self {
            entries: Arc::new(Mutex::new(Vec::new())),
            next_id: Arc::new(Mutex::new(1)),
        }
    }

    /// 记录一条审计日志条目，自动设置 id 和 timestamp
    ///
    /// 注意：message 仅记录工具名和决策，不记录命令参数内容（T-02-04 缓解）。
    pub fn record(&self, mut entry: AuditEntry) {
        let mut next_id = self.next_id.lock().unwrap();
        entry.id = *next_id;
        entry.timestamp = chrono::Local::now();
        *next_id += 1;

        self.entries.lock().unwrap().push(entry);
    }

    /// 获取所有审计日志条目（克隆）
    pub fn entries(&self) -> Vec<AuditEntry> {
        self.entries.lock().unwrap().clone()
    }
}
