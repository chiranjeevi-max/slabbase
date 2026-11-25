import { create } from "zustand";
import { api, frameRecords, isCancelledError, SlabError, type RowValues } from "./api";
import {
  applyAppearance,
  initialAppearance,
  mergeAppearance,
  resolveTheme,
  saveAppearance,
  type AppearancePrefs,
  type ResolvedTheme,
} from "./appearance";
import type {
  AuditEntry,
  ColumnDraft,
  Filter,
  IndexHint,
  IndexSpec,
  IntegrityReportDto,
  Layout,
  PageQuery,
  Record as SlabRecord,
  RecordBundle,
  RecordRevision,
  RelationDraft,
  Schema,
  SortSpec,
  TableDraft,
  UndoState,
  Value,
  WorkspaceInfoDto,
} from "./bindings";

export type Tone = "info" | "success" | "error";

export interface Toast {
  id: number;
  tone: Tone;
  message: string;
  code?: string;
}

export interface FormStatus {
  name: string;
  items: number;
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
let inspectorRequest = 0;
let toastCounter = 0;

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
  appearance: AppearancePrefs;
  resolvedTheme: ResolvedTheme;
  appearanceOpen: boolean;
  formStatus: FormStatus | null;
  toast: Toast | null;

  activeTable: string | null;
  activeLayoutId: string | null;
  view: "table" | "form" | "schema";
  inspectorOpen: boolean;
  rows: SlabRecord[];
  nextCursor: PageQuery["cursor"];
  hasMore: boolean;
  elapsedMs: number;
  loadingRows: boolean;
  sort: SortSpec[];
  filters: Filter[];
  filterColumn: string;
  filterText: string;
  indexHint: IndexHint | null;

  selectedId: number | null;
  bundle: RecordBundle | null;
  bundleTable: string | null;
  creating: boolean;
  undoState: UndoState;
  historyOpen: boolean;
  recordHistory: RecordRevision[];
  historyLoading: boolean;

  paletteOpen: boolean;
  schemaOpen: boolean;
  schemaTab: "table" | "column" | "relation";
  schemaTargetTable: string | null;
  importOpen: boolean;
  printOpen: boolean;
  auditOpen: boolean;
  audit: AuditEntry[];
  saveTick: number;
  historyTick: number;
  integrity: string;
  integrityReport: IntegrityReportDto | null;
  integrityCheckedAt: number | null;

  initialize: () => Promise<void>;
  openWorkspace: (path: string) => Promise<void>;
  createWorkspace: (path: string, name: string) => Promise<void>;
  loadSample: () => Promise<void>;
  closeWorkspace: () => Promise<void>;

  notify: (tone: Tone, message: string, code?: string) => void;
  clearToast: () => void;
  setAppearance: (patch: Partial<Omit<AppearancePrefs, "version">>) => void;
  setAppearanceOpen: (open: boolean) => void;
  setFormStatus: (status: FormStatus | null) => void;

  selectTable: (table: string) => Promise<void>;
  selectLayout: (id: string) => void;
  setView: (view: "table" | "form" | "schema") => void;
  toggleInspector: () => void;
  setInspectorOpen: (open: boolean) => void;
  setSort: (column: string) => Promise<void>;
  setFilterColumn: (column: string) => void;
  applyFilter: (text: string) => Promise<void>;
  clearFilter: () => Promise<void>;
  loadFirstPage: () => Promise<void>;
  loadMore: () => Promise<void>;

