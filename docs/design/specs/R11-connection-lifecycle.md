# R11 连接生命周期重构

## 需求

新建连接**自动保存**并归属项目显示；断开**不删除**连接、可复用（重连）；移除侧边栏「已保存连接」专栏；连接节点右键菜单显示**打开/关闭连接**。

## 设计方案

### 1. 后端改动

#### 1.1 数据结构

**`ActiveConnection`** (state.rs):
```rust
pub struct ActiveConnection {
    pub id: u64,
    pub name: String,
    pub driver_id: String,
    pub project_id: String,
    pub database: String,
    pub server_version: String,
    pub config_id: Option<String>,  // 新增：关联的配置 ID
    pub params: ConnectParams,
    pub needs_reconnect: bool,
    pub conn: Box<dyn Connection + Send>,
}
```

**`ConnectionSummary`** (commands.rs):
```rust
pub struct ConnectionSummary {
    pub id: u64,
    pub name: String,
    pub driver_id: String,
    pub project_id: String,
    pub database: String,
    pub server_version: String,
    pub config_id: Option<String>,  // 新增：关联的配置 ID
}
```

#### 1.2 命令改动

**`connect` 命令**：
- 如果 `config_id` 为 `None`，自动保存配置并生成 `config_id`
- 如果 `config_id` 为 `Some(id)`，使用现有配置（upsert 模式）
- 返回 `config_id` 给前端

**`disconnect` 命令**：
- 仅关闭连接，不删除配置
- 保留 `config_id` 在配置文件中

**新增 `reconnect` 命令**（如果不存在）：
```rust
#[tauri::command]
pub async fn reconnect(
    state: State<'_, AppState>,
    config_id: String,
) -> Result<ConnectResponse> {
    // 1. 从配置文件读取连接配置
    // 2. 从 keyring 读取密码
    // 3. 调用 connect 逻辑
    // 4. 如果密码缺失，返回错误（前端弹出对话框补录）
}
```

### 2. 前端改动

#### 2.1 SchemaTree 右键菜单

已连接的节点（`activeId === conn.id`）：
- **关闭连接**：调用 `disconnect(conn.id)`，不删除配置

未连接但有 `config_id` 的节点：
- **打开连接**：调用 `reconnect(conn.config_id)`
  - 成功：刷新连接列表，选中该连接
  - 失败（密码缺失）：弹出连接对话框，预填配置，让用户补录密码

#### 2.2 App.tsx

- ✅ 移除侧边栏"已保存连接"模块（已完成）
- 修改 `handleDisconnect`：仅调用 `api.disconnect`，不删除配置
- 新增 `handleReconnect`：调用 `api.reconnect`，失败时弹出对话框

#### 2.3 ConnectionDialog

- 支持预填模式（传入 `config_id` 和配置数据）
- 提交时使用原 `config_id`（upsert）

### 3. 修复的缺陷

- **#6**: 连接自动保存，取代"保存连接开关"
- **#49**: 列表末尾启发式（连接 ID 自增，最新连接在末尾）
- **#70**: 重连失败时弹出对话框补录密码

## 实施顺序

1. ✅ 移除"已保存连接"模块（已完成）
2. 后端：添加 `config_id` 字段到 `ActiveConnection` 和 `ConnectionSummary`
3. 后端：修改 `connect` 命令自动保存配置
4. 后端：确认 `reconnect` 命令存在并正确实现
5. 前端：修改 `ConnectionSummary` 类型定义
6. 前端：SchemaTree 右键菜单添加"打开/关闭连接"
7. 前端：实现 `handleReconnect` 和失败时的对话框补录
8. 测试：连接→断开→重连流程
9. 测试：密码缺失时的补录流程

## 风险

- **向后兼容**：老配置没有 `config_id`，需要在 `connect` 时自动生成
- **密码补录 UX**：失败时的错误提示和对话框预填逻辑需要清晰

## 验收标准

1. 新建连接后，`list_connections` 返回的 `config_id` 不为空
2. 断开连接后，`list_saved_connections` 仍能找到该配置
3. 右键"关闭连接"后，该节点变为未连接状态
4. 右键"打开连接"后，成功重连并选中
5. 密码缺失时，弹出对话框，补录后成功连接
