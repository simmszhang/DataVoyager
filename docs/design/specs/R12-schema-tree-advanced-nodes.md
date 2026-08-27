# R12 完整版：Schema 树高级节点补全

> **任务编号**：R12-extended  
> **优先级**：P1  
> **规模**：中型  
> **状态**：设计中  
> **依赖**：R12 简化版（已实现）

## 一、背景

### 当前状态
R12 简化版已实现：
- ✅ 后端：`drop_view/drop_routine/drop_trigger/truncate_table` 命令
- ✅ 前端：表节点右键菜单（重命名/删除/复制名称/清空表/查看 DDL/编辑表结构）
- ✅ ResultsGrid 右键菜单

### 缺失功能
- ❌ 视图（Views）列表查询 + 树节点 + 右键菜单
- ❌ 函数（Functions）列表查询 + 树节点 + 右键菜单
- ❌ 存储过程（Procedures）列表查询 + 树节点 + 右键菜单
- ❌ 触发器（Triggers）列表查询 + 树节点 + 右键菜单

### 目标
补全 Schema 树高级节点，使其支持 MySQL 全部对象类型（表/视图/函数/存储过程/触发器）。

## 二、设计

### 2.1 数据库对象层次结构

```
项目 (Project)
└── 连接 (Connection)
    └── 数据库 (Database)
        ├── 表 (Tables)
        │   ├── 表1 (Table)
        │   │   └── 列 (Columns)
        │   └── 表2
        ├── 视图 (Views)
        │   └── 视图1 (View)
        ├── 函数 (Functions)
        │   └── 函数1 (Function)
        ├── 存储过程 (Procedures)
        │   └── 存储过程1 (Procedure)
        └── 触发器 (Triggers)
            └── 触发器1 (Trigger)
```

### 2.2 后端 API 设计

