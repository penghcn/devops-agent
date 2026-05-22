//! ToolUseLoop — LLM ↔ 工具调用闭环
//!
//! 负责 LLM 返回 tool_calls → 执行工具 → 结果注入 → 再次调用 LLM 的循环，
//! 直到 LLM 返回纯文本或达到最大轮次。

use std::collections::{HashMap, HashSet, VecDeque};
use std::pin::Pin;
use std::sync::Arc;

use super::{ChatRequest, ChatResponse, LlmError, LlmProvider, Message, ToolCall};

/// 工具执行结果
#[derive(Debug, Clone)]
pub enum ToolCallResult {
    Ok(String),
    Err(String),
}

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
    pub async fn execute(&self, call: &ToolCall) -> ToolCallResult {
        match self.tools.get(&call.name) {
            Some(tool_fn) => tool_fn(&call.arguments).await,
            None => ToolCallResult::Err(format!("未知工具: {}", call.name)),
        }
    }

    /// 批量执行一轮所有 tool_calls，返回 Message::ToolResult 列表。
    pub async fn execute_batch(&self, calls: &[ToolCall]) -> Vec<Message> {
        let mut results = Vec::new();
        for call in calls {
            let result = self.execute(call).await;
            let content = match result {
                ToolCallResult::Ok(s) => s,
                ToolCallResult::Err(e) => format!("工具执行错误: {}", e),
            };
            results.push(Message::ToolResult {
                tool_call_id: call.id.clone(),
                content,
            });
        }
        results
    }

    /// 执行工具调用并自动重试（默认按 Unknown 策略重试 3 次）
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

    /// 按安全分级将 tool_calls 分为可并行和需串行两组
    pub fn partition_calls(&self, calls: &[ToolCall]) -> (Vec<ToolCall>, Vec<ToolCall>) {
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

// ── 工具重试策略 ──

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

// ── 死循环检测 ──

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

// ── 信号投票 ──

/// 负面信号类型
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum NegativeSignal {
    /// 重复工具调用
    RepeatedToolCall,
    /// 工具执行失败
    ToolExecutionFailed,
    /// Token 超出预算
    TokenOverBudget,
    /// 内容异常
    ContentAnomaly,
    /// 响应超时
    ResponseTimeout,
    /// 轮次超过阈值
    RoundExceeded,
}

impl NegativeSignal {
    /// 是否为强信号（单独即可引起重视）
    pub fn is_strong(&self) -> bool {
        matches!(
            self,
            NegativeSignal::RepeatedToolCall | NegativeSignal::ToolExecutionFailed
        )
    }
}

/// 信号投票器 — 收集负面信号，决定是否升级
pub struct SignalVoter {
    signals: HashSet<NegativeSignal>,
}

impl SignalVoter {
    pub fn new() -> Self {
        Self {
            signals: HashSet::new(),
        }
    }

    /// 添加负面信号（自动去重）
    pub fn add(&mut self, signal: NegativeSignal) {
        self.signals.insert(signal);
    }

    /// 当前信号数量
    pub fn signal_count(&self) -> usize {
        self.signals.len()
    }

    /// 判断是否需要升级：≥3 个信号且至少一个强信号
    pub fn should_escalate(&self) -> bool {
        self.signals.len() >= 3 && self.signals.iter().any(|s| s.is_strong())
    }
}

// ── tool_search 元工具 ──

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
    pub definition: super::ToolDefinition,
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
    pub fn register(&mut self, name: &str, source: ToolSource, def: super::ToolDefinition) {
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

// ── ToolUseLoop ──

/// ToolUseLoop 执行结果
#[derive(Debug)]
pub struct ToolUseResult {
    /// 最终 LLM 响应（纯文本）
    pub response: ChatResponse,
    /// 完整消息历史（包含所有中间 tool_calls 和 tool_results）
    pub messages: Vec<Message>,
    /// 实际循环轮次
    pub iterations: usize,
}

/// 管理 LLM ↔ 工具调用闭环
pub struct ToolUseLoop {
    provider: Arc<dyn LlmProvider>,
    executor: ToolExecutor,
    request: ChatRequest,
    max_iterations: usize,
}

impl ToolUseLoop {
    pub fn new(
        provider: Arc<dyn LlmProvider>,
        executor: ToolExecutor,
        request: ChatRequest,
    ) -> Self {
        Self {
            provider,
            executor,
            request,
            max_iterations: 15,
        }
    }

    pub fn set_max_iterations(&mut self, max: usize) -> &mut Self {
        self.max_iterations = max;
        self
    }

    /// 执行 tool-use 循环。
    pub async fn execute(self) -> Result<ToolUseResult, LlmError> {
        let mut messages = self.request.messages.clone();

        for iteration in 1..=self.max_iterations {
            let req = ChatRequest {
                model: self.request.model.clone(),
                messages: messages.clone(),
                tools: self.request.tools.clone(),
                temperature: self.request.temperature,
                tool_choice: self.request.tool_choice.clone(),
                stop_sequences: self.request.stop_sequences.clone(),
                prefill: self.request.prefill.clone(),
            };

            let response = self.provider.llm_call(&req).await?;

            if !response.has_tool_calls() {
                return Ok(ToolUseResult {
                    response,
                    messages,
                    iterations: iteration,
                });
            }

            messages.push(Message::Assistant {
                content: response.content.clone(),
                tool_calls: response.tool_calls.clone(),
            });

            let tool_results = self.executor.execute_batch(&response.tool_calls).await;
            messages.extend(tool_results);

            tracing::debug!(
                iteration,
                tool_calls = response.tool_calls.len(),
                "tool-use loop iteration"
            );
        }

        Err(LlmError::ApiError {
            status: 0,
            body: format!("tool-use 循环超过最大轮次限制 ({})", self.max_iterations),
        })
    }
}

// ── 降级交接 ──

/// 降级原因
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FallbackReason {
    /// 信号投票触发升级
    SignalEscalation,
    /// 死循环检测 Level 3
    LoopDetected,
    /// 超过最大轮次
    MaxIterationsExceeded,
    /// 超时
    Timeout,
}

impl std::fmt::Display for FallbackReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FallbackReason::SignalEscalation => write!(f, "信号投票触发降级"),
            FallbackReason::LoopDetected => write!(f, "检测到死循环，触发降级"),
            FallbackReason::MaxIterationsExceeded => write!(f, "超过最大轮次，触发降级"),
            FallbackReason::Timeout => write!(f, "超时，触发降级"),
        }
    }
}

