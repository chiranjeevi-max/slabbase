import type { StoreApi } from "zustand";
import { api } from "./api";
import type { Layout, Schema } from "./bindings";
import type { AppStore } from "./storeTypes";

export function firstTable(schema: Schema | null): string | null {
  return schema?.tables[0]?.name ?? null;
}

export function layoutFor(schema: Schema | null, table: string | null): Layout | null {
  if (!schema || !table) return null;
  return schema.layouts.find((layout) => layout.table === table) ?? null;
}

function firstEditableColumn(schema: Schema | null, table: string | null): string {
  const definition = schema?.tables.find((candidate) => candidate.name === table);
  return definition?.columns[0]?.name ?? "id";
}

export function workspaceActions({
  set,
  get,
  notify,
  notifyError,
  inspectorRequest,
  invalidatePage,
  loadFirstPage,
}: {
  set: StoreApi<AppStore>["setState"];
  get: StoreApi<AppStore>["getState"];
  notify: AppStore["notify"];
  notifyError: (error: unknown) => void;
  inspectorRequest: { token: number };
  invalidatePage: () => void;
  loadFirstPage: AppStore["loadFirstPage"];
}): Pick<AppStore,
  | "initialize"
  | "openWorkspace"
  | "createWorkspace"
  | "loadSample"
  | "closeWorkspace"
  | "openAudit"
  | "closeAudit"
  | "backup"
  | "checkIntegrity"
> {
  return {
    initialize: async () => {
      invalidatePage();
      inspectorRequest.token += 1;
      set({
        booting: true,
        bundle: null,
        bundleTable: null,
        selectedId: null,
        loadingRows: false,
      });
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
      invalidatePage();
      inspectorRequest.token += 1;
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
      invalidatePage();
      inspectorRequest.token += 1;
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
      invalidatePage();
      inspectorRequest.token += 1;
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
      invalidatePage();
      inspectorRequest.token += 1;
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
          loadingRows: false,
        });
      } catch (error) {
        notifyError(error);
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
  };
}
