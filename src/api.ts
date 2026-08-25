import { Channel, invoke } from "@tauri-apps/api/core";

// ---------- 值类型（带 tag 的 envelope） ----------

export type CellValue =
  | { t: "null" }
  | { t: "bool"; v: boolean }
  | { t: "i64"; v: string }
  | { t: "u64"; v: string }
  | { t: "f64"; v: number }
  | { t: "decimal"; v: string }
  | { t: "str"; v: string }
  | { t: "bytes"; v: number[] }
  | { t: "date"; v: string }
  | { t: "time"; v: string }
  | { t: "datetime"; v: string }
  | { t: "json"; v: unknown }
  | { t: "uuid"; v: string }
  | { t: "array"; v: CellValue[] }
  | { t: "map"; v: [string, CellValue][] };

export function displayCell(v: CellValue): string {
  switch (v.t) {
    case "null":
      return "NULL";
    case "bool":
      return String(v.v);
    case "i64":
    case "u64":
      // 十进制字符串原样渲染，绝不 Number()（>2^53 无损，design §4.3）。
      return v.v;
    case "f64":
      return String(v.v);
    case "decimal":
    case "str":
    case "date":
    case "time":
    case "datetime":
    case "uuid":
      return v.v;
    case "bytes":
      return "0x" + v.v.map((b) => b.toString(16).padStart(2, "0")).join("");
    case "json":
      return JSON.stringify(v.v);
    case "array":
      return "[" + v.v.map(displayCell).join(", ") + "]";
    case "map":
      return "{" + v.v.map(([k, val]) => `${k}: ${displayCell(val)}`).join(", ") + "}";
  }
}

// ---------- 驱动 / 能力 ----------

export interface Capabilities {
  supports_sql: boolean;
  supports_transactions: boolean;
  supports_catalogs: boolean;
  supports_schemas: boolean;
  supports_procedures: boolean;
  supports_cancel: boolean;
  supports_data_edit: boolean;
}

export interface DriverInfo {
  id: string;
  display_name: string;
  capabilities: Capabilities;
}

export interface ConnectParams {
  driver?: string;
  host: string;
  port: number;
  user: string;
  password?: string | null;
  database?: string | null;
  ssl?: { enabled: boolean; verify_cert?: boolean } | null;
  ssh?: {
    enabled: boolean;
    host: string;
    port: number;
    user: string;
    password?: string | null;
    /** TOFU 已信任主机指纹（OpenSSH 风格 "SHA256:…"），由探针确认后回填 */
    host_key_fingerprint?: string;
  } | null;
}

// ---------- 元数据 ----------

export interface TableInfo {
  name: string;
  kind?: string | null;
  comment?: string | null;
}

export interface ColumnInfo {
  name: string;
  type_name: string;
  column_type?: ColumnType | null;
  nullable?: boolean | null;
  primary_key?: boolean | null;
  default?: string | null;
  comment?: string | null;
}

/// 结构化列类型（与 dby-core `ColumnTypeBase` 的 serde snake_case tag 一致，#32）。
export type ColumnTypeBase =
  | "bool"
  | "i8"
  | "i16"
  | "i32"
  | "i64"
  | "u8"
  | "u16"
  | "u32"
  | "u64"
  | "f32"
  | "f64"
  | "decimal"
  | "str"
  | "bytes"
  | "date"
  | "time"
  | "datetime"
  | "json"
  | "uuid"
  | "array"
  | "map"
  | "unknown";

export interface ColumnType {
  base: ColumnTypeBase;
  numeric_precision?: number | null;
  numeric_scale?: number | null;
  unsigned?: boolean;
  char_max_length?: number | null;
  temporal_precision?: number | null;
  charset?: string | null;
  collation?: string | null;
}

/// 无列类型信息时的兜底（与 dby-core `ColumnType::unknown()` 序列化一致）。
export const UNKNOWN_COLUMN_TYPE: ColumnType = { base: "unknown" };

/// 编辑单元格载荷：列名 + 列类型 + 原始输入串（design §4.6，#11/#69）。
/// 由后端 `parse_value` 按列类型解析，前端不再做正则类型猜测。
export type EditCell = [string, ColumnType, string];

export interface ResultSet {
  columns: ColumnInfo[];
  rows: CellValue[][];
  truncated: boolean;
}

