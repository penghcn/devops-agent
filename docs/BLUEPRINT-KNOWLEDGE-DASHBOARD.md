# 知识库 + 效能看板 — 架构蓝图

> 确认日期：2026-05-28
> 实施范围：方案 A（一期只读）

---

## 一、已确认的架构决策

| 决策项 | 选择 | 理由 |
|--------|------|------|
| 知识库 | 独立 `knowledge/` 模块 | 与记忆系统生命周期不同，关注点分离 |
| 向量检索 | 混合模式：本地指纹哈希 + 远程 Embedding API | 80% 重复错误哈希可解，Embedding 只处理边缘 |
| 知识质量 | 用户点赞入库 + 置信度衰减 + TTL 淘汰 | 保证质量 + 防止知识腐烂 |
| 效能看板 | 定时物化（5 分钟聚合）+ 4 个核心图表 | 管理视图无需秒级实时，查询快 |
| 前端结构 | Vue Router 多页面，组件按领域分目录 | App.vue 已超 300 行限制 |
| 灰度写入 | 二期（重新构建按钮 → Draft MR → 条件自动合并） | 一期聚焦只读 |
| 权限 | 一个 Jenkins Token + 项目白名单 | 团队内信任边界，可见即可用 |
| 认证 | GitLab OAuth，30 天 Refresh Token | 已有 GitLab 基础设施，无需记新密码 |
| 数据库 | 现有 PostgreSQL（知识/统计/用户）+ SQLite 保留（记忆） | 多用户并发 + pg-vec 直用 |

---

## 二、后端模块新增

```
backend/src/
├── auth/                     # 认证模块（新增）
│   ├── mod.rs
│   ├── jwt.rs                # JWT 签发 + 验证
│   ├── gitlab_oauth.rs       # GitLab OAuth 流程
│   └── middleware.rs         # Axum 中间件：API Key → JWT 双模式
│
├── knowledge/                # 知识库模块（新增）
│   ├── mod.rs
│   ├── fingerprint.rs        # 错误特征码提取（堆栈哈希）
│   ├── embedding.rs          # 远程 Embedding API 调用
│   ├── store.rs              # PostgreSQL 存储（pg-vec）
│   ├── retriever.rs          # 两层检索器（哈希 → Embedding）
│   └── learner.rs            # 知识写入（用户反馈驱动）
│
├── stats/                    # 统计模块（新增）
│   ├── mod.rs
│   ├── collector.rs          # 从构建记录聚合
│   ├── aggregator.rs         # 定时物化任务（5 分钟）
│   └── store.rs              # PostgreSQL 聚合表
│
├── permissions/              # 权限模块（新增）
│   ├── mod.rs
│   ├── config.rs             # 从 TOML 加载项目白名单
│   └── checker.rs            # can_access_project()
│
└── (现有模块保持不变)
```

---

## 三、前端页面结构

```
frontend/src/
├── views/
│   ├── ChatView.vue              # 从 App.vue 拆分
│   ├── DashboardView.vue         # 效能看板（新增）
│   └── LoginView.vue             # GitLab OAuth 登录（新增）
│
├── router/
│   └── index.ts                  # Vue Router（新增）
│
├── components/
│   ├── Chat/
│   │   ├── ChatWindow.vue        # 聊天窗口
│   │   └── StructuredResponse.vue # 结构化响应（迁移）
│   ├── Dashboard/
│   │   ├── FailureRateChart.vue  # 项目失败率排行（横向柱状图）
│   │   ├── ErrorPieChart.vue     # 错误原因占比（饼图）
│   │   ├── TrendChart.vue        # 构建趋势（折线图）
│   │   └── KnowledgeTable.vue    # 知识库命中排行（表格）
│   └── common/
│       └── FeedbackBar.vue       # 解决方案反馈（点赞/点踩）
│
├── api/
│   ├── agent.ts                  # 现有
│   ├── auth.ts                   # 认证 API（新增）
│   ├── stats.ts                  # 统计 API（新增）
│   └── knowledge.ts              # 知识库 API（新增）
```

---

## 四、数据库设计

### PostgreSQL 新表

