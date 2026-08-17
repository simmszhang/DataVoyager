import { invoke } from "@tauri-apps/api/core";

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

export const api = {
  listDrivers: () => invoke<DriverInfo[]>("list_drivers"),
  testConnection: (params: ConnectParams) =>
    invoke<string>("test_connection", { params }),

  connect: (params: ConnectParams, projectId?: string | null) =>
    invoke<ConnectResponse>("connect", { params, projectId }),
  disconnect: (id: number) => invoke<void>("disconnect", { id }),
  listConnections: () => invoke<ConnectionSummary[]>("list_connections"),

  listDatabases: (id: number) => invoke<string[]>("list_databases", { id }),
  listTables: (id: number, database: string) =>
    invoke<TableInfo[]>("list_tables", { id, database }),
  listColumns: (id: number, database: string, table: string) =>
    invoke<ColumnInfo[]>("list_columns", { id, database, table }),
  executeQuery: (id: number, database: string | null, sql: string) =>
    invoke<QueryOutput>("execute_query", { id, database, sql }),

  listProjects: () => invoke<Project[]>("list_projects"),
  createProject: (name: string) => invoke<Project>("create_project", { name }),

  searchHistory: (query: string, projectId?: string | null) =>
    invoke<StatementHit[]>("search_history", { query, projectId }),
  listHistory: (projectId?: string | null) =>
    invoke<StatementHit[]>("list_history", { projectId }),
  listExecutions: (projectId?: string | null) =>
    invoke<ExecutionRecord[]>("list_executions", { projectId }),
};
