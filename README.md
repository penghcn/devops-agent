## 决策

### 为什么用 Rust 做后端？
- 性能：Claude Code 调用可能耗时，Rust 的异步模型能高效处理并发
- 类型安全：减少运行时错误，面试可展示对 `Result`/`Option` 的处理
- 与 Claude Code 的集成：Rust 的 `Command` 封装比 Python 更健壮

### 为什么让 Claude Code 做 Agent 而非自己实现？
- 复用成熟能力：Claude Code 已有 Subagent、Skill、文件编辑等
- 降低复杂度：200 行 Rust 代码 vs 2000 行自研 Agent 循环
- 可演进性：未来可以替换为其他 CLI Agent（如 OpenAgentic AI）

### 为什么封装 Jenkins 为独立工具？
- 安全：Token 不暴露给 Claude
- 可审计：所有操作有日志
- 可靠性：Rust 的错误处理比 LLM 生成的 curl 更可控

## 架构
```
claude-devops-agent/
├── frontend/      # BUN + TS + Vite + Vue 3 + tailwindcss 前端
│   ├── src/
│   │   ├── App.vue
│   │   ├── components/
│   │   │   ├── ChatWindow.vue
│   │   │   └── ConfigPanel.vue
│   │   └── api/
│   │       └── agent.ts
│   └── package.json
├── backend/                  # Rust 后端
│   ├── src/
│   │   ├── main.rs           # Axum Web 服务入口
│   │   ├── agent/
│   │   │   ├── mod.rs        # Agent 调用逻辑
│   │   │   └── claude.rs     # Claude Code 调用封装
│   │   ├── tools/
│   │   │   ├── jenkins.rs    # Jenkins API 封装
│   │   │   └── gitlab.rs     # GitLab API 封装
│   │   └── config.rs         # 配置管理
│   ├── Cargo.toml
│   └── .env
├── scripts/                  # Claude Code 调用的辅助脚本
│   ├── trigger_jenkins.sh
│   └── check_deploy.sh
├── docker-compose.yml
└── README.md
```

## 部署、测试
```
# 1. 启动 Rust 后端
cd backend
./run-signed.sh

# 2. 启动前端（另一个终端）
cd frontend
bun install
bun run dev

# 3. 访问 http://localhost:5173
```
## 效果图
![预检失败](./images/jda1.png)
![构建成功](./images/jda2.png)