```sql
-- 用户表
CREATE TABLE users (
    id            SERIAL PRIMARY KEY,
    username      TEXT UNIQUE NOT NULL,
    gitlab_id     TEXT UNIQUE NOT NULL,
    avatar_url    TEXT,
    role          TEXT DEFAULT 'user',  -- admin | user
    refresh_token TEXT UNIQUE,
    token_expires_at TIMESTAMP,
    created_at    TIMESTAMP DEFAULT now()
);

-- 项目授权表
CREATE TABLE user_project_access (
    user_id  INTEGER NOT NULL REFERENCES users(id),
    project  TEXT NOT NULL,
    PRIMARY KEY (user_id, project)
);

-- 知识库表
CREATE TABLE knowledge_entries (
    id            SERIAL PRIMARY KEY,
    fingerprint   TEXT NOT NULL,           -- 错误特征码（SHA256）
    error_text    TEXT NOT NULL,           -- 原始错误文本（脱敏后）
    solution      TEXT NOT NULL,           -- AI 解决方案
    embedding     vector(768),             -- pg-vec，远程 Embedding 结果
    category      TEXT NOT NULL DEFAULT 'other',
    confidence    REAL NOT NULL DEFAULT 0.5,
    hit_count     INTEGER NOT NULL DEFAULT 0,
    confirm_count INTEGER NOT NULL DEFAULT 0,
    deny_count    INTEGER NOT NULL DEFAULT 0,
    source_build  TEXT,                    -- 来源构建号
    created_at    TIMESTAMP DEFAULT now(),
    expires_at    TIMESTAMP DEFAULT (now() + INTERVAL '30 days')
);

CREATE INDEX idx_knowledge_fingerprint ON knowledge_entries(fingerprint);
CREATE INDEX idx_knowledge_embedding ON knowledge_entries USING hnsw (embedding vector_cosine_ops);

-- 统计聚合表（小时级）
CREATE TABLE stats_hourly (
    hour           TIMESTAMP NOT NULL,
    project_name   TEXT NOT NULL,
    total_builds   INTEGER NOT NULL DEFAULT 0,
    failed_builds  INTEGER NOT NULL DEFAULT 0,
    error_category TEXT NOT NULL DEFAULT 'other',
    avg_duration   REAL,
    PRIMARY KEY (hour, project_name, error_category)
);

-- 统计聚合表（天级）
CREATE TABLE stats_daily (
    day            DATE NOT NULL,
    project_name   TEXT NOT NULL,
    total_builds   INTEGER NOT NULL DEFAULT 0,
    failed_builds  INTEGER NOT NULL DEFAULT 0,
    success_rate   REAL,
    PRIMARY KEY (day, project_name)
);
```

---

## 五、检索流程

```
构建日志到来
    ↓
┌─┬──────────────────────────────────┐
│ │ 第一层：指纹哈希精确匹配           │
│ │ 提取堆栈签名 → SHA256 → 哈希查找   │
│ │ 命中率预估：30-40%                 │
│ │ O(1) 毫秒返回                     │
└─┴──────────────────────────────────┘
    │ 未命中
    ↓
┌─┬──────────────────────────────────┐
│ │ 第二层：远程 Embedding 语义检索    │
│ │ 调 DashScope text-embedding-v3    │
│ │ pg-vec cosine similarity Top 3    │
│ │ 延迟 ~200ms                       │
└─┴──────────────────────────────────┘
    │ 未命中
    ↓
BuildAnalysisStep → LLM 分析
    ↓
前端展示 + 反馈按钮
    ↓
用户 👍 → 写入知识库（confidence = 0.5）
用户 👎 → 记录反馈，不写入
```

---

## 六、API 端点新增

```
POST /api/auth/gitlab/login       # GitLab OAuth 跳转
GET  /api/auth/gitlab/callback    # OAuth 回调
POST /api/auth/refresh            # 刷新 Token
POST /api/auth/logout             # 登出

GET  /api/stats/daily?days=7      # 近 7 天每日聚合
GET  /api/stats/categories        # 错误分类占比
GET  /api/stats/top-failures      # 失败率 Top 10 项目
GET  /api/stats/knowledge-top     # 知识库命中排行

POST /api/knowledge/search        # 搜索知识库
POST /api/knowledge/feedback      # 提交反馈（点赞/点踩）
GET  /api/knowledge/entries       # 知识库列表
```

---

## 七、实施计划（方案 A，一期只读）

### Phase 1: 基础设施（Week 1）
- [ ] PostgreSQL 连接配置（config.toml + 迁移脚本）
- [ ] GitLab OAuth 认证流程
- [ ] JWT + Refresh Token 机制
- [ ] 项目白名单权限校验
- [ ] 清理 BuildAnalysis A/B 实验代码

### Phase 2: 知识库（Week 2）
- [ ] 指纹哈希提取器
- [ ] 远程 Embedding 调用
- [ ] pg-vec 存储 + 检索
- [ ] 两层检索器
- [ ] 知识写入（用户反馈驱动）
- [ ] BuildAnalysisStep 接入知识库检索

### Phase 3: 效能看板（Week 3）
- [ ] 前端拆分（Vue Router + 组件迁移）
- [ ] 统计聚合器（定时物化）
- [ ] DashboardView + 4 个图表组件
- [ ] ECharts 集成
- [ ] 统计 API 端点

### Phase 4: 联调 + 清理
- [ ] E2E 测试（登录 → 看板 → 知识库检索）
- [ ] 代码审查
- [ ] README 更新

---

## 八、二期规划（灰度写入）

- [ ] 阶段 1: [重新构建] 按钮
- [ ] 阶段 2: Draft MR 提交流程
- [ ] 阶段 3: 高置信度自动合并（条件性）
