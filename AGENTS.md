# AGENTS.md — 本仓库的 Agent 工作约定（每次会话先读此文件）

> 本文件是跨会话的持久约定：无论哪个会话、哪个 agent 接手，都先读这里，再决定做什么。

## 项目是什么

**dby** —— 轻量级、可无限拓展、跨平台的数据库客户端。当前 v1（开发中），仅支持 MySQL。

| 层 | 技术 |
| --- | --- |
| 桌面壳 | [Tauri 2](https://tauri.app)（系统 WebView） |
| 引擎 | `dby-core`（纯 Rust，无 GUI 依赖，可独立 `cargo test`） |
| 前端 | React 19 + TypeScript + Vite + Zustand |
| SQL 编辑器 | CodeMirror 6 |
| MySQL 驱动 | `mysql_async`（异步、流式、可取消） |
| 历史存储 | SQLite（`rusqlite` bundled + FTS5） |
| 凭据 | OS 钥匙串（`keyring`，磁盘 JSON 不存明文） |

## 目录结构（Cargo workspace）

```
dby/
├── crates/
│   ├── dby-core/             # 纯 Rust 引擎（零 Tauri 依赖）
│   │   └── src/              # driver / value / dialect / metadata / query /
│   │                         #   project / config / history / edit / ddl /
│   │                         #   export / danger / error
│   └── dby-driver-mysql/     # MySQL 驱动（impl core traits，含 SSH 隧道 tunnel.rs）
├── src-tauri/                # Tauri 桌面壳（薄层）
│   └── src/                  # main.rs / lib.rs（注册所有 commands）/ state.rs / commands.rs
├── src/                      # React 前端
│   ├── api.ts                # 类型 + invoke 封装
│   ├── store.ts              # Zustand 全局状态（workspaces / tabs）
│   ├── App.tsx
│   └── components/           # 连接对话框 / Schema 树 / 编辑器 / 结果网格 / 历史面板 / 导出
├── deploy/
│   ├── database/mysql/       # docker-compose 测试库（MySQL 5.7 + 8.0）
│   └── linux/                # Linux 跨平台构建镜像
└── docs/design/              # 设计文档 + 缺陷清单（评审核心）
```

## 构建 / 测试 / 运行

前置：Rust stable（Windows 建议 **MSVC** 工具链）、Node 18+、pnpm。

```bash
pnpm install                                   # 前端依赖
cargo test -p dby-core -p dby-driver-mysql     # 纯引擎/驱动单测（无需 WebView/MySQL）
pnpm tauri dev                                 # 开发模式（Vite + cargo run）
pnpm build                                     # 前端构建（tsc && vite build）
pnpm tauri build                               # 打包发布
```

### CI 质量门禁（提交前本地应通过，见 `.github/workflows/ci.yml`）

```bash
cargo fmt --all --check                                # 格式（rustfmt）
cargo clippy --workspace --all-targets -- -D warnings  # 警告即失败
cargo test -p dby-core -p dby-driver-mysql
pnpm build
```

### 集成测试（需真实 MySQL，默认 `--ignored`）

```bash
cd deploy/database/mysql && docker compose up -d mysql80   # 8.0 监听 127.0.0.1:33061
DBY_TEST_MYSQL_PORT=33061 DBY_TEST_MYSQL_PASSWORD=dby-test \
  cargo test -p dby-driver-mysql --test mysql_integration -- --ignored --nocapture
```

连接参数环境变量：`DBY_TEST_MYSQL_HOST/PORT/USER/PASSWORD/DB`（默认 `127.0.0.1:3306`、`root/dby-test/dby_test`）。详见 `deploy/database/README.md`。

## 架构边界（改动必须遵守）

- **三层解耦**：`dby-core`（引擎，零 Tauri/前端依赖）→ `dby-driver-*`（驱动实现）→ `src-tauri`（壳，薄层）→ `src`（前端）。引擎可被 CLI/MCP 复用。
- **新增数据库驱动** = 实现 `Driver` / `Connection` 两个 trait + 在 `src-tauri/src/state.rs` 注册一行。前端只消费 `Value` / `ColumnInfo` / `QueryOutput` 等通用结构，不感知具体库。
- **SQL 生成必须走 `Dialect`**（`quote_identifier` / `quote_string` / `limit_clause`）并落在 `dby-core`，前端只传结构化参数。当前前端存在硬编码 MySQL 反引号（见 defects #4），评审通过前**不得新增此类硬编码**。
- **所有 SQL 执行**（手动 + 工具生成）必须经引擎 `execute*` 统一归因到历史（`origin` 共 8 种：manual_editor / data_edit / schema_edit / export / ai / plugin / cli / other）。
- **`Value` 类型系统**：跨进程用带 tag 的 JSON envelope（`{"t": ..., "v": ...}`），前端据此着色/格式化/选编辑控件，不要绕过它。

## 工作流铁律（不可跳过）

先做需求分析、判断任务规模（大/中/小），再按规模自动选择执行方式：

1. **小型任务**：设计先行（简要方案即可），无需评审门禁，直接改代码 + 测试。
2. **中型任务**：设计先行（写设计文档）→ 评审门禁通过 → 单代理串行实现，或视情况多代理并行。
3. **大型任务**：设计先行（写设计文档）→ 评审门禁通过 → 按独立功能模块拆成多个子任务，多子代理并行实现。

规模判定参考（非硬性，按需判断）：
- **小**：单文件局部改动、简单 bug 修复、文案/配置微调。
- **中**：跨 1–2 个子系统的单条功能/缺陷，边界清晰。
- **大**：架构级改动、新增驱动/子系统、跨多层（core/驱动/壳/前端）或多模块联动。

**标注规范（必须）**：所有需求/缺陷都必须标注 **优先级（P0–P3）** 和 **任务规模（大/中/小）**：
- 优先级：`P0` 阻塞/安全紧急 > `P1` 高 > `P2` 中 > `P3` 低。
- 缺陷清单里每条必须带 `P级 + 规模` 标签，评审与排期以此为准。

**backlog 是唯一权威**：`docs/design/defects.md`（缺陷）+ `docs/design/requirements.md`（需求/变更）共同构成唯一 backlog，不另起炉灶。每个新需求/修改点/缺陷**先登记（带 P级 + 规模）再执行**，执行产物回链到登记条目。

## 当前状态

- M0（地基）+ M1（生产级 MySQL 全部功能）**已实现并完成首轮逐层代码评审**（证据见 `docs/design/review.md`）。
- **档位 C（需统一方案，10 个方案组）已全部实现、评审通过并合并进 `master`**：`#9`（SSH 指纹 TOFU）、`#22`（SSH 凭据 keyring）、`#48`（CSP+ACL）、`#1`（Value 类型映射）、`#4`（方言感知 SQL）、`#5`（取消秒断）、`#24`（SSH 隧道生命周期）、`#28`（多结果集）、`#45`（数值精度）、`#18`（i18n）。TDD + CI 门禁（cargo fmt/clippy/test + pnpm build）+ 集成测试（docker MySQL）全绿。
- `docs/design/defects.md` 现列 **76 项缺陷**：档位 C 的 10 个方案组（及其覆盖的 `#3/#19/#21/#23/#34/#36/#42/#55/#59/#61/#63` 等次级缺陷）已勾掉；档位 B（需评审）与档位 A（直接修）的其余缺陷仍待处理。
- 缺陷修复按铁律分规模执行：小缺陷直接修；中/大缺陷先定方案、评审通过后再修。
- **最近修复**：
  - `#70`（视图同时出现在表和视图节点下，P2 小型）已修复 — 在 MySQL 驱动的 `tables()` 函数中添加 `TABLE_TYPE = 'BASE TABLE'` 过滤条件。
  - `#71`（切换项目后保存的连接不显示，P1 中型）已修复 — 前端加载并合并 `listSavedConnections` 与活动连接，切换项目时更新显示。
  - `#72`（reconnect 静默吞没 keyring 错误 / Windows keyring v3 bug，P1 小型）已修复 — 升级 `keyring 3.6.3 → 4.1.6` 修复 Windows 上新 Entry 实例无法读取密码的 bug，重连时密码不再丢失。
  - `#73`（保存的连接无法删除，P2 小型）已修复 — 在 SchemaTree 连接右键菜单中添加"删除连接"选项，调用后端 API 并刷新连接列表。
  - `#74`（双击占位符连接报错，P2 小型）已修复 — `handleSelectConnection` 检测占位符 ID（-1），自动调用 `handleReconnect` 而不是 `openConnection`。
  - `#75`（Schema 树缺少创建对象的右键菜单，P2 中型）已修复 — 在表/视图/函数/存储过程分组节点添加右键菜单，点击插入对应 DDL 模板到编辑器；数据库节点改为"查看表/视图/函数/存储过程"快速展开功能。
  - `#76`（视图/函数/存储过程/触发器节点缺少"查看 DDL"菜单，P2 小型）已修复 — 添加通用 `show_create_object` 后端命令，前端在各对象节点右键菜单中添加"查看 DDL"选项。
- 后续修复顺序建议：档位 B（🔴/🟡 优先：`#25/#26/#44/#29` 等）→ 档位 A（直接修）。

## 新会话启动检查清单

1. 读 `docs/design/README.md` → `requirements.md` → `defects.md`，确认当前需求登记/评审/修复进度。
2. 若有"已评审、待修复"的方案，按方案执行修复 + 测试。
3. 若有新需求：先需求分析、判断规模（大/中/小），再按铁律执行（小直接改，中/大先设计评审再实现）。
4. 更新 `docs/design/defects.md`（修复完成则勾掉/降级），并同步本文件的"当前状态"。

## 相关文档

- `docs/design/architecture.md` — 总体架构与核心抽象
- `docs/design/subsystems.md` — 各子系统设计
- `docs/design/requirements.md` — 需求/变更登记（唯一 backlog · 需求侧）
- `docs/design/defects.md` — 缺陷清单（评审核心）
- `docs/design/review.md` — 逐层代码审查证据（file:line）
- `deploy/database/README.md` — 本地 MySQL 集成测试配方
- `deploy/linux/README.md` — Linux 跨平台构建验证
