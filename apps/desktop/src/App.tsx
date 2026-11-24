import { useEffect } from "react";
import { open as openDialog, save as saveDialog } from "@tauri-apps/plugin-dialog";
import { useStore } from "./store";
import { Icon } from "./components/Icons";
import { DataGrid } from "./components/DataGrid";
import { RecordPanel } from "./components/RecordPanel";
import { FormStudio } from "./components/FormStudio";
import { SchemaDialog } from "./components/SchemaDialog";
import { ImportDialog } from "./components/ImportDialog";
import { PrintDialog } from "./components/PrintDialog";
import { Palette } from "./components/Palette";
import { AuditDrawer } from "./components/AuditDrawer";
import { Toasts } from "./components/Toasts";

export function App() {
  const theme = useStore((state) => state.theme);
  const booting = useStore((state) => state.booting);
  const info = useStore((state) => state.info);
  const view = useStore((state) => state.view);

  useEffect(() => {
    document.documentElement.dataset["theme"] = theme;
  }, [theme]);

  useEffect(() => {
    void useStore.getState().initialize();
  }, []);

  useEffect(() => {
    const handler = (event: KeyboardEvent) => {
      const meta = event.metaKey || event.ctrlKey;
      const state = useStore.getState();
      if (meta && event.key.toLowerCase() === "k") {
        event.preventDefault();
        state.setPaletteOpen(true);
      } else if (meta && event.key.toLowerCase() === "z" && !event.shiftKey) {
        event.preventDefault();
        void state.undo();
      } else if (meta && event.shiftKey && event.key.toLowerCase() === "z") {
        event.preventDefault();
        void state.redo();
      } else if (meta && event.key.toLowerCase() === "n") {
        event.preventDefault();
        state.beginCreate();
      } else if (meta && event.key.toLowerCase() === "p") {
        event.preventDefault();
        state.setPrintOpen(true);
      } else if (meta && event.shiftKey && event.key.toLowerCase() === "i") {
        event.preventDefault();
        state.setImportOpen(true);
      } else if (event.key === "Escape") {
        if (state.paletteOpen) state.setPaletteOpen(false);
        else if (state.schemaOpen) state.setSchemaOpen(false);
        else if (state.importOpen) state.setImportOpen(false);
        else if (state.printOpen) state.setPrintOpen(false);
        else if (state.auditOpen) state.closeAudit();
      }
    };
    window.addEventListener("keydown", handler);
    return () => window.removeEventListener("keydown", handler);
  }, []);

  return (
    <div className="app">
      <Titlebar />
      {booting ? (
        <div className="splash">
          <BrandMark />
          <p>Opening local workspace…</p>
        </div>
      ) : info ? (
        <>
          <div className="app-body">
            <Sidebar />
            <main className={`workspace ${view === "form" ? "studio-view" : ""}`}>
              {view === "form" ? <FormStudio /> : <DataGrid />}
            </main>
            {view === "table" && <RecordPanel />}
          </div>
          <Statusbar />
        </>
      ) : (
        <Launcher />
      )}
      <SchemaDialog />
      <ImportDialog />
      <PrintDialog />
      <Palette />
      <AuditDrawer />
      <Toasts />
    </div>
  );
}

function Titlebar() {
  const info = useStore((state) => state.info);
  const theme = useStore((state) => state.theme);
  const undoState = useStore((state) => state.undoState);
  const setPaletteOpen = useStore((state) => state.setPaletteOpen);
  const toggleTheme = useStore((state) => state.toggleTheme);
  const openAudit = useStore((state) => state.openAudit);
  const backup = useStore((state) => state.backup);

  const runBackup = async () => {
    const name = info?.name.replace(/[^\w -]+/g, "") || "workspace";
    const path = await saveDialog({
      defaultPath: `${name} backup.slab`,
      filters: [{ name: "SlabBase workspace", extensions: ["slab"] }],
    });
    if (path) await backup(path);
  };

  return (
    <header className="titlebar" data-tauri-drag-region>
      <div className="titlebar-left">
        <BrandMark />
        <span className="brand-name">SlabBase</span>
        {info && (
          <>
            <span className="titlebar-divider" />
            <span className="workspace-name" title={info.path ?? undefined}>
              {info.name}
            </span>
            <span className="wal-badge" title={info.journalMode.toUpperCase()}>
              WAL
            </span>
          </>
        )}
      </div>
      <button
        type="button"
        className="search-trigger"
        onClick={() => setPaletteOpen(true)}
        disabled={!info}
      >
        <Icon name="search" size={14} />
        <span>{info ? "Search records or commands" : "Open a workspace"}</span>
        <kbd>⌘K</kbd>
      </button>
      <div className="titlebar-actions">
        {info && (
          <>
            <button
              type="button"
              className="icon-button"
              title={undoState.undo_label ?? "Undo"}
              disabled={!undoState.can_undo}
              onClick={() => void useStore.getState().undo()}
            >
              <Icon name="undo" size={15} />
            </button>
            <button
              type="button"
              className="icon-button"
              title={undoState.redo_label ?? "Redo"}
              disabled={!undoState.can_redo}
              onClick={() => void useStore.getState().redo()}
            >
              <Icon name="redo" size={15} />
            </button>
            <span className="titlebar-divider" />
            <button
              type="button"
              className="icon-button"
              title="Audit log"
              onClick={() => void openAudit()}
            >
              <Icon name="history" size={15} />
            </button>
            <button
              type="button"
              className="icon-button"
              title="Atomic backup"
              onClick={() => void runBackup()}
            >
              <Icon name="download" size={15} />
            </button>
          </>
        )}
        <button
          type="button"
          className="icon-button"
          title={theme === "light" ? "Switch to dark" : "Switch to light"}
          onClick={toggleTheme}
        >
          <Icon name={theme === "light" ? "moon" : "sun"} size={15} />
        </button>
      </div>
    </header>
  );
}

