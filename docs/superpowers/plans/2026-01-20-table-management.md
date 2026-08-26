# R13 表管理与数据编辑增强 - 实施计划

**需求编号**: R13  
**设计文档**: `docs/design/specs/2026-01-20-table-management-enhancement.md`  
**分支**: `feat/table-management-r13`  
**BASE**: `feat/context-menu-r12` (1398e83)

---

## 任务分解

本计划将 R13 拆分为 **10 个任务**，按优先级和依赖关系串行执行。

---

## Task 1: 后端 - show_create_table 命令（R13-A 后端）

**规模**: 小  
**文件**:
- `src-tauri/src/commands.rs` (新增 `show_create_table` 命令)
- `src-tauri/src/lib.rs` (注册命令)
- `src/api.ts` (前端 API 封装)

**实现**:

```rust
// src-tauri/src/commands.rs
#[tauri::command]
pub async fn show_create_table(
    state: State<'_, Arc<AppState>>,
    id: u64,
    database: String,
    table: String,
) -> Result<String> {
    let entry = state
        .connections
        .lock()
        .unwrap()
        .get(&id)
        .cloned()
        .ok_or_else(|| DbError::ConnectionNotFound(id.to_string()))?;
    
    let mut active = entry.lock().await;
    ensure_connected(state.inner(), &mut active).await?;
    
    let sql = format!("SHOW CREATE TABLE {}", 
        active.conn.as_ref().unwrap().dialect().quote_identifier(&table));
    
    let mut result = execute_buffered(
        active.conn.as_mut(),
        Some(&database),
        &sql,
        &ExecOpts::default(),
    )
    .await?;
    
    // 提取第二列（Create Table）
    if let Some(row) = result.rows.first() {
        if let Some(cell) = row.get(1) {
            return Ok(displayCell(cell));
        }
    }
    
    Err(DbError::Other("No CREATE TABLE result".into()))
}
```

```typescript
// src/api.ts
showCreateTable: (id: number, database: string, table: string) =>
  invoke<string>("show_create_table", { id, database, table }),
```

**验证**:
- `cargo build -p dby`
- `pnpm build`

---

## Task 2: 前端 - SchemaTree "查看 DDL" 菜单项（R13-A 前端）

**规模**: 小  
**文件**:
- `src/components/SchemaTree.tsx` (新增菜单项)
- `src/App.tsx` (传递 `onShowDDL` 回调)
- `src/locales/zh-CN.json` + `en-US.json`

**实现**:

```tsx
// SchemaTree.tsx Props 新增
interface Props {
  // ... 现有 props
  onShowDDL: (connId: number, database: string, table: string) => void;
}

// 菜单项构建逻辑（第 219 行后）
} else if (node.kind === "table") {
  menuItems.push({
    label: t("tree.menu.queryData"),
    action: () => onOpenTable(node.connId, node.database, node.name),
  });
  menuItems.push({
    label: t("tree.menu.showDDL"),  // 新增
    action: () => onShowDDL(node.connId, node.database, node.name),
  });
  menuItems.push({ label: t("tree.menu.copyName"), action: () => copyToClipboard(node.name) });
  // ...
}
```

```tsx
// App.tsx
async function handleShowDDL(connId: number, database: string, table: string) {
  try {
    const ddl = await api.showCreateTable(connId, database, table);
    updateWorkspace(connId, { query: ddl });  // 插入编辑器
    setStatus(t("app.status.ddlLoaded"));
  } catch (e) {
    setStatus(errToString(e));
  }
}

// 传递给 SchemaTree
<SchemaTree
  // ... 现有 props
  onShowDDL={handleShowDDL}
/>
```

**i18n**:
```json
// zh-CN.json
{
  "tree": {
    "menu": {
      "showDDL": "查看 DDL"
    }
  },
  "app": {
    "status": {
      "ddlLoaded": "DDL 已加载到编辑器"
    }
  }
}

// en-US.json
{
  "tree": {
    "menu": {
      "showDDL": "Show DDL"
    }
  },
  "app": {
    "status": {
      "ddlLoaded": "DDL loaded to editor"
    }
  }
}
```

**验证**:
- `node src/locales/check-keys.mjs`
- `pnpm build`
- 手动测试：右键表节点 → 点击"查看 DDL" → 编辑器显示 CREATE TABLE

**Commit**:
```bash
git add src-tauri/src/commands.rs src-tauri/src/lib.rs src/api.ts \
        src/components/SchemaTree.tsx src/App.tsx src/locales/
git commit -m "feat(tree): 新增「查看 DDL」菜单项

- show_create_table 命令（调用 SHOW CREATE TABLE）
- SchemaTree 表节点菜单新增「查看 DDL」
- 点击后 DDL 自动插入查询编辑器
- i18n 补充中英文案"
```

---

## Task 3: 后端 - get_primary_key 命令（R13-C 依赖）

**规模**: 小  
**文件**:
- `src-tauri/src/commands.rs`
- `src-tauri/src/lib.rs`
- `src/api.ts`

**实现**:

