# 需求与变更登记（唯一 backlog · 需求侧）

> 与 [`defects.md`](./defects.md)（缺陷侧）共同构成唯一 backlog。
>
> **操作约定**：用户提出的**每个**需求 / 功能 / 修改点 / 决策，先在此登记（必填 **P级 + 规模** + 状态），再按铁律执行；执行产物（设计文档 / 代码 / 缺陷编号）回链到对应条目。缺陷的「现状/影响/方向」继续由 `defects.md` 承载，此处只做登记与索引。

## 一、产品需求 / 功能

| 编号 | 需求 | P | 规模 | 状态 | 关联 |
|---|---|---|---|---|---|
| R1 | 无限拓展：目标支持传统/NoSQL/NewSQL/向量/时序等所有数据库，支持自定义拓展 | P1 | 大 | 长期目标（Driver/Dialect/Capabilities 已预留） | architecture.md |
| R2 | 极速启动、极度轻量级、多平台（Win/Linux/macOS） | P1 | 大 | 进行中（Windows 28MB 已验证；Linux 构建已验证） | — |
| R3 | v1 仅支持 MySQL | P1 | 中 | 已实现（M1） | subsystems.md |
| R4 | 技术栈：Rust + 桌面 GUI（Tauri 2） | P1 | 大 | 已定 | architecture.md |
| R5 | 项目/工作区管理：连接必须归属项目 | P2 | 中 | 已实现（M1） | subsystems.md |
| R6 | 项目内所有执行 SQL 自动缓存 + 可搜索历史 | P2 | 中 | 已实现（M1，SQLite FTS5） | subsystems.md |
| R7 | M1 生产级 MySQL：异步驱动 / 流式+取消 / 多连接标签 / 项目 CRUD / keyring / 虚拟网格 / 事务 / 数据编辑导出 / 历史面板 / 危险确认 / SSH+SSL | P1 | 大 | 已实现（M1） | subsystems.md |
| R8 | 侧边栏 schema 管理（库/表 CRUD）+ 右键菜单 + 项目→连接→库表树 | P2 | 中 | 已实现 | — |
| R9 | 大型任务代码走多子代理并行（需求分析后按规模自动执行） | P2 | 中 | 已定（见 D1） | AGENTS.md |
| R10 | 安装 superpowers 开发技能 | P2 | 小 | 已实现 | ~/.dsh/skills/ |
| R11 | 连接生命周期重构：新建连接**自动保存**并归属项目显示；断开**不删除**连接、可复用（重连）；移除侧边栏「已保存连接」专栏；连接节点右键菜单显示 **打开/关闭连接**（方向变更，取代 #6 的「保存连接开关」） | P1 | 中 | 已实现（命令面加 `config_id` 关联 + 前端树改造）；**重连失败反馈 + 凭据补救路径已补（#70）** | commands.rs / state.rs / App.tsx / SchemaTree.tsx / ConnectionDialog.tsx；顺带修复 #49、#70 |
| R12 | 右键菜单完善：(1) 全局屏蔽浏览器原生右键菜单；(2) Schema 树补齐各级节点右键菜单（分类/视图/函数/存储过程/触发器/列）+ 修复视图删除 bug（DROP TABLE → DROP VIEW）；(3) ResultsGrid 新增右键菜单（复制单元格/行/INSERT、设 NULL） | P1 | 大 | 已实现（feat/context-menu-r12，6 commits）| specs/2026-08-19-context-menu-design.md；新增 5 个后端命令（drop_view/drop_routine/drop_trigger/truncate_table/build_insert_sql）+ 表节点「复制名称/清空表」+ ResultsGrid 4 菜单项；**实际简化版**：因 Schema 树暂未实现分类/视图/函数等高级节点，仅为现有表节点补齐菜单，视图/函数菜单待未来 Schema 树扩展时补充 |
| R13 | 表管理与数据编辑增强：(A) 表右键菜单新增"查看 DDL"（插入编辑器）；(B) 可视化表结构编辑器（列定义+索引管理，ALTER 语句显示在侧边栏/底部）；(C) ResultsGrid 工具栏（新增/删除/保存/刷新按钮 + 行多选 checkbox） | P1 | 大 | 待评审 | 拆分为 3 个子任务：A(小)、B(大)、C(中)；依赖 R12 的右键菜单基础设施 |

## 二、流程 / 决策变更

| 编号 | 决策 / 变更 | 落地位置 |
|---|---|---|
| D1 | 铁律从「一律设计+评审」改为「按规模分档：小直接修 / 中评审后串行或并行 / 大评审后多子代理并行」 | AGENTS.md + preset persona |
| D2 | 所有需求/缺陷必须标注优先级（P0–P3）和任务规模（大/中/小），评审排期以此为准 | AGENTS.md + defects.md |
| D3 | 引入本登记表（requirements.md），与 defects.md 共同构成唯一 backlog，先登记再执行 | requirements.md + AGENTS.md |

## 三、状态约定

- 状态流转：`待分析` → `设计中` → `待评审` → `已评审待实现` → `已实现` → `已合并`（长期目标/进行中 等标注同样可用）。
- 编号唯一（R=需求/功能，D=决策/流程）；`P级` 与 `规模` 必填；能指向设计文档或代码时必填「关联」。
- 缺陷本身仍登记在 `defects.md`（带 P级 + 规模 + A/B/C 档位）；本表只在需求/决策侧做索引，不重复维护缺陷明细。
