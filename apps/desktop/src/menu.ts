import { open as openDialog, save as saveDialog } from "@tauri-apps/plugin-dialog";
import { api } from "./api";
import { pageQueryFor, useStore } from "./store";

function isTextEditing(): boolean {
  const active = document.activeElement;
  return (
    active instanceof HTMLInputElement ||
    active instanceof HTMLTextAreaElement ||
    (active instanceof HTMLElement && active.isContentEditable)
  );
}

function textUndo(command: "undo" | "redo"): void {
  try {
    document.execCommand(command);
  } catch {
    // The focused control may not expose a native undo stack.
  }
}

function baseName(path: string): string {
  const parts = path.split(/[\\/]/);
  return parts[parts.length - 1] ?? path;
}

export async function runMenuAction(
  action: string,
  toggleSidebar: () => void,
): Promise<void> {
  const state = useStore.getState();
  switch (action) {
    case "appearance":
      state.setAppearanceOpen(true);
      return;
    case "new-workspace": {
      const path = await saveDialog({
        defaultPath: "Untitled.slab",
        filters: [{ name: "SlabBase workspace", extensions: ["slab"] }],
      });
      if (typeof path !== "string") return;
      const name = baseName(path).replace(/\.slab$/i, "") || "Untitled";
      window.localStorage.setItem("slabbase.recent", path);
      await state.createWorkspace(path, name);
      return;
    }
    case "open-workspace": {
      const path = await openDialog({
        multiple: false,
        filters: [{ name: "SlabBase workspace", extensions: ["slab"] }],
      });
      if (typeof path !== "string") return;
      window.localStorage.setItem("slabbase.recent", path);
      await state.openWorkspace(path);
      return;
    }
    case "load-sample": {
      const path = await saveDialog({
        defaultPath: "Design Studio.slab",
        filters: [{ name: "SlabBase workspace", extensions: ["slab"] }],
      });
      if (typeof path !== "string") return;
      window.localStorage.setItem("slabbase.recent", path);
      await state.createWorkspace(path, "Design Studio");
      await useStore.getState().loadSample();
      return;
    }
    case "new-table":
      if (!state.info) return;
      state.openSchema("table");
      return;
    case "new-record":
      if (!state.info || !state.activeTable) return;
      state.setView("table");
      state.beginCreate();
      return;
    case "import":
      if (!state.info || (state.schema?.tables.length ?? 0) === 0) return;
      state.setImportOpen(true);
      return;
    case "export": {
      if (!state.info || !state.activeTable) return;
      const path = await saveDialog({
        defaultPath: `${state.activeTable}.csv`,
        filters: [
          { name: "CSV", extensions: ["csv"] },
          { name: "JSON", extensions: ["json"] },
          { name: "SQL", extensions: ["sql"] },
        ],
      });
      if (typeof path !== "string") return;
      const query = pageQueryFor(useStore.getState(), null, 500);
      try {
        if (path.toLowerCase().endsWith(".json")) await api.exportJson(path, query);
        else if (path.toLowerCase().endsWith(".sql")) await api.exportSql(path);
        else await api.exportCsv(path, query);
        useStore.getState().notify("success", "Export written");
      } catch (error) {
        useStore
          .getState()
          .notify("error", error instanceof Error ? error.message : "Export failed");
      }
      return;
    }
    case "backup": {
      if (!state.info) return;
      const name = state.info.name.replace(/\.slab$/i, "") || "Workspace";
      const path = await saveDialog({
        defaultPath: `${name} backup.slab`,
        filters: [{ name: "SlabBase workspace", extensions: ["slab"] }],
      });
      if (typeof path === "string") await state.backup(path);
      return;
    }
    case "print":
      if (!state.info || !state.activeTable) return;
      state.setPrintOpen(true);
      return;
    case "close-workspace":
      if (!state.info) return;
      await state.closeWorkspace();
      return;
    case "edit-undo":
      if (isTextEditing()) textUndo("undo");
      else if (state.info) await state.undo();
      return;
    case "edit-redo":
      if (isTextEditing()) textUndo("redo");
      else if (state.info) await state.redo();
      return;
    case "search":
      if (!state.info) return;
      state.setPaletteOpen(true);
      return;
    case "view-table":
      if (state.info) state.setView("table");
      return;
    case "view-form":
      if (state.info) state.setView("form");
      return;
    case "view-schema":
      if (state.info) state.setView("schema");
      return;
    case "view-audit":
      if (state.info) await state.openAudit();
      return;
    case "toggle-sidebar":
      if (state.info) toggleSidebar();
      return;
    case "toggle-inspector":
      if (state.info && state.view !== "schema") state.toggleInspector();
      return;
    case "check-integrity":
      if (state.info) await state.checkIntegrity();
      return;
    default:
      return;
  }
}