```rust
#[tauri::command]
pub async fn get_primary_key(
    state: State<'_, Arc<AppState>>,
    id: u64,
    database: String,
    table: String,
) -> Result<Vec<String>> {
    let entry = state
        .connections
        .lock()
        .unwrap()
        .get(&id)
        .cloned()
        .ok_or_else(|| DbError::ConnectionNotFound(id.to_string()))?;
    
    let mut active = entry.lock().await;
    ensure_connected(state.inner(), &mut active).await?;
    
    let sql = format!("SHOW KEYS FROM {} WHERE Key_name = 'PRIMARY'",
        active.conn.as_ref().unwrap().dialect().quote_identifier(&table));
    
    let result = execute_buffered(
        active.conn.as_mut(),
        Some(&database),
        &sql,
        &ExecOpts::default(),
    )
    .await?;
    
    let pk_columns: Vec<String> = result
        .rows
        .iter()
        .filter_map(|row| {
            // Column_name 在第 5 列（从 0 开始索引为 4）
            row.get(4).map(|cell| displayCell(cell))
        })
        .collect();
    
    Ok(pk_columns)
}
```

```typescript
// src/api.ts
getPrimaryKey: (id: number, database: string, table: string) =>
  invoke<string[]>("get_primary_key", { id, database, table }),
```

**验证**:
- `cargo build -p dby`
- `pnpm build`

---

## Task 4: 后端 - batch_delete_rows 命令（R13-C 后端）

**规模**: 小  
**文件**:
- `src-tauri/src/commands.rs`
- `src-tauri/src/lib.rs`
- `src/api.ts`

**实现**:

```rust
#[tauri::command]
pub async fn batch_delete_rows(
    state: State<'_, Arc<AppState>>,
    id: u64,
    database: String,
    table: String,
    pk_column: String,
    pk_values: Vec<String>,
    confirmed: bool,
) -> Result<QueryOutput> {
    let entry = state
        .connections
        .lock()
        .unwrap()
        .get(&id)
        .cloned()
        .ok_or_else(|| DbError::ConnectionNotFound(id.to_string()))?;
    
    let driver_id = entry.lock().await.driver_id.clone();
    let driver = state.registry.resolve(&driver_id)?;
    let dialect = driver.dialect();
    
    // 生成 DELETE 语句
    let placeholders = pk_values
        .iter()
        .map(|v| dialect.quote_string(v))
        .collect::<Vec<_>>()
        .join(", ");
    
    let sql = format!(
        "DELETE FROM {} WHERE {} IN ({})",
        dialect.quote_identifier(&table),
        dialect.quote_identifier(&pk_column),
        placeholders
    );
    
    // 走 guard_dangerous 确认
    if !confirmed {
        guard_dangerous(&sql)?;
    }
    
    run_ddl(state, id, database, sql, SqlOrigin::DataEdit).await
}
```

```typescript
// src/api.ts
batchDeleteRows: (
  id: number,
  database: string,
  table: string,
  pkColumn: string,
  pkValues: string[],
  confirmed: boolean
) => invoke<QueryOutput>("batch_delete_rows", {
  id, database, table, pk_column: pkColumn, pk_values: pkValues, confirmed
}),
```

**验证**:
- `cargo clippy -p dby -- -D warnings`
- `pnpm build`

---

## Task 5: 后端 - batch_insert_rows 命令（R13-C 后端）

**规模**: 小  
**文件**:
- `src-tauri/src/commands.rs`
- `src-tauri/src/lib.rs`
- `src/api.ts`

**实现**:

```rust
#[tauri::command]
pub async fn batch_insert_rows(
    state: State<'_, Arc<AppState>>,
    id: u64,
    database: String,
    table: String,
    rows: Vec<Vec<(String, ColumnType, String)>>,
) -> Result<QueryOutput> {
    let entry = state
        .connections
        .lock()
        .unwrap()
        .get(&id)
        .cloned()
        .ok_or_else(|| DbError::ConnectionNotFound(id.to_string()))?;
    
    let driver_id = entry.lock().await.driver_id.clone();
    let driver = state.registry.resolve(&driver_id)?;
    let dialect = driver.dialect();
    
    if rows.is_empty() {
        return Err(DbError::Other("No rows to insert".into()));
    }
    
    // 提取列名（从第一行）
    let columns: Vec<String> = rows[0].iter().map(|(name, _, _)| name.clone()).collect();
    
    // 解析每行的值
    let mut values_clauses = Vec::new();
    for row in &rows {
        let parsed = parse_cells(row)?;
        let values_str = parsed
            .iter()
            .map(|(_, v)| quote_value(dialect, v))
            .collect::<Vec<_>>()
            .join(", ");
        values_clauses.push(format!("({})", values_str));
    }
    
    let sql = format!(
        "INSERT INTO {} ({}) VALUES {}",
        dialect.quote_identifier(&table),
        columns.iter().map(|c| dialect.quote_identifier(c)).collect::<Vec<_>>().join(", "),
        values_clauses.join(", ")
    );
    
    run_ddl(state, id, database, sql, SqlOrigin::DataEdit).await
}
```

```typescript
// src/api.ts
batchInsertRows: (
  id: number,
  database: string,
  table: string,
  rows: EditCell[][]
) => invoke<QueryOutput>("batch_insert_rows", { id, database, table, rows }),
```