  applyIndexHint: () => Promise<void>;
  dismissIndexHint: () => void;
  openHistory: () => Promise<void>;
  closeHistory: () => void;
  restoreRevision: (auditId: number) => Promise<boolean>;

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
  openSchema: (tab: "table" | "column" | "relation", table?: string) => void;
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
    const selectionIsCurrent =
      state.selectedId !== null && state.bundleTable === activeTable;
    set({
      schema,
      info: info ?? state.info,
      undoState,
      activeTable,
      activeLayoutId,
      ...(selectionIsCurrent
        ? {}
        : { selectedId: null, bundle: null, bundleTable: null }),
    });
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
        rows: frameRecords(page),
        nextCursor: page.next_cursor,
        hasMore: page.next_cursor !== null,
        elapsedMs: page.elapsed_ms,
        indexHint: page.index_hint,
        loadingRows: false,
      });
    } catch (error) {
      if (token !== pageRequest) return;
      set({ loadingRows: false });
      if (isCancelledError(error)) return;
      notifyError(error);
    }
  };

  return {
    info: null,
    schema: null,
    booting: true,
    busy: false,
    appearance: initialAppearance,
    resolvedTheme: resolveTheme(initialAppearance.theme),
    appearanceOpen: false,
    formStatus: null,
    toast: null,

    activeTable: null,
    activeLayoutId: null,
    view: "table",
    inspectorOpen: false,
    rows: [],
    nextCursor: null,
    hasMore: false,
    elapsedMs: 0,
    loadingRows: false,
    sort: [],
    filters: [],
    filterColumn: "id",
    filterText: "",
    indexHint: null,

    selectedId: null,
    bundle: null,
    bundleTable: null,
    creating: false,
    undoState: EMPTY_UNDO,
    historyOpen: false,
    recordHistory: [],
    historyLoading: false,

    paletteOpen: false,
    schemaOpen: false,
    schemaTab: "table",
    schemaTargetTable: null,
    importOpen: false,
    printOpen: false,
    auditOpen: false,
    audit: [],
    saveTick: 0,
    historyTick: 0,
    integrity: "ok",
    integrityReport: null,
    integrityCheckedAt: null,

    initialize: async () => {
      pageRequest += 1;
      inspectorRequest += 1;
      set({ booting: true, bundle: null, bundleTable: null, selectedId: null });
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
          bundleTable: null,
          creating: false,
          inspectorOpen: false,
        });
        if (activeTable) await loadFirstPage();
      } catch (error) {
        set({ booting: false, info: null, schema: null });
        notifyError(error);
      }
    },

    openWorkspace: async (path) => {
      pageRequest += 1;
      inspectorRequest += 1;
      set({ busy: true });
      try {
        await api.openWorkspace(path);
        set({ busy: false, view: "table", sort: [], filters: [], filterText: "", inspectorOpen: false });
        await get().initialize();
        notify("success", "Workspace opened");
      } catch (error) {
        set({ busy: false });
        notifyError(error);
      }
    },

    createWorkspace: async (path, name) => {
      pageRequest += 1;
      inspectorRequest += 1;
      set({ busy: true });
      try {
        await api.createWorkspace(path, name);
        set({ busy: false, view: "table", sort: [], filters: [], filterText: "", inspectorOpen: false });
        await get().initialize();
        notify("success", "Workspace created");
      } catch (error) {
        set({ busy: false });
        notifyError(error);
      }
    },

    loadSample: async () => {
      pageRequest += 1;
      inspectorRequest += 1;
      set({ busy: true });
      try {
        await api.loadSample();
        set({ busy: false, view: "table", sort: [], filters: [], filterText: "", inspectorOpen: false });
        await get().initialize();
        notify("success", "Sample studio loaded");
      } catch (error) {
        set({ busy: false });
        notifyError(error);
      }
    },

    closeWorkspace: async () => {
      pageRequest += 1;
      inspectorRequest += 1;
      try {
        await api.closeWorkspace();
        set({
          info: null,
          schema: null,
          rows: [],
          bundle: null,
          bundleTable: null,
          selectedId: null,
          activeTable: null,
          view: "table",
          inspectorOpen: false,
        });
      } catch (error) {
        notifyError(error);
      }
    },

    notify,
    clearToast: () => set({ toast: null }),

    setAppearance: (patch) => {
      const appearance = mergeAppearance(get().appearance, patch);
      saveAppearance(appearance);
      const resolvedTheme = applyAppearance(appearance);
      set({ appearance, resolvedTheme });
    },

    setAppearanceOpen: (open) => set({ appearanceOpen: open }),

    setFormStatus: (status) => set({ formStatus: status }),

    selectTable: async (table) => {
      const state = get();
      if (state.activeTable === table) return;
      inspectorRequest += 1;
      const definition = state.schema?.tables.find((candidate) => candidate.name === table);
      set({
        activeTable: table,
        activeLayoutId: layoutFor(state.schema, table)?.id ?? null,
        selectedId: null,
        bundle: null,
        bundleTable: null,
        creating: false,
        rows: [],
        nextCursor: null,
        hasMore: true,
        sort: [],
        filters: [],
        filterText: "",
        indexHint: null,
        filterColumn: definition?.columns[0]?.name ?? "id",
        view: "table",
        inspectorOpen: false,
        historyOpen: false,
        historyLoading: false,
        recordHistory: [],
      });
      await loadFirstPage();
    },

    selectLayout: (id) => {
      set({ activeLayoutId: id, view: "form", inspectorOpen: true });
    },

    setView: (view) => {
      if (view === "table") {
        const state = get();
        set({
          view,
          inspectorOpen: state.selectedId !== null || state.creating,
        });
      } else if (view === "form") set({ view, inspectorOpen: true });
      else set({ view, inspectorOpen: false });
    },

    toggleInspector: () => set((state) => ({ inspectorOpen: !state.inspectorOpen })),

    setInspectorOpen: (open) => set({ inspectorOpen: open }),

    setSort: async (column) => {
      const state = get();
      const current = state.sort[0];
      const sort: SortSpec[] =
        current?.column === column
          ? [{ column, direction: current.direction === "asc" ? "desc" : "asc" }]
          : [{ column, direction: "asc" }];
      void api.cancelSearch().catch(() => undefined);
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
      void api.cancelSearch().catch(() => undefined);
      await get().loadFirstPage();
    },

    clearFilter: async () => {
      set({ filters: [], filterText: "" });
      await get().loadFirstPage();
    },

    loadFirstPage,

    applyIndexHint: async () => {
      const state = get();
      const hint = state.indexHint;
      if (!hint) return;
      try {
        await api.createIndex(hint.table, hint.column);
        set({ indexHint: null });
        await get().loadFirstPage();
        notify("success", `Index created on ${hint.column}`);
      } catch (error) {
        notifyError(error);
      }
    },

    dismissIndexHint: () => set({ indexHint: null }),

    openHistory: async () => {
      const state = get();
      if (!state.activeTable || state.selectedId === null) return;
      const table = state.activeTable;
      const id = state.selectedId;
      const token = inspectorRequest;
      set({ historyOpen: true, historyLoading: true });
      try {
        const recordHistory = await api.recordHistory(table, id);
        if (token !== inspectorRequest) return;
        const current = get();
        if (current.activeTable !== table || current.selectedId !== id) return;
        set({ recordHistory, historyLoading: false });
      } catch (error) {
        if (token !== inspectorRequest) return;
        set({ historyLoading: false });
        notifyError(error);
      }
    },

    closeHistory: () => set({ historyOpen: false, recordHistory: [] }),

    restoreRevision: async (auditId) => {
      const state = get();
      if (!state.activeTable || state.selectedId === null) return false;
      const table = state.activeTable;
      const id = state.selectedId;
      try {
        await api.restoreRecordRevision(table, id, auditId);
        const [undoState, recordHistory] = await Promise.all([
          api.undoState(),
          api.recordHistory(table, id),
        ]);
        const current = get();
        if (current.activeTable === table && current.selectedId === id) {
          set({ undoState, recordHistory });
        } else {
          set({ undoState });
        }
        await get().loadFirstPage();
        await get().refreshBundle();
        notify("success", "Revision restored");
        return true;
      } catch (error) {
        notifyError(error);
        return false;
      }
    },

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
          const appended = frameRecords(page).filter((row) => !seen.has(row.id));
          return {
            rows: [...current.rows, ...appended],
            nextCursor: page.next_cursor,
            hasMore: page.next_cursor !== null,
            loadingRows: false,
            elapsedMs: page.elapsed_ms,
            indexHint: page.index_hint ?? current.indexHint,
          };
        });
      } catch (error) {
        if (token !== pageRequest) return;
        set({ loadingRows: false });
        if (isCancelledError(error)) return;
        notifyError(error);
      }
    },

    selectRow: async (id) => {
      const state = get();
      if (!state.activeTable) return;
      const table = state.activeTable;
      const token = ++inspectorRequest;
      set({
        selectedId: id,
        creating: false,
        inspectorOpen: true,
        historyOpen: false,
        historyLoading: false,
        recordHistory: [],
      });
      try {
        const record = await api.readRecord(table, id);
        if (token !== inspectorRequest) return;
        const current = get();
        if (current.activeTable !== table || current.selectedId !== id) return;
        set({ bundle: { record, portals: {} }, bundleTable: table });
      } catch (error) {
        if (token !== inspectorRequest) return;
        if (!isCancelledError(error)) notifyError(error);
      }
    },

    jumpToRecord: async (table, id) => {
      if (get().activeTable !== table) {
        await get().selectTable(table);
      }
      await get().selectRow(id);
    },

    closeRecord: () => {
      inspectorRequest += 1;
      set({
        selectedId: null,
        bundle: null,
        bundleTable: null,
        inspectorOpen: false,
        historyOpen: false,
        recordHistory: [],
      });
    },

    refreshBundle: async () => {
      const state = get();
      if (!state.activeTable || state.selectedId === null) return;
      const table = state.activeTable;
      const id = state.selectedId;
      const token = inspectorRequest;
      try {
        const record = await api.readRecord(table, id);
        if (token !== inspectorRequest) return;
        const current = get();
        if (current.activeTable !== table || current.selectedId !== id) return;
        set({ bundle: { record, portals: {} }, bundleTable: table });
      } catch (error) {
        if (token !== inspectorRequest) return;
        if (!isCancelledError(error)) notifyError(error);
      }
    },

    beginCreate: () => {
      inspectorRequest += 1;
      set({
        creating: true,
        selectedId: null,
        bundle: null,
        bundleTable: null,
        inspectorOpen: true,
      });
    },

    cancelCreate: () => set({ creating: false }),

    updateCell: async (table, id, column, value) => {
      try {
        const record = await api.updateRow(table, id, { [column]: value });
        const undoState = await api.undoState();
        // Check the *current* table after the round trip. A pre-await snapshot
        // would still claim the old table and splice a foreign record into the
        // new table's rows under a colliding id.
        const current = get();
        if (current.activeTable === table) {
          set((latest) => ({
            rows: latest.rows.map((row) => (row.id === id ? record : row)),
            undoState,
            saveTick: latest.saveTick + 1,
          }));
        } else {
          set({ undoState, saveTick: current.saveTick + 1 });
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
      const table = state.activeTable;
      try {
        const record = await api.insertRow(table, values);
        const [undoState, info] = await Promise.all([api.undoState(), api.workspaceInfo()]);
        if (get().activeTable !== table) {
          // The user moved to another table while the insert was in flight.
          // Commit the row, refresh what is on screen, but do not hijack the
          // inspector with a record from the previous table.
          set({
            creating: false,
            undoState,
            info: info ?? get().info,
            saveTick: get().saveTick + 1,
          });
          await loadFirstPage();
          notify("success", "Record created");
          return true;
        }
        const token = ++inspectorRequest;
        set({
          creating: false,
          selectedId: record.id,
          inspectorOpen: true,
          undoState,
          info: info ?? get().info,
          saveTick: get().saveTick + 1,
        });
        await loadFirstPage();
        if (
          token !== inspectorRequest ||
          get().activeTable !== table ||
          get().selectedId !== record.id
        ) {
          return true;
        }
        set({ bundle: { record, portals: {} }, bundleTable: table });
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
      const table = state.activeTable;
      try {
        await api.deleteRow(table, id);
        const [undoState, info] = await Promise.all([api.undoState(), api.workspaceInfo()]);
        if (get().activeTable !== table) {
          set({ undoState, info: info ?? get().info });
          return;
        }
        inspectorRequest += 1;
        set({
          selectedId: null,
          bundle: null,
          bundleTable: null,
          undoState,
          info: info ?? get().info,
          inspectorOpen: false,
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
          const schema = get().schema;
          set({
            activeTable: newName,
            activeLayoutId: layoutFor(schema, newName)?.id ?? null,
            rows: [],
            nextCursor: null,
            hasMore: true,
            selectedId: null,
            bundle: null,
            bundleTable: null,
            creating: false,
            inspectorOpen: false,
          });
        }
        await get().loadFirstPage();
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
        set({
          rows: [],
          nextCursor: null,
          hasMore: true,
          selectedId: null,
          bundle: null,
          bundleTable: null,
          creating: false,
          inspectorOpen: false,
        });
        if (get().activeTable) await get().loadFirstPage();
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
        const healthy =
          report.integrity === "ok" &&
          report.foreignKeyViolations === 0 &&
          report.auditChain;
        set({
          integrity: healthy ? "ok" : "check",
          integrityReport: report,
          integrityCheckedAt: Date.now(),
        });
        notify(
          healthy ? "success" : "error",
          report.auditTamper
            ? `Audit chain: ${report.auditTamper}`
            : `SQLite integrity: ${report.integrity}; foreign keys: ${report.foreignKeyViolations}; audit entries: ${report.auditEntries}`,
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
    openSchema: (tab, table) =>
      set({
        schemaOpen: true,
        schemaTab: tab,
        schemaTargetTable: table ?? get().activeTable,
      }),
    setImportOpen: (open) => set({ importOpen: open }),
    setPrintOpen: (open) => set({ printOpen: open }),

    updatePortalRow: async (table, id, column, value) => {
      try {
        const record = await api.updateRow(table, id, { [column]: value });
        const undoState = await api.undoState();
        const current = get();
        set({ undoState, saveTick: current.saveTick + 1 });
        await get().refreshBundle();
        // Compare the live table, not a snapshot taken before the await.
        if (get().activeTable === table) {
          set((latest) => ({
            rows: latest.rows.map((row) => (row.id === id ? record : row)),
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

  };
});

if (typeof window !== "undefined") {
  const media = window.matchMedia("(prefers-color-scheme: dark)");
  media.addEventListener("change", () => {
    const state = useStore.getState();
    if (state.appearance.theme !== "system") return;
    const resolvedTheme = applyAppearance(state.appearance);
    useStore.setState({ resolvedTheme });
  });

  const transparency = window.matchMedia("(prefers-reduced-transparency: reduce)");
  transparency.addEventListener("change", () => {
    const state = useStore.getState();
    const resolvedTheme = applyAppearance(state.appearance);
    useStore.setState({ resolvedTheme });
  });
}