function Sidebar() {
  const schema = useStore((state) => state.schema);
  const info = useStore((state) => state.info);
  const activeTable = useStore((state) => state.activeTable);
  const activeLayoutId = useStore((state) => state.activeLayoutId);
  const selectTable = useStore((state) => state.selectTable);
  const selectLayout = useStore((state) => state.selectLayout);
  const openSchema = useStore((state) => state.openSchema);
  const setImportOpen = useStore((state) => state.setImportOpen);
  const setPrintOpen = useStore((state) => state.setPrintOpen);
  const openAudit = useStore((state) => state.openAudit);
  const checkIntegrity = useStore((state) => state.checkIntegrity);
  const closeWorkspace = useStore((state) => state.closeWorkspace);
  const integrity = useStore((state) => state.integrity);
  const theme = useStore((state) => state.theme);

  const counts = new Map(info?.tables.map((table) => [table.name, table.rowCount]) ?? []);
  const layouts = schema?.layouts.filter((layout) => layout.table === activeTable) ?? [];

  return (
    <nav className="sidebar" aria-label="Tables and actions">
      <section className="sidebar-section">
        <div className="sidebar-heading">
          <span>Tables</span>
          <button
            type="button"
            className="icon-button tiny"
            title="New table"
            onClick={() => openSchema("table")}
          >
            <Icon name="plus" size={13} />
          </button>
        </div>
        <div className="table-list">
          {schema?.tables.map((table) => (
            <button
              type="button"
              key={table.name}
              className={`table-row ${activeTable === table.name ? "active" : ""}`}
              onClick={() => void selectTable(table.name)}
            >
              <Icon name="table" size={14} />
              <span className="table-name">{table.name}</span>
              <span className="table-count">{counts.get(table.name) ?? 0}</span>
            </button>
          ))}
          {schema?.tables.length === 0 && (
            <p className="sidebar-empty">No tables yet.</p>
          )}
        </div>
      </section>

      <section className="sidebar-section">
        <div className="sidebar-heading">
          <span>Forms</span>
          <button
            type="button"
            className="icon-button tiny"
            title="Form studio"
            onClick={() => useStore.getState().setView("form")}
          >
            <Icon name="form" size={13} />
          </button>
        </div>
        <div className="layout-list">
          {layouts.map((layout) => (
            <button
              type="button"
              key={layout.id}
              className={`layout-row ${activeLayoutId === layout.id ? "active" : ""}`}
              onClick={() => selectLayout(layout.id)}
            >
              <Icon name="form" size={13} />
              <span>{layout.name}</span>
            </button>
          ))}
          {layouts.length === 0 && (
            <p className="sidebar-empty">No saved forms for this table.</p>
          )}
        </div>
      </section>

      <section className="sidebar-section grow">
        <div className="sidebar-heading">
          <span>Actions</span>
        </div>
        <button type="button" className="sidebar-action" onClick={() => setImportOpen(true)}>
          <Icon name="upload" size={14} /> Import data
        </button>
        <button type="button" className="sidebar-action" onClick={() => setPrintOpen(true)}>
          <Icon name="printer" size={14} /> Print / PDF
        </button>
        <button type="button" className="sidebar-action" onClick={() => openSchema("relation")}>
          <Icon name="sliders" size={14} /> Add relation
        </button>
        <button type="button" className="sidebar-action" onClick={() => void openAudit()}>
          <Icon name="history" size={14} /> Audit log
        </button>
        <button type="button" className="sidebar-action" onClick={() => void checkIntegrity()}>
          <Icon name="shield" size={14} /> Integrity
          <span className={`status-dot ${integrity === "ok" ? "ok" : "warn"}`} />
        </button>
      </section>

      <footer className="sidebar-footer">
        <div className="workspace-path" title={info?.path ?? undefined}>
          <Icon name="database" size={13} />
          <span>{info?.path ? basename(info.path) : "In memory"}</span>
        </div>
        <div className="sidebar-footer-row">
          <span className="muted">{theme === "light" ? "Warm light" : "Graphite"}</span>
          <button type="button" className="text-button subtle" onClick={() => void closeWorkspace()}>
            Close
          </button>
        </div>
      </footer>
    </nav>
  );
}

