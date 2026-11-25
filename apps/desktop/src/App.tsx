import { useEffect, useRef, useState, type CSSProperties } from "react";
import { open as openDialog, save as saveDialog } from "@tauri-apps/plugin-dialog";
import { confirmDestructive, isTauri } from "./native";
import { usePresence } from "./presence";
import { useStore } from "./store";
import { Icon } from "./components/Icons";
import { DataGrid } from "./components/DataGrid";
import { RecordPanel } from "./components/RecordPanel";
import { FormStudio } from "./components/FormStudio";
import { SchemaView } from "./components/SchemaView";
import { SchemaDialog } from "./components/SchemaDialog";
import { ImportDialog } from "./components/ImportDialog";
import { PrintDialog } from "./components/PrintDialog";
import { Palette } from "./components/Palette";
import { AuditDrawer } from "./components/AuditDrawer";
import { Toasts } from "./components/Toasts";
import { AppearancePopover } from "./components/AppearancePopover";
import { ContextMenu, type ContextMenuItem } from "./components/ContextMenu";
import { formatTimestamp } from "./types";

const SIDEBAR_DEFAULT = 240;
const SIDEBAR_MIN = 200;
const SIDEBAR_MAX = 300;
const INSPECTOR_DEFAULT = 360;
const INSPECTOR_MIN = 320;
const INSPECTOR_MAX = 480;

