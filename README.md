# dby

一个**轻量级、可无限拓展、跨平台**的数据库客户端。

- **无限拓展**：所有数据库后端通过统一的 `Driver` / `Connection` 抽象接入，新增一种数据库只需实现两个 trait 并在注册表中登记；第三方扩展走 WASM 插件 SDK（规划中）。
- **极速启动、极度轻量**：Rust + Tauri 2，使用系统 WebView，二进制远小于 Electron 方案。
- **多平台**：Windows / Linux / macOS。
- **项目化管理 + SQL 历史**：连接归属项目；项目内所有执行过的 SQL（手动 + 工具生成）自动落库，可全文检索复用。

> 当前为 v1（开发中），仅支持 **MySQL**。

## 技术栈

| 层 | 技术 |
| --- | --- |
| 桌面壳 | [Tauri 2](https://tauri.app)（系统 WebView） |
| 引擎 | `dby-core`（纯 Rust，无 GUI 依赖，可独立 `cargo test`） |
| 前端 | React 19 + TypeScript + Vite |
| SQL 编辑器 | CodeMirror 6 |
| MySQL 驱动 | [mysql](https://crates.io/crates/mysql) 28（纯 Rust flate2） |
| 历史存储 | SQLite（`rusqlite` bundled + FTS5） |

## 功能

- **项目管理**：项目为顶层实体，连接必须归属某项目；首次启动自动创建"默认项目"。
- **连接管理**：新建 / 测试 / 断开，多连接并存（凭据持久化规划中）。
- **Schema 浏览**：数据库 → 表 → 列（元数据经统一 trait 获取）。
- **SQL 查询编辑器**：语法高亮、自动补全、`Ctrl+Enter` 运行。
- **结果集表格**：无损 `Value` 类型（NULL/数字/二进制/日期/JSON 区分）、类型着色。
- **SQL 历史缓存**：引擎 `execute()` 统一捕获所有执行的 SQL（手动 + 工具生成），SQLite FTS5 全文检索、去重语句库、执行流水。

## 目录结构（Cargo workspace）

```
dby/
├── crates/
│   ├── dby-core/             # 纯 Rust 引擎（零 Tauri 依赖）
│   │   └── src/
│   │       ├── driver.rs     # Driver / Connection trait + 注册表 + 能力矩阵
│   │       ├── value.rs      # 统一 Value 类型系统
│   │       ├── dialect.rs    # SQL 方言抽象 + 语句切分
│   │       ├── metadata.rs   # 表/列/索引/外键/触发器/过程 模型
│   │       ├── query.rs      # ExecOpts / QueryOutput / ResultSet
│   │       ├── project.rs    # 项目模型
│   │       ├── config.rs     # 项目/连接/设置 JSON 持久化
│   │       ├── history.rs    # SQL 历史缓存（SQLite FTS5）
│   │       └── error.rs      # DbError
│   └── dby-driver-mysql/     # MySQL 驱动（impl core traits）
├── src-tauri/                # Tauri 桌面壳（薄层）
│   └── src/
│       ├── main.rs
│       ├── lib.rs            # 入口 + setup（初始化 config/history）
│       ├── state.rs          # AppState（注册表/连接表/配置/历史）
│       └── commands.rs       # Tauri commands
└── src/                      # React 前端
    ├── api.ts                # 类型 + invoke 封装
    ├── App.tsx
    └── components/           # 连接对话框 / Schema 面板 / 编辑器 / 结果表格
```

## 开发

前置要求：Rust（stable）、Node 18+、pnpm，以及平台 WebView（Windows 自带 WebView2）。

```bash
pnpm install                    # 安装前端依赖
cargo test -p dby-core -p dby-driver-mysql   # 纯引擎/驱动测试
pnpm tauri dev                  # 开发模式运行（Vite + cargo run）
pnpm tauri build                # 打包发布
```

> Windows 下建议 CI/发布用 **MSVC** 工具链（`rustup default stable-x86_64-pc-windows-msvc`）；GNU 工具链可开发，但 cdylib 导出序数 / WebView2 测试二进制有已知限制。

## 架构：如何新增一个数据库驱动

可拓展性集中在 `crates/dby-core`（抽象）与 `crates/dby-driver-*`（实现）下。

### 1. 新建驱动 crate

创建 `crates/dby-driver-postgres`，依赖 `dby-core`，实现两个 trait：

```rust
impl Driver for PostgresDriver {
    fn id(&self) -> &'static str { "postgres" }
    fn display_name(&self) -> &'static str { "PostgreSQL" }
    fn capabilities(&self) -> Capabilities { /* 能力矩阵 */ }
    fn dialect(&self) -> &dyn Dialect { &PostgresDialect }
    fn connect(&self, params: &ConnectParams) -> Result<Box<dyn Connection + Send>> { /* ... */ }
}

impl Connection for PostgresConnection {
    fn ping(&mut self) -> Result<()> { /* ... */ }
    fn server_version(&self) -> String { /* ... */ }
    fn catalogs(&mut self) -> Result<Vec<String>> { /* ... */ }
    fn schemas(&mut self, catalog: Option<&str>) -> Result<Vec<String>> { /* ... */ }
    fn tables(&mut self, schema: &str) -> Result<Vec<TableInfo>> { /* ... */ }
    fn columns(&mut self, schema: &str, table: &str) -> Result<Vec<ColumnInfo>> { /* ... */ }
    fn execute(&mut self, schema: Option<&str>, sql: &str, opts: &ExecOpts) -> Result<QueryOutput> { /* ... */ }
    // begin / commit / rollback / cancel ...
}
```

`Connection` 只返回 `ColumnInfo` / `TableInfo` / `Value` / `QueryOutput` 等通用结构，前端与具体数据库完全解耦。

### 2. 注册驱动

在 `src-tauri/src/state.rs` 的 `AppState::new()` 里登记一行即可：

```rust
registry.register(Arc::new(dby_driver_postgres::PostgresDriver));
```

### 面向非 SQL 数据库

Redis、MongoDB、向量库、时序库等同样只需实现 `Connection`：`schemas` / `tables` / `columns` 映射到该库自身的元数据模型，`execute` 映射到其命令/查询接口；`Capabilities.supports_sql = false` 让前端自适应。

## 路线图

- [x] M0：Cargo workspace 拆分、`dby-core` 抽象（Value/Driver/Dialect/Project/Config/History）、MySQL 驱动、可测试地基
- [ ] M1：生产级 MySQL（项目 CRUD、历史面板、流式查询、事务/取消、虚拟滚动、数据编辑/导出、SSH/SSL、危险操作确认）
- [ ] M2：PostgreSQL + SQLite 驱动、SSH 隧道、表结构编辑器、历史聚合
- [ ] M3：WASM 插件 SDK（Extism）+ 更多驱动（Redis/MongoDB/DuckDB…）
- [ ] M4：AI SQL 助手 / MCP / CLI / Docker 自托管
- [ ] M5：1.0 GA（签名、自动更新、性能达标、文档）
