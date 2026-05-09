# 实时步骤展示设计

## 背景

SSE 流式响应改造已完成，后端每完成一个 Step 就推送事件。但前端仍将执行步骤折叠在 `<details>` 标签中，用户无法看到实时进度。

## 目标

将执行步骤从折叠改为始终展开，实时显示每步的状态（进行中 / 成功 / 失败 / 中止），让用户像看进度条一样了解构建部署进展。

## 设计

### 效果对比

**之前：**
```
┌─ Agent 消息 ───────────────┐
│ 正在处理...                 │
│ [折叠] 执行步骤             │
└────────────────────────────┘
```

**之后：**
```
┌─ Agent 消息 ───────────────────────┐
│ ⚠️ 原始分支 'de5' 已修正为 'dev'   │
│                                    │
│ ✅ 意图识别: 部署 ds-pkg (0.8s)    │
│ ✅ 预检通过 (1.2s)                 │
│ ⏳ 触发构建... [spinner]           │
│                                    │
│ [结构化结果卡片]                   │
│ 耗时 49秒                          │
└────────────────────────────────────┘
```

### 数据结构变更

`frontend/src/types.ts` — `ChatMessage` 新增 `status` 字段：

```typescript
export interface ChatMessage {
  id: number
  user: string
  agent: string
  steps: AgentStep[]
  structured_output?: Record<string, any>
  branch_correction?: string
  _elapsed?: number
  status?: 'running' | 'success' | 'error'  // 新增
}
```

### UI 变更

#### 1. 步骤列表始终展开

移除 `<details>/<summary>` 折叠结构，步骤列表直接渲染在消息气泡中。

#### 2. 步骤状态图标

每行步骤根据状态显示不同图标：

| 状态 | 图标 | 条件 |
|---|---|---|
| 进行中 | `⏳` + spinner | 有 `action` 但无 `elapsed` |
| 成功 | `✅` | `result` 包含"成功" |
| 失败 | `❌` | `result` 包含"失败" |
| 中止 | `⚠️` | `result` 包含"中止" |
| 其他 | `•` | 默认 |

#### 3. 消息布局顺序

```
1. 分支修正警告（如有）
2. 步骤列表（始终展开）
   - 若步骤为空，显示 "正在处理..." + spinner
3. 结构化结果卡片（如有）
4. 耗时（底部）
```

#### 4. SSE 事件处理

- `step_start` → 在步骤列表末尾追加进行中项（⏳ + spinner）
- `step_done` → 更新对应步骤为完成状态（✅/❌ + 耗时）
- `branch_correction` → 在消息顶部显示警告
- `complete` → 更新结构化结果、耗时、status

### 修改文件

| 文件 | 变更 |
|---|---|
| `frontend/src/types.ts` | `ChatMessage` 加 `status` 字段 |
| `frontend/src/App.vue` | 移除折叠结构，步骤始终展开；添加步骤状态图标逻辑；调整消息布局 |

### 不做什么

- 不改后端 SSE 事件格式
- 不添加 SSE 连接断开重试机制（后续迭代）
- 不添加进度条组件（状态图标已足够）
