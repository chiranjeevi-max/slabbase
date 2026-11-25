import { create } from "zustand";
import { api, SlabError, type RowValues } from "./api";
import type {
  AuditEntry,
  ColumnDraft,
  Filter,
  IndexSpec,
  Layout,
  PageQuery,
  Record as SlabRecord,
  RecordBundle,
  RelationDraft,
  Schema,
  SortSpec,
  TableDraft,
  UndoState,
  Value,
  WorkspaceInfoDto,
} from "./bindings";

export type Theme = "light" | "dark";
export type Tone = "info" | "success" | "error";

export interface Toast {
  id: number;
  tone: Tone;
  message: string;
  code?: string;
}

const PAGE_SIZE = 200;

const EMPTY_UNDO: UndoState = {
  can_undo: false,
  can_redo: false,
  undo_label: null,
  redo_label: null,
  depth: 0,
};

let pageRequest = 0;
let toastCounter = 0;

function initialTheme(): Theme {
  if (typeof window === "undefined") return "light";
  const stored = window.localStorage.getItem("slabbase.theme");
  if (stored === "light" || stored === "dark") return stored;
  return window.matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light";
}

function firstTable(schema: Schema | null): string | null {
  return schema?.tables[0]?.name ?? null;
}

function layoutFor(schema: Schema | null, table: string | null): Layout | null {
  if (!schema || !table) return null;
  return schema.layouts.find((layout) => layout.table === table) ?? null;
}

function firstEditableColumn(schema: Schema | null, table: string | null): string {
  const definition = schema?.tables.find((candidate) => candidate.name === table);
  return definition?.columns[0]?.name ?? "id";
}

function describeError(error: unknown): { message: string; code?: string } {
  if (error instanceof SlabError) return { message: error.message, code: error.code };
  if (error instanceof Error) return { message: error.message };
  return { message: "Unexpected failure" };
}

export function errorText(error: unknown): string {
  const described = describeError(error);
  return described.code ? `${described.code}: ${described.message}` : described.message;
}

export function pageQueryFor(
  state: Pick<AppStore, "activeTable" | "sort" | "filters">,
  cursor: PageQuery["cursor"] = null,
  limit = PAGE_SIZE,
): PageQuery {
  return {
    table: state.activeTable ?? "",
    limit,
    cursor,
    sort: state.sort,
    filters: state.filters,
    search: null,
  };
}

export interface AppStore {
  info: WorkspaceInfoDto | null;
  schema: Schema | null;
  booting: boolean;
  busy: boolean;
  theme: Theme;
  toast: Toast | null;

  activeTable: string | null;
  activeLayoutId: string | null;
  view: "table" | "form";
  rows: SlabRecord[];
  nextCursor: PageQuery["cursor"];
  hasMore: boolean;
  elapsedMs: number;
  loadingRows: boolean;
  sort: SortSpec[];
  filters: Filter[];
  filterColumn: string;
  filterText: string;

  selectedId: number | null;
  bundle: RecordBundle | null;
  creating: boolean;
  undoState: UndoState;

  paletteOpen: boolean;
  schemaOpen: boolean;
  schemaTab: "table" | "column" | "relation" | "manage";
  importOpen: boolean;
  printOpen: boolean;
  auditOpen: boolean;
  audit: AuditEntry[];
  saveTick: number;
  historyTick: number;
  integrity: string;

  initialize: () => Promise<void>;
  openWorkspace: (path: string) => Promise<void>;
  createWorkspace: (path: string, name: string) => Promise<void>;
  loadSample: () => Promise<void>;
  closeWorkspace: () => Promise<void>;

  notify: (tone: Tone, message: string, code?: string) => void;
  clearToast: () => void;
  setTheme: (theme: Theme) => void;
  toggleTheme: () => void;

  selectTable: (table: string) => Promise<void>;
  selectLayout: (id: string) => void;
  setView: (view: "table" | "form") => void;
  setSort: (column: string) => Promise<void>;
  setFilterColumn: (column: string) => void;
  applyFilter: (text: string) => Promise<void>;
  clearFilter: () => Promise<void>;
  loadFirstPage: () => Promise<void>;
  loadMore: () => Promise<void>;

