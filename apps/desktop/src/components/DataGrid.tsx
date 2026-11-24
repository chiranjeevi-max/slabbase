import { useEffect, useMemo, useRef, useState } from "react";
import { save } from "@tauri-apps/plugin-dialog";
import type { Column } from "../bindings";
import { api } from "../api";
import { pageQueryFor, useStore } from "../store";
import { cellText, isComputedColumn, isNumericColumn } from "../types";
import { FieldInput } from "./FieldInput";
import { Icon } from "./Icons";

const ROW_HEIGHT = 34;
const HEADER_HEIGHT = 38;
const COLUMN_WIDTH = 190;

export function DataGrid() {
  const schema = useStore((state) => state.schema);
  const activeTable = useStore((state) => state.activeTable);
  const rows = useStore((state) => state.rows);
  const hasMore = useStore((state) => state.hasMore);
  const loadingRows = useStore((state) => state.loadingRows);
  const elapsedMs = useStore((state) => state.elapsedMs);
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
  const setView = useStore((state) => state.setView);
  const setPrintOpen = useStore((state) => state.setPrintOpen);
  const updateCell = useStore((state) => state.updateCell);

  const table = schema?.tables.find((candidate) => candidate.name === activeTable);
  const columns = table?.columns ?? [];
  const gridTemplate = useMemo(
    () => `56px ${columns.map(() => `${COLUMN_WIDTH}px`).join(" ")}`,
    [columns],
  );
  const minWidth = 56 + columns.length * COLUMN_WIDTH;

  const scrollRef = useRef<HTMLDivElement>(null);
  const [scrollTop, setScrollTop] = useState(0);
  const [viewport, setViewport] = useState(600);
  const [editing, setEditing] = useState<{ id: number; column: string } | null>(null);
  const [active, setActive] = useState<{ row: number; column: number } | null>(null);
  const [filterDraft, setFilterDraft] = useState(filterText);
  const [exportOpen, setExportOpen] = useState(false);

  useEffect(() => {
    setEditing(null);
    setActive(null);
    setScrollTop(0);
    setFilterDraft(filterText);
    scrollRef.current?.scrollTo({ top: 0 });
  }, [activeTable, filterText]);

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
  const start = Math.max(0, Math.floor(bodyScroll / ROW_HEIGHT) - 6);
  const visibleCount = Math.ceil(viewport / ROW_HEIGHT) + 12;
  const end = Math.min(rows.length, start + visibleCount);
  const visibleRows = rows.slice(start, end);
  const sortSpec = sort[0];

  const ensureVisible = (index: number) => {
    const element = scrollRef.current;
    if (!element) return;
    const top = HEADER_HEIGHT + index * ROW_HEIGHT;
    if (top < element.scrollTop + HEADER_HEIGHT) {
      element.scrollTo({ top: top - HEADER_HEIGHT });
    } else if (top + ROW_HEIGHT > element.scrollTop + viewport) {
      element.scrollTo({ top: top + ROW_HEIGHT - viewport + 12 });
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
      <div className="grid-empty-state">
        <Icon name="table" size={24} />
        <h3>No table selected</h3>
        <p>Create a table or load the sample workspace to begin.</p>
      </div>
    );
  }

  return (
    <div className="data-grid">
      <div className="grid-toolbar">
        <div className="grid-title">
          <h1>{table.name}</h1>
          <span className="row-count">
            {rows.length.toLocaleString()}
            {hasMore ? "+" : ""} rows
            <span className="dot" />
            {elapsedMs.toFixed(1)} ms
          </span>
        </div>
        <div className="toolbar-actions">
          <button type="button" className="button ghost" onClick={() => setView("form")}>
            <Icon name="form" size={14} /> Form
          </button>
          <button type="button" className="button ghost" onClick={() => openSchema("column")}>
            <Icon name="sliders" size={14} /> Column
          </button>
          <div className="menu-anchor">
            <button
              type="button"
              className="button ghost"
              onClick={() => setExportOpen((open) => !open)}
            >
              <Icon name="download" size={14} /> Export
              <Icon name="chevronDown" size={12} />
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
                  SQL script
                </button>
              </div>
            )}
          </div>
          <button type="button" className="button ghost" onClick={() => setPrintOpen(true)}>
            <Icon name="printer" size={14} /> Print
          </button>
          <button type="button" className="button primary" onClick={beginCreate}>
            <Icon name="plus" size={14} /> New record
          </button>
        </div>
      </div>

      <div className="filter-bar">
        <div className="filter-input">
          <Icon name="search" size={14} />
          <input
            value={filterDraft}
            placeholder={`Filter by ${filterColumn}…`}
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
              className="icon-button tiny"
              title="Clear filter"
              onClick={() => {
                setFilterDraft("");
                void clearFilter();
              }}
            >
              <Icon name="x" size={12} />
            </button>
          )}
        </div>
        <select
          className="select compact-select"
          value={filterColumn}
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
        {sortSpec && (
          <button type="button" className="chip" onClick={() => void setSort(sortSpec.column)}>
            {sortSpec.column} {sortSpec.direction === "asc" ? "↑" : "↓"}
          </button>
        )}
        {filterText && (
          <span className="chip muted">
            contains “{filterText}”
            <button type="button" className="chip-x" onClick={() => void clearFilter()}>
              <Icon name="x" size={11} />
            </button>
          </span>
        )}
        <span className="filter-spacer" />
        <span className="keyboard-hint">
          <Icon name="keyboard" size={13} /> ↑↓ navigate · Enter edit
        </span>
      </div>

      <div
        ref={scrollRef}
        className="grid-scroll"
        tabIndex={0}
        onKeyDown={handleKeyDown}
        onScroll={(event) => {
          const element = event.currentTarget;
          setScrollTop(element.scrollTop);
          if (
            hasMore &&
            !loadingRows &&
            element.scrollTop + element.clientHeight >= rows.length * ROW_HEIGHT - 240
          ) {
            void loadMore();
          }
        }}
      >
        <div className="grid-inner" style={{ minWidth }}>
          <div className="grid-header" style={{ gridTemplateColumns: gridTemplate }}>
            <div className="grid-head-cell id-cell">ID</div>
            {columns.map((column) => (
              <button
                type="button"
                className={`grid-head-cell ${
                  sortSpec?.column === column.name ? "sorted" : ""
                }`}
                key={column.name}
                onClick={() => void setSort(column.name)}
                title={column.name}
              >
                <span className="head-name">
                  {column.name}
                  {isComputedColumn(column) && <span className="computed-mark"> ƒ</span>}
                </span>
                <span className="head-kind">{kindLabel(column)}</span>
                {sortSpec?.column === column.name && (
                  <Icon
                    name={sortSpec.direction === "asc" ? "arrowUp" : "arrowDown"}
                    size={11}
                  />
                )}
              </button>
            ))}
          </div>

          <div className="grid-body" style={{ height: rows.length * ROW_HEIGHT }}>
            {visibleRows.map((record, offset) => {
              const index = start + offset;
              return (
                <div
                  className={`grid-row ${selectedId === record.id ? "selected" : ""} ${
                    active?.row === index ? "active" : ""
                  }`}
                  key={record.id}
                  style={{
                    gridTemplateColumns: gridTemplate,
                    transform: `translateY(${index * ROW_HEIGHT}px)`,
                  }}
                  onMouseDown={() => setActive({ row: index, column: active?.column ?? 0 })}
                  onClick={() => void selectRow(record.id)}
                >
                  <div className="grid-cell id-cell">{record.id}</div>
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
                          active?.row === index && active.column === columnIndex ? "focused" : ""
                        }`}
                        onDoubleClick={() => {
                          if (!isComputedColumn(column)) {
                            setEditing({ id: record.id, column: column.name });
                          }
                        }}
                      >
                        {isEditing ? (
                          <FieldInput
                            autoFocus
                            column={column}
                            value={value}
                            onCommit={async (next) => {
                              const ok = await updateCell(table.name, record.id, column.name, next);
                              if (ok) setEditing(null);
                              return ok;
                            }}
                          />
                        ) : (
                          <span className="cell-text" title={cellText(value)}>
                            {cellText(value) || <span className="dash">—</span>}
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
          <div className="grid-empty">
            <p>No records in {table.name}.</p>
            <button type="button" className="button primary" onClick={beginCreate}>
              <Icon name="plus" size={14} /> Create the first record
            </button>
          </div>
        )}
        {loadingRows && (
          <div className="grid-loading">
            <span className="loader" /> Loading rows
          </div>
        )}
      </div>
    </div>
  );
}

function kindLabel(column: Column): string {
  const kind = column.ty.kind;
  if (kind === "formula" || kind === "lookup") return "computed";
  if (kind === "currency") return column.ty.currency;
  if (kind === "decimal") return `dec ${column.ty.scale}`;
  return kind;
}
