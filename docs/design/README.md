# dby 设计文档

本文档是对**已实现代码**的回溯性设计记录 + 已知缺陷清单，用于评审。目标是：先对齐设计与缺陷，评审通过后再统一修复。

> 阅读顺序建议：`architecture.md`（总体架构与核心抽象）→ `subsystems.md`（各子系统设计）→ `defects.md`（缺陷清单，评审核心）。

| 文档 | 内容 |
| --- | --- |
| [architecture.md](./architecture.md) | 分层架构、核心抽象（Value/Driver/Dialect）、持久化、IPC 数据流 |
| [subsystems.md](./subsystems.md) | M0/M1 各子系统的目标、方案、边界与取舍 |
| [defects.md](./defects.md) | 已知设计缺陷清单（严重度 + 现状 + 建议修复方向） |

## 状态

- 代码已实现（M0 + M1 全部功能），**未经设计评审**。
- 本目录是回溯补写的设计记录，其中 `defects.md` 逐项标出当前实现与理想设计的差距。
- 待评审确认后，按缺陷清单统一修复。
