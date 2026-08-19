# dby 设计文档

本文档是设计/需求/缺陷的权威来源：需求先登记 → 设计先行 → 评审门禁 → 修复/实现闭环。

> 阅读顺序建议：`architecture.md`（总体架构与核心抽象）→ `subsystems.md`（各子系统设计）→ `requirements.md`（需求/变更登记）→ `defects.md`（缺陷清单，评审核心）。

| 文档 | 内容 |
| --- | --- |
| [architecture.md](./architecture.md) | 分层架构、核心抽象、持久化、IPC 数据流、类型映射/错误模型/并发/安全 |
| [subsystems.md](./subsystems.md) | M0/M1 各子系统的目标、方案、边界与取舍 |
| [requirements.md](./requirements.md) | 需求/功能/决策登记（与 defects.md 共同构成唯一 backlog） |
| [defects.md](./defects.md) | 缺陷清单（P级 + 规模 + A/B/C 档位，评审核心） |
| [review.md](./review.md) | 逐层代码审查证据（file:line） |

## 状态

- M0 + M1 已实现；**档位 C（10 个方案组，架构级）已全部实现、评审通过并合并进 `master`**。
- `defects.md` 69 项缺陷已标注 P级 + 规模，分 A/B/C 三档；C 档已清，B/A 档待处理。
- 后续按铁律：需求先登记（`requirements.md`）→ 判规模 → 小直接修 / 中先评审 / 大先定方案再并行。
