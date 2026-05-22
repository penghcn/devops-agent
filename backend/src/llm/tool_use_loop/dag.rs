//! DAG 编排 — 拓扑排序 → 层级并行 → 节点级重试

use std::collections::{HashMap, HashSet};

/// DAG 节点
#[derive(Debug, Clone)]
pub struct DagNode {
    /// 节点 ID
    pub id: String,
    /// 任务描述
    pub task: String,
    /// 依赖的节点 ID 列表
    pub dependencies: Vec<String>,
}

/// DAG 编排执行结果
#[derive(Debug)]
pub struct DagResult {
    /// 成功节点数
    success_count: usize,
    /// 失败节点数
    failure_count: usize,
    /// 节点结果详情
    details: Vec<(String, bool, String)>,
}

impl DagResult {
    pub fn new() -> Self {
        Self {
            success_count: 0,
            failure_count: 0,
            details: Vec::new(),
        }
    }

    pub fn record(&mut self, id: &str, success: bool, message: &str) {
        if success {
            self.success_count += 1;
        } else {
            self.failure_count += 1;
        }
        self.details
            .push((id.to_string(), success, message.to_string()));
    }

    pub fn success_count(&self) -> usize {
        self.success_count
    }

    pub fn failure_count(&self) -> usize {
        self.failure_count
    }

    pub fn all_success(&self) -> bool {
        self.failure_count == 0
    }
}

/// DAG 编排器 — 拓扑排序 → 层级并行 → 节点级重试
pub struct DagOrchestrator {
    nodes: Vec<DagNode>,
}

impl DagOrchestrator {
    pub fn new(nodes: Vec<DagNode>) -> Self {
        Self { nodes }
    }

    /// 验证 DAG：检查环和缺失依赖
    pub fn validate(&self) -> Result<(), String> {
        // 检查依赖是否存在
        let ids: HashSet<&str> = self.nodes.iter().map(|n| n.id.as_str()).collect();
        for node in &self.nodes {
            for dep in &node.dependencies {
                if !ids.contains(dep.as_str()) {
                    return Err(format!("节点 {} 依赖的 {} 不存在", node.id, dep));
                }
            }
        }

        // 检测环（Kahn 算法）
        let mut in_degree: HashMap<String, usize> = HashMap::new();
        let mut adj: HashMap<String, Vec<String>> = HashMap::new();

        for node in &self.nodes {
            in_degree.entry(node.id.clone()).or_insert(0);
            for dep in &node.dependencies {
                adj.entry(dep.clone()).or_default().push(node.id.clone());
                *in_degree.entry(node.id.clone()).or_insert(0) += 1;
            }
        }

        let mut queue: Vec<String> = in_degree
            .iter()
            .filter(|(_, d)| **d == 0)
            .map(|(k, _)| k.clone())
            .collect();
        let mut visited = 0;

        while let Some(node_id) = queue.pop() {
            visited += 1;
            if let Some(neighbors) = adj.get(&node_id) {
                for neighbor in neighbors {
                    let deg = in_degree.entry(neighbor.clone()).or_insert(0);
                    *deg -= 1;
                    if *deg == 0 {
                        queue.push(neighbor.clone());
                    }
                }
            }
        }

        if visited != self.nodes.len() {
            return Err("DAG 检测到环，无法进行拓扑排序".to_string());
        }

        Ok(())
    }

    /// 计算层级（拓扑排序）：同一层级的节点可以并行执行
    pub fn compute_levels(&self) -> Vec<Vec<&str>> {
        if self.nodes.is_empty() {
            return vec![];
        }

        let mut levels: HashMap<String, usize> = HashMap::new();
        let mut result: Vec<Vec<&str>> = vec![];

        for node in &self.nodes {
            if node.dependencies.is_empty() {
                levels.insert(node.id.clone(), 0);
            } else {
                let max_dep_level = node
                    .dependencies
                    .iter()
                    .filter_map(|d| levels.get(d).copied())
                    .max()
                    .unwrap_or(0);
                levels.insert(node.id.clone(), max_dep_level + 1);
            }
        }

        if levels.is_empty() {
            return result;
        }

        let max_level = *levels.values().max().unwrap();
        for level in 0..=max_level {
            let nodes_at_level: Vec<&str> = self
                .nodes
                .iter()
                .filter(|n| levels.get(&n.id) == Some(&level))
                .map(|n| n.id.as_str())
                .collect();
            result.push(nodes_at_level);
        }

        result
    }
}