export function App() {
  const booting = useStore((state) => state.booting);
  const info = useStore((state) => state.info);
  const view = useStore((state) => state.view);
  const inspectorOpen = useStore((state) => state.inspectorOpen);
  const tables = useStore((state) => state.schema?.tables.length ?? 0);
  const isNarrowSidebar = useMediaQuery("(max-width: 1079px)");
  const isInspectorOverlay = useMediaQuery("(max-width: 1199px)");
  const native = isTauri();

  const [sidebarWidth, setSidebarWidth] = useState(() =>
    readWidth("slabbase.sidebarWidth", SIDEBAR_DEFAULT, SIDEBAR_MIN, SIDEBAR_MAX),
  );
  const [inspectorWidth, setInspectorWidth] = useState(() =>
    readWidth(
      "slabbase.inspectorWidth",
      INSPECTOR_DEFAULT,
      INSPECTOR_MIN,
      INSPECTOR_MAX,
    ),
  );
  const [sidebarDocked, setSidebarDocked] = useState(true);
  const [sidebarOverlay, setSidebarOverlay] = useState(false);
  const inspectorPresence = usePresence(view === "table" && inspectorOpen, 110);

  useEffect(() => {
    if (isNarrowSidebar) setSidebarOverlay(false);
  }, [isNarrowSidebar]);

  useEffect(() => {
    window.localStorage.setItem("slabbase.sidebarWidth", String(sidebarWidth));
  }, [sidebarWidth]);

  useEffect(() => {
    window.localStorage.setItem("slabbase.inspectorWidth", String(inspectorWidth));
  }, [inspectorWidth]);

  useEffect(() => {
    const root = document.documentElement;
    const apply = (focused: boolean) => {
      root.dataset["windowFocused"] = focused ? "true" : "false";
    };
    apply(true);
    const onFocus = () => apply(true);
    const onBlur = () => apply(false);
    window.addEventListener("focus", onFocus);
    window.addEventListener("blur", onBlur);
    return () => {
      window.removeEventListener("focus", onFocus);
      window.removeEventListener("blur", onBlur);
    };
  }, []);

  useEffect(() => {
    void useStore.getState().initialize();
  }, []);

  useEffect(() => {
    const handler = (event: KeyboardEvent) => {
      const state = useStore.getState();
      const meta = event.metaKey || event.ctrlKey;
      const key = event.key.toLowerCase();
      const editing = isTextEntry(event.target);

      if (meta && key === "k") {
        if (!state.info) return;
        event.preventDefault();
        state.setPaletteOpen(true);
      } else if (meta && key === "z") {
        if (editing || !state.info) return;
        event.preventDefault();
        if (event.shiftKey) void state.redo();
        else void state.undo();
      } else if (meta && key === "n") {
        if (editing || !state.info) return;
        event.preventDefault();
        state.beginCreate();
      } else if (meta && key === "p") {
        if (!state.info) return;
        event.preventDefault();
        state.setPrintOpen(true);
      } else if (meta && event.shiftKey && key === "i") {
        if (!state.info) return;
        event.preventDefault();
        state.setImportOpen(true);
      } else if (event.key === "Escape") {
        if (state.paletteOpen) state.setPaletteOpen(false);
        else if (state.schemaOpen) state.setSchemaOpen(false);
        else if (state.importOpen) state.setImportOpen(false);
        else if (state.printOpen) state.setPrintOpen(false);
        else if (state.auditOpen) state.closeAudit();
        else if (state.appearanceOpen) state.setAppearanceOpen(false);
        else if (isNarrowSidebar && sidebarOverlay) {
          document.querySelector<HTMLElement>('[data-testid="sidebar-toggle"]')?.focus();
          setSidebarOverlay(false);
        }
      }
    };
    window.addEventListener("keydown", handler);
    return () => window.removeEventListener("keydown", handler);
  }, [isNarrowSidebar, sidebarOverlay]);

  const sidebarVisible = isNarrowSidebar ? sidebarOverlay : sidebarDocked;
  const showSidebar = Boolean(info);
  const reserveTrafficLights = native && (!info || isNarrowSidebar || !sidebarDocked);

  const toggleSidebar = () => {
    if (isNarrowSidebar) setSidebarOverlay((open) => !open);
    else setSidebarDocked((open) => !open);
  };

  return (
    <div className="app">
      {info && isNarrowSidebar && sidebarOverlay && (
        <div
          className="sidebar-scrim"
          role="presentation"
          onMouseDown={() => setSidebarOverlay(false)}
        />
      )}

      {showSidebar && (
        <Sidebar
          width={sidebarWidth}
          overlay={isNarrowSidebar}
          open={sidebarVisible}
          onToggle={toggleSidebar}
        />
      )}

      {info && !isNarrowSidebar && sidebarDocked && (
        <PaneResizer
          side="left"
          width={sidebarWidth}
          min={SIDEBAR_MIN}
          max={SIDEBAR_MAX}
          onResize={setSidebarWidth}
        />
      )}

      <div
        className="main-column"
        data-sidebar-docked={!isNarrowSidebar && sidebarDocked ? "true" : "false"}
      >
        <Toolbar
          reserveTrafficLights={reserveTrafficLights}
          showSidebarToggle={Boolean(info) && (isNarrowSidebar || !sidebarDocked)}
          onToggleSidebar={toggleSidebar}
        />

        {booting ? (
          <div className="splash">
            <Icon name="database" size={24} />
            <p>Opening workspace…</p>
          </div>
        ) : info ? (
          <>
            <div className="app-body">
              <main className={`workspace ${view === "form" ? "studio-view" : ""}`}>
                {tables === 0 ? (
                  <EmptyWorkspace />
                ) : view === "schema" ? (
                  <SchemaView />
                ) : view === "form" ? (
                  <FormStudio />
                ) : (
                  <DataGrid />
                )}
              </main>
              {view === "table" && inspectorPresence.mounted && (
                <>
                  {inspectorOpen && !isInspectorOverlay && (
                    <PaneResizer
                      side="right"
                      width={inspectorWidth}
                      min={INSPECTOR_MIN}
                      max={INSPECTOR_MAX}
                      onResize={setInspectorWidth}
                    />
                  )}
                  <RecordPanel
                    width={inspectorWidth}
                    closing={inspectorPresence.closing}
                  />
                </>
              )}
            </div>
            <Statusbar />
          </>
        ) : (
          <Launcher />
        )}
      </div>

      <SchemaDialog />
      <ImportDialog />
      <PrintDialog />
      <Palette />
      <AuditDrawer />
      <Toasts />
    </div>
  );
}

