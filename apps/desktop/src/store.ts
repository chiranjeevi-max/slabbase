import { layoutFor, workspaceActions } from "./storeWorkspace";
import { schemaActions } from "./storeSchema";
import { recordActions } from "./storeRecords";
import { create } from "zustand";
import { api, frameRecords, isCancelledError, SlabError } from "./api";
import {
  applyAppearance,
  initialAppearance,
  mergeAppearance,
  resolveTheme,
  saveAppearance,
} from "./appearance";
import type {
  Filter,
  PageQuery,
  SortSpec,
  UndoState,
} from "./bindings";

import type { AppStore, Tone, Toast } from "./storeTypes";

export type { AppStore, FormStatus, Tone, Toast } from "./storeTypes";

const PAGE_SIZE = 200;

const EMPTY_UNDO: UndoState = {
  can_undo: false,
  can_redo: false,
  undo_label: null,
  redo_label: null,
  depth: 0,
};

let pageRequest = 0;
const inspectorRequest = { token: 0 };
let toastCounter = 0;

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

  const loadFirstPage = async () => {
    const state = get();
    if (!state.activeTable) {
      set({
        rows: [],
        hasMore: false,
        nextCursor: null,
        elapsedMs: 0,
        loadingRows: false,
      });
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
    ...recordActions({ set, get, notify, notifyError, inspectorRequest, loadFirstPage }),
    ...schemaActions({ set, get, notify, notifyError }),
    ...workspaceActions({
      set,
      get,
      notify,
      notifyError,
      inspectorRequest,
      invalidatePage: () => { pageRequest += 1; },
      loadFirstPage,
    }),
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
      inspectorRequest.token += 1;
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
        loadingRows: false,
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
