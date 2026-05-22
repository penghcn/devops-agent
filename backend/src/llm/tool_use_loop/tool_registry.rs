//! 工具注册表 — 支持按名称、同义词、分类搜索

use std::collections::{HashMap, HashSet};

use crate::llm::ToolDefinition;

/// 工具来源分类
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolSource {
    /// 内置工具（read/write/bash/git）
    Builtin,
    /// 动态场景工具（Jenkins/GitLab 等）
    Dynamic,
    /// MCP 代理工具
    Mcp,
    /// Skill 包装工具
    Skill,
}

/// 工具搜索结果（包含来源信息）
#[derive(Debug, Clone)]
pub struct ToolSearchResult {
    /// 工具定义
    pub definition: ToolDefinition,
    /// 来源
    pub source: ToolSource,
    /// 分类标签
    pub category: String,
}

impl ToolSearchResult {
    pub fn name(&self) -> &str {
        &self.definition.name
    }
}

/// 工具注册表 — 支持按名称、同义词、分类搜索
pub struct ToolRegistry {
    /// 工具名 → 搜索结果
    tools: HashMap<String, ToolSearchResult>,
    /// 同义词 → 工具名列表
    synonyms: HashMap<String, Vec<String>>,
    /// 分类 → 工具名列表
    categories: HashMap<String, Vec<String>>,
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self {
            tools: HashMap::new(),
            synonyms: HashMap::new(),
            categories: HashMap::new(),
        }
    }

    /// 注册工具
    pub fn register(&mut self, name: &str, source: ToolSource, def: ToolDefinition) {
        let category = Self::infer_category(name);
        let result = ToolSearchResult {
            definition: def,
            source,
            category: category.clone(),
        };
        self.tools.insert(name.to_string(), result);
        self.categories
            .entry(category)
            .or_insert_with(Vec::new)
            .push(name.to_string());
    }

    /// 添加同义词映射
    pub fn add_synonyms(&mut self, tool_name: &str, synonyms: &[&str]) {
        for syn in synonyms {
            self.synonyms
                .entry(syn.to_string())
                .or_insert_with(Vec::new)
                .push(tool_name.to_string());
        }
    }

    /// 搜索工具（精确 → 同义词 → 子串兜底）
    pub fn search(&self, query: &str) -> Vec<ToolSearchResult> {
        let query_lower = query.to_lowercase();
        let mut results = Vec::new();
        let mut seen = HashSet::new();

        // 1. 精确匹配
        if let Some(result) = self.tools.get(query) {
            results.push(result.clone());
            seen.insert(query);
        }

        // 2. 同义词匹配
        if let Some(names) = self.synonyms.get(&query_lower) {
            for name in names {
                if seen.insert(name) {
                    if let Some(result) = self.tools.get(name) {
                        results.push(result.clone());
                    }
                }
            }
        }

        // 3. 子串兜底
        if results.is_empty() {
            for (name, result) in &self.tools {
                if name.to_lowercase().contains(&query_lower) {
                    results.push(result.clone());
                }
            }
        }

        results
    }

    /// 按分类批量召回
    pub fn search_category(&self, category: &str) -> Vec<ToolSearchResult> {
        let mut results = Vec::new();
        if let Some(names) = self.categories.get(category) {
            for name in names {
                if let Some(result) = self.tools.get(name) {
                    results.push(result.clone());
                }
            }
        }
        results
    }

    /// 列出所有工具
    pub fn list_tools(&self) -> Vec<ToolSearchResult> {
        self.tools.values().cloned().collect()
    }

    /// 从工具名推断分类
    fn infer_category(name: &str) -> String {
        if name.starts_with("jenkins") {
            "jenkins".to_string()
        } else if name.starts_with("gitlab") {
            "gitlab".to_string()
        } else if name.starts_with("docker") {
            "docker".to_string()
        } else if name.starts_with("k8s") || name.starts_with("kubectl") {
            "k8s".to_string()
        } else {
            "builtin".to_string()
        }
    }
}