function Toolbar({
  reserveTrafficLights,
  showSidebarToggle,
  onToggleSidebar,
}: {
  reserveTrafficLights: boolean;
  showSidebarToggle: boolean;
  onToggleSidebar: () => void;
}) {
  const info = useStore((state) => state.info);
  const view = useStore((state) => state.view);
  const activeTable = useStore((state) => state.activeTable);
  const hasTables = useStore((state) => (state.schema?.tables.length ?? 0) > 0);
  const inspectorOpen = useStore((state) => state.inspectorOpen);
  const undoState = useStore((state) => state.undoState);
  const setPaletteOpen = useStore((state) => state.setPaletteOpen);
  const setView = useStore((state) => state.setView);
  const toggleInspector = useStore((state) => state.toggleInspector);
  const setImportOpen = useStore((state) => state.setImportOpen);
  const setPrintOpen = useStore((state) => state.setPrintOpen);

  const [menuOpen, setMenuOpen] = useState(false);
  const menuRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!menuOpen) return;
    const close = (event: MouseEvent) => {
      if (!menuRef.current?.contains(event.target as Node)) setMenuOpen(false);
    };
    window.addEventListener("mousedown", close);
    return () => window.removeEventListener("mousedown", close);
  }, [menuOpen]);

  const openExisting = async () => {
    setMenuOpen(false);
    const path = await openDialog({
      multiple: false,
      filters: [{ name: "SlabBase workspace", extensions: ["slab"] }],
    });
    if (typeof path !== "string") return;
    window.localStorage.setItem("slabbase.recent", path);
    await useStore.getState().openWorkspace(path);
  };

  const runBackup = async () => {
    setMenuOpen(false);
    const name = info?.name.replace(/\.slab$/i, "") || "Workspace";
    const path = await saveDialog({
      defaultPath: `${name} backup.slab`,
      filters: [{ name: "SlabBase workspace", extensions: ["slab"] }],
    });
    if (path) await useStore.getState().backup(path);
  };

  const loadSample = async () => {
    setMenuOpen(false);
    await useStore.getState().loadSample();
  };

  return (
    <header
      className={`toolbar ${reserveTrafficLights ? "traffic-space" : ""}`}
      data-testid="toolbar"
      data-tauri-drag-region
    >
      <div className="toolbar-left" data-tauri-drag-region>
        {showSidebarToggle && (
          <button
            type="button"
            className="icon-control"
            title="Toggle Sidebar"
            aria-label="Toggle Sidebar"
            data-testid="sidebar-toggle"
            onClick={onToggleSidebar}
          >
            <Icon name="sidebar" size={14} strokeWidth={1.5} />
          </button>
        )}
        {info ? (
          <div className="workspace-anchor" ref={menuRef}>
            <button
              type="button"
              className={`workspace-switcher ${menuOpen ? "open" : ""}`}
              onClick={() => setMenuOpen((open) => !open)}
              title={info.path ?? info.name}
              aria-haspopup="menu"
              aria-expanded={menuOpen}
              data-testid="workspace-switcher"
            >
              <span className="workspace-name">{fileLabel(info.name)}</span>
              <Icon name="chevronDown" size={11} strokeWidth={1.5} />
            </button>
            {menuOpen && (
              <div className="menu workspace-menu" role="menu">
                <button type="button" role="menuitem" onClick={() => void openExisting()}>
                  <Icon name="folder" size={13} />
                  <span>Open Workspace…</span>
                </button>
                <button type="button" role="menuitem" onClick={() => void loadSample()}>
                  <Icon name="spark" size={13} />
                  <span>Load Sample Workspace</span>
                </button>
                <button type="button" role="menuitem" onClick={() => void runBackup()}>
                  <Icon name="download" size={13} />
                  <span>Backup Snapshot…</span>
                </button>
                <div className="menu-separator" />
                <button
                  type="button"
                  role="menuitem"
                  className="danger"
                  onClick={() => {
                    setMenuOpen(false);
                    void useStore.getState().closeWorkspace();
                  }}
                >
                  <Icon name="x" size={13} />
                  <span>Close Workspace</span>
                </button>
              </div>
            )}
          </div>
        ) : (
          <span className="toolbar-drag" data-tauri-drag-region />
        )}
      </div>

      <div className="toolbar-center">
        {info && activeTable && (
          <div className="view-switcher" role="tablist" aria-label="Workspace view">
            {(
              [
                ["table", "Table"],
                ["form", "Form"],
                ["schema", "Schema"],
              ] as const
            ).map(([value, label]) => (
              <button
                type="button"
                role="tab"
                key={value}
                aria-selected={view === value}
                className={view === value ? "active" : ""}
                onClick={() => setView(value)}
              >
                {label}
              </button>
            ))}
          </div>
        )}
        {info && (
          <button
            type="button"
            className="search-trigger"
            onClick={() => setPaletteOpen(true)}
            title="Search records and commands (⌘K)"
            data-testid="search-trigger"
          >
            <Icon name="search" size={13} strokeWidth={1.5} />
            <span>Search</span>
            <kbd>⌘K</kbd>
          </button>
        )}
      </div>

      <div className="toolbar-right">
        {info && (
          <>
            <div className="toolbar-group">
              <button
                type="button"
                className="icon-control"
                title={`${undoState.undo_label ?? "Undo"} (⌘Z)`}
                aria-label={undoState.undo_label ?? "Undo"}
                disabled={!undoState.can_undo}
                onClick={() => void useStore.getState().undo()}
              >
                <Icon name="undo" size={14} strokeWidth={1.5} />
              </button>
              <button
                type="button"
                className="icon-control"
                title={`${undoState.redo_label ?? "Redo"} (⇧⌘Z)`}
                aria-label={undoState.redo_label ?? "Redo"}
                disabled={!undoState.can_redo}
                onClick={() => void useStore.getState().redo()}
              >
                <Icon name="redo" size={14} strokeWidth={1.5} />
              </button>
            </div>
            <span className="toolbar-divider" />
            <div className="toolbar-group">
              <button
                type="button"
                className="icon-control"
                title="Import CSV or JSON (⇧⌘I)"
                aria-label="Import"
                disabled={!hasTables}
                onClick={() => setImportOpen(true)}
              >
                <Icon name="upload" size={14} strokeWidth={1.5} />
              </button>
              <button
                type="button"
                className="icon-control"
                title="Print or Export PDF (⌘P)"
                aria-label="Print or Export PDF"
                disabled={!activeTable}
                onClick={() => setPrintOpen(true)}
              >
                <Icon name="printer" size={14} strokeWidth={1.5} />
              </button>
            </div>
            <span className="toolbar-divider" />
          </>
        )}
        <div className="toolbar-group">
          <AppearancePopover />
          {info && (
            <button
              type="button"
              className={`icon-control ${
                inspectorOpen && view !== "schema" ? "active" : ""
              }`}
              title={
                view === "schema"
                  ? "Inspector is available in Table and Form views"
                  : inspectorOpen
                    ? "Hide Inspector"
                    : "Show Inspector"
              }
              aria-label="Toggle Inspector"
              aria-pressed={inspectorOpen}
              disabled={view === "schema"}
              data-testid="inspector-toggle"
              onClick={toggleInspector}
            >
              <Icon name="panelRight" size={14} strokeWidth={1.5} />
            </button>
          )}
        </div>
      </div>
    </header>
  );
}

