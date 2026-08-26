# R13 表管理与数据编辑增强 - 设计方案

**需求编号**: R13  
**优先级**: P1  
**规模**: 大（拆分为 3 个子任务）  
**状态**: 待评审  
**依赖**: R12（右键菜单基础设施）

---

## 一、需求背景

R12 已实现右键菜单基础设施，但表管理和数据编辑能力仍不完整：
1. 无法快速查看表 DDL
2. 修改表结构需手写 ALTER 语句
3. ResultsGrid 缺少批量操作能力（新增/删除行）

---

##二、需求拆分

### R13-A: 查看表 DDL（小任务）

**功能**: SchemaTree 表节点右键菜单新增"查看 DDL"，点击后将 `SHOW CREATE TABLE` 结果插入查询编辑器。

**用户故事**:
```
用户在侧边栏右键点击表 `users`
→ 点击"查看 DDL"
→ 查询编辑器自动插入：
  CREATE TABLE `users` (
    `id` INT NOT NULL AUTO_INCREMENT,
    `name` VARCHAR(100),
    PRIMARY KEY (`id`)
  ) ENGINE=InnoDB;
```

**工作量**: 30 分钟

---

### R13-B: 可视化表结构编辑器（大任务）

**功能**: 表节点右键菜单新增"编辑表结构"，打开可视化编辑器。

**界面布局**:
```
┌─────────────────────────────────────────────────────────────┐
│ 编辑表结构: database.table_name                   [×] 关闭   │
├─────────────────────────────────────────────────────────────┤
│ [列定义]  [索引]                                             │
├─────────────────────────────────────────────────────────────┤
│ ┌───────────────────────────────────────────────────────┐   │
│ │ ☑ 列名        类型      长度  可空  默认值   备注       │   │
│ │ ☑ id         INT        -    ☐    AUTO_INC  主键      │   │
│ │ ☑ name       VARCHAR   100   ☑    NULL      用户名    │   │
│ │ ☑ email      VARCHAR   255   ☐    ''        邮箱      │   │
│ │                                                         │   │
│ │ [+ 新增列]  [- 删除选中]                                 │   │
│ └───────────────────────────────────────────────────────┘   │
│                                                               │
│ [生成的 SQL 预览] ▼                                          │
│ ┌───────────────────────────────────────────────────────┐   │
│ │ ALTER TABLE `users`                                    │   │
│ │   MODIFY COLUMN `name` VARCHAR(150) NULL,              │   │
│ │   ADD COLUMN `phone` VARCHAR(20) NULL AFTER `email`;   │   │
│ └───────────────────────────────────────────────────────┘   │
│                                                               │
│                          [取消]  [应用并执行]                │
└─────────────────────────────────────────────────────────────┘
```

**功能清单**:

#### 列定义编辑
- 修改列名称、类型、长度、可空性、默认值、备注
- 新增列（指定插入位置：FIRST / AFTER column_name）
- 删除列（多选 checkbox）
- 拖拽调整列顺序（生成 MODIFY COLUMN ... AFTER）

#### 索引管理
- 查看现有索引（主键/唯一键/普通索引/全文索引）
- 新增索引（选择列、指定索引类型）
- 删除索引
- 显示索引涵盖的列

#### 实时 SQL 预览
- 底部或右侧面板显示生成的 `ALTER TABLE` 语句
- 每次修改立即更新预览
- 支持复制 SQL 到剪贴板

#### 应用执行
- "应用并执行"按钮提交所有修改
- 危险操作（删除列/删除索引）二次确认
- 执行失败显示错误信息

**技术方案**:

##### 后端 API
```rust
// commands.rs
#[tauri::command]
pub async fn get_table_structure(
    state: State<'_, Arc<AppState>>,
    id: u64,
    database: String,
    table: String,
) -> Result<TableStructure> {
    // 查询列定义、索引、约束
    // SELECT * FROM information_schema.COLUMNS WHERE ...
    // SHOW INDEX FROM ...
}

#[tauri::command]
pub async fn alter_table(
    state: State<'_, Arc<AppState>>,
    id: u64,
    database: String,
    table: String,
    operations: Vec<AlterOperation>,
    confirmed: bool,
) -> Result<QueryOutput> {
    // 生成并执行 ALTER TABLE 语句
    // 走 guard_dangerous 确认机制
}
```

