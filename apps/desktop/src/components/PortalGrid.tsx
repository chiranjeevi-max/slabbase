import { useEffect, useRef, useState } from "react";
import type { Column, Cursor, Record as SlabRecord, Value } from "../bindings";
import { api } from "../api";
import { useStore } from "../store";
import { columnTypeLabel, isComputedColumn } from "../types";
import { fallbackValue } from "./DraftFields";
import { FieldInput } from "./FieldInput";
import { Icon } from "./Icons";

const ROW_HEIGHT = 30;
const PORTAL_HEIGHT = 240;
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
  const [rows, setRows] = useState<SlabRecord[]>([]);
  const [cursor, setCursor] = useState<Cursor | null>(null);
  const [total, setTotal] = useState(0);
  const [hasMore, setHasMore] = useState(false);
  const [loading, setLoading] = useState(false);
  const [scrollTop, setScrollTop] = useState(0);
  const [pulse, setPulse] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const requestRef = useRef(0);

  const visible: Column[] =
    !child
      ? []
      : columns.length > 0
        ? columns
            .map((name) => child.columns.find((column) => column.name === name))
            .filter((column): column is Column => column !== undefined)
        : child.columns.filter((column) => !isComputedColumn(column));

  useEffect(() => {
    if (!relation || !child) return;
    const token = ++requestRef.current;
    setRows([]);
    setCursor(null);
    setTotal(0);
    setHasMore(false);
    setScrollTop(0);
    setError(null);
    setLoading(true);
    api
      .pagePortal({
        relation: relation.name,
        parent_id: parentId,
        limit: PAGE_SIZE,
        cursor: null,
      })
      .then((page) => {
        if (requestRef.current !== token) return;
        setRows(page.rows);
        setCursor(page.next_cursor);
        setTotal(page.total_rows);
        setHasMore(page.next_cursor !== null);
      })
      .catch((reason: unknown) => {
        if (requestRef.current !== token) return;
        setError(reason instanceof Error ? reason.message : "Could not load child rows");
        useStore.getState().notify("error", errorText(reason));
      })
      .finally(() => {
        if (requestRef.current === token) setLoading(false);
      });
  }, [relation?.name, child?.name, parentId, historyTick]);

  if (!relation || !child) return null;

  const loadMore = () => {
    if (!hasMore || loading || !cursor) return;
    const token = requestRef.current;
    setLoading(true);
    api
      .pagePortal({
        relation: relation.name,
        parent_id: parentId,
        limit: PAGE_SIZE,
        cursor,
      })
      .then((page) => {
        if (requestRef.current !== token) return;
        setRows((current) => {
          const seen = new Set(current.map((row) => row.id));
          return [...current, ...page.rows.filter((row) => !seen.has(row.id))];
        });
        setCursor(page.next_cursor);
        setTotal(page.total_rows);
        setHasMore(page.next_cursor !== null);
      })
      .catch((reason: unknown) => {
        if (requestRef.current !== token) return;
        useStore.getState().notify("error", errorText(reason));
      })
      .finally(() => {
        if (requestRef.current === token) setLoading(false);
      });
  };

  const reload = () => {
    const token = ++requestRef.current;
    setLoading(true);
    setRows([]);
    setCursor(null);
    api
      .pagePortal({
        relation: relation.name,
        parent_id: parentId,
        limit: PAGE_SIZE,
        cursor: null,
      })
      .then((page) => {
        if (requestRef.current !== token) return;
        setRows(page.rows);
        setCursor(page.next_cursor);
        setTotal(page.total_rows);
        setHasMore(page.next_cursor !== null);
      })
      .catch((reason: unknown) => {
        if (requestRef.current !== token) return;
        useStore.getState().notify("error", errorText(reason));
      })
      .finally(() => {
        if (requestRef.current === token) setLoading(false);
      });
  };

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

  const start = Math.max(0, Math.floor(scrollTop / ROW_HEIGHT) - 3);
  const visibleCount = Math.ceil(PORTAL_HEIGHT / ROW_HEIGHT) + 6;
  const end = Math.min(rows.length, start + visibleCount);
  const bodyHeight = Math.max(
    ROW_HEIGHT,
    Math.min(rows.length * ROW_HEIGHT + (rows.length > 0 ? 2 : 0), PORTAL_HEIGHT),
  );

  return (
    <div className="form-portal">
      <div className="portal-header">
        <span>
          {relation.name}
          <small className="portal-count">
            {rows.length.toLocaleString()} of {total.toLocaleString()}
          </small>
        </span>
        <button type="button" className="text-button" onClick={() => void addRow()}>
          <Icon name="plus" size={13} /> Add line
        </button>
      </div>
      <div className="portal-grid-head" style={{ gridTemplateColumns: gridColumns(visible) }}>
        {visible.map((column) => (
          <span key={column.name} className={isComputedColumn(column) ? "computed" : ""}>
            {column.name}
            {isComputedColumn(column) && <span className="computed-mark"> ƒ</span>}
            <small>{columnTypeLabel(column.ty)}</small>
          </span>
        ))}
        <span aria-hidden="true" />
      </div>
      <div
        className="portal-scroll"
        style={{ height: bodyHeight }}
        onScroll={(event) => {
          const element = event.currentTarget;
          setScrollTop(element.scrollTop);
          const remaining =
            rows.length * ROW_HEIGHT - (element.scrollTop + element.clientHeight);
          if (remaining < ROW_HEIGHT * 4) loadMore();
        }}
      >
        <div className="portal-spacer" style={{ height: rows.length * ROW_HEIGHT }}>
          {rows.slice(start, end).map((row, offset) => {
            const index = start + offset;
            return (
              <div
                className="portal-grid-row"
                key={row.id}
                style={{
                  gridTemplateColumns: gridColumns(visible),
                  transform: `translateY(${index * ROW_HEIGHT}px)`,
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
                    className="icon-button tiny"
                    title="Delete line"
                    onClick={() => {
                      if (!window.confirm("Delete this line?")) return;
                      void useStore
                        .getState()
                        .deletePortalRow(child.name, row.id)
                        .then((ok) => {
                          if (ok) {
                            setRows((current) => current.filter((candidate) => candidate.id !== row.id));
                            setTotal((current) => Math.max(0, current - 1));
                          }
                        });
                    }}
                  >
                    <Icon name="x" size={12} />
                  </button>
                </div>
              </div>
            );
          })}
        </div>
        {rows.length === 0 && !loading && (
          <div className="portal-empty">
            {error ?? "No child rows yet."}
          </div>
        )}
        {loading && (
          <div className="portal-loading">
            <span className="loader" /> Loading lines
          </div>
        )}
        {hasMore && !loading && (
          <button type="button" className="portal-more" onClick={loadMore}>
            Load more
          </button>
        )}
      </div>
    </div>
  );
}

function gridColumns(columns: Column[]): string {
  if (columns.length === 0) return "1fr 26px";
  return `${columns
    .map((_, index) => (index === 0 ? "minmax(100px, 1.3fr)" : "minmax(64px, 1fr)"))
    .join(" ")} 26px`;
}

function errorText(error: unknown): string {
  if (error instanceof Error) return error.message;
  return "Could not load child rows";
}