function Sidebar({
  width,
  overlay,
  open,
  onToggle,
}: {
  width: number;
  overlay: boolean;
  open: boolean;
  onToggle: () => void;
}) {
  const schema = useStore((state) => state.schema);
  const info = useStore((state) => state.info);
  const view = useStore((state) => state.view);
  const activeTable = useStore((state) => state.activeTable);
  const activeLayoutId = useStore((state) => state.activeLayoutId);
  const selectTable = useStore((state) => state.selectTable);
  const selectLayout = useStore((state) => state.selectLayout);
  const setView = useStore((state) => state.setView);
  const openSchema = useStore((state) => state.openSchema);
  const openAudit = useStore((state) => state.openAudit);
  const dropTable = useStore((state) => state.dropTable);
  const notify = useStore((state) => state.notify);

  const [contextMenu, setContextMenu] = useState<{
    x: number;
    y: number;
    table: string;
  } | null>(null);

  const closeOverlay = () => {
    if (!overlay) return;
    document.querySelector<HTMLElement>('[data-testid="sidebar-toggle"]')?.focus();
    onToggle();
  };

  const tableMenuItems = (table: string): ContextMenuItem[] => [
    {
      label: "Open Table",
      icon: "table",
      onSelect: () => void selectTable(table),
    },
    {
      label: "New Form",
      icon: "form",
      onSelect: () => {
        void selectTable(table).then(() => setView("form"));
      },
    },
    {
      label: "Manage Columns",
      icon: "grid",
      onSelect: () => openSchema("column", table),
    },
    {
      label: "Delete Table…",
      icon: "trash",
      danger: true,
      separatorBefore: true,
      onSelect: () => {
        void (async () => {
          const ok = await confirmDestructive(
            `Delete the table "${table}" and all of its rows?`,
            "Delete Table",
            "Delete",
          );
          if (!ok) return;
          const deleted = await dropTable(table);
          if (deleted) notify("success", `Deleted table ${table}`);
        })();
      },
    },
  ];

  const counts = new Map(info?.tables.map((table) => [table.name, table.rowCount]) ?? []);
  const layouts = schema?.layouts.filter((layout) => layout.table === activeTable) ?? [];

  return (
    <>
      <nav
        className={`sidebar ${overlay ? "overlay" : ""} ${open ? "open" : ""}`}
      style={{ "--sidebar-w": `${width}px` } as CSSProperties}
      aria-label="Workspace"
      data-testid="sidebar"
      aria-hidden={overlay && !open}
      inert={overlay && !open ? true : undefined}
    >
      <div className="sidebar-header" data-tauri-drag-region>
        <button
          type="button"
          className="icon-control sidebar-toggle"
          title={overlay ? "Close Sidebar" : "Collapse Sidebar"}
          aria-label={overlay ? "Close Sidebar" : "Collapse Sidebar"}
          data-testid="sidebar-header-toggle"
          onClick={onToggle}
        >
          <Icon name="sidebar" size={14} strokeWidth={1.5} />
        </button>
      </div>

      <div className="sidebar-scroll">
        <section className="source-section">
          <div className="source-heading">
            <span>Tables</span>
            <button
              type="button"
              className="icon-control tiny"
              title="New table"
              aria-label="New table"
              onClick={() => openSchema("table")}
            >
              <Icon name="plus" size={12} strokeWidth={1.5} />
            </button>
          </div>
          <div className="source-list">
            {schema?.tables.map((table) => (
              <button
                type="button"
                key={table.name}
                className={`source-row ${
                  view === "table" && activeTable === table.name ? "active" : ""
                }`}
                data-testid="source-row"
                data-table={table.name}
                onClick={() => {
                  void selectTable(table.name);
                  closeOverlay();
                }}
                onContextMenu={(event) => {
                  event.preventDefault();
                  setContextMenu({
                    x: event.clientX,
                    y: event.clientY,
                    table: table.name,
                  });
                }}
                title={table.name}
              >
                <Icon name="table" size={14} strokeWidth={1.5} />
                <span className="source-name">{table.name}</span>
                <span className="source-count">
                  {(counts.get(table.name) ?? 0).toLocaleString()}
                </span>
              </button>
            ))}
            {schema?.tables.length === 0 && <p className="source-empty">No tables yet</p>}
          </div>
        </section>

        <section className="source-section">
          <div className="source-heading">
            <span>Forms</span>
            <button
              type="button"
              className="icon-control tiny"
              title="New form"
              aria-label="New form"
              onClick={() => {
                setView("form");
                closeOverlay();
              }}
            >
              <Icon name="plus" size={12} strokeWidth={1.5} />
            </button>
          </div>
          <div className="source-list">
            {layouts.map((layout) => (
              <button
                type="button"
                key={layout.id}
                className={`source-row ${
                  view === "form" && activeLayoutId === layout.id ? "active" : ""
                }`}
                data-testid="source-row"
                data-layout={layout.id}
                onClick={() => {
                  selectLayout(layout.id);
                  closeOverlay();
                }}
                title={layout.name}
              >
                <Icon name="form" size={13} strokeWidth={1.5} />
                <span className="source-name">{layout.name}</span>
              </button>
            ))}
            {layouts.length === 0 && (
              <p className="source-empty">
                {activeTable ? "No forms for this table" : "Select a table"}
              </p>
            )}
          </div>
        </section>

        <section className="source-section">
          <div className="source-heading">
            <span>Workspace</span>
          </div>
          <div className="source-list">
            <button
              type="button"
              className={`source-row ${view === "schema" ? "active" : ""}`}
              data-testid="source-row"
              onClick={() => {
                setView("schema");
                closeOverlay();
              }}
            >
              <Icon name="grid" size={14} strokeWidth={1.5} />
              <span className="source-name">Relations &amp; Schema</span>
            </button>
            <button
              type="button"
              className="source-row"
              data-testid="source-row"
              onClick={() => {
                void openAudit();
                closeOverlay();
              }}
            >
              <Icon name="history" size={14} strokeWidth={1.5} />
              <span className="source-name">Audit Log</span>
            </button>
          </div>
        </section>
      </div>
      </nav>

      {contextMenu && (
        <ContextMenu
          x={contextMenu.x}
          y={contextMenu.y}
          onClose={() => setContextMenu(null)}
          items={tableMenuItems(contextMenu.table)}
        />
      )}
    </>
  );
}

