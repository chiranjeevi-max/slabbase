import { useEffect, useState } from "react";
import type { Column, IndexSpec, Schema } from "../bindings";
import { api } from "../api";
import { useStore } from "../store";
import { columnTypeLabel, isComputedColumn } from "../types";
import { Icon } from "./Icons";

interface SchemaManageProps {
  schema: Schema | null;
  activeTable: string;
  onTableSelected: (table: string) => void;
  onClose: () => void;
}

export function SchemaManage({
  schema,
  activeTable,
  onTableSelected,
  onClose,
}: SchemaManageProps) {
  const renameTable = useStore((state) => state.renameTable);
  const renameColumn = useStore((state) => state.renameColumn);
  const dropTable = useStore((state) => state.dropTable);
  const dropColumn = useStore((state) => state.dropColumn);
  const dropRelation = useStore((state) => state.dropRelation);
  const toggleIndex = useStore((state) => state.toggleIndex);
  const notify = useStore((state) => state.notify);

  const [selectedTable, setSelectedTable] = useState(activeTable);
  const [tableName, setTableName] = useState(activeTable);
  const [renames, setRenames] = useState<Record<string, string>>({});
  const [indexes, setIndexes] = useState<IndexSpec[]>([]);
  const [busy, setBusy] = useState(false);

  const table = schema?.tables.find((candidate) => candidate.name === selectedTable);

  useEffect(() => {
    setSelectedTable(activeTable);
    setTableName(activeTable);
  }, [activeTable]);

  useEffect(() => {
    setTableName(selectedTable);
    setRenames({});
    if (!selectedTable) {
      setIndexes([]);
      return;
    }
    let cancelled = false;
    api
      .listIndexes(selectedTable)
      .then((result) => {
        if (!cancelled) setIndexes(Array.isArray(result) ? result : []);
      })
      .catch((error: unknown) => {
        if (!cancelled) notify("error", errorText(error));
      });
    return () => {
      cancelled = true;
    };
  }, [selectedTable, schema, notify]);

  if (!schema) return null;
  if (!table) {
    return (
      <div className="manage-empty">
        <Icon name="database" size={22} />
        <p>No table selected.</p>
        <button type="button" className="button ghost" onClick={onClose}>
          Close
        </button>
      </div>
    );
  }

  const run = async (action: () => Promise<boolean>) => {
    setBusy(true);
    const ok = await action();
    setBusy(false);
    return ok;
  };

  const onRenameTable = async () => {
    if (!tableName.trim() || tableName.trim() === table.name) return;
    const next = tableName.trim();
    const ok = await run(() => renameTable(table.name, next));
    if (ok) {
      setSelectedTable(next);
      onTableSelected(next);
    }
  };

  const onRenameColumn = async (column: Column) => {
    const next = (renames[column.id] ?? column.name).trim();
    if (!next || next === column.name) return;
    await run(() => renameColumn(table.name, column.name, next));
  };

  const onDropTable = async () => {
    if (
      !window.confirm(
        `Drop table “${table.name}” and all of its rows, forms, and indexes? Undo can restore them.`,
      )
    ) {
      return;
    }
    const ok = await run(() => dropTable(table.name));
    if (!ok) return;
    const next = useStore.getState().activeTable;
    if (next) {
      setSelectedTable(next);
      onTableSelected(next);
    } else {
      onClose();
    }
  };

  const onDropColumn = async (column: Column) => {
    if (
      !window.confirm(
        `Drop column “${column.name}” from “${table.name}”? Stored values are retained for undo.`,
      )
    ) {
      return;
    }
    await run(() => dropColumn(table.name, column.name));
  };

  const onToggleIndex = async (column: Column, enabled: boolean) => {
    const result = await toggleIndex(table.name, column.name, enabled);
    if (Array.isArray(result)) setIndexes(result);
  };

  return (
    <div className="manage">
      <div className="manage-toolbar">
        <label className="field-label">
          Table
          <select
            className="select"
            value={selectedTable}
            onChange={(event) => setSelectedTable(event.target.value)}
          >
            {schema.tables.map((candidate) => (
              <option key={candidate.name} value={candidate.name}>
                {candidate.name}
              </option>
            ))}
          </select>
        </label>
        <label className="field-label grow">
          Rename table
          <input
            className="field-input"
            value={tableName}
            onChange={(event) => setTableName(event.target.value)}
            onKeyDown={(event) => {
              if (event.key === "Enter") void onRenameTable();
            }}
          />
        </label>
        <button
          type="button"
          className="button ghost"
          disabled={busy || tableName.trim() === table.name}
          onClick={() => void onRenameTable()}
        >
          Rename
        </button>
        <button type="button" className="button danger-ghost" disabled={busy} onClick={() => void onDropTable()}>
          <Icon name="trash" size={14} /> Drop table
        </button>
      </div>

      <div className="manage-grid">
        {table.columns.map((column) => {
          const indexed = indexes.some(
            (index) => index.columns.length === 1 && index.columns[0] === column.name,
          );
          return (
            <div className="manage-row" key={column.id}>
              <div className="manage-row-title">
                <strong>{column.name}</strong>
                <span>{columnTypeLabel(column.ty)}</span>
                {column.required && <span className="badge neutral">required</span>}
                {column.unique && <span className="badge neutral">unique</span>}
                {isComputedColumn(column) && <span className="badge info">computed</span>}
              </div>
              <input
                className="field-input"
                value={renames[column.id] ?? column.name}
                onChange={(event) =>
                  setRenames((current) => ({ ...current, [column.id]: event.target.value }))
                }
                onKeyDown={(event) => {
                  if (event.key === "Enter") void onRenameColumn(column);
                }}
                aria-label={`Rename ${column.name}`}
              />
              <button
                type="button"
                className="button ghost"
                disabled={
                  busy ||
                  (renames[column.id] ?? column.name).trim() === column.name
                }
                onClick={() => void onRenameColumn(column)}
              >
                Rename
              </button>
              <label className="inline-check" title="Toggle a single-column SQLite index">
                <input
                  type="checkbox"
                  checked={indexed}
                  disabled={busy || isComputedColumn(column)}
                  onChange={(event) => void onToggleIndex(column, event.target.checked)}
                />
                Index
              </label>
              <button
                type="button"
                className="icon-button danger"
                title={`Drop ${column.name}`}
                disabled={busy}
                onClick={() => void onDropColumn(column)}
              >
                <Icon name="trash" size={12} />
              </button>
            </div>
          );
        })}
      </div>

      <div className="manage-relations">
        <h3>Relations</h3>
        {schema.relations.length === 0 && <p className="muted">No relations.</p>}
        {schema.relations.map((relation) => (
          <div className="manage-relation-row" key={relation.id}>
            <span>
              {relation.parent_table} → {relation.child_table}.{relation.child_column}
            </span>
            <span className="muted">{relation.name}</span>
            <button
              type="button"
              className="text-button danger-text"
              disabled={busy}
              onClick={() => {
                if (!window.confirm(`Drop relation “${relation.name}”?`)) return;
                void run(() => dropRelation(relation.name));
              }}
            >
              Drop
            </button>
          </div>
        ))}
      </div>
    </div>
  );
}

function errorText(error: unknown): string {
  if (error instanceof Error) return error.message;
  return "Schema operation failed";
}
