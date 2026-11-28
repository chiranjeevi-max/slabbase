import { useEffect, useRef, useState } from "react";
import { open as openDialog, save as saveDialog } from "@tauri-apps/plugin-dialog";
import { useStore } from "../store";
import { Icon } from "./Icons";
import { AppearancePopover } from "./AppearancePopover";

export function Toolbar({
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

function fileLabel(name: string): string {
  const base = name.replace(/\.slab$/i, "");
  return `${base}.slab`;
}