function Statusbar() {
  const info = useStore((state) => state.info);
  const activeTable = useStore((state) => state.activeTable);
  const rows = useStore((state) => state.rows);
  const elapsedMs = useStore((state) => state.elapsedMs);
  const integrity = useStore((state) => state.integrity);
  const report = useStore((state) => state.integrityReport);
  const checkedAt = useStore((state) => state.integrityCheckedAt);
  const checkIntegrity = useStore((state) => state.checkIntegrity);

  const [detailsOpen, setDetailsOpen] = useState(false);
  const anchorRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!detailsOpen) return;
    const close = (event: MouseEvent) => {
      if (!anchorRef.current?.contains(event.target as Node)) setDetailsOpen(false);
    };
    window.addEventListener("mousedown", close);
    return () => window.removeEventListener("mousedown", close);
  }, [detailsOpen]);

  const count = activeTable
    ? (info?.tables.find((table) => table.name === activeTable)?.rowCount ??
      rows.length)
    : 0;
  const healthy = integrity === "ok";
  const showLatency = rows.length > 0 && elapsedMs > 0;

  return (
    <footer className="statusbar" data-testid="status-bar" aria-label="Status">
      <div className="status-left">
        {!info ? (
          <span>Empty workspace</span>
        ) : activeTable ? (
          <>
            <span className="status-table">{activeTable}</span>
            <span className="status-separator" />
            <span>{count.toLocaleString()} rows</span>
            {showLatency && (
              <>
                <span className="status-separator" />
                <span className="status-mono">{elapsedMs.toFixed(1)} ms</span>
              </>
            )}
          </>
        ) : (
          <span>No table selected</span>
        )}
      </div>
      <div className="status-right">
        <div className="status-anchor" ref={anchorRef}>
          <button
            type="button"
            className="integrity-button"
            onClick={() => {
              if (report) setDetailsOpen((open) => !open);
              else
                void checkIntegrity().then(() => {
                  if (useStore.getState().integrityReport) setDetailsOpen(true);
                });
            }}
            title="Run SQLite integrity check"
          >
            <span className={`status-dot ${healthy ? "ok" : "warn"}`} />
            {healthy ? "Integrity OK" : "Integrity issue"}
          </button>
          {detailsOpen && (
            <div className="integrity-popover" role="dialog" aria-label="Integrity details">
              <div className="integrity-popover-head">
                <span>Integrity</span>
                <button
                  type="button"
                  className="icon-control tiny"
                  aria-label="Close"
                  onClick={() => setDetailsOpen(false)}
                >
                  <Icon name="x" size={11} />
                </button>
              </div>
              <dl>
                <div>
                  <dt>SQLite</dt>
                  <dd>{report?.integrity ?? "Not run"}</dd>
                </div>
                <div>
                  <dt>Foreign keys</dt>
                  <dd>{report ? `${report.foreignKeyViolations} violations` : "Not run"}</dd>
                </div>
                <div>
                  <dt>Pages</dt>
                  <dd>{report ? report.pageCount.toLocaleString() : "…"}</dd>
                </div>
                <div>
                  <dt>Journal</dt>
                  <dd>
                    {report
                      ? report.journalMode.toUpperCase()
                      : info?.journalMode.toUpperCase() ?? "WAL"}
                  </dd>
                </div>
                {checkedAt && (
                  <div>
                    <dt>Checked</dt>
                    <dd>{formatTimestamp(checkedAt)}</dd>
                  </div>
                )}
              </dl>
              <button
                type="button"
                className="button"
                onClick={() => void checkIntegrity()}
              >
                Run Again
              </button>
            </div>
          )}
        </div>
        <span className="status-mono">SQLite WAL</span>
      </div>
    </footer>
  );
}

