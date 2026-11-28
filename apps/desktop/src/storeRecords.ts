import type { StoreApi } from "zustand";
import { api, isCancelledError } from "./api";
import type { AppStore } from "./storeTypes";

export function recordActions({
  set,
  get,
  notify,
  notifyError,
  inspectorRequest,
  loadFirstPage,
}: {
  set: StoreApi<AppStore>["setState"];
  get: StoreApi<AppStore>["getState"];
  notify: AppStore["notify"];
  notifyError: (error: unknown) => void;
  inspectorRequest: { token: number };
  loadFirstPage: AppStore["loadFirstPage"];
}): Pick<AppStore,
  | "openHistory"
  | "closeHistory"
  | "restoreRevision"
  | "selectRow"
  | "jumpToRecord"
  | "closeRecord"
  | "refreshBundle"
  | "beginCreate"
  | "cancelCreate"
  | "updateCell"
  | "insertRecord"
  | "deleteRecord"
  | "updatePortalRow"
  | "insertPortalRow"
  | "deletePortalRow"
> {
  const loadBundle = async (table: string, id: number, token: number) => {
    try {
      const record = await api.readRecord(table, id);
      if (token !== inspectorRequest.token) return;
      const current = get();
      if (current.activeTable !== table || current.selectedId !== id) return;
      set({ bundle: { record, portals: {} }, bundleTable: table });
    } catch (error) {
      if (token !== inspectorRequest.token) return;
      if (!isCancelledError(error)) notifyError(error);
    }
  };

  return {
    openHistory: async () => {
      const state = get();
      if (!state.activeTable || state.selectedId === null) return;
      const table = state.activeTable;
      const id = state.selectedId;
      const token = inspectorRequest.token;
      set({ historyOpen: true, historyLoading: true });
      try {
        const recordHistory = await api.recordHistory(table, id);
        if (token !== inspectorRequest.token) return;
        const current = get();
        if (current.activeTable !== table || current.selectedId !== id) return;
        set({ recordHistory, historyLoading: false });
      } catch (error) {
        if (token !== inspectorRequest.token) return;
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

    selectRow: async (id) => {
      const state = get();
      if (!state.activeTable) return;
      const table = state.activeTable;
      const token = ++inspectorRequest.token;
      set({
        selectedId: id,
        creating: false,
        inspectorOpen: true,
        historyOpen: false,
        historyLoading: false,
        recordHistory: [],
      });
      await loadBundle(table, id, token);
    },

    jumpToRecord: async (table, id) => {
      if (get().activeTable !== table) {
        await get().selectTable(table);
      }
      await get().selectRow(id);
    },

    closeRecord: () => {
      inspectorRequest.token += 1;
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
      const token = inspectorRequest.token;
      await loadBundle(table, id, token);
    },

    beginCreate: () => {
      inspectorRequest.token += 1;
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
      const token = inspectorRequest.token;
      try {
        const record = await api.insertRow(table, values);
        const [undoState, info] = await Promise.all([api.undoState(), api.workspaceInfo()]);
        if (get().activeTable !== table || token !== inspectorRequest.token) {
          // The user moved to another table or selected another row while the
          // insert was in flight. Commit the row, refresh what is on screen,
          // but do not hijack the inspector.
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
        const claim = ++inspectorRequest.token;
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
          claim !== inspectorRequest.token ||
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
        const current = get();
        if (current.activeTable !== table) {
          set({ undoState, info: info ?? current.info });
          return;
        }
        if (current.selectedId !== id) {
          // The user selected another row, or opened a draft, while the delete
          // was in flight. Leave that selection alone.
          set({ undoState, info: info ?? current.info });
          await loadFirstPage();
          notify("success", "Record deleted");
          return;
        }
        inspectorRequest.token += 1;
        set({
          selectedId: null,
          bundle: null,
          bundleTable: null,
          undoState,
          info: info ?? current.info,
          inspectorOpen: false,
        });
        await loadFirstPage();
        notify("success", "Record deleted");
      } catch (error) {
        notifyError(error);
      }
    },

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
}
