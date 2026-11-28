import type { StoreApi } from "zustand";
import { api } from "./api";
import { firstTable, layoutFor } from "./storeWorkspace";
import type { AppStore } from "./storeTypes";

export function schemaActions({
  set,
  get,
  notify,
  notifyError,
}: {
  set: StoreApi<AppStore>["setState"];
  get: StoreApi<AppStore>["getState"];
  notify: AppStore["notify"];
  notifyError: (error: unknown) => void;
}): Pick<AppStore,
  | "undo"
  | "redo"
  | "createTable"
  | "addColumn"
  | "createRelation"
  | "renameTable"
  | "renameColumn"
  | "dropTable"
  | "dropColumn"
  | "dropRelation"
  | "toggleIndex"
  | "saveLayout"
  | "refreshAfterImport"
> {
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
  };

  const refreshWorkspaceData = async () => {
    await refreshMeta();
    await get().loadFirstPage();
    await get().refreshBundle();
  };

  return {
    undo: async () => {
      try {
        const undoState = await api.undo();
        set((current) => ({ undoState, historyTick: current.historyTick + 1 }));
        await refreshWorkspaceData();
        notify("info", undoState.redo_label ? `Undid change` : "Undone");
      } catch (error) {
        notifyError(error);
      }
    },

    redo: async () => {
      try {
        const undoState = await api.redo();
        set((current) => ({ undoState, historyTick: current.historyTick + 1 }));
        await refreshWorkspaceData();
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
        await refreshWorkspaceData();
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
        await refreshWorkspaceData();
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
        await refreshWorkspaceData();
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

    refreshAfterImport: async () => {
      await refreshWorkspaceData();
    },
  };
}
