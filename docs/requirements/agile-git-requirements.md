# Vibe Kanban 敏捷与 Git 集成需求分析文档

本文档基于 Vibe Kanban 现有架构（Rust + React + Agents），针对敏捷开发项目管理方向进行深入的需求分析与规划。

## 1. 补全基于敏捷开发的项目管理功能 (Agile Project Management)

**优先级：** P0 (最高)
**目标：** 将 Vibe Kanban 从基础的任务看板升级为支持 Scrum/Kanban 敏捷方法的完整项目管理工具。

### 1.1 冲刺管理 (Sprints/Iterations)
*   **功能描述：** 引入“冲刺”概念，允许用户规划固定周期的开发迭代。
*   **数据模型变更：**
    *   新增 `sprints` 表：包含 `id`, `project_id`, `name`, `goal`, `start_date`, `end_date`, `status` (planned, active, completed, archived)。
    *   `tasks` 表新增 `sprint_id` 字段。
*   **UI/UX 需求：**
    *   **Backlog 视图：** 一个独立于当前看板的待办事项列表，用于存放未规划的任务。
    *   **冲刺规划：** 支持从 Backlog 拖拽任务到“当前冲刺”或“未来冲刺”。
    *   **冲刺切换：** 看板顶部可切换显示“当前冲刺”或其他冲刺的视图。

### 1.2 史诗与用户故事 (Epics & User Stories)
*   **功能描述：** 建立任务的层级结构，支持从宏观业务价值到具体开发任务的拆解。
*   **数据模型变更：**
    *   `tasks` 表新增 `type` 字段 (枚举：Epic, Story, Task, Bug)。
    *   `tasks` 表新增 `epic_id` 字段（关联到作为 Epic 的 Task）。
*   **UI/UX 需求：**
    *   **泳道视图 (Swimlanes)：** 支持按 Epic 分组显示泳道，横向展示 Story/Task 的进度。
    *   **层级展示：** 在卡片上清晰标识其所属的 Epic。

### 1.3 估算与点数 (Estimation)
*   **功能描述：** 支持对任务工作量进行估算，用于计算速度和燃尽图。
*   **数据模型变更：**
    *   `tasks` 表新增 `story_points` (整数) 或 `estimate_minutes` (整数)。
*   **UI/UX 需求：**
    *   卡片上直接显示故事点。
    *   （未来规划）集成简单的扑克估算工具。

---

## 2. AI Agent 驱动的敏捷自动化 (AI Agile Automation)

**优先级：** P0
**目标：** 利用现有 Agent 基础设施，自动化敏捷流程中的繁琐环节，基于当前 GitHub Flow 实现。

### 2.1 智能需求拆解 (Agentic Backlog Grooming)
*   **触发场景：** 用户创建一个只有标题或简略描述的 Story。
*   **Agent 行为：**
    *   分析标题，调用 LLM 根据代码库上下文生成详细的 Acceptance Criteria (验收标准)。
    *   自动拆解为 3-5 个具体的 Subtasks (开发任务)。
    *   根据历史数据尝试预估 Story Points。
*   **实现路径：** 扩展 `crates/executors`，增加 `BacklogGroomer` executor。

### 2.2 AI 辅助冲突解决 (AI Rebase Helper)
*   **触发场景：** Git 操作检测到 Merge Conflict (Rebase 或 Merge 时)。
*   **Agent 行为：**
    *   读取冲突文件和 git status。
    *   分析双方变更意图。
    *   自动生成解决冲突的代码并尝试应用。
    *   运行测试确保无破坏。
*   **实现路径：** 利用 `crates/executors` 中的 coding agent 能力，专注于 `git` 命令交互。

### 2.3 自动化 Release Notes
*   **触发场景：** Sprint 完成或 Release 分支合并。
*   **Agent 行为：**
    *   收集该 Sprint 内所有 `done` 状态的 Tasks。
    *   分析关联的 Git Commits 和 PR 描述。
    *   生成一份结构化、人类可读的版本发布说明。

---

## 3. 协作与评审 (Review-Centric Design)

**优先级：** P1
**目标：** 将代码评审无缝集成到看板中，减少工具切换。

### 3.1 看板内 Diff 审查
*   **功能描述：** 点击任务卡片，直接查看该任务关联分支的代码变更。
*   **技术实现：**
    *   利用 `shared/types.ts` 中的 `Diff` 结构。
    *   后端 `crates/server` 提供获取特定 Branch/Commit diff 的 API。
    *   前端实现类似 GitHub/VS Code 的 Diff Viewer 组件。

### 3.2 双向同步评论
*   **功能描述：** 在 Vibe Kanban 的 Diff 视图中发表评论，自动同步到 GitHub/GitLab PR 评论区；反之亦然。
*   **技术实现：** 扩展 GitHub/GitLab API 客户端，处理 Comment 对象的映射。

---

## 4. 事件驱动架构 (Event-Driven Architecture)

**优先级：** P1
**目标：** 实现看板状态与远程代码仓库的毫秒级同步。

### 4.1 Webhooks 监听器
*   **功能描述：** 在 `crates/server` 中引入 Webhook 接收端点。
*   **支持事件：**
    *   `push`: 更新关联任务的 Commits 信息。
    *   `pull_request`: 更新卡片上的 PR 状态 (Open, Merged, Closed)。
    *   `check_run`: 更新 CI/CD 状态图标。
*   **实现路径：** 使用 `Axum` 路由处理 `/api/webhooks/github` 等请求，验证签名。

### 4.2 实时推送 (Real-time Updates)
*   **机制：** 当 Webhook 更新数据库状态后，通过 Server-Sent Events (SSE) 或 WebSocket 通知前端刷新，无需用户手动刷新页面。

---

## 5. 敏捷效能分析 (Vibe Analytics)

**优先级：** P2
**目标：** 提供数据洞察，优化团队和 Agent 的协作效率。

### 5.1 核心图表
*   **燃尽图 (Burndown Chart):** 追踪 Sprint 剩余工作量。
*   **累积流图 (Cumulative Flow Diagram):** 识别瓶颈（如任务堆积在“Review”阶段）。
*   **周期时间 (Cycle Time):** 任务从“开始”到“完成”的平均耗时。

### 5.2 开发者体验指标 (DevEx)
*   **AI 贡献率：** 统计有多少代码行是由 Agent 生成的 vs 人类编写的。
*   **Hotspot 分析：** 识别哪些模块频繁发生 Bug 修复 (Hotfix)。

---

## 6. 深度 Git 工作流集成 (Deep Git Workflow)

**优先级：** P2
**目标：** 将 Git 分支策略可视化地映射到看板泳道。

### 6.1 泳道即环境 (Lanes as Environments)
*   **概念：** 看板的列不再仅仅是 `TODO -> DOING -> DONE`，而是映射到环境分支 `Local -> Origin/Feature -> Staging -> Production`。
*   **自动化流转：**
    *   将卡片拖入 "Staging" 列 -> 自动触发 Merge to `staging` 分支 -> 触发 CI/CD 部署。
    *   卡片状态显示部署进度（Pending -> Deploying -> Success）。

### 6.2 自动分支策略
*   **功能：** 创建任务时选择类型（Feature/Hotfix），系统自动按 Git Flow 规范创建分支（`feature/xxx`, `hotfix/xxx`）。