export interface QueryOutput {
  result_sets: ResultSet[];
  affected_rows: number;
  last_insert_id: string | null;
  info?: string | null;
}

// ---------- 流式 ----------

export type StreamEvent =
  | { event: "columns"; data: ColumnInfo[] }
  | { event: "rows"; data: CellValue[][] }
  | { event: "affected"; data: { affected_rows: number; last_insert_id: string | null } }
  | { event: "info"; data: string | null }
  | { event: "result_set_end" }
  | { event: "truncated" }
  | { event: "done" }
  | { event: "error"; data: { kind: string; message: string } };

export interface StreamResult {
  columns: ColumnInfo[] | null;
  rows: CellValue[][];
  affected_rows: number;
  last_insert_id: string | null;
  truncated: boolean;
}

// ---------- 项目 / 连接 ----------

export interface Project {
  id: string;
  name: string;
  description?: string | null;
  color?: string | null;
  created_at: string;
  updated_at: string;
}

export interface ConnectResponse {
  id: number;
  name: string;
  driver_id: string;
  project_id: string;
  database: string;
  server_version: string;
}

export interface ConnectionSummary {
  id: number;
  name: string;
  driver_id: string;
  project_id: string;
  database: string;
  server_version: string;
}

/// list_saved_connections 的脱敏视图：不含任何 secret 字段（#22）。
export interface SavedConnection {
  id: string;
  project_id: string;
  name: string;
  driver: string;
  host: string;
  port: number;
  user: string;
  database?: string | null;
  has_ssh: boolean;
  ssh_host?: string | null;
  ssh_port?: number | null;
  ssh_user?: string | null;
  color?: string | null;
}

/// update_saved_connection 的载荷：全部可选，只传需要修改的字段（#63）。
/// 仅非敏感字段：敏感凭据（密码/私钥）不进此载荷，走 connect/凭据更新单独处理。
export interface SavedConnectionUpdate {
  name?: string;
  color?: string;
  ssh?: {
    enabled: boolean;
    host: string;
    port: number;
    user: string;
    /** TOFU 已信任主机指纹（OpenSSH 风格 "SHA256:…"） */
    host_key_fingerprint?: string;
  } | null;
}

// ---------- 历史 ----------

export interface StatementHit {
  hash: string;
  sql: string;
  project_id: string;
  run_count: number;
  last_run_at: string;
  pinned: boolean;
  tags: string[];
}

export interface ExecutionRecord {
  id: string;
  project_id: string;
  connection_id?: string | null;
  connection_name?: string | null;
  database?: string | null;
  sql: string;
  origin: string;
  status: string;
  rows_affected: number;
  row_count?: number | null;
  duration_ms: number;
  started_at: string;
}

export type DangerLevel =
  | { level: "safe" }
  | { level: "warn" }
  | { level: "dangerous"; reasons: string[] };

export interface ColumnDef {
  name: string;
  type_name: string;
  nullable?: boolean;
  primary_key?: boolean;
}

