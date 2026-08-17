import { create } from "zustand";
import {
  ColumnInfo,
  ConnectionSummary,
  Project,
  StreamResult,
  TableInfo,
} from "./api";

/// 每个连接独立的工作区状态（多连接并存互不影响的关键）。
export interface WorkspaceState {
  databases: string[];
  selectedDb: string;
  tables: TableInfo[];
  selectedTable: string | null;
  columns: ColumnInfo[];
  query: string;
  result: StreamResult | null;
  /// 流式缓冲变更版本号（result 就地追加，靠版本号触发渲染）。
  resultVersion: number;
  inTransaction: boolean;
  autocommit: boolean;
  running: boolean;
  error: string | null;
}

export function emptyWorkspace(): WorkspaceState {
  return {
    databases: [],
    selectedDb: "",
    tables: [],
    selectedTable: null,
    columns: [],
    query: "SELECT 1",
    result: null,
    resultVersion: 0,
    inTransaction: false,
    autocommit: true,
    running: false,
    error: null,
  };
}

interface AppStore {
  projects: Project[];
  projectId: string | null;
  connections: ConnectionSummary[];
  activeId: number | null;
  tabs: number[];
  workspaces: Record<number, WorkspaceState>;

  setProjects: (p: Project[]) => void;
  setProjectId: (id: string | null) => void;
  setConnections: (c: ConnectionSummary[]) => void;
  openConnection: (c: ConnectionSummary) => void;
  closeConnection: (id: number) => void;
  setActive: (id: number | null) => void;
  updateWorkspace: (id: number, patch: Partial<WorkspaceState>) => void;
  /// 就地变更 result 并 bump 版本（流式 O(1) 追加）。
  mutateResult: (id: number, fn: (r: StreamResult) => void) => void;
}

export const useStore = create<AppStore>((set) => ({
  projects: [],
  projectId: null,
  connections: [],
  activeId: null,
  tabs: [],
  workspaces: {},

  setProjects: (projects) => set({ projects }),
  setProjectId: (projectId) => set({ projectId }),
  setConnections: (connections) => set({ connections }),

  openConnection: (c) =>
    set((s) => {
      if (s.workspaces[c.id]) {
        return { activeId: c.id };
      }
      return {
        activeId: c.id,
        tabs: [...s.tabs, c.id],
        workspaces: { ...s.workspaces, [c.id]: emptyWorkspace() },
      };
    }),

  closeConnection: (id) =>
    set((s) => {
      const workspaces = { ...s.workspaces };
      delete workspaces[id];
      const tabs = s.tabs.filter((t) => t !== id);
      const activeId =
        s.activeId === id ? (tabs[tabs.length - 1] ?? null) : s.activeId;
      return { workspaces, tabs, activeId };
    }),

  setActive: (activeId) => set({ activeId }),

  updateWorkspace: (id, patch) =>
    set((s) => {
      const ws = s.workspaces[id];
      if (!ws) return s;
      return { workspaces: { ...s.workspaces, [id]: { ...ws, ...patch } } };
    }),

  mutateResult: (id, fn) =>
    set((s) => {
      const ws = s.workspaces[id];
      if (!ws || !ws.result) return s;
      fn(ws.result);
      return {
        workspaces: {
          ...s.workspaces,
          [id]: { ...ws, resultVersion: ws.resultVersion + 1 },
        },
      };
    }),
}));