**验证**:
- `cargo build -p dby`
- `pnpm build`

**Commit**:
```bash
git add src-tauri/src/commands.rs src-tauri/src/lib.rs src/api.ts
git commit -m "feat(tauri): 新增 get_primary_key/batch_delete_rows/batch_insert_rows 命令

- get_primary_key: 查询表主键列（用于判断是否支持删除）
- batch_delete_rows: 批量删除行（DELETE WHERE pk IN (...)）
- batch_insert_rows: 批量插入行（INSERT VALUES (...), (...)）
- batch_delete_rows 走 guard_dangerous 确认机制"
```

---

## Task 6: 前端 - GridToolbar 组件（R13-C UI）

**规模**: 中  
**文件**:
- `src/components/GridToolbar.tsx` (新建)
- `src/components/ResultsGrid.tsx` (集成工具栏)

**实现**:

```tsx
// src/components/GridToolbar.tsx
import { useTranslation } from "react-i18next";

interface Props {
  selectedCount: number;
  totalRows: number;
  canDelete: boolean;  // 是否有主键
  onAdd: () => void;
  onDelete: () => void;
  onSave: () => void;
  onRefresh: () => void;
}

export default function GridToolbar({
  selectedCount,
  totalRows,
  canDelete,
  onAdd,
  onDelete,
  onSave,
  onRefresh,
}: Props) {
  const { t } = useTranslation();

  return (
    <div className="grid-toolbar">
      <button className="btn-icon" onClick={onAdd} title={t("grid.toolbar.add")}>
        ➕ {t("grid.toolbar.add")}
      </button>
      <button
        className="btn-icon"
        onClick={onDelete}
        disabled={!canDelete || selectedCount === 0}
        title={t("grid.toolbar.delete")}
      >
        ➖ {t("grid.toolbar.delete")}
      </button>
      <button className="btn-icon" onClick={onSave} title={t("grid.toolbar.save")}>
        💾 {t("grid.toolbar.save")}
      </button>
      <button className="btn-icon" onClick={onRefresh} title={t("grid.toolbar.refresh")}>
        🔄 {t("grid.toolbar.refresh")}
      </button>
      <span className="toolbar-info">
        {selectedCount > 0 && `${t("grid.toolbar.selected", { count: selectedCount })} · `}
        {t("grid.toolbar.totalRows", { count: totalRows })}
      </span>
    </div>
  );
}
```

**CSS**:
```css
/* App.css 新增 */
.grid-toolbar {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 8px 12px;
  border-bottom: 1px solid #e0e0e0;
  background: #f9f9f9;
}

.btn-icon {
  padding: 6px 12px;
  border: 1px solid #ccc;
  border-radius: 4px;
  background: white;
  cursor: pointer;
  font-size: 13px;
}

.btn-icon:hover:not(:disabled) {
  background: #f0f0f0;
}

.btn-icon:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}

.toolbar-info {
  margin-left: auto;
  font-size: 12px;
  color: #666;
}
```

**i18n**:
```json
// zh-CN.json
{
  "grid": {
    "toolbar": {
      "add": "新增",
      "delete": "删除",
      "save": "保存",
      "refresh": "刷新",
      "selected": "已选 {{count}} 行",
      "totalRows": "共 {{count}} 行"
    }
  }
}

// en-US.json
{
  "grid": {
    "toolbar": {
      "add": "Add",
      "delete": "Delete",
      "save": "Save",
      "refresh": "Refresh",
      "selected": "{{count}} selected",
      "totalRows": "{{count}} rows"
    }
  }
}
```

---

## Task 7: 前端 - ResultsGrid 行多选逻辑（R13-C 核心）

**规模**: 中  
**文件**:
- `src/components/ResultsGrid.tsx` (重构)

**实现**:

```tsx
// ResultsGrid.tsx Props 新增
interface Props {
  result: StreamResult;
  onEditCell?: (rowIndex: number, colIndex: number, newValue: string) => void;
  tableName?: string | null;
  connId?: number | null;
  database?: string | null;  // 新增，用于批量操作
  onRefresh?: () => void;     // 新增，刷新回调
}

// 状态新增
const [selectedRows, setSelectedRows] = useState<Set<number>>(new Set());
const [pendingInserts, setPendingInserts] = useState<Map<number, CellValue[]>>(new Map());
const [primaryKey, setPrimaryKey] = useState<string[]>([]);

// 获取主键
useEffect(() => {
  if (connId && database && tableName) {
    api.getPrimaryKey(connId, database, tableName)
      .then(setPrimaryKey)
      .catch(() => setPrimaryKey([]));
  }
}, [connId, database, tableName]);

// 工具栏回调
function handleAdd() {
  const newRow = result.columns?.map(() => ({ t: "null" } as CellValue)) ?? [];
  const newIndex = result.rows.length + pendingInserts.size;
  setPendingInserts(new Map(pendingInserts).set(newIndex, newRow));
}

async function handleDelete() {
  if (!connId || !database || !tableName || primaryKey.length === 0) return;
  
  const count = selectedRows.size;
  if (!window.confirm(t("grid.deleteConfirm", { count }))) return;
  
  const pkColumn = primaryKey[0];  // 简化：仅支持单列主键
  const pkValues: string[] = [];
  
  selectedRows.forEach((rowIdx) => {
    const row = result.rows[rowIdx];
    const pkColIdx = result.columns?.findIndex((c) => c.name === pkColumn) ?? -1;
    if (pkColIdx >= 0) {
      pkValues.push(displayCell(row[pkColIdx]));
    }
  });
  
  try {
    await api.batchDeleteRows(connId, database, tableName, pkColumn, pkValues, true);
    setSelectedRows(new Set());
    onRefresh?.();
  } catch (e) {
    alert(errToString(e));
  }
}

async function handleSave() {
  if (!connId || !database || !tableName || pendingInserts.size === 0) return;
  
  const rows: EditCell[][] = Array.from(pendingInserts.values()).map((row) =>
    result.columns!.map((col, i) => [
      col.name,
      col.column_type ?? UNKNOWN_COLUMN_TYPE,
      displayCell(row[i]),
    ])
  );
  
  try {
    await api.batchInsertRows(connId, database, tableName, rows);
    setPendingInserts(new Map());
    onRefresh?.();
  } catch (e) {
    alert(errToString(e));
  }
}

function handleRefresh() {
  setPendingInserts(new Map());
  setSelectedRows(new Set());
  onRefresh?.();
}

// 渲染 checkbox 列
function renderCheckbox(rowIndex: number) {
  return (
    <input
      type="checkbox"
      checked={selectedRows.has(rowIndex)}
      onChange={(e) => {
        const newSet = new Set(selectedRows);
        if (e.target.checked) {
          newSet.add(rowIndex);
        } else {
          newSet.delete(rowIndex);
        }
        setSelectedRows(newSet);
      }}
    />
  );
}

// 表头全选 checkbox
function renderHeaderCheckbox() {
  const allSelected = result.rows.length > 0 && selectedRows.size === result.rows.length;
  return (
    <input
      type="checkbox"
      checked={allSelected}
      onChange={(e) => {
        if (e.target.checked) {
          setSelectedRows(new Set(result.rows.map((_, i) => i)));
        } else {
          setSelectedRows(new Set());
        }
      }}
    />
  );
}
```

**i18n**:
```json
// zh-CN.json
{
  "grid": {
    "deleteConfirm": "确定删除选中的 {{count}} 行？此操作不可撤销。"
  }
}

// en-US.json
{
  "grid": {
    "deleteConfirm": "Delete {{count}} selected rows? This cannot be undone."
  }
}
```

**验证**:
- `pnpm build`
- 手动测试：勾选行 → 点击删除 → 确认 → 行消失

**Commit**:
```bash
git add src/components/GridToolbar.tsx src/components/ResultsGrid.tsx \
        src/App.tsx src/locales/ src/App.css
git commit -m "feat(grid): 新增 CRUD 工具栏 + 行多选删除

- GridToolbar 组件（新增/删除/保存/刷新按钮）
- ResultsGrid 左侧 checkbox 列（支持多选）
- 批量删除功能（需主键，走二次确认）
- 新增行功能（插入空白行，保存后生成 INSERT）
- i18n 补充中英文案"
```

---

## Task 8: 后端 - get_table_structure 命令（R13-B 后端）

**规模**: 中  
**文件**:
- `src-tauri/src/commands.rs`
- `src-tauri/src/lib.rs`
- `src/api.ts`

**实现**:

```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TableStructure {
    pub columns: Vec<ColumnDefinition>,
    pub indexes: Vec<IndexDefinition>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ColumnDefinition {
    pub name: String,
    pub data_type: String,
    pub length: Option<u32>,
    pub nullable: bool,
    pub default_value: Option<String>,
    pub comment: String,
    pub extra: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexDefinition {
    pub name: String,
    pub index_type: String,  // PRIMARY, UNIQUE, INDEX, FULLTEXT
    pub columns: Vec<String>,
}

#[tauri::command]
pub async fn get_table_structure(
    state: State<'_, Arc<AppState>>,
    id: u64,
    database: String,
    table: String,
) -> Result<TableStructure> {
    let entry = state
        .connections
        .lock()
        .unwrap()
        .get(&id)
        .cloned()
        .ok_or_else(|| DbError::ConnectionNotFound(id.to_string()))?;
    
    let mut active = entry.lock().await;
    ensure_connected(state.inner(), &mut active).await?;
    
    // 查询列定义
    let columns_sql = format!(
        "SELECT COLUMN_NAME, DATA_TYPE, CHARACTER_MAXIMUM_LENGTH, IS_NULLABLE, \
         COLUMN_DEFAULT, COLUMN_COMMENT, EXTRA \
         FROM information_schema.COLUMNS \
         WHERE TABLE_SCHEMA = {} AND TABLE_NAME = {} \
         ORDER BY ORDINAL_POSITION",
        active.conn.as_ref().unwrap().dialect().quote_string(&database),
        active.conn.as_ref().unwrap().dialect().quote_string(&table)
    );
    
    let columns_result = execute_buffered(
        active.conn.as_mut(),
        None,
        &columns_sql,
        &ExecOpts::default(),
    )
    .await?;
    
    let columns: Vec<ColumnDefinition> = columns_result
        .rows
        .iter()
        .map(|row| ColumnDefinition {
            name: displayCell(&row[0]),
            data_type: displayCell(&row[1]),
            length: row[2].as_i64().and_then(|v| v.try_into().ok()),
            nullable: displayCell(&row[3]) == "YES",
            default_value: if row[4].t == "null" { None } else { Some(displayCell(&row[4])) },
            comment: displayCell(&row[5]),
            extra: displayCell(&row[6]),
        })
        .collect();
    
    // 查询索引
    let indexes_sql = format!("SHOW INDEX FROM {}", 
        active.conn.as_ref().unwrap().dialect().quote_identifier(&table));
    
    let indexes_result = execute_buffered(
        active.conn.as_mut(),
        Some(&database),
        &indexes_sql,
        &ExecOpts::default(),
    )
    .await?;
    
    let mut indexes_map: std::collections::HashMap<String, IndexDefinition> = std::collections::HashMap::new();
    
    for row in &indexes_result.rows {
        let index_name = displayCell(&row[2]);
        let column_name = displayCell(&row[4]);
        let non_unique = displayCell(&row[1]) == "1";
        let index_type = displayCell(&row[10]);
        
        indexes_map
            .entry(index_name.clone())
            .or_insert_with(|| IndexDefinition {
                name: index_name.clone(),
                index_type: if index_name == "PRIMARY" {
                    "PRIMARY".into()
                } else if !non_unique {
                    "UNIQUE".into()
                } else if index_type == "FULLTEXT" {
                    "FULLTEXT".into()
                } else {
                    "INDEX".into()
                },
                columns: Vec::new(),
            })
            .columns
            .push(column_name);
    }
    
    let indexes: Vec<IndexDefinition> = indexes_map.into_values().collect();
    
    Ok(TableStructure { columns, indexes })
}
```

```typescript
// src/api.ts
export interface TableStructure {
  columns: ColumnDefinition[];
  indexes: IndexDefinition[];
}

export interface ColumnDefinition {
  name: string;
  data_type: string;
  length?: number;
  nullable: boolean;
  default_value?: string;
  comment: string;
  extra: string;
}

export interface IndexDefinition {
  name: string;
  index_type: string;
  columns: string[];
}

// API
getTableStructure: (id: number, database: string, table: string) =>
  invoke<TableStructure>("get_table_structure", { id, database, table }),
```

**验证**:
- `cargo build -p dby`
- `pnpm build`

---

## Task 9: 后端 - alter_table 命令（R13-B 后端）

**规模**: 中  
**文件**:
- `src-tauri/src/commands.rs`
- `src-tauri/src/lib.rs`
- `src/api.ts`

**实现**:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AlterOperation {
    AddColumn {
        column: ColumnDefinition,
        after: Option<String>,
    },
    ModifyColumn {
        old_name: String,
        new_definition: ColumnDefinition,
    },
    DropColumn {
        name: String,
    },
    AddIndex {
        index: IndexDefinition,
    },
    DropIndex {
        name: String,
    },
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
    let entry = state
        .connections
        .lock()
        .unwrap()
        .get(&id)
        .cloned()
        .ok_or_else(|| DbError::ConnectionNotFound(id.to_string()))?;
    
    let driver_id = entry.lock().await.driver_id.clone();
    let driver = state.registry.resolve(&driver_id)?;
    let dialect = driver.dialect();
    
    // 生成 ALTER TABLE 子句
    let mut clauses = Vec::new();
    
    for op in operations {
        match op {
            AlterOperation::AddColumn { column, after } => {
                let mut clause = format!(
                    "ADD COLUMN {} {}",
                    dialect.quote_identifier(&column.name),
                    build_column_definition(dialect, &column)
                );
                if let Some(after_col) = after {
                    clause.push_str(&format!(" AFTER {}", dialect.quote_identifier(&after_col)));
                }
                clauses.push(clause);
            }
            AlterOperation::ModifyColumn { old_name, new_definition } => {
                clauses.push(format!(
                    "CHANGE COLUMN {} {} {}",
                    dialect.quote_identifier(&old_name),
                    dialect.quote_identifier(&new_definition.name),
                    build_column_definition(dialect, &new_definition)
                ));
            }
            AlterOperation::DropColumn { name } => {
                clauses.push(format!("DROP COLUMN {}", dialect.quote_identifier(&name)));
            }
            AlterOperation::AddIndex { index } => {
                let index_type = match index.index_type.as_str() {
                    "PRIMARY" => "PRIMARY KEY".to_string(),
                    "UNIQUE" => "UNIQUE INDEX".to_string(),
                    "FULLTEXT" => "FULLTEXT INDEX".to_string(),
                    _ => "INDEX".to_string(),
                };
                let cols = index.columns
                    .iter()
                    .map(|c| dialect.quote_identifier(c))
                    .collect::<Vec<_>>()
                    .join(", ");
                clauses.push(format!(
                    "ADD {} {} ({})",
                    index_type,
                    dialect.quote_identifier(&index.name),
                    cols
                ));
            }
            AlterOperation::DropIndex { name } => {
                if name == "PRIMARY" {
                    clauses.push("DROP PRIMARY KEY".to_string());
                } else {
                    clauses.push(format!("DROP INDEX {}", dialect.quote_identifier(&name)));
                }
            }
        }
    }
    
    let sql = format!(
        "ALTER TABLE {} {}",
        dialect.quote_identifier(&table),
        clauses.join(", ")
    );
    
    if !confirmed {
        guard_dangerous(&sql)?;
    }
    
    run_ddl(state, id, database, sql, SqlOrigin::SchemaEdit).await
}