function Statusbar() {
  const rows = useStore((state) => state.rows);
  const hasMore = useStore((state) => state.hasMore);
  const elapsedMs = useStore((state) => state.elapsedMs);
  const info = useStore((state) => state.info);
  const activeTable = useStore((state) => state.activeTable);
  const undoState = useStore((state) => state.undoState);

  return (
    <footer className="statusbar">
      <span className="status-item">
        <Icon name="table" size={12} /> {activeTable ?? "No table"}
      </span>
      <span className="status-item">
        {rows.length.toLocaleString()}
        {hasMore ? "+" : ""} loaded
      </span>
      <span className="status-item">{elapsedMs.toFixed(1)} ms page</span>
      <span className="status-spacer" />
      {undoState.undo_label && <span className="status-item">Last: {undoState.undo_label}</span>}
      <span className="status-item mono">{info?.formatVersion ? `format ${info.formatVersion}` : ""}</span>
      <span className="status-item mono">SQLite · WAL</span>
    </footer>
  );
}

function Launcher() {
  const busy = useStore((state) => state.busy);
  const openWorkspace = useStore((state) => state.openWorkspace);
  const createWorkspace = useStore((state) => state.createWorkspace);
  const loadSample = useStore((state) => state.loadSample);
  const recent = window.localStorage.getItem("slabbase.recent");

  const openExisting = async () => {
    const path = await openDialog({
      multiple: false,
      filters: [{ name: "SlabBase workspace", extensions: ["slab"] }],
    });
    if (typeof path !== "string") return;
    window.localStorage.setItem("slabbase.recent", path);
    await openWorkspace(path);
  };

  const newWorkspace = async () => {
    const path = await saveDialog({
      defaultPath: "Untitled.slab",
      filters: [{ name: "SlabBase workspace", extensions: ["slab"] }],
    });
    if (!path) return;
    const name = basename(path).replace(/\.slab$/i, "") || "Untitled";
    window.localStorage.setItem("slabbase.recent", path);
    await createWorkspace(path, name);
  };

  const newSample = async () => {
    const path = await saveDialog({
      defaultPath: "Design Studio.slab",
      filters: [{ name: "SlabBase workspace", extensions: ["slab"] }],
    });
    if (!path) return;
    window.localStorage.setItem("slabbase.recent", path);
    await createWorkspace(path, "Design Studio");
    await loadSample();
  };

  return (
    <main className="launcher">
      <div className="launcher-card">
        <div className="launcher-brand">
          <BrandMark large />
          <div>
            <h1>SlabBase</h1>
            <p>Local records. Open SQLite. Forms that hold their shape.</p>
          </div>
        </div>
        <div className="launcher-actions">
          <button type="button" className="launcher-action" onClick={() => void openExisting()}>
            <span className="action-icon">
              <Icon name="folder" size={18} />
            </span>
            <span>
              <strong>Open workspace</strong>
              <small>Choose an existing .slab file</small>
            </span>
            <Icon name="chevronRight" size={14} />
          </button>
          {recent && (
            <button
              type="button"
              className="launcher-action subtle"
              onClick={() => void openWorkspace(recent)}
            >
              <span className="action-icon">
                <Icon name="history" size={18} />
              </span>
              <span>
                <strong>{basename(recent)}</strong>
                <small>{dirname(recent)}</small>
              </span>
              <Icon name="chevronRight" size={14} />
            </button>
          )}
          <button type="button" className="launcher-action" onClick={() => void newWorkspace()}>
            <span className="action-icon">
              <Icon name="plus" size={18} />
            </span>
            <span>
              <strong>Create workspace</strong>
              <small>A single SQLite database in WAL mode</small>
            </span>
            <Icon name="chevronRight" size={14} />
          </button>
          <button
            type="button"
            className="launcher-action accent"
            onClick={() => void newSample()}
            disabled={busy}
          >
            <span className="action-icon">
              <Icon name="spark" size={18} />
            </span>
            <span>
              <strong>Load sample studio</strong>
              <small>Clients → Projects → Invoices → Line Items</small>
            </span>
            <Icon name="chevronRight" size={14} />
          </button>
        </div>
        <footer className="launcher-footer">
          <span>Apache-2.0 · no account · no cloud</span>
          <span>v1.0.0</span>
        </footer>
      </div>
      <div className="launcher-bg" aria-hidden="true">
        <span className="bg-slab one" />
        <span className="bg-slab two" />
        <span className="bg-seal" />
      </div>
    </main>
  );
}

function BrandMark({ large = false }: { large?: boolean }) {
  return (
    <span className={`brand-mark ${large ? "large" : ""}`} aria-hidden="true">
      <span className="brand-slab left" />
      <span className="brand-slab right" />
      <span className="brand-seal" />
    </span>
  );
}

function basename(path: string): string {
  const parts = path.split(/[\\/]/);
  return parts[parts.length - 1] ?? path;
}

function dirname(path: string): string {
  const parts = path.split(/[\\/]/);
  parts.pop();
  return parts.join("/") || "/";
}