/// 降级执行结果
#[derive(Debug)]
pub struct FallbackResult {
    /// 是否成功
    pub success: bool,
    /// 输出内容
    pub output: String,
    /// 降级原因
    pub reason: Option<FallbackReason>,
    /// Token 消耗
    pub token_usage: u32,
}

impl FallbackResult {
    pub fn success(output: impl Into<String>, token_usage: u32) -> Self {
        Self {
            success: true,
            output: output.into(),
            reason: None,
            token_usage,
        }
    }

    pub fn failure(output: impl Into<String>, reason: FallbackReason) -> Self {
        Self {
            success: false,
            output: output.into(),
            reason: Some(reason),
            token_usage: 0,
        }
    }
}

/// 降级处理器 — 负责将任务交接给 Claude Code CLI
pub struct FallbackHandler {
    /// Claude Code CLI 路径
    claude_path: String,
    /// 超时时间（秒）
    timeout_secs: u64,
}

impl FallbackHandler {
    pub fn new(claude_path: impl Into<String>) -> Self {
        Self {
            claude_path: claude_path.into(),
            timeout_secs: 300, // 默认 5 分钟
        }
    }

    /// 设置超时时间
    pub fn set_timeout_secs(&mut self, secs: u64) -> &mut Self {
        self.timeout_secs = secs;
        self
    }

    /// 获取超时时间
    pub fn timeout_secs(&self) -> u64 {
        self.timeout_secs
    }

    /// 构建降级命令
    pub fn build_command(&self, task: &str, branch: &str) -> Vec<String> {
        vec![
            self.claude_path.clone(),
            "-p".to_string(),
            task.to_string(),
            "--branch".to_string(),
            branch.to_string(),
            "--timeout".to_string(),
            self.timeout_secs.to_string(),
        ]
    }
}

// ── DAG 编排 ──

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