  selectRow: (id: number) => Promise<void>;
  updatePortalRow: (
    table: string,
    id: number,
    column: string,
    value: Value,
  ) => Promise<SlabRecord | null>;
  insertPortalRow: (table: string, values: RowValues) => Promise<SlabRecord | null>;
  deletePortalRow: (table: string, id: number) => Promise<boolean>;
  jumpToRecord: (table: string, id: number) => Promise<void>;
  closeRecord: () => void;
  refreshBundle: () => Promise<void>;
  beginCreate: () => void;
  cancelCreate: () => void;

  updateCell: (
    table: string,
    id: number,
    column: string,
    value: Value,
  ) => Promise<boolean>;
  insertRecord: (values: RowValues) => Promise<boolean>;
  deleteRecord: (id: number) => Promise<void>;

  undo: () => Promise<void>;
  redo: () => Promise<void>;

  createTable: (draft: TableDraft) => Promise<boolean>;
  addColumn: (table: string, draft: ColumnDraft) => Promise<boolean>;
  createRelation: (draft: RelationDraft) => Promise<boolean>;
  renameTable: (oldName: string, newName: string) => Promise<boolean>;
  renameColumn: (table: string, oldName: string, newName: string) => Promise<boolean>;
  dropTable: (name: string) => Promise<boolean>;
  dropColumn: (table: string, column: string) => Promise<boolean>;
  dropRelation: (name: string) => Promise<boolean>;
  toggleIndex: (table: string, column: string, enabled: boolean) => Promise<IndexSpec[] | null>;
  saveLayout: (layout: Layout) => Promise<boolean>;

  openAudit: () => Promise<void>;
  closeAudit: () => void;
  backup: (path: string) => Promise<boolean>;
  checkIntegrity: () => Promise<void>;
  refreshAfterImport: () => Promise<void>;

  setPaletteOpen: (open: boolean) => void;
  setSchemaOpen: (open: boolean) => void;
  openSchema: (tab: "table" | "column" | "relation" | "manage") => void;
  insertChildRow: (table: string, values: RowValues) => Promise<boolean>;
  deleteChildRow: (table: string, id: number) => Promise<void>;
  setImportOpen: (open: boolean) => void;
  setPrintOpen: (open: boolean) => void;
}

