import { useCallback, useEffect, useRef, useState } from "react";
import type { Column, Cursor, Record as SlabRecord, Value } from "../bindings";
import { DENSITY_ROW_HEIGHT } from "../appearance";
import { api, frameRecords, isCancelledError } from "../api";
import { useStore } from "../store";
import { columnTypeGlyph, isComputedColumn } from "../types";
import { confirmDestructive } from "../native";
import { fallbackValue } from "./DraftFields";
import { FieldInput } from "./FieldInput";
import { Icon } from "./Icons";

const PORTAL_HEIGHT = 224;
const PAGE_SIZE = 100;

interface PortalGridProps {
  relationName: string;
  columns: string[];
  parentId: number;
}

export function PortalGrid({ relationName, columns, parentId }: PortalGridProps) {
  const schema = useStore((state) => state.schema);
  const historyTick = useStore((state) => state.historyTick);
  const relation = schema?.relations.find((candidate) => candidate.name === relationName);
  const child = schema?.tables.find((candidate) => candidate.name === relation?.child_table);
  const { rows, setRows, total, setTotal, hasMore, loading, scrollTop, setScrollTop, error, loadMore, reload } =
    usePortalPages(relation?.name, child?.name, parentId, historyTick);
  const [pulse, setPulse] = useState<string | null>(null);
  const density = useStore((state) => state.appearance.density);
  const rowHeight = DENSITY_ROW_HEIGHT[density];

  const visible: Column[] =
    !child
      ? []
      : columns.length > 0
        ? columns
            .map((name) => child.columns.find((column) => column.name === name))
            .filter((column): column is Column => column !== undefined)
        : child.columns.filter((column) => !isComputedColumn(column));

  if (!relation || !child) return null;

  const update = async (rowId: number, column: Column, value: Value) => {
    const record = await useStore
      .getState()
      .updatePortalRow(child.name, rowId, column.name, value);
    if (!record) return false;
    setRows((current) => current.map((row) => (row.id === rowId ? record : row)));
    const pulseKey = `${rowId}:${column.name}`;
    setPulse(pulseKey);
    window.setTimeout(() => {
      setPulse((current) => (current === pulseKey ? null : current));
    }, 460);
    return true;
  };

  const addRow = async () => {
    const values: Record<string, Value> = {
      [relation.child_column]: { Integer: parentId },
    };
    for (const column of child.columns) {
      if (column.name === relation.child_column || isComputedColumn(column)) continue;
      if (column.required) values[column.name] = fallbackValue(column);
    }
    const record = await useStore.getState().insertPortalRow(child.name, values);
    if (record) reload();
  };

  const start = Math.max(0, Math.floor(Math.max(0, scrollTop - rowHeight) / rowHeight) - 3);
  const visibleCount = Math.ceil(PORTAL_HEIGHT / rowHeight) + 6;
  const end = Math.min(rows.length, start + visibleCount);
  const bodyHeight = Math.max(
    rowHeight,
    Math.min(rows.length * rowHeight + (rows.length > 0 ? 2 : 0), PORTAL_HEIGHT),
  );
  const addLabel = addRowLabel(child.name);

  return (
    <div className="form-portal">
      <div className="portal-header">
        <span className="portal-title">
          {relation.name}
          <small className="portal-count">
            {rows.length.toLocaleString()} of {total.toLocaleString()}
          </small>
        </span>
        <button type="button" className="text-button" onClick={() => void addRow()}>
          {addLabel}
        </button>
      </div>
      <div
        className="portal-scroll"
        data-testid="portal-scroll"
        style={{ height: bodyHeight + rowHeight }}
        onScroll={(event) => {
          const element = event.currentTarget;
          setScrollTop(element.scrollTop);
          const remaining =
            rows.length * rowHeight - (element.scrollTop - rowHeight + element.clientHeight);
          if (remaining < rowHeight * 4) loadMore();
        }}
      >
        <div
          className={`portal-grid-head ${scrollTop > 0 ? "scrolled" : ""}`}
          style={{ gridTemplateColumns: gridColumns(visible) }}
          data-testid="portal-grid-head"
        >
          {visible.map((column) => (
            <span key={column.name} className={isComputedColumn(column) ? "computed" : ""}>
              <span className="type-glyph">{columnTypeGlyph(column.ty)}</span>
              <span className="portal-column-name">{column.name}</span>
            </span>
          ))}
          <span aria-hidden="true" />
        </div>
        <div className="portal-spacer" style={{ height: rows.length * rowHeight }}>
          {rows.slice(start, end).map((row, offset) => {
            const index = start + offset;
            return (
              <div
                className="portal-grid-row"
                key={row.id}
                data-row-id={row.id}
                style={{
                  gridTemplateColumns: gridColumns(visible),
                  transform: `translateY(${index * rowHeight}px)`,
                }}
              >
                {visible.map((column) => (
                  <div
                    className={`portal-grid-cell ${
                      isComputedColumn(column) ? "computed" : ""
                    }`}
                    key={column.name}
                  >
                    <FieldInput
                      compact
                      column={column}
                      value={row.values[column.name] ?? "Null"}
                      pulse={pulse === `${row.id}:${column.name}`}
                      onCommit={(value) => update(row.id, column, value)}
                    />
                  </div>
                ))}
                <div className="portal-grid-cell portal-actions">
                  <button
                    type="button"
                    className="icon-control micro"
                    title="Delete row"
                    aria-label="Delete row"
                    onClick={() => {
                      void (async () => {
                        const ok = await confirmDestructive(
                          "Delete this row? Undo can restore it.",
                          "Delete Row",
                          "Delete",
                        );
                        if (!ok) return;
                        const deleted = await useStore
                          .getState()
                          .deletePortalRow(child.name, row.id);
                        if (deleted) {
                          setRows((current) =>
                            current.filter((candidate) => candidate.id !== row.id),
                          );
                          setTotal((current) => Math.max(0, current - 1));
                        }
                      })();
                    }}
                  >
                    <Icon name="x" size={10} />
                  </button>
                </div>
              </div>
            );
          })}
        </div>
        {rows.length === 0 && !loading && (
          <div className="portal-empty">{error ?? "No child rows"}</div>
        )}
        {loading && (
          <div className="portal-loading">
            <span className="loader" /> Loading
          </div>
        )}
        {hasMore && !loading && (
          <button type="button" className="portal-more" onClick={loadMore}>
            Load More
          </button>
        )}
      </div>
    </div>
  );
}

