import { useState, type CSSProperties } from "react";
import { confirmDestructive } from "../native";
import { useStore } from "../store";
import { Icon } from "./Icons";
import { ContextMenu, type ContextMenuItem } from "./ContextMenu";

export function Sidebar({
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
          onClick={overlay ? closeOverlay : onToggle}
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
                // Keep the navigation drawer mounted beneath the audit sheet so
                // dismissal can return focus to this exact row.
                void openAudit();
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
