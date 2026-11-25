import { useEffect, useMemo, useRef, useState } from "react";
import { save } from "@tauri-apps/plugin-dialog";
import type { Column } from "../bindings";
import { api } from "../api";
import { DENSITY_ROW_HEIGHT } from "../appearance";
import { pageQueryFor, useStore } from "../store";
import { confirmDestructive } from "../native";
import { cellText, columnTypeGlyph, isComputedColumn, isNull, isNumericColumn } from "../types";
import { ContextMenu, type ContextMenuItem } from "./ContextMenu";
import { FieldInput } from "./FieldInput";
import { Icon } from "./Icons";

const HEADER_HEIGHT = 28;
const MIN_COLUMN_WIDTH = 200;

export function DataGrid() {
  const schema = useStore((state) => state.schema);
  const activeTable = useStore((state) => state.activeTable);
  const rows = useStore((state) => state.rows);
  const hasMore = useStore((state) => state.hasMore);
  const loadingRows = useStore((state) => state.loadingRows);
  const sort = useStore((state) => state.sort);
  const filterText = useStore((state) => state.filterText);
  const filterColumn = useStore((state) => state.filterColumn);
  const selectedId = useStore((state) => state.selectedId);
  const setSort = useStore((state) => state.setSort);
  const setFilterColumn = useStore((state) => state.setFilterColumn);
  const applyFilter = useStore((state) => state.applyFilter);
  const clearFilter = useStore((state) => state.clearFilter);
  const loadMore = useStore((state) => state.loadMore);
  const selectRow = useStore((state) => state.selectRow);
  const beginCreate = useStore((state) => state.beginCreate);
  const openSchema = useStore((state) => state.openSchema);
  const updateCell = useStore((state) => state.updateCell);
  const density = useStore((state) => state.appearance.density);
  const rowHeight = DENSITY_ROW_HEIGHT[density];

  const table = schema?.tables.find((candidate) => candidate.name === activeTable);
  const columns = table?.columns ?? [];
  const gridTemplate = useMemo(
    () =>
      `56px ${columns.map(() => `minmax(${MIN_COLUMN_WIDTH}px, 1fr)`).join(" ")}`.trim(),
    [columns],
  );

  const scrollRef = useRef<HTMLDivElement>(null);
  const selectTimer = useRef<number | null>(null);
  const [scrollTop, setScrollTop] = useState(0);
  const [viewport, setViewport] = useState(600);
  const [editing, setEditing] = useState<{ id: number; column: string } | null>(null);
  const [active, setActive] = useState<{ row: number; column: number } | null>(null);
  const [filterDraft, setFilterDraft] = useState(filterText);
  const [exportOpen, setExportOpen] = useState(false);
  const [contextMenu, setContextMenu] = useState<{
    x: number;
    y: number;
    id: number;
  } | null>(null);

  useEffect(() => {
    // A delayed inspector select carries a row id from the previous table or
    // filter. Cancel it before the window changes underneath it.
    if (selectTimer.current !== null) {
      window.clearTimeout(selectTimer.current);
      selectTimer.current = null;
    }
    setEditing(null);
    setActive(null);
    setScrollTop(0);
    setFilterDraft(filterText);
    scrollRef.current?.scrollTo({ top: 0 });
  }, [activeTable, filterText]);

  useEffect(
    () => () => {
      if (selectTimer.current !== null) window.clearTimeout(selectTimer.current);
    },
    [],
  );

  const selectSoon = (id: number) => {
    if (selectTimer.current !== null) window.clearTimeout(selectTimer.current);
    // A double-click must reach the cell before the inspector opens and
    // narrows the grid, otherwise horizontally scrolled columns move between
    // the two clicks and the edit never starts.
    selectTimer.current = window.setTimeout(() => {
      selectTimer.current = null;
      void selectRow(id);
    }, 180);
  };

  useEffect(() => {
    const element = scrollRef.current;
    if (!element) return;
    const observer = new ResizeObserver((entries) => {
      const entry = entries[0];
      if (entry) setViewport(entry.contentRect.height);
    });
    observer.observe(element);
    return () => observer.disconnect();
  }, []);

  const bodyScroll = Math.max(0, scrollTop - HEADER_HEIGHT);
  const start = Math.max(0, Math.floor(bodyScroll / rowHeight) - 6);
  const visibleCount = Math.ceil(viewport / rowHeight) + 12;
  const end = Math.min(rows.length, start + visibleCount);
  const visibleRows = rows.slice(start, end);
  const sortSpec = sort[0];

  const ensureVisible = (index: number) => {
    const element = scrollRef.current;
    if (!element) return;
    const top = HEADER_HEIGHT + index * rowHeight;
    if (top < element.scrollTop + HEADER_HEIGHT) {
      element.scrollTo({ top: top - HEADER_HEIGHT });
    } else if (top + rowHeight > element.scrollTop + viewport) {
      element.scrollTo({ top: top + rowHeight - viewport + 12 });
    }
  };

  const handleKeyDown = (event: React.KeyboardEvent<HTMLDivElement>) => {
    if (editing || !active) return;
    if (event.key === "ArrowDown" || event.key === "ArrowUp") {
      event.preventDefault();
      const direction = event.key === "ArrowDown" ? 1 : -1;
      const nextRow = Math.max(0, Math.min(rows.length - 1, active.row + direction));
      setActive({ ...active, row: nextRow });
      const record = rows[nextRow];
      if (record) void selectRow(record.id);
      ensureVisible(nextRow);
      if (nextRow > rows.length - 10 && hasMore) void loadMore();
    } else if (event.key === "ArrowRight" || event.key === "ArrowLeft") {
      event.preventDefault();
      const direction = event.key === "ArrowRight" ? 1 : -1;
      const nextColumn = Math.max(0, Math.min(columns.length - 1, active.column + direction));
      setActive({ ...active, column: nextColumn });
    } else if (event.key === "Enter") {
      const record = rows[active.row];
      const column = columns[active.column];
      if (record && column && !isComputedColumn(column)) {
        event.preventDefault();
        setEditing({ id: record.id, column: column.name });
      }
    }
  };

  const exportTable = async (kind: "csv" | "json" | "sql") => {
    setExportOpen(false);
    if (!activeTable) return;
    const extension = kind === "sql" ? "sql" : kind;
    const path = await save({
      defaultPath: `${activeTable}.${extension}`,
      filters: [{ name: extension.toUpperCase(), extensions: [extension] }],
    });
    if (!path) return;
    const state = useStore.getState();
    const query = pageQueryFor(state, null, 500);
    try {
      if (kind === "csv") await api.exportCsv(path, query);
      else if (kind === "json") await api.exportJson(path, query);
      else await api.exportSql(path);
      state.notify("success", `${extension.toUpperCase()} export written`);
    } catch (error) {
      state.notify("error", error instanceof Error ? error.message : "Export failed");
    }
  };

  if (!table) {
    return (
      <div className="canvas-empty">
        <span className="empty-glyph">
          <Icon name="table" size={32} strokeWidth={1.2} />
        </span>
        <h2>No table selected</h2>
        <p>Choose a table in the sidebar, or create one to begin.</p>
        <div className="empty-actions">
          <button type="button" className="button primary" onClick={() => openSchema("table")}>
            New Table
          </button>
        </div>
      </div>
    );
  }

  return (
    <div className="data-grid" data-testid="grid-pane">
      <div className="table-toolbar" data-testid="sub-toolbar">
        <div className="table-filter">
          <Icon name="search" size={13} strokeWidth={1.5} />
          <input
            value={filterDraft}
            placeholder={`Filter ${filterColumn}`}
            aria-label={`Filter by ${filterColumn}`}
            onChange={(event) => setFilterDraft(event.target.value)}
            onKeyDown={(event) => {
              if (event.key === "Enter") void applyFilter(filterDraft);
              if (event.key === "Escape") {
                setFilterDraft("");
                void clearFilter();
              }
            }}
          />
          {filterDraft && (
            <button
              type="button"
              className="icon-control micro"
              title="Clear filter"
              aria-label="Clear filter"
              onClick={() => {
                setFilterDraft("");
                void clearFilter();
              }}
            >
              <Icon name="x" size={10} />
            </button>
          )}
        </div>
        <select
          className="select toolbar-select"
          value={filterColumn}
          aria-label="Filter column"
          onChange={(event) => setFilterColumn(event.target.value)}
        >
          {columns
            .filter((column) => !isComputedColumn(column))
            .map((column) => (
              <option key={column.name} value={column.name}>
                {column.name}
              </option>
            ))}
        </select>
        {filterText && (
          <span className="filter-chip">
            contains “{filterText}”
            <button
              type="button"
              className="chip-x"
              aria-label="Clear filter"
              onClick={() => void clearFilter()}
            >
              <Icon name="x" size={9} />
            </button>
          </span>
        )}
        <span className="toolbar-spacer" />
        <button
          type="button"
          className="button compact"
          onClick={() => openSchema("column")}
        >
          + Column
        </button>
        <div className="menu-anchor">
          <button
            type="button"
            className="button compact"
            onClick={() => setExportOpen((open) => !open)}
          >
            Export
            <Icon name="chevronDown" size={10} />
          </button>
          {exportOpen && (
            <div className="menu" onMouseLeave={() => setExportOpen(false)}>
              <button type="button" onClick={() => void exportTable("csv")}>
                CSV
              </button>
              <button type="button" onClick={() => void exportTable("json")}>
                JSON
              </button>
              <button type="button" onClick={() => void exportTable("sql")}>
                SQL Script
              </button>
            </div>
          )}
        </div>
        <button type="button" className="button primary compact" onClick={beginCreate}>
          + New Record
        </button>
      </div>

      <div
        ref={scrollRef}
        className="grid-viewport"
        data-testid="grid-scroll"
        tabIndex={0}
        onFocus={() => {
          if (!active && rows.length > 0) {
            setActive({ row: 0, column: 0 });
            void selectRow(rows[0]!.id);
          }
        }}
        onKeyDown={handleKeyDown}
        onScroll={(event) => {
          const element = event.currentTarget;
          setScrollTop(element.scrollTop);
          if (
            hasMore &&
            !loadingRows &&
            element.scrollTop + element.clientHeight >= rows.length * rowHeight - 240
          ) {
            void loadMore();
          }
        }}
      >
        <div className="grid-inner">
          <div
            className={`grid-header ${scrollTop > 0 ? "scrolled" : ""}`}
            style={{ gridTemplateColumns: gridTemplate }}
            data-testid="grid-header"
          >
            <div className="grid-head-cell id-header" data-column="id">
              <span className="type-glyph">#</span>
              <span>ID</span>
            </div>
            {columns.map((column) => {
              const sorted = sortSpec?.column === column.name;
              const numeric = isNumericColumn(column);
              return (
                <button
                  type="button"
                  className={`grid-head-cell column-header ${numeric ? "numeric" : ""} ${
                    sorted ? "sorted" : ""
                  }`}
                  key={column.name}
                  data-column={column.name}
                  data-numeric={numeric ? "true" : "false"}
                  onClick={() => void setSort(column.name)}
                  title={`${column.name} · ${kindLabel(column)}`}
                >
                  <span className="type-glyph">{columnTypeGlyph(column.ty)}</span>
                  <span className="column-name">{column.name}</span>
                  {sorted && (
                    <Icon
                      name={sortSpec.direction === "asc" ? "arrowUp" : "arrowDown"}
                      size={10}
                      className="sort-chevron"
                    />
                  )}
                </button>
              );
            })}
          </div>

          <div className="grid-body" style={{ height: rows.length * rowHeight }}>
            {visibleRows.map((record, offset) => {
              const index = start + offset;
              return (
                <div
                  className={`grid-row ${selectedId === record.id ? "selected" : ""}`}
                  key={record.id}
                  data-testid="grid-row"
                  data-record-id={record.id}
                  style={{
                    gridTemplateColumns: gridTemplate,
                    transform: `translateY(${index * rowHeight}px)`,
                  }}
                  onMouseDown={() =>
                    setActive({ row: index, column: active?.column ?? 0 })
                  }
                  onClick={() => selectSoon(record.id)}
                  onContextMenu={(event) => {
                    event.preventDefault();
                    setContextMenu({ x: event.clientX, y: event.clientY, id: record.id });
                  }}
                >
                  <div className="grid-cell id-cell" data-column="id">
                    <span className="cell-text">{record.id}</span>
                  </div>
                  {columns.map((column, columnIndex) => {
                    const isEditing =
                      editing?.id === record.id && editing.column === column.name;
                    const value = record.values[column.name] ?? "Null";
                    return (
                      <div
                        key={column.name}
                        className={`grid-cell ${
                          isNumericColumn(column) ? "numeric" : ""
                        } ${isComputedColumn(column) ? "computed" : ""} ${
                          active?.row === index && active.column === columnIndex
                            ? "focused"
                            : ""
                        }`}
                        data-column={column.name}
                        data-numeric={isNumericColumn(column) ? "true" : "false"}
                        onDoubleClick={() => {
                          if (isComputedColumn(column)) return;
                          if (selectTimer.current !== null) {
                            window.clearTimeout(selectTimer.current);
                            selectTimer.current = null;
                          }
                          useStore.setState({ selectedId: record.id });
                          setEditing({ id: record.id, column: column.name });
                        }}
                      >
                        {isEditing ? (
                          <FieldInput
                            autoFocus
                            column={column}
                            value={value}
                            onCancel={() => setEditing(null)}
                            onCommit={async (next) => {
                              const ok = await updateCell(
                                table.name,
                                record.id,
                                column.name,
                                next,
                              );
                              if (ok) setEditing(null);
                              return ok;
                            }}
                          />
                        ) : isNull(value) ? (
                          <span className="cell-null">NULL</span>
                        ) : (
                          <span
                            className="cell-text"
                            dir="auto"
                            title={cellText(value)}
                          >
                            {cellText(value)}
                          </span>
                        )}
                      </div>
                    );
                  })}
                </div>
              );
            })}
          </div>
        </div>
        {rows.length === 0 && !loadingRows && (
          <div className="grid-empty-state">
            <Icon name="table" size={22} strokeWidth={1.3} />
            <p>No records in {table.name}</p>
            <button type="button" className="button primary" onClick={beginCreate}>
              + New Record
            </button>
          </div>
        )}
        {loadingRows && (
          <div className="grid-loading">
            <span className="loader" /> Loading rows
          </div>
        )}
      </div>

      {contextMenu && (
        <ContextMenu
          x={contextMenu.x}
          y={contextMenu.y}
          onClose={() => setContextMenu(null)}
          items={recordMenuItems(contextMenu.id)}
        />
      )}
    </div>
  );
}

function recordMenuItems(id: number): ContextMenuItem[] {
  const state = useStore.getState();
  return [
    {
      label: "Inspect Record",
      icon: "panelRight",
      onSelect: () => {
        void state.selectRow(id);
        state.setInspectorOpen(true);
      },
    },
    {
      label: "Copy Record ID",
      icon: "copy",
      onSelect: () => {
        void navigator.clipboard
          .writeText(String(id))
          .then(() => state.notify("success", "Record ID copied"))
          .catch(() => state.notify("error", "Could not copy record ID"));
      },
    },
    {
      label: "Delete Record…",
      icon: "trash",
      danger: true,
      separatorBefore: true,
      onSelect: () => {
        void (async () => {
          const ok = await confirmDestructive(
            "Delete this record and its child rows? Undo can restore it.",
            "Delete Record",
            "Delete",
          );
          if (ok) await state.deleteRecord(id);
        })();
      },
    },
  ];
}

function kindLabel(column: Column): string {
  const kind = column.ty.kind;
  if (kind === "formula" || kind === "lookup") return "computed";
  if (kind === "currency") return column.ty.currency;
  if (kind === "decimal") return `decimal ${column.ty.scale}`;
  return kind;
}
