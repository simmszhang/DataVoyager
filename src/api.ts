import { Channel, invoke } from "@tauri-apps/api/core";

// ---------- 值类型（带 tag 的 envelope） ----------

export type CellValue =
  | { t: "null" }
  | { t: "bool"; v: boolean }
  | { t: "i64"; v: number }
  | { t: "u64"; v: number }
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
  nullable?: boolean | null;
  primary_key?: boolean | null;
  default?: string | null;
  comment?: string | null;
}

export interface ResultSet {
  columns: ColumnInfo[];
  rows: CellValue[][];
  truncated: boolean;
}

export interface QueryOutput {
  result_sets: ResultSet[];
  affected_rows: number;
  last_insert_id: number | null;
  info?: string | null;
}

// ---------- 流式 ----------

export type StreamEvent =
  | { event: "columns"; data: ColumnInfo[] }
  | { event: "rows"; data: CellValue[][] }
  | { event: "affected"; data: { affected_rows: number; last_insert_id: number | null } }
  | { event: "info"; data: string | null };

export interface StreamResult {
  columns: ColumnInfo[] | null;
  rows: CellValue[][];
  affected_rows: number;
  last_insert_id: number | null;
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

export interface SavedConnection {
  id: string;
  project_id: string;
  name: string;
  driver: string;
  host: string;
  port: number;
  user: string;
  database?: string | null;
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

  connect: (params: ConnectParams, projectId?: string | null) =>
    invoke<ConnectResponse>("connect", { params, projectId }),
  disconnect: (id: number) => invoke<void>("disconnect", { id }),
  listConnections: () => invoke<ConnectionSummary[]>("list_connections"),
  listSavedConnections: (projectId?: string | null) =>
    invoke<SavedConnection[]>("list_saved_connections", { projectId }),
  reconnect: (configId: string) => invoke<ConnectResponse>("reconnect", { configId }),
  deleteSavedConnection: (configId: string) =>
    invoke<void>("delete_saved_connection", { configId }),

  listDatabases: (id: number) => invoke<string[]>("list_databases", { id }),
  listTables: (id: number, database: string) =>
    invoke<TableInfo[]>("list_tables", { id, database }),
  listColumns: (id: number, database: string, table: string) =>
    invoke<ColumnInfo[]>("list_columns", { id, database, table }),
  executeQuery: (id: number, database: string | null, sql: string) =>
    invoke<QueryOutput>("execute_query", { id, database, sql }),
  executeQueryStream: (
    channel: Channel<StreamEvent>,
    id: number,
    database: string | null,
    sql: string,
  ) => invoke<void>("execute_query_stream", { channel, id, database, sql }),
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
  ) => invoke<string>("export_result", { id, database, sql, format, table }),
  buildEditSql: (
    id: number,
    table: string,
    pk: [string, CellValue][],
    set: [string, CellValue][],
  ) => invoke<string>("build_edit_sql", { id, table, pk, set }),
  executeEdit: (
    id: number,
    database: string | null,
    table: string,
    pk: [string, CellValue][],
    set: [string, CellValue][],
  ) => invoke<QueryOutput>("execute_edit", { id, database, table, pk, set }),

  createDatabase: (id: number, name: string) =>
    invoke<QueryOutput>("create_database", { id, name }),
  dropDatabase: (id: number, name: string) =>
    invoke<QueryOutput>("drop_database", { id, name }),
  createTable: (id: number, database: string, name: string, columns: ColumnDef[]) =>
    invoke<QueryOutput>("create_table", { id, database, name, columns }),
  renameTable: (id: number, database: string, oldName: string, newName: string) =>
    invoke<QueryOutput>("rename_table", { id, database, oldName, newName }),
  dropTable: (id: number, database: string, name: string) =>
    invoke<QueryOutput>("drop_table", { id, database, name }),

  listProjects: () => invoke<Project[]>("list_projects"),
  createProject: (name: string) => invoke<Project>("create_project", { name }),
  renameProject: (id: string, name: string) =>
    invoke<Project>("rename_project", { id, name }),
  deleteProject: (id: string) => invoke<void>("delete_project", { id }),

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
