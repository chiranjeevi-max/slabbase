import { useEffect, useState } from "react";
import type { Column, IndexSpec, Schema } from "../bindings";
import { api } from "../api";
import { useStore } from "../store";
import { columnTypeLabel, isComputedColumn } from "../types";
import { confirmDestructive } from "../native";
import { Icon } from "./Icons";

interface SchemaManageProps {
  schema: Schema | null;
  activeTable: string;
  onTableSelected: (table: string) => void;
}

export function SchemaManage({ schema, activeTable, onTableSelected }: SchemaManageProps) {
  const renameTable = useStore((state) => state.renameTable);
  const renameColumn = useStore((state) => state.renameColumn);
  const dropTable = useStore((state) => state.dropTable);
  const dropColumn = useStore((state) => state.dropColumn);
  const dropRelation = useStore((state) => state.dropRelation);
  const toggleIndex = useStore((state) => state.toggleIndex);
  const notify = useStore((state) => state.notify);

  const [tableName, setTableName] = useState(activeTable);
  const [renames, setRenames] = useState<Record<string, string>>({});
  const [indexes, setIndexes] = useState<IndexSpec[]>([]);
  const [busy, setBusy] = useState(false);

  const table = schema?.tables.find((candidate) => candidate.name === activeTable);

  useEffect(() => {
    setTableName(activeTable);
    setRenames({});
    if (!activeTable) {
      setIndexes([]);
      return;
    }
    let cancelled = false;
    api
      .listIndexes(activeTable)
      .then((result) => {
        if (!cancelled) setIndexes(Array.isArray(result) ? result : []);
      })
      .catch((error: unknown) => {
        if (!cancelled) notify("error", errorText(error));
      });
    return () => {
      cancelled = true;
    };
  }, [activeTable, schema, notify]);

  if (!schema) return null;
  if (!table) {
    return (
      <div className="manage-empty">
        <p>No table selected</p>
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
    const next = tableName.trim();
    if (!next || next === table.name) return;
    const ok = await run(() => renameTable(table.name, next));
    if (ok) onTableSelected(next);
  };

  const onRenameColumn = async (column: Column) => {
    const next = (renames[column.id] ?? column.name).trim();
    if (!next || next === column.name) return;
    await run(() => renameColumn(table.name, column.name, next));
  };

  const onDropTable = async () => {
    const ok = await confirmDestructive(
      `Drop “${table.name}” and all of its rows, forms, and indexes? Undo can restore them.`,
      "Drop Table",
      "Drop Table",
    );
    if (!ok) return;
    const dropped = await run(() => dropTable(table.name));
    if (!dropped) return;
    const next = useStore.getState().activeTable;
    if (next) onTableSelected(next);
  };

  const onDropColumn = async (column: Column) => {
    const ok = await confirmDestructive(
      `Drop “${column.name}” from “${table.name}”? Stored values are retained for undo.`,
      "Drop Column",
      "Drop Column",
    );
    if (!ok) return;
    await run(() => dropColumn(table.name, column.name));
  };

  const onToggleIndex = async (column: Column, enabled: boolean) => {
    const result = await toggleIndex(table.name, column.name, enabled);
    if (Array.isArray(result)) setIndexes(result);
  };

  return (
    <div className="manage">
      <div className="manage-header">
        <select
          className="select manage-table-select"
          value={activeTable}
          aria-label="Managed table"
          onChange={(event) => onTableSelected(event.target.value)}
        >
          {schema.tables.map((candidate) => (
            <option key={candidate.name} value={candidate.name}>
              {candidate.name}
            </option>
          ))}
        </select>
        <input
          className="field-input"
          value={tableName}
          aria-label="Table name"
          onChange={(event) => setTableName(event.target.value)}
          onKeyDown={(event) => {
            if (event.key === "Enter") void onRenameTable();
          }}
        />
        <button
          type="button"
          className="button compact"
          disabled={busy || tableName.trim() === table.name}
          onClick={() => void onRenameTable()}
        >
          Rename
        </button>
        <button
          type="button"
          className="button danger-ghost compact"
          disabled={busy}
          onClick={() => void onDropTable()}
        >
          Drop Table
        </button>
      </div>

      <div className="manage-section">
        <div className="manage-section-head">
          <span>Columns</span>
          <span className="manage-count">{table.columns.length}</span>
        </div>
        <div className="manage-grid">
          {table.columns.map((column) => {
            const indexed = indexes.some(
              (index) => index.columns.length === 1 && index.columns[0] === column.name,
            );
            return (
              <div className="manage-row" key={column.id}>
                <input
                  className="field-input manage-name"
                  value={renames[column.id] ?? column.name}
                  aria-label={`Rename ${column.name}`}
                  onChange={(event) =>
                    setRenames((current) => ({ ...current, [column.id]: event.target.value }))
                  }
                  onBlur={() => void onRenameColumn(column)}
                  onKeyDown={(event) => {
                    if (event.key === "Enter") {
                      event.currentTarget.blur();
                    }
                  }}
                />
                <span className="manage-type">{columnTypeLabel(column.ty)}</span>
                <span className="manage-flags">
                  {column.required && <span className="badge neutral">required</span>}
                  {column.unique && <span className="badge neutral">unique</span>}
                  {isComputedColumn(column) && <span className="badge info">computed</span>}
                </span>
                <label className="inline-check" title="Single-column SQLite index">
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
                  className="icon-control micro danger"
                  title={`Drop ${column.name}`}
                  aria-label={`Drop ${column.name}`}
                  disabled={busy}
                  onClick={() => void onDropColumn(column)}
                >
                  <Icon name="trash" size={12} strokeWidth={1.5} />
                </button>
              </div>
            );
          })}
        </div>
      </div>

      <div className="manage-relations">
        <div className="manage-section-head">
          <span>Relations</span>
          <span className="manage-count">{schema.relations.length}</span>
        </div>
        {schema.relations.length === 0 && <p className="manage-empty-line">No relations</p>}
        {schema.relations.map((relation) => (
          <div className="manage-relation-row" key={relation.id}>
            <span className="relation-path">
              {relation.parent_table}
              <span className="relation-arrow">→</span>
              {relation.child_table}.{relation.child_column}
            </span>
            <span className="relation-name">{relation.name}</span>
            <button
              type="button"
              className="text-button danger-text"
              disabled={busy}
              onClick={() => {
                void (async () => {
                  const ok = await confirmDestructive(
                    `Drop relation “${relation.name}”?`,
                    "Drop Relation",
                    "Drop Relation",
                  );
                  if (ok) await run(() => dropRelation(relation.name));
                })();
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