fn build_column_definition(dialect: &dyn Dialect, col: &ColumnDefinition) -> String {
    let mut def = col.data_type.clone();
    
    if let Some(len) = col.length {
        def.push_str(&format!("({})", len));
    }
    
    if !col.nullable {
        def.push_str(" NOT NULL");
    }
    
    if let Some(default) = &col.default_value {
        def.push_str(&format!(" DEFAULT {}", dialect.quote_string(default)));
    }
    
    if !col.comment.is_empty() {
        def.push_str(&format!(" COMMENT {}", dialect.quote_string(&col.comment)));
    }
    
    if !col.extra.is_empty() {
        def.push_str(&format!(" {}", col.extra));
    }
    
    def
}
```

```typescript
// src/api.ts
export type AlterOperation =
  | { type: 'add_column'; column: ColumnDefinition; after?: string }
  | { type: 'modify_column'; old_name: string; new_definition: ColumnDefinition }
  | { type: 'drop_column'; name: string }
  | { type: 'add_index'; index: IndexDefinition }
  | { type: 'drop_index'; name: string };

// API
alterTable: (
  id: number,
  database: string,
  table: string,
  operations: AlterOperation[],
  confirmed: boolean
) => invoke<QueryOutput>("alter_table", { id, database, table, operations, confirmed }),
```

**验证**:
- `cargo clippy -p dby -- -D warnings`
- `pnpm build`

**Commit**:
```bash
git add src-tauri/src/commands.rs src-tauri/src/lib.rs src/api.ts
git commit -m "feat(tauri): 新增 get_table_structure/alter_table 命令

- get_table_structure: 查询列定义 + 索引（from information_schema）
- alter_table: 执行批量 ALTER TABLE 操作
- 支持新增/修改/删除列，新增/删除索引
- alter_table 走 guard_dangerous 确认机制"
```

---

## Task 10: 前端 - TableStructureEditor 组件（R13-B UI）

**规模**: 大  
**文件**:
- `src/components/TableStructureEditor.tsx` (新建)
- `src/components/SchemaTree.tsx` (新增菜单项)
- `src/App.tsx` (状态管理)
- `src/locales/zh-CN.json` + `en-US.json`

**实现**:

```tsx
// src/components/TableStructureEditor.tsx
import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { api, ColumnDefinition, IndexDefinition, AlterOperation } from "../api";

interface Props {
  connId: number;
  database: string;
  table: string;
  onClose: () => void;
  onApplied: () => void;
}