#### 2.2.1 数据结构（`src-tauri/src/commands.rs`）

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ViewInfo {
    pub name: String,
    pub definer: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoutineInfo {
    pub name: String,
    pub kind: String, // "FUNCTION" | "PROCEDURE"
    pub definer: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TriggerInfo {
    pub name: String,
    pub table: String,
    pub event: String,      // "INSERT" | "UPDATE" | "DELETE"
    pub timing: String,     // "BEFORE" | "AFTER"
    pub definer: Option<String>,
}
```

#### 2.2.2 查询命令

```rust
/// 列出数据库中的所有视图
#[tauri::command]
pub async fn list_views(
    conn_id: i64,
    database: String,
    state: State<'_, AppState>,
) -> Result<Vec<ViewInfo>> {
    // SELECT TABLE_NAME, DEFINER 
    // FROM information_schema.VIEWS 
    // WHERE TABLE_SCHEMA = ?
}

/// 列出数据库中的所有函数
#[tauri::command]
pub async fn list_functions(
    conn_id: i64,
    database: String,
    state: State<'_, AppState>,
) -> Result<Vec<RoutineInfo>> {
    // SELECT ROUTINE_NAME, DEFINER 
    // FROM information_schema.ROUTINES 
    // WHERE ROUTINE_SCHEMA = ? AND ROUTINE_TYPE = 'FUNCTION'
}

/// 列出数据库中的所有存储过程
#[tauri::command]
pub async fn list_procedures(
    conn_id: i64,
    database: String,
    state: State<'_, AppState>,
) -> Result<Vec<RoutineInfo>> {
    // SELECT ROUTINE_NAME, DEFINER 
    // FROM information_schema.ROUTINES 
    // WHERE ROUTINE_SCHEMA = ? AND ROUTINE_TYPE = 'PROCEDURE'
}

/// 列出数据库中的所有触发器（可选按表过滤）
#[tauri::command]
pub async fn list_triggers(
    conn_id: i64,
    database: String,
    table: Option<String>,
    state: State<'_, AppState>,
) -> Result<Vec<TriggerInfo>> {
    // SELECT TRIGGER_NAME, EVENT_OBJECT_TABLE, EVENT_MANIPULATION, ACTION_TIMING, DEFINER
    // FROM information_schema.TRIGGERS 
    // WHERE TRIGGER_SCHEMA = ? [AND EVENT_OBJECT_TABLE = ?]
}

/// 查看视图定义
#[tauri::command]
pub async fn show_create_view(
    conn_id: i64,
    database: String,
    view: String,
    state: State<'_, AppState>,
) -> Result<String> {
    // SHOW CREATE VIEW `database`.`view`
}

/// 查看函数/存储过程定义
#[tauri::command]
pub async fn show_create_routine(
    conn_id: i64,
    database: String,
    name: String,
    kind: String, // "FUNCTION" | "PROCEDURE"
    state: State<'_, AppState>,
) -> Result<String> {
    // SHOW CREATE FUNCTION/PROCEDURE `database`.`name`
}

/// 查看触发器定义
#[tauri::command]
pub async fn show_create_trigger(
    conn_id: i64,
    database: String,
    trigger: String,
    state: State<'_, AppState>,
) -> Result<String> {
    // SHOW CREATE TRIGGER `database`.`trigger`
}
```

### 2.3 前端设计

#### 2.3.1 节点类型扩展（`src/components/SchemaTree.tsx`）

```typescript
type NodeKey =
  | { kind: "conn"; connId: number }
  | { kind: "db"; connId: number; db: string }
  // 分类节点（用于分组展示）
  | { kind: "tables-category"; connId: number; db: string }
  | { kind: "views-category"; connId: number; db: string }
  | { kind: "functions-category"; connId: number; db: string }
  | { kind: "procedures-category"; connId: number; db: string }
  | { kind: "triggers-category"; connId: number; db: string }
  // 对象节点
  | { kind: "table"; connId: number; db: string; table: string }
  | { kind: "column"; connId: number; db: string; table: string; column: string }
  | { kind: "view"; connId: number; db: string; view: string }
  | { kind: "function"; connId: number; db: string; name: string }
  | { kind: "procedure"; connId: number; db: string; name: string }
  | { kind: "trigger"; connId: number; db: string; name: string };
```

#### 2.3.2 UI 结构

**数据库节点展开后显示 5 个分类节点**：
```
📁 test_db
  ├── 📊 表 (3)
  │   ├── users
  │   ├── orders
  │   └── products
  ├── 👁 视图 (2)
  │   ├── active_users
  │   └── order_summary
  ├── ƒ 函数 (1)
  │   └── calculate_total
  ├── ⚙️ 存储过程 (1)
  │   └── update_inventory
  └── ⚡ 触发器 (2)
      ├── users_audit_trigger
      └── orders_timestamp_trigger
```

#### 2.3.3 右键菜单

**视图节点**：
- 查看定义（插入编辑器）
- 复制名称
- 删除视图（调用 `drop_view`）

**函数节点**：
- 查看定义（插入编辑器）
- 复制名称
- 删除函数（调用 `drop_routine`）

**存储过程节点**：
- 查看定义（插入编辑器）
- 复制名称
- 删除存储过程（调用 `drop_routine`）

**触发器节点**：
- 查看定义（插入编辑器）
- 复制名称
- 删除触发器（调用 `drop_trigger`）

### 2.4 i18n 翻译键

```json
{
  "tree": {
    "category": {
      "tables": "表",
      "views": "视图",
      "functions": "函数",
      "procedures": "存储过程",
      "triggers": "触发器"
    },
    "menu": {
      "showCreateView": "查看定义",
      "showCreateRoutine": "查看定义",
      "showCreateTrigger": "查看定义",
      "dropView": "删除视图",
      "dropFunction": "删除函数",
      "dropProcedure": "删除存储过程",
      "dropTrigger": "删除触发器"
    },
    "confirm": {
      "dropView": "确定删除视图「{{name}}」吗？",
      "dropFunction": "确定删除函数「{{name}}」吗？",
      "dropProcedure": "确定删除存储过程「{{name}}」吗？",
      "dropTrigger": "确定删除触发器「{{name}}」吗？"
    }
  }
}
```

## 三、实施计划

### Phase 1: 后端列表命令（1 小时）
1. `commands.rs`: 定义 `ViewInfo/RoutineInfo/TriggerInfo` 结构
2. 实现 `list_views/list_functions/list_procedures/list_triggers` 命令
3. 实现 `show_create_view/show_create_routine/show_create_trigger` 命令
4. `lib.rs`: 注册命令
5. 编译验证

### Phase 2: 前端类型与 API（30 分钟）
1. `api.ts`: 添加 TypeScript 类型 + 封装函数
2. 编译验证

### Phase 3: SchemaTree 扩展（1 小时）
1. 扩展 `NodeKey` 类型（添加分类节点 + 对象节点）
2. 修改 `loadChildren` 函数：
   - 数据库节点展开 → 加载 5 个分类节点（显示计数）
   - 分类节点展开 → 加载对应列表
3. 修改右键菜单逻辑：
   - 添加 view/function/procedure/trigger 菜单项
   - 实现 `handleShowCreate*` 函数（插入编辑器）
   - 实现 `handleDrop*` 函数（调用 API + 刷新）
4. 修改渲染逻辑：
   - 添加分类节点图标
   - 添加对象节点图标

### Phase 4: i18n 翻译（15 分钟）
1. `locales/zh-CN.json`: 添加翻译键
2. `locales/en-US.json`: 添加翻译键
3. 运行 `pnpm check:i18n` 验证

### Phase 5: 测试与验证（15 分钟）
1. 手动测试：
   - 展开数据库 → 验证 5 个分类节点
   - 展开各分类 → 验证对象列表
   - 右键菜单 → 验证查看定义/删除功能
2. 编译验证：
   - `cargo check -p dby`
   - `pnpm build`

**总计**：2.5-3 小时

## 四、非功能性考虑

### 4.1 性能
- 分类节点懒加载：只在展开时查询
- 缓存策略：与现有表/列缓存一致（#66 待修复）

### 4.2 错误处理
- `information_schema` 查询失败 → toast 错误提示
- 删除操作失败 → toast 错误提示 + 不刷新列表

### 4.3 权限
- 查询 `information_schema` 需要 `SELECT` 权限
- 删除对象需要对应的 `DROP` 权限
- 权限不足时 MySQL 返回错误 → 前端显示

### 4.4 兼容性
- MySQL 5.5+（`information_schema` 表存在）
- 触发器查询兼容 MySQL 5.7/8.0

## 五、验收标准

1. ✅ 数据库节点展开后显示 5 个分类节点（表/视图/函数/存储过程/触发器）
2. ✅ 各分类节点显示对象计数（如"表 (3)"）
3. ✅ 展开分类节点后显示对应对象列表
4. ✅ 视图/函数/存储过程/触发器节点右键菜单功能正常
5. ✅ "查看定义"功能：SQL 插入编辑器
6. ✅ "删除"功能：调用后端命令 + 刷新列表
7. ✅ i18n 键对齐检查通过
8. ✅ Rust 和前端编译通过
9. ✅ 无 TypeScript 类型错误
10. ✅ 手动测试通过

## 六、风险与限制

### 风险
- `information_schema` 查询在大库时可能较慢（数千对象）
  - 缓解：懒加载 + 虚拟滚动（未来优化）

### 限制
- 不支持查看列定义（需要额外 `SHOW CREATE TABLE` 解析）
- 不支持对象搜索/过滤（未来增强）
- 分类节点不支持右键菜单（创建对象需要 SQL 编辑器）

## 七、后续增强（超出本次范围）

- [ ] 对象搜索/过滤
- [ ] 列节点右键菜单（复制列名/查看定义）
- [ ] 数据库节点右键菜单（创建数据库/刷新）
- [ ] 对象创建向导（CREATE VIEW/FUNCTION/PROCEDURE/TRIGGER）
- [ ] Schema 对比/同步功能
- [ ] 对象依赖关系可视化

## 八、参考

- `defects.md` #3/#19（已修复）
- `requirements.md` R12
- MySQL `information_schema` 文档：https://dev.mysql.com/doc/refman/8.0/en/information-schema.html