##### 数据结构
```typescript
interface TableStructure {
  columns: ColumnDefinition[];
  indexes: IndexDefinition[];
}

interface ColumnDefinition {
  name: string;
  type: string;          // INT, VARCHAR, DATETIME, etc.
  length?: number;       // VARCHAR(100) 的 100
  nullable: boolean;
  default_value?: string;
  comment?: string;
  extra?: string;        // AUTO_INCREMENT, ON UPDATE CURRENT_TIMESTAMP
}

interface IndexDefinition {
  name: string;
  type: 'PRIMARY' | 'UNIQUE' | 'INDEX' | 'FULLTEXT';
  columns: string[];
}

type AlterOperation =
  | { type: 'add_column'; column: ColumnDefinition; after?: string }
  | { type: 'modify_column'; old_name: string; new_definition: ColumnDefinition }
  | { type: 'drop_column'; name: string }
  | { type: 'add_index'; index: IndexDefinition }
  | { type: 'drop_index'; name: string };
```

##### 前端组件
- `TableStructureEditor.tsx` - 主编辑器对话框
- `ColumnEditor.tsx` - 单列编辑行组件
- `IndexEditor.tsx` - 索引管理面板
- `SqlPreview.tsx` - SQL 预览区

**工作量**: 3-4 小时

---

### R13-C: ResultsGrid 行级 CRUD 工具栏（中任务）

**功能**: ResultsGrid 顶部添加工具栏，支持新增行、批量删除、刷新等操作。

**界面布局**:
```
┌─────────────────────────────────────────────────────────────┐
│ [+ 新增]  [- 删除]  [💾 保存]  [🔄 刷新]        (243 rows)  │
├─────────────────────────────────────────────────────────────┤
│ ☐  #  │ id  │ name       │ email                │ ...       │
├─────────────────────────────────────────────────────────────┤
│ ☐  1  │ 101 │ Alice      │ alice@example.com    │ ...       │
│ ☑  2  │ 102 │ Bob        │ bob@example.com      │ ...       │  ← 选中
│ ☐  3  │ 103 │ Charlie    │ charlie@example.com  │ ...       │
│ ...                                                           │
└─────────────────────────────────────────────────────────────┘
```

**功能清单**:

#### 工具栏按钮
- **+ 新增**: 在表格底部插入空白可编辑行（标记为 `pending_insert`）
- **- 删除**: 删除所有选中行（生成 `DELETE FROM ... WHERE pk IN (...)`）
- **💾 保存**: 提交所有未保存的新增/编辑（生成 INSERT/UPDATE）
- **🔄 刷新**: 重新执行当前查询（丢弃未保存的修改）

#### 行多选 Checkbox
- 最左侧列固定显示 checkbox（在行号前）
- 表头 checkbox 全选/反选
- 支持 Shift + 点击批量选择
- 选中行高亮显示

#### 新增行交互
1. 点击"+ 新增" → 表格底部出现空白行
2. 双击单元格填写数据
3. 点击"💾 保存" → 后端生成 `INSERT INTO ... VALUES (...)`
4. 成功后刷新表格，失败显示错误

#### 删除行交互
1. 勾选多行（或单行）
2. 点击"- 删除"
3. 二次确认弹窗："确定删除 N 行？"
4. 确认后生成 `DELETE FROM ... WHERE pk IN (...)` 执行
5. **前置条件**: 表必须有主键，否则禁用删除按钮

#### 编辑行交互
- 保留现有双击编辑单元格功能
- 修改后单元格标记为"已修改"（背景色变化）
- 点击"💾 保存"提交所有修改（生成 UPDATE）

**技术方案**:

