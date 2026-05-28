//! 权限校验器。
//!
//! 管理员全部可见，普通用户只能看到被授权的项目。

use std::collections::{HashMap, HashSet};

/// 权限校验器
pub struct PermissionChecker {
    /// 管理员用户列表
    admin_users: HashSet<String>,
    /// 用户 → 项目映射
    user_projects: HashMap<String, HashSet<String>>,
}

impl PermissionChecker {
    pub fn new(admin_users: Vec<String>, user_projects: Vec<(String, Vec<String>)>) -> Self {
        let mut up = HashMap::new();
        for (user, projects) in user_projects {
            up.insert(user, projects.into_iter().collect());
        }

        Self {
            admin_users: admin_users.into_iter().collect(),
            user_projects: up,
        }
    }

    /// 检查用户是否有权访问项目
    pub fn can_access(&self, username: &str, project: &str) -> bool {
        // 管理员全部可见
        if self.admin_users.contains(username) {
            return true;
        }

        // 检查用户的项目授权
        if let Some(projects) = self.user_projects.get(username) {
            return projects.contains(project);
        }

        false
    }

    /// 获取用户可见的项目列表
    pub fn visible_projects(&self, username: &str) -> Option<Vec<String>> {
        if self.admin_users.contains(username) {
            // 管理员返回所有项目（由调用方提供完整列表）
            return None;
        }

        self.user_projects
            .get(username)
            .map(|projects| projects.iter().cloned().collect())
    }
}

impl Default for PermissionChecker {
    fn default() -> Self {
        Self::new(Vec::new(), Vec::new())
    }
}