export default function TableStructureEditor({
  connId,
  database,
  table,
  onClose,
  onApplied,
}: Props) {
  const { t } = useTranslation();
  const [loading, setLoading] = useState(true);
  const [columns, setColumns] = useState<ColumnDefinition[]>([]);
  const [indexes, setIndexes] = useState<IndexDefinition[]>([]);
  const [operations, setOperations] = useState<AlterOperation[]>([]);
  const [sqlPreview, setSqlPreview] = useState("");
  const [activeTab, setActiveTab] = useState<"columns" | "indexes">("columns");

  useEffect(() => {
    api
      .getTableStructure(connId, database, table)
      .then((structure) => {
        setColumns(structure.columns);
        setIndexes(structure.indexes);
        setLoading(false);
      })
      .catch((e) => {
        alert(e);
        onClose();
      });
  }, [connId, database, table]);

  // 实时生成 SQL 预览
  useEffect(() => {
    if (operations.length === 0) {
      setSqlPreview("-- 暂无修改");
      return;
    }
    
    // 简化预览：仅显示操作类型
    const preview = operations
      .map((op, i) => {
        switch (op.type) {
          case "add_column":
            return `-- ${i + 1}. 新增列 ${op.column.name}`;
          case "modify_column":
            return `-- ${i + 1}. 修改列 ${op.old_name} → ${op.new_definition.name}`;
          case "drop_column":
            return `-- ${i + 1}. 删除列 ${op.name}`;
          case "add_index":
            return `-- ${i + 1}. 新增索引 ${op.index.name}`;
          case "drop_index":
            return `-- ${i + 1}. 删除索引 ${op.name}`;
        }
      })
      .join("\n");
    
    setSqlPreview(`ALTER TABLE \`${table}\`\n${preview};`);
  }, [operations, table]);

  async function handleApply() {
    if (operations.length === 0) {
      alert(t("tableEditor.noChanges"));
      return;
    }
    
    try {
      await api.alterTable(connId, database, table, operations, true);
      alert(t("tableEditor.success"));
      onApplied();
      onClose();
    } catch (e) {
      alert(e);
    }
  }

  if (loading) return <div className="dialog-overlay">加载中...</div>;

  return (
    <div className="dialog-overlay" onClick={onClose}>
      <div className="dialog" onClick={(e) => e.stopPropagation()} style={{ width: 800, maxHeight: "80vh" }}>
        <div className="dialog-header">
          <h2>{t("tableEditor.title", { table })}</h2>
          <button className="btn-close" onClick={onClose}>
            ×
          </button>
        </div>
        
        <div className="dialog-body">
          <div className="tabs">
            <button
              className={activeTab === "columns" ? "tab active" : "tab"}
              onClick={() => setActiveTab("columns")}
            >
              {t("tableEditor.columnsTab")}
            </button>
            <button
              className={activeTab === "indexes" ? "tab active" : "tab"}
              onClick={() => setActiveTab("indexes")}
            >
              {t("tableEditor.indexesTab")}
            </button>
          </div>

          {activeTab === "columns" && (
            <div className="columns-panel">
              <table className="editor-table">
                <thead>
                  <tr>
                    <th>☑</th>
                    <th>{t("tableEditor.columnName")}</th>
                    <th>{t("tableEditor.dataType")}</th>
                    <th>{t("tableEditor.length")}</th>
                    <th>{t("tableEditor.nullable")}</th>
                    <th>{t("tableEditor.defaultValue")}</th>
                    <th>{t("tableEditor.comment")}</th>
                  </tr>
                </thead>
                <tbody>
                  {columns.map((col, i) => (
                    <tr key={i}>
                      <td><input type="checkbox" /></td>
                      <td><input value={col.name} readOnly /></td>
                      <td><input value={col.data_type} readOnly /></td>
                      <td><input value={col.length ?? ""} readOnly /></td>
                      <td><input type="checkbox" checked={col.nullable} readOnly /></td>
                      <td><input value={col.default_value ?? ""} readOnly /></td>
                      <td><input value={col.comment} readOnly /></td>
                    </tr>
                  ))}
                </tbody>
              </table>
              <div className="panel-actions">
                <button className="btn">+ {t("tableEditor.addColumn")}</button>
                <button className="btn">- {t("tableEditor.deleteSelected")}</button>
              </div>
            </div>
          )}

          {activeTab === "indexes" && (
            <div className="indexes-panel">
              <table className="editor-table">
                <thead>
                  <tr>
                    <th>{t("tableEditor.indexName")}</th>
                    <th>{t("tableEditor.indexType")}</th>
                    <th>{t("tableEditor.indexColumns")}</th>
                    <th>{t("tableEditor.actions")}</th>
                  </tr>
                </thead>
                <tbody>
                  {indexes.map((idx, i) => (
                    <tr key={i}>
                      <td>{idx.name}</td>
                      <td>{idx.index_type}</td>
                      <td>{idx.columns.join(", ")}</td>
                      <td>
                        <button className="btn-sm">{t("tableEditor.delete")}</button>
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
              <div className="panel-actions">
                <button className="btn">+ {t("tableEditor.addIndex")}</button>
              </div>
            </div>
          )}

          <div className="sql-preview">
            <h3>{t("tableEditor.sqlPreview")}</h3>
            <pre>{sqlPreview}</pre>
          </div>
        </div>

        <div className="dialog-footer">
          <button className="btn" onClick={onClose}>
            {t("tableEditor.cancel")}
          </button>
          <button className="btn btn-primary" onClick={handleApply} disabled={operations.length === 0}>
            {t("tableEditor.applyAndExecute")}
          </button>
        </div>
      </div>
    </div>
  );
}
```

**i18n**:
```json
// zh-CN.json
{
  "tableEditor": {
    "title": "编辑表结构: {{table}}",
    "columnsTab": "列定义",
    "indexesTab": "索引",
    "columnName": "列名",
    "dataType": "类型",
    "length": "长度",
    "nullable": "可空",
    "defaultValue": "默认值",
    "comment": "备注",
    "addColumn": "新增列",
    "deleteSelected": "删除选中",
    "indexName": "索引名",
    "indexType": "类型",
    "indexColumns": "列",
    "actions": "操作",
    "delete": "删除",
    "addIndex": "新增索引",
    "sqlPreview": "生成的 SQL 预览",
    "cancel": "取消",
    "applyAndExecute": "应用并执行",
    "noChanges": "暂无修改",
    "success": "表结构修改成功"
  }
}

// en-US.json
{
  "tableEditor": {
    "title": "Edit Table Structure: {{table}}",
    "columnsTab": "Columns",
    "indexesTab": "Indexes",
    "columnName": "Column Name",
    "dataType": "Data Type",
    "length": "Length",
    "nullable": "Nullable",
    "defaultValue": "Default Value",
    "comment": "Comment",
    "addColumn": "Add Column",
    "deleteSelected": "Delete Selected",
    "indexName": "Index Name",
    "indexType": "Type",
    "indexColumns": "Columns",
    "actions": "Actions",
    "delete": "Delete",
    "addIndex": "Add Index",
    "sqlPreview": "Generated SQL Preview",
    "cancel": "Cancel",
    "applyAndExecute": "Apply and Execute",
    "noChanges": "No changes",
    "success": "Table structure updated successfully"
  }
}
```

**SchemaTree 集成**:
```tsx
// SchemaTree.tsx
} else if (node.kind === "table") {
  menuItems.push({
    label: t("tree.menu.queryData"),
    action: () => onOpenTable(node.connId, node.database, node.name),
  });
  menuItems.push({
    label: t("tree.menu.showDDL"),
    action: () => onShowDDL(node.connId, node.database, node.name),
  });
  menuItems.push({
    label: t("tree.menu.editStructure"),  // 新增
    action: () => onEditStructure(node.connId, node.database, node.name),
  });
  // ...
}
```

**i18n**:
```json
// zh-CN.json
{
  "tree": {
    "menu": {
      "editStructure": "编辑表结构"
    }
  }
}

// en-US.json
{
  "tree": {
    "menu": {
      "editStructure": "Edit Structure"
    }
  }
}
```

**App.tsx 状态管理**:
```tsx
const [editingStructure, setEditingStructure] = useState<{
  connId: number;
  database: string;
  table: string;
} | null>(null);

function handleEditStructure(connId: number, database: string, table: string) {
  setEditingStructure({ connId, database, table });
}

// 渲染
{editingStructure && (
  <TableStructureEditor
    connId={editingStructure.connId}
    database={editingStructure.database}
    table={editingStructure.table}
    onClose={() => setEditingStructure(null)}
    onApplied={() => {
      // 刷新 Schema 树
      handleOpenTable(editingStructure.connId, editingStructure.database, editingStructure.table);
    }}
  />
)}
```

**验证**:
- `node src/locales/check-keys.mjs`
- `pnpm build`
- 手动测试：右键表节点 → "编辑表结构" → 打开对话框 → 显示列和索引

**Commit**:
```bash
git add src/components/TableStructureEditor.tsx src/components/SchemaTree.tsx \
        src/App.tsx src/locales/ src/App.css
git commit -m "feat(editor): 可视化表结构编辑器（基础版）

- TableStructureEditor 组件（列定义 + 索引管理）
- 实时 SQL 预览（底部显示 ALTER TABLE 语句）
- SchemaTree 表节点菜单新增「编辑表结构」
- 基础 UI（暂未实现列编辑和索引操作，仅显示）
- i18n 补充中英文案

TODO: 下一阶段实现列/索引的增删改交互"
```

---

## 实施顺序总结

| Task | 规模 | 预计时间 | 累计时间 |
|------|------|---------|----------|
| 1. show_create_table 后端 | 小 | 15 分钟 | 15 分钟 |
| 2. "查看 DDL" 菜单项 | 小 | 15 分钟 | 30 分钟 |
| 3. get_primary_key 后端 | 小 | 15 分钟 | 45 分钟 |
| 4. batch_delete_rows 后端 | 小 | 20 分钟 | 1h05 |
| 5. batch_insert_rows 后端 | 小 | 20 分钟 | 1h25 |
| 6. GridToolbar 组件 | 中 | 30 分钟 | 1h55 |
| 7. ResultsGrid 行多选 | 中 | 1 小时 | 2h55 |
| 8. get_table_structure 后端 | 中 | 40 分钟 | 3h35 |
| 9. alter_table 后端 | 中 | 40 分钟 | 4h15 |
| 10. TableStructureEditor UI | 大 | 2 小时 | 6h15 |

**总工作量**: 约 6.25 小时

---

## 验收标准

### R13-A (查看 DDL)
- [ ] 表节点右键菜单显示"查看 DDL"
- [ ] 点击后查询编辑器插入 CREATE TABLE 语句
- [ ] 错误处理正确（连接断开/表不存在）

### R13-B (表结构编辑器)
- [ ] 打开编辑器显示当前列定义和索引
- [ ] 底部 SQL 预览区实时显示 ALTER TABLE 语句
- [ ] "应用并执行"按钮提交修改
- [ ] 危险操作走二次确认
- [ ] 成功后刷新 Schema 树

### R13-C (CRUD 工具栏)
- [ ] 工具栏显示新增/删除/保存/刷新按钮
- [ ] Checkbox 列固定在最左侧（行号前）
- [ ] 多选行后点删除 → 二次确认 → DELETE 执行成功
- [ ] 点新增 → 底部出现空白行 → 填写数据 → 点保存 → INSERT 成功
- [ ] 无主键表禁用删除按钮
- [ ] 刷新丢弃未保存的修改

---

## 技术债务

1. **TableStructureEditor 列编辑交互**：当前仅显示，未实现修改/新增/删除列的交互（Task 10 标记为 TODO）
2. **复合主键支持**：当前仅支持单列主键的删除，复合主键需生成 WHERE (pk1, pk2) IN ((...), (...))
3. **外键约束管理**：未实现外键的可视化编辑
4. **数据验证**：前端暂不做深度验证，依赖数据库约束

---

**评审结论**: 待用户确认后创建分支开始实施。