function EmptyWorkspace() {
  const openSchema = useStore((state) => state.openSchema);
  const loadSample = useStore((state) => state.loadSample);
  const busy = useStore((state) => state.busy);

  return (
    <div className="canvas-empty" data-testid="empty-workspace">
      <span className="empty-glyph">
        <Icon name="table" size={32} strokeWidth={1.2} />
      </span>
      <h2>No tables yet</h2>
      <p>Create a table, or load the sample studio.</p>
      <div className="empty-actions">
        <button type="button" className="button primary" onClick={() => openSchema("table")}>
          New Table
        </button>
        <button
          type="button"
          className="button"
          disabled={busy}
          onClick={() => void loadSample()}
        >
          Load Sample Workspace
        </button>
      </div>
    </div>
  );
}

function Launcher() {
  const busy = useStore((state) => state.busy);
  const openWorkspace = useStore((state) => state.openWorkspace);
  const createWorkspace = useStore((state) => state.createWorkspace);
  const [recent, setRecent] = useState<string | null>(() =>
    window.localStorage.getItem("slabbase.recent"),
  );

  const openExisting = async () => {
    const path = await openDialog({
      multiple: false,
      filters: [{ name: "SlabBase workspace", extensions: ["slab"] }],
    });
    if (typeof path !== "string") return;
    window.localStorage.setItem("slabbase.recent", path);
    setRecent(path);
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
    setRecent(path);
    await createWorkspace(path, name);
  };

  const newSample = async () => {
    const path = await saveDialog({
      defaultPath: "Design Studio.slab",
      filters: [{ name: "SlabBase workspace", extensions: ["slab"] }],
    });
    if (!path) return;
    window.localStorage.setItem("slabbase.recent", path);
    setRecent(path);
    await createWorkspace(path, "Design Studio");
    await useStore.getState().loadSample();
  };

  return (
    <main className="launcher">
      <div className="launcher-content">
        <span className="launcher-glyph">
          <Icon name="database" size={28} strokeWidth={1.2} />
        </span>
        <span className="launcher-kicker">Local SQLite workspace</span>
        <h1>SlabBase</h1>
        <p>Relational records, visual forms, and reports, all stored in a single database file.</p>
        <div className="launcher-actions">
          <button type="button" className="button primary" onClick={() => void openExisting()}>
            Open Workspace
          </button>
          <button type="button" className="button" onClick={() => void newWorkspace()}>
            New Workspace
          </button>
          <button
            type="button"
            className="button"
            disabled={busy}
            onClick={() => void newSample()}
          >
            Load Sample Workspace
          </button>
        </div>
        {recent && (
          <button
            type="button"
            className="recent-button"
            onClick={() => void openWorkspace(recent)}
            title={recent}
          >
            <Icon name="history" size={13} strokeWidth={1.5} />
            <span>{basename(recent)}</span>
            <span className="recent-folder">{dirname(recent)}</span>
          </button>
        )}
        <p className="launcher-footnote">Apache-2.0 · no account · no cloud</p>
      </div>
    </main>
  );
}

