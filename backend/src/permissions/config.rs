//! 权限配置。
//!
//! 从 TOML 加载项目白名单。
//! yunli 平铺后: permissions.admin_users.0, permissions.admin_users.1, ...
//!               permissions.user.0.name, permissions.user.0.projects.0, ...

use std::collections::BTreeMap;

use super::checker::PermissionChecker;

/// 从 flat map 中提取权限配置
pub fn load_permissions(conf: &BTreeMap<String, String>) -> PermissionChecker {
    // 提取管理员列表
    let mut admin_users = Vec::new();
    let mut i = 0;
    loop {
        match conf.get(&format!("permissions.admin_users.{}", i)) {
            Some(v) if !v.is_empty() => admin_users.push(v.clone()),
            _ => break,
        }
        i += 1;
    }

    // 提取用户项目授权
    let mut user_projects: Vec<(String, Vec<String>)> = Vec::new();
    let mut j = 0;
    loop {
        let name_key = format!("permissions.user.{}.name", j);
        let name = match conf.get(&name_key) {
            Some(n) if !n.is_empty() => n.clone(),
            None => break,
            _ => break,
        };

        let mut projects = Vec::new();
        let mut k = 0;
        loop {
            let proj_key = format!("permissions.user.{}.projects.{}", j, k);
            match conf.get(&proj_key) {
                Some(p) if !p.is_empty() => projects.push(p.clone()),
                _ => break,
            }
            k += 1;
        }

        if !projects.is_empty() {
            user_projects.push((name, projects));
        }
        j += 1;
    }

    PermissionChecker::new(admin_users, user_projects)
}