##### 状态管理
```typescript
// ResultsGrid.tsx
interface RowState {
  type: 'normal' | 'pending_insert' | 'modified';
  originalData?: CellValue[];  // 用于生成 UPDATE WHERE 条件
  currentData: CellValue[];
}

const [selectedRows, setSelectedRows] = useState<Set<number>>(new Set());
const [rowStates, setRowStates] = useState<Map<number, RowState>>(new Map());
```

##### 后端 API
```rust
// commands.rs
#[tauri::command]
pub async fn batch_delete_rows(
    state: State<'_, Arc<AppState>>,
    id: u64,
    database: String,
    table: String,
    pk_column: String,
    pk_values: Vec<String>,  // 主键值列表
    confirmed: bool,
) -> Result<QueryOutput> {
    // DELETE FROM `table` WHERE `pk_column` IN (?, ?, ...)
    // 走 guard_dangerous
}

#[tauri::command]
pub async fn batch_insert_rows(
    state: State<'_, Arc<AppState>>,
    id: u64,
    database: String,
    table: String,
    rows: Vec<Vec<EditCell>>,  // 多行数据
) -> Result<QueryOutput> {
    // INSERT INTO `table` (...) VALUES (...), (...), ...
}

#[tauri::command]
pub async fn get_primary_key(
    state: State<'_, Arc<AppState>>,
    id: u64,
    database: String,
    table: String,
) -> Result<Vec<String>> {
    // 查询主键列名
    // SHOW KEYS FROM `table` WHERE Key_name = 'PRIMARY'
}
```

##### UI 组件
- `GridToolbar.tsx` - 工具栏组件
- `RowCheckbox.tsx` - 行选择 checkbox
- 修改 `ResultsGrid.tsx` 添加行状态管理

**工作量**: 1.5-2 小时

---

## 三、实施优先级

按用户价值和技术依赖排序：

1. **R13-A (查看 DDL)** - 30 分钟，快速见效
2. **R13-C (CRUD 工具栏)** - 1.5-2 小时，高频使用
3. **R13-B (表结构编辑器)** - 3-4 小时，复杂但完整

**总工作量**: 5-6.5 小时

---

## 四、技术约束

1. **方言兼容**: 所有 SQL 生成走 `Dialect::quote_identifier` / `quote_string`
2. **危险操作确认**: ALTER TABLE / DELETE 走 `guard_dangerous` + `confirmed` 参数
3. **错误处理**: 前端统一用 `errToString` 转换后端错误
4. **i18n**: 所有用户可见文本走 `t()` 翻译函数

---

## 五、数据验证（低优先级，暂不实现）

- 列类型合法性校验（如 VARCHAR 必须有长度）
- 默认值与列类型匹配校验
- 主键列不可为 NULL
- 外键引用完整性检查

**决策**: 先依赖数据库自身的约束检查，前端暂不做深度验证。

---

## 六、测试计划

### R13-A 测试
- [ ] 右键点击表节点 → 显示"查看 DDL"菜单项
- [ ] 点击后查询编辑器插入 CREATE TABLE 语句
- [ ] 多方言兼容（MySQL/PostgreSQL/SQLite）

### R13-B 测试
- [ ] 打开编辑器显示当前列定义和索引
- [ ] 修改列名称、类型、长度 → SQL 预览正确
- [ ] 新增列（FIRST / AFTER 位置）→ SQL 正确
- [ ] 删除列 → 二次确认 + SQL 正确
- [ ] 新增/删除索引 → SQL 正确
- [ ] 应用执行 → 后端执行成功，失败显示错误
- [ ] 危险操作走确认流程

### R13-C 测试
- [ ] 工具栏按钮显示正确
- [ ] Checkbox 多选/全选功能正常
- [ ] 新增行 → 填写数据 → 保存 → INSERT 成功
- [ ] 删除行 → 二次确认 → DELETE 成功
- [ ] 编辑行 → 保存 → UPDATE 成功
- [ ] 无主键表禁用删除按钮
- [ ] 刷新丢弃未保存修改

---

## 七、未来扩展

- 外键约束管理
- 表分区管理
- 触发器可视化编辑
- 列注释批量导入/导出
- 表结构 diff 和同步工具

---

**评审结论**: 待用户确认后进入实施阶段。
