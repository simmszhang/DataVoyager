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
- **所有 SQL 执行**（手动 + 工具生成）必须经引擎 `execute*` 统一归因到历史（`origin`：manual_editor / data_edit / schema_edit / export）。
- **`Value` 类型系统**：跨进程用带 tag 的 JSON envelope（`{"t": ..., "v": ...}`），前端据此着色/格式化/选编辑控件，不要绕过它。

## 工作流铁律（不可跳过）

1. **设计先行**：任何功能/修复，先写设计文档（`docs/design/`），经人评审通过后，才允许写代码。
2. **评审门禁**：未经评审的方案一律不落代码；评审意见要落实到设计文档，再实现。
3. **缺陷清单是唯一 backlog**：`docs/design/defects.md` 是当前待评审 + 待修复的权威清单，不另起炉灶。

## 当前状态

- M0（地基）+ M1（生产级 MySQL 全部功能）**已实现，但未经设计评审**——这是在"无文档无评审直接写代码"下完成的，已被用户指出存在设计缺陷。
- `docs/design/defects.md` 列出了 **20 项已知缺陷**（🔴高 4 / 🟡中 12 / 🟢低 4）。
- **评审通过前禁止修改代码**，唯一例外是缺陷清单中经用户明确批准的修复项。
- 修复顺序建议：🔴 → 🟡 → 🟢；其中 `#9`（SSH 指纹）、`#5`（秒断取消）、`#4`（前端方言感知）、`#1`（类型映射）是架构级缺陷，需一次性定方案避免返工。

## 新会话启动检查清单

1. 读 `docs/design/README.md` → `defects.md`，确认当前评审/修复进度。
2. 若有"已评审、待修复"的方案，按方案执行修复 + 测试。
3. 若有新需求：先写设计文档 → 评审 → 再实现（回到铁律 1）。
4. 更新 `docs/design/defects.md`（修复完成则勾掉/降级），并同步本文件的"当前状态"。

## 相关文档

- `docs/design/architecture.md` — 总体架构与核心抽象
- `docs/design/subsystems.md` — 各子系统设计
- `docs/design/defects.md` — 缺陷清单（评审核心）
- `deploy/database/README.md` — 本地 MySQL 集成测试配方
- `deploy/linux/README.md` — Linux 跨平台构建验证
