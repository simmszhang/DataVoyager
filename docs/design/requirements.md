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
