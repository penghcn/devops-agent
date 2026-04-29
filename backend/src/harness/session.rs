use chrono::{DateTime, Utc};
use uuid::Uuid;

/// Session 状态枚举
#[derive(Debug, Clone, PartialEq)]
pub enum SessionStatus {
    Active,
    Paused,
    Completed,
    Failed,
}

/// Session — 会话生命周期管理
pub struct Session {
    pub id: Uuid,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub status: SessionStatus,
}

impl Session {
    pub fn new() -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::new_v4(),
            created_at: now,
            updated_at: now,
            status: SessionStatus::Active,
        }
    }

    pub fn complete(&mut self) {
        self.updated_at = Utc::now();
        self.status = SessionStatus::Completed;
    }

    pub fn fail(&mut self) {
        self.updated_at = Utc::now();
        self.status = SessionStatus::Failed;
    }

    pub fn pause(&mut self) {
        self.updated_at = Utc::now();
        self.status = SessionStatus::Paused;
    }
}

impl Default for Session {
    fn default() -> Self {
        Self::new()
    }
}