function PaneResizer({
  side,
  width,
  min,
  max,
  onResize,
}: {
  side: "left" | "right";
  width: number;
  min: number;
  max: number;
  onResize: (width: number) => void;
}) {
  const [active, setActive] = useState(false);
  const origin = useRef({ x: 0, width });

  useEffect(() => {
    if (!active) return;
    const move = (event: PointerEvent) => {
      const delta = event.clientX - origin.current.x;
      const next = side === "left" ? origin.current.width + delta : origin.current.width - delta;
      onResize(Math.min(max, Math.max(min, next)));
    };
    const up = () => setActive(false);
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up, { once: true });
    document.body.classList.add("resizing");
    return () => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", up);
      document.body.classList.remove("resizing");
    };
  }, [active, side, min, max, onResize]);

  return (
    <div
      role="separator"
      aria-orientation="vertical"
      aria-label={side === "left" ? "Resize sidebar" : "Resize inspector"}
      className={`pane-resizer ${side} ${active ? "active" : ""}`}
      onPointerDown={(event) => {
        event.preventDefault();
        origin.current = { x: event.clientX, width };
        setActive(true);
      }}
    />
  );
}

function useMediaQuery(query: string): boolean {
  const [matches, setMatches] = useState(() =>
    typeof window === "undefined" ? false : window.matchMedia(query).matches,
  );

  useEffect(() => {
    const media = window.matchMedia(query);
    const update = () => setMatches(media.matches);
    update();
    media.addEventListener("change", update);
    return () => media.removeEventListener("change", update);
  }, [query]);

  return matches;
}

function isTextEntry(target: EventTarget | null): boolean {
  if (!(target instanceof HTMLElement)) return false;
  const tag = target.tagName;
  return tag === "INPUT" || tag === "TEXTAREA" || target.isContentEditable;
}

function fileLabel(name: string): string {
  const base = name.replace(/\.slab$/i, "");
  return `${base}.slab`;
}

function readWidth(key: string, fallback: number, min: number, max: number): number {
  const raw = Number(window.localStorage.getItem(key));
  if (!Number.isFinite(raw) || raw <= 0) return fallback;
  return Math.min(max, Math.max(min, raw));
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