function usePortalPages(
  relationName: string | undefined,
  childName: string | undefined,
  parentId: number,
  historyTick: number,
) {
  const [rows, setRows] = useState<SlabRecord[]>([]);
  const [cursor, setCursor] = useState<Cursor | null>(null);
  const [total, setTotal] = useState(0);
  const [hasMore, setHasMore] = useState(false);
  const [loading, setLoading] = useState(false);
  const [scrollTop, setScrollTop] = useState(0);
  const [error, setError] = useState<string | null>(null);
  const requestRef = useRef(0);

  const requestPage = useCallback(async (
    relation: string,
    pageCursor: Cursor | null,
    token: number,
    showError = false,
  ) => {
    try {
      const page = await api.pagePortal({
        relation,
        parent_id: parentId,
        limit: PAGE_SIZE,
        cursor: pageCursor,
      });
      if (requestRef.current !== token) return;
      if (pageCursor === null) {
        setRows(frameRecords(page));
      } else {
        setRows((current) => {
          const seen = new Set(current.map((row) => row.id));
          return [
            ...current,
            ...frameRecords(page).filter((row) => !seen.has(row.id)),
          ];
        });
      }
      setCursor(page.next_cursor);
      setTotal(page.total_rows);
      setHasMore(page.next_cursor !== null);
    } catch (reason) {
      if (requestRef.current !== token || isCancelledError(reason)) return;
      const message = errorText(reason);
      if (showError) setError(message);
      useStore.getState().notify("error", message);
    } finally {
      if (requestRef.current === token) setLoading(false);
    }
  }, [parentId]);

  useEffect(() => {
    if (relationName === undefined || childName === undefined) return;
    const token = ++requestRef.current;
    setRows([]);
    setCursor(null);
    setTotal(0);
    setHasMore(false);
    setScrollTop(0);
    setError(null);
    setLoading(true);
    void requestPage(relationName, null, token, true);
  }, [relationName, childName, historyTick, requestPage]);

  const loadMore = () => {
    if (relationName === undefined || !hasMore || loading || !cursor) return;
    setLoading(true);
    void requestPage(relationName, cursor, requestRef.current);
  };

  const reload = () => {
    if (relationName === undefined) return;
    const token = ++requestRef.current;
    setLoading(true);
    setRows([]);
    setCursor(null);
    void requestPage(relationName, null, token);
  };

  return { rows, setRows, total, setTotal, hasMore, loading, scrollTop, setScrollTop, error, loadMore, reload };
}

function gridColumns(columns: Column[]): string {
  if (columns.length === 0) return "1fr 24px";
  return `${columns
    .map((column, index) => {
      const kind = column.ty.kind;
      const priority = index === 0 || /name|title|description/i.test(column.name);
      if (kind === "select") return "minmax(66px, 0.8fr)";
      if (kind === "date" || kind === "timestamp") return "minmax(88px, 1fr)";
      if (kind === "currency" || kind === "decimal" || kind === "integer") {
        return "minmax(68px, 0.9fr)";
      }
      if (kind === "formula" || kind === "lookup") return "minmax(70px, 1fr)";
      if (priority) return "minmax(110px, 1.6fr)";
      return "minmax(76px, 1fr)";
    })
    .join(" ")} 24px`;
}

function addRowLabel(tableName: string): string {
  const target = singularize(tableName.trim());
  return target ? `+ Add ${target}` : "+ Add Row";
}

function singularize(name: string): string {
  if (!name) return "";
  if (/ies$/i.test(name)) return name.replace(/ies$/i, "y");
  if (/ses$/i.test(name)) return name.replace(/es$/i, "");
  if (/s$/i.test(name) && !/ss$/i.test(name)) return name.replace(/s$/i, "");
  return name;
}

function errorText(error: unknown): string {
  if (error instanceof Error) return error.message;
  return "Could not load child rows";
}