export const api = {
  listDrivers: () => invoke<DriverInfo[]>("list_drivers"),
  testConnection: (params: ConnectParams) =>
    invoke<string>("test_connection", { params }),
  probeHostKey: (params: ConnectParams) =>
    invoke<string>("probe_host_key", { params }),

  connect: (
    params: ConnectParams,
    projectId?: string | null,
    save: boolean = true,
    rememberPassword: boolean = true,
  ) => invoke<ConnectResponse>("connect", { params, projectId, save, rememberPassword }),
  disconnect: (id: number) => invoke<void>("disconnect", { id }),
  listConnections: () => invoke<ConnectionSummary[]>("list_connections"),
  listSavedConnections: (projectId?: string | null) =>
    invoke<SavedConnection[]>("list_saved_connections", { projectId }),
  reconnect: (configId: string) => invoke<ConnectResponse>("reconnect", { configId }),
  deleteSavedConnection: (configId: string) =>
    invoke<void>("delete_saved_connection", { configId }),
  updateSavedConnection: (configId: string, update: SavedConnectionUpdate) =>
    invoke<void>("update_saved_connection", { configId, update }),

  listDatabases: (id: number) => invoke<string[]>("list_databases", { id }),
  listTables: (id: number, database: string) =>
    invoke<TableInfo[]>("list_tables", { id, database }),
  listColumns: (id: number, database: string, table: string) =>
    invoke<ColumnInfo[]>("list_columns", { id, database, table }),
  buildTableSelect: (connId: number, table: string) =>
    invoke<string>("build_table_select", { id: connId, table }),
  executeQuery: (id: number, database: string | null, sql: string, confirmed: boolean) =>
    invoke<QueryOutput>("execute_query", { id, database, sql, confirmed }),
  executeQueryStream: (
    channel: Channel<StreamEvent>,
    id: number,
    database: string | null,
    sql: string,
    confirmed: boolean,
  ) => invoke<void>("execute_query_stream", { channel, id, database, sql, confirmed }),
  cancelQuery: (id: number) => invoke<void>("cancel_query", { id }),
  analyzeDanger: (sql: string) => invoke<DangerLevel>("analyze_danger", { sql }),

  begin: (id: number) => invoke<void>("begin", { id }),
  commit: (id: number) => invoke<void>("commit", { id }),
  rollback: (id: number) => invoke<void>("rollback", { id }),
  setAutocommit: (id: number, enabled: boolean) =>
    invoke<void>("set_autocommit", { id, enabled }),
  exportResult: (
    id: number,
    database: string | null,
    sql: string,
    format: string,
    table?: string | null,
    confirmed: boolean = false,
  ) => invoke<string>("export_result", { id, database, sql, format, table, confirmed }),
  buildEditSql: (
    id: number,
    table: string,
    pk: EditCell[],
    set: EditCell[],
  ) => invoke<string>("build_edit_sql", { id, table, pk, set }),
  buildInsertSql: (id: number, table: string, cells: EditCell[]) =>
    invoke<string>("build_insert_sql", { id, table, cells }),
  executeEdit: (
    id: number,
    database: string | null,
    table: string,
    pk: EditCell[],
    set: EditCell[],
  ) => invoke<QueryOutput>("execute_edit", { id, database, table, pk, set }),

  createDatabase: (id: number, name: string) =>
    invoke<QueryOutput>("create_database", { id, name }),
  dropDatabase: (id: number, name: string, confirmed: boolean) =>
    invoke<QueryOutput>("drop_database", { id, name, confirmed }),
  createTable: (id: number, database: string, name: string, columns: ColumnDef[]) =>
    invoke<QueryOutput>("create_table", { id, database, name, columns }),
  renameTable: (
    id: number,
    database: string,
    oldName: string,
    newName: string,
    confirmed: boolean,
  ) => invoke<QueryOutput>("rename_table", { id, database, oldName, newName, confirmed }),
  dropTable: (id: number, database: string, name: string, confirmed: boolean) =>
    invoke<QueryOutput>("drop_table", { id, database, name, confirmed }),
  dropView: (id: number, database: string, name: string, confirmed: boolean) =>
    invoke<QueryOutput>("drop_view", { id, database, name, confirmed }),
  dropRoutine: (id: number, database: string, kind: string, name: string, confirmed: boolean) =>
    invoke<QueryOutput>("drop_routine", { id, database, kind, name, confirmed }),
  dropTrigger: (id: number, database: string, name: string, confirmed: boolean) =>
    invoke<QueryOutput>("drop_trigger", { id, database, name, confirmed }),
  truncateTable: (id: number, database: string, name: string, confirmed: boolean) =>
    invoke<QueryOutput>("truncate_table", { id, database, name, confirmed }),

  listProjects: () => invoke<Project[]>("list_projects"),
  createProject: (name: string) => invoke<Project>("create_project", { name }),
  renameProject: (id: string, name: string) =>
    invoke<Project>("rename_project", { id, name }),
  deleteProject: (id: string, confirmed: boolean) =>
    invoke<void>("delete_project", { id, confirmed }),

  searchHistory: (query: string, projectId?: string | null) =>
    invoke<StatementHit[]>("search_history", { query, projectId }),
  listHistory: (projectId?: string | null) =>
    invoke<StatementHit[]>("list_history", { projectId }),
  listExecutions: (projectId?: string | null) =>
    invoke<ExecutionRecord[]>("list_executions", { projectId }),
  pinStatement: (hash: string, pinned: boolean) =>
    invoke<void>("pin_statement", { hash, pinned }),
  deleteExecution: (id: string) => invoke<void>("delete_execution", { id }),
};