export const useStore = create<AppStore>((set, get) => {
  const notify = (tone: Tone, message: string, code?: string) => {
    const toast: Toast = { id: ++toastCounter, tone, message, code };
    set({ toast });
    window.setTimeout(() => {
      set((state) => (state.toast?.id === toast.id ? { toast: null } : {}));
    }, 4_800);
  };

  const notifyError = (error: unknown) => {
    const described = describeError(error);
    notify("error", described.message, described.code);
  };

  const refreshMeta = async () => {
    const [schema, info, undoState] = await Promise.all([
      api.getSchema(),
      api.workspaceInfo(),
      api.undoState(),
    ]);
    const state = get();
    const activeTable =
      state.activeTable && schema.tables.some((table) => table.name === state.activeTable)
        ? state.activeTable
        : firstTable(schema);
    const activeLayoutId =
      schema.layouts.find(
        (layout) => layout.id === state.activeLayoutId && layout.table === activeTable,
      )?.id ??
      layoutFor(schema, activeTable)?.id ??
      null;
    set({ schema, info: info ?? state.info, undoState, activeTable, activeLayoutId });
    return { schema, activeTable };
  };

  const loadFirstPage = async () => {
    const state = get();
    if (!state.activeTable) {
      set({ rows: [], hasMore: false, nextCursor: null, elapsedMs: 0 });
      return;
    }
    const token = ++pageRequest;
    set({ loadingRows: true });
    try {
      const query: PageQuery = {
        table: state.activeTable,
        limit: PAGE_SIZE,
        cursor: null,
        sort: state.sort,
        filters: state.filters,
        search: null,
      };
      const page = await api.pageRows(query);
      if (token !== pageRequest) return;
      set({
        rows: page.rows,
        nextCursor: page.next_cursor,
        hasMore: page.next_cursor !== null,
        elapsedMs: page.elapsed_ms,
        loadingRows: false,
      });
    } catch (error) {
      if (token !== pageRequest) return;
      set({ loadingRows: false });
      notifyError(error);
    }
  };

  return {
    info: null,
    schema: null,
    booting: true,
    busy: false,
    theme: initialTheme(),
    toast: null,

    activeTable: null,
    activeLayoutId: null,
    view: "table",
    rows: [],
    nextCursor: null,
    hasMore: false,
    elapsedMs: 0,
    loadingRows: false,
    sort: [],
    filters: [],
    filterColumn: "id",
    filterText: "",

    selectedId: null,
    bundle: null,
    creating: false,
    undoState: EMPTY_UNDO,

    paletteOpen: false,
    schemaOpen: false,
    schemaTab: "table",
    importOpen: false,
    printOpen: false,
    auditOpen: false,
    audit: [],
    saveTick: 0,
    historyTick: 0,
    integrity: "ok",

    initialize: async () => {
      set({ booting: true });
      try {
        const info = await api.workspaceInfo();
        if (!info) {
          set({ info: null, schema: null, booting: false, activeTable: null });
          return;
        }
        const schema = await api.getSchema();
        const undoState = await api.undoState();
        const activeTable = firstTable(schema);
        set({
          info,
          schema,
          undoState,
          booting: false,
          activeTable,
          activeLayoutId: layoutFor(schema, activeTable)?.id ?? null,
          filterColumn: firstEditableColumn(schema, activeTable),
          rows: [],
          selectedId: null,
          bundle: null,
          creating: false,
        });
        if (activeTable) await loadFirstPage();
      } catch (error) {
        set({ booting: false, info: null, schema: null });
        notifyError(error);
      }
    },

    openWorkspace: async (path) => {
      set({ busy: true });
      try {
        await api.openWorkspace(path);
        set({ busy: false, view: "table", sort: [], filters: [], filterText: "" });
        await get().initialize();
        notify("success", "Workspace opened");
      } catch (error) {
        set({ busy: false });
        notifyError(error);
      }
    },

    createWorkspace: async (path, name) => {
      set({ busy: true });
      try {
        await api.createWorkspace(path, name);
        set({ busy: false, view: "table", sort: [], filters: [], filterText: "" });
        await get().initialize();
        notify("success", "Workspace created");
      } catch (error) {
        set({ busy: false });
        notifyError(error);
      }
    },

    loadSample: async () => {
      set({ busy: true });
      try {
        await api.loadSample();
        set({ busy: false, view: "table", sort: [], filters: [], filterText: "" });
        await get().initialize();
        notify("success", "Sample studio loaded");
      } catch (error) {
        set({ busy: false });
        notifyError(error);
      }
    },

    closeWorkspace: async () => {
      try {
        await api.closeWorkspace();
        set({
          info: null,
          schema: null,
          rows: [],
          bundle: null,
          selectedId: null,
          activeTable: null,
          view: "table",
        });
      } catch (error) {
        notifyError(error);
      }
    },

    notify,
    clearToast: () => set({ toast: null }),

    setTheme: (theme) => {
      window.localStorage.setItem("slabbase.theme", theme);
      document.documentElement.dataset["theme"] = theme;
      set({ theme });
    },

    toggleTheme: () => {
      const theme = get().theme === "light" ? "dark" : "light";
      get().setTheme(theme);
    },

    selectTable: async (table) => {
      const state = get();
      if (state.activeTable === table) return;
      const definition = state.schema?.tables.find((candidate) => candidate.name === table);
      set({
        activeTable: table,
        activeLayoutId: layoutFor(state.schema, table)?.id ?? null,
        selectedId: null,
        bundle: null,
        creating: false,
        rows: [],
        nextCursor: null,
        hasMore: true,
        sort: [],
        filters: [],
        filterText: "",
        filterColumn: definition?.columns[0]?.name ?? "id",
        view: "table",
      });
      await loadFirstPage();
    },

    selectLayout: (id) => {
      set({ activeLayoutId: id, view: "form" });
    },

    setView: (view) => set({ view }),

    setSort: async (column) => {
      const state = get();
      const current = state.sort[0];
      const sort: SortSpec[] =
        current?.column === column
          ? [{ column, direction: current.direction === "asc" ? "desc" : "asc" }]
          : [{ column, direction: "asc" }];
      set({ sort });
      await get().loadFirstPage();
    },

    setFilterColumn: (column) => set({ filterColumn: column }),

    applyFilter: async (text) => {
      const state = get();
      const filters: Filter[] = text.trim()
        ? [
            {
              column: state.filterColumn,
              op: "contains",
              value: { Text: text },
            },
          ]
        : [];
      set({ filters, filterText: text });
      await get().loadFirstPage();
    },

    clearFilter: async () => {
      set({ filters: [], filterText: "" });
      await get().loadFirstPage();
    },

    loadFirstPage,

    loadMore: async () => {
      const state = get();
      if (!state.activeTable || state.loadingRows || !state.hasMore) return;
      const token = pageRequest;
      const query: PageQuery = {
        table: state.activeTable,
        limit: PAGE_SIZE,
        cursor: state.nextCursor,
        sort: state.sort,
        filters: state.filters,
        search: null,
      };
      set({ loadingRows: true });
      try {
        const page = await api.pageRows(query);
        if (token !== pageRequest) return;
        set((current) => {
          const seen = new Set(current.rows.map((row) => row.id));
          const appended = page.rows.filter((row) => !seen.has(row.id));
          return {
            rows: [...current.rows, ...appended],
            nextCursor: page.next_cursor,
            hasMore: page.next_cursor !== null,
            loadingRows: false,
            elapsedMs: page.elapsed_ms,
          };
        });
      } catch (error) {
        if (token !== pageRequest) return;
        set({ loadingRows: false });
        notifyError(error);
      }
    },

    selectRow: async (id) => {
      const state = get();
      if (!state.activeTable) return;
      set({ selectedId: id, creating: false });
      try {
        const record = await api.readRecord(state.activeTable, id);
        set({ bundle: { record, portals: {} } });
      } catch (error) {
        notifyError(error);
      }
    },

    jumpToRecord: async (table, id) => {
      if (get().activeTable !== table) {
        await get().selectTable(table);
      }
      await get().selectRow(id);
    },

    closeRecord: () => set({ selectedId: null, bundle: null }),

    refreshBundle: async () => {
      const state = get();
      if (!state.activeTable || state.selectedId === null) return;
      try {
        const record = await api.readRecord(state.activeTable, state.selectedId);
        set({ bundle: { record, portals: {} } });
      } catch (error) {
        notifyError(error);
      }
    },

    beginCreate: () => set({ creating: true, selectedId: null, bundle: null }),

    cancelCreate: () => set({ creating: false }),

    updateCell: async (table, id, column, value) => {
      const state = get();
      try {
        const record = await api.updateRow(table, id, { [column]: value });
        const undoState = await api.undoState();
        if (state.activeTable === table) {
          set((current) => ({
            rows: current.rows.map((row) => (row.id === id ? record : row)),
            undoState,
            saveTick: current.saveTick + 1,
          }));
        } else {
          set({ undoState, saveTick: state.saveTick + 1 });
        }
        await get().refreshBundle();
        const info = await api.workspaceInfo();
        if (info) set({ info });
        return true;
      } catch (error) {
        notifyError(error);
        return false;
      }
    },

    insertRecord: async (values) => {
      const state = get();
      if (!state.activeTable) return false;
      try {
        const record = await api.insertRow(state.activeTable, values);
        set({ creating: false, selectedId: record.id });
        await loadFirstPage();
        const bundle = { record, portals: {} };
        const [undoState, info] = await Promise.all([api.undoState(), api.workspaceInfo()]);
        set({ bundle, undoState, info: info ?? state.info, saveTick: state.saveTick + 1 });
        notify("success", "Record created");
        return true;
      } catch (error) {
        notifyError(error);
        return false;
      }
    },

    deleteRecord: async (id) => {
      const state = get();
      if (!state.activeTable) return;
      try {
        await api.deleteRow(state.activeTable, id);
        const [undoState, info] = await Promise.all([api.undoState(), api.workspaceInfo()]);
        set({
          selectedId: null,
          bundle: null,
          undoState,
          info: info ?? state.info,
        });
        await loadFirstPage();
        notify("success", "Record deleted");
      } catch (error) {
        notifyError(error);
      }
    },

    undo: async () => {
      try {
        const undoState = await api.undo();
        set((current) => ({ undoState, historyTick: current.historyTick + 1 }));
        await refreshMeta();
        await get().loadFirstPage();
        await get().refreshBundle();
        notify("info", undoState.redo_label ? `Undid change` : "Undone");
      } catch (error) {
        notifyError(error);
      }
    },

    redo: async () => {
      try {
        const undoState = await api.redo();
        set((current) => ({ undoState, historyTick: current.historyTick + 1 }));
        await refreshMeta();
        await get().loadFirstPage();
        await get().refreshBundle();
        notify("info", "Redone");
      } catch (error) {
        notifyError(error);
      }
    },

    createTable: async (draft) => {
      try {
        const table = await api.createTable(draft);
        await refreshMeta();
        await get().selectTable(table.name);
        notify("success", `Table ${table.name} created`);
        return true;
      } catch (error) {
        notifyError(error);
        return false;
      }
    },

    addColumn: async (table, draft) => {
      try {
        await api.addColumn(table, draft);
        await refreshMeta();
        await get().loadFirstPage();
        await get().refreshBundle();
        notify("success", `Column ${draft.name} added`);
        return true;
      } catch (error) {
        notifyError(error);
        return false;
      }
    },

    createRelation: async (draft) => {
      try {
        await api.createRelation(draft);
        await refreshMeta();
        await get().refreshBundle();
        notify("success", `Relation ${draft.name} created`);
        return true;
      } catch (error) {
        notifyError(error);
        return false;
      }
    },

    renameTable: async (oldName, newName) => {
      const state = get();
      try {
        await api.renameTable(oldName, newName);
        await refreshMeta();
        if (state.activeTable === oldName) {
          await get().selectTable(newName);
        } else {
          await get().loadFirstPage();
        }
        notify("success", `Table renamed to ${newName}`);
        return true;
      } catch (error) {
        notifyError(error);
        return false;
      }
    },

    renameColumn: async (table, oldName, newName) => {
      try {
        await api.renameColumn(table, oldName, newName);
        await refreshMeta();
        await get().loadFirstPage();
        await get().refreshBundle();
        notify("success", `Column renamed to ${newName}`);
        return true;
      } catch (error) {
        notifyError(error);
        return false;
      }
    },

    dropTable: async (name) => {
      try {
        await api.dropTable(name);
        await refreshMeta();
        const next = get().activeTable;
        if (next) await get().selectTable(next);
        notify("success", `Table ${name} dropped`);
        return true;
      } catch (error) {
        notifyError(error);
        return false;
      }
    },

    dropColumn: async (table, column) => {
      try {
        await api.dropColumn(table, column);
        await refreshMeta();
        await get().loadFirstPage();
        await get().refreshBundle();
        notify("success", `Column ${column} dropped`);
        return true;
      } catch (error) {
        notifyError(error);
        return false;
      }
    },

    dropRelation: async (name) => {
      try {
        await api.dropRelation(name);
        await refreshMeta();
        await get().refreshBundle();
        notify("success", `Relation ${name} dropped`);
        return true;
      } catch (error) {
        notifyError(error);
        return false;
      }
    },

    toggleIndex: async (table, column, enabled) => {
      try {
        const indexes = await api.toggleIndex(table, column, enabled);
        notify("success", enabled ? `Index enabled on ${column}` : `Index removed from ${column}`);
        return indexes;
      } catch (error) {
        notifyError(error);
        return null;
      }
    },

    saveLayout: async (layout) => {
      try {
        const saved = await api.saveLayout(layout);
        await refreshMeta();
        set({ activeLayoutId: saved.id, view: "form" });
        notify("success", `Form ${saved.name} saved`);
        return true;
      } catch (error) {
        notifyError(error);
        return false;
      }
    },

    openAudit: async () => {
      try {
        const audit = await api.auditEntries(100, 0);
        set({ audit, auditOpen: true });
      } catch (error) {
        notifyError(error);
      }
    },

    closeAudit: () => set({ auditOpen: false }),

    backup: async (path) => {
      try {
        await api.backup(path);
        notify("success", "Backup written");
        return true;
      } catch (error) {
        notifyError(error);
        return false;
      }
    },

    checkIntegrity: async () => {
      try {
        const report = await api.integrityCheck();
        set({
          integrity:
            report.integrity === "ok" && report.foreignKeyViolations === 0
              ? "ok"
              : "check",
        });
        notify(
          report.integrity === "ok" && report.foreignKeyViolations === 0 ? "success" : "error",
          `SQLite integrity: ${report.integrity}; foreign key violations: ${report.foreignKeyViolations}`,
        );
      } catch (error) {
        notifyError(error);
      }
    },

    refreshAfterImport: async () => {
      await refreshMeta();
      await get().loadFirstPage();
      await get().refreshBundle();
    },

    setPaletteOpen: (open) => set({ paletteOpen: open }),
    setSchemaOpen: (open) => set({ schemaOpen: open }),
    openSchema: (tab) => set({ schemaOpen: true, schemaTab: tab }),
    setImportOpen: (open) => set({ importOpen: open }),
    setPrintOpen: (open) => set({ printOpen: open }),

    updatePortalRow: async (table, id, column, value) => {
      const state = get();
      try {
        const record = await api.updateRow(table, id, { [column]: value });
        const undoState = await api.undoState();
        set({ undoState, saveTick: state.saveTick + 1 });
        await get().refreshBundle();
        if (state.activeTable === table) {
          set((current) => ({
            rows: current.rows.map((row) => (row.id === id ? record : row)),
          }));
        }
        const info = await api.workspaceInfo();
        if (info) set({ info });
        return record;
      } catch (error) {
        notifyError(error);
        return null;
      }
    },

    insertPortalRow: async (table, values) => {
      const state = get();
      try {
        const record = await api.insertRow(table, values);
        const [undoState, info] = await Promise.all([api.undoState(), api.workspaceInfo()]);
        set({ undoState, info: info ?? state.info, saveTick: state.saveTick + 1 });
        await get().refreshBundle();
        if (state.activeTable === table) await get().loadFirstPage();
        return record;
      } catch (error) {
        notifyError(error);
        return null;
      }
    },

    deletePortalRow: async (table, id) => {
      try {
        await api.deleteRow(table, id);
        const [undoState, info] = await Promise.all([api.undoState(), api.workspaceInfo()]);
        set({ undoState, info: info ?? get().info });
        await get().refreshBundle();
        return true;
      } catch (error) {
        notifyError(error);
        return false;
      }
    },

    insertChildRow: async (table, values) => {
      const state = get();
      try {
        await api.insertRow(table, values);
        const [undoState, info] = await Promise.all([api.undoState(), api.workspaceInfo()]);
        set({ undoState, info: info ?? state.info, saveTick: state.saveTick + 1 });
        await get().refreshBundle();
        if (state.activeTable === table) await get().loadFirstPage();
        return true;
      } catch (error) {
        notifyError(error);
        return false;
      }
    },

    deleteChildRow: async (table, id) => {
      const state = get();
      try {
        await api.deleteRow(table, id);
        const [undoState, info] = await Promise.all([api.undoState(), api.workspaceInfo()]);
        set({ undoState, info: info ?? state.info });
        await get().refreshBundle();
        if (state.activeTable === table) await get().loadFirstPage();
      } catch (error) {
        notifyError(error);
      }
    },
  };
});
