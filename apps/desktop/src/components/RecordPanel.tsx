import { useEffect, useState } from "react";
import type { Column, LayoutItem, Table, Value } from "../bindings";
import { useStore } from "../store";
import { isComputedColumn, recordTitle } from "../types";
import { FieldInput } from "./FieldInput";
import { DraftField, DraftFieldLabel, fallbackValue } from "./DraftFields";
import { Icon } from "./Icons";

export function RecordPanel() {
  const schema = useStore((state) => state.schema);
  const activeTable = useStore((state) => state.activeTable);
  const selectedId = useStore((state) => state.selectedId);
  const bundle = useStore((state) => state.bundle);
  const creating = useStore((state) => state.creating);
  const activeLayoutId = useStore((state) => state.activeLayoutId);
  const [pulse, setPulse] = useState<string | null>(null);

  const table = schema?.tables.find((candidate) => candidate.name === activeTable);

  useEffect(() => {
    if (!pulse) return;
    const timer = window.setTimeout(() => setPulse(null), 480);
    return () => window.clearTimeout(timer);
  }, [pulse]);

  if (!table) {
    return (
      <aside className="record-panel">
        <div className="empty-panel">
          <Icon name="table" size={20} />
          <p>Select a table to begin.</p>
        </div>
      </aside>
    );
  }

  if (creating) {
    return <DraftRecord table={table} />;
  }

  if (!bundle || selectedId === null) {
    return (
      <aside className="record-panel">
        <div className="empty-panel">
          <div className="empty-glyph">
            <Icon name="keyboard" size={22} />
          </div>
          <h3>No record selected</h3>
          <p>Click a row to open its form. Double-click a cell to edit in place.</p>
          <dl className="shortcuts">
            <div>
              <dt>⌘K</dt>
              <dd>Search</dd>
            </div>
            <div>
              <dt>⌘N</dt>
              <dd>New record</dd>
            </div>
            <div>
              <dt>⌘Z</dt>
              <dd>Undo</dd>
            </div>
          </dl>
        </div>
      </aside>
    );
  }

  const layout =
    schema?.layouts.find(
      (candidate) => candidate.id === activeLayoutId && candidate.table === table.name,
    ) ??
    schema?.layouts.find((candidate) => candidate.table === table.name) ??
    null;
  const items = layout ? orderedItems(layout.items) : defaultItems(table);

  const commit = async (column: string, value: Value): Promise<boolean> => {
    const ok = await useStore
      .getState()
      .updateCell(table.name, bundle.record.id, column, value);
    if (ok) setPulse(column);
    return ok;
  };

  return (
    <aside className="record-panel">
      <header className="record-header">
        <div>
          <span className="eyebrow">{table.name}</span>
          <h2>{recordTitle(table, bundle.record)}</h2>
        </div>
        <div className="record-header-actions">
          <button
            type="button"
            className="icon-button danger"
            title="Delete record"
            onClick={() => {
              if (window.confirm("Delete this record and its child rows?")) {
                void useStore.getState().deleteRecord(bundle.record.id);
              }
            }}
          >
            <Icon name="trash" size={15} />
          </button>
          <button
            type="button"
            className="icon-button"
            title="Close record"
            onClick={() => useStore.getState().closeRecord()}
          >
            <Icon name="x" size={15} />
          </button>
        </div>
      </header>

      <div className="record-scroll">
        <div className="record-form">
          {items.map((item) => {
            if (item.kind.kind === "section") {
              return (
                <div className="form-section" key={item.id}>
                  <span>{item.kind.title}</span>
                  <span className="form-rule" />
                </div>
              );
            }
            if (item.kind.kind === "label") {
              return (
                <div
                  className="form-item label-item"
                  key={item.id}
                  style={{ gridColumn: `span ${Math.max(1, item.w)}` }}
                >
                  {item.kind.text}
                </div>
              );
            }
            if (item.kind.kind === "field") {
              const columnName = item.kind.column;
              const column = table.columns.find(
                (candidate) => candidate.name === columnName,
              );
              if (!column) return null;
              const value = bundle.record.values[column.name] ?? "Null";
              return (
                <div
                  className="form-item"
                  key={item.id}
                  style={{ gridColumn: `span ${Math.max(1, item.w)}` }}
                >
                  <span className="field-label">
                    {column.name}
                    {isComputedColumn(column) && <span className="field-type">computed</span>}
                  </span>
                  <FieldInput
                    column={column}
                    value={value}
                    pulse={pulse === column.name}
                    onCommit={(next) => commit(column.name, next)}
                  />
                </div>
              );
            }
            if (item.kind.kind === "portal") {
              return (
                <Portal
                  key={item.id}
                  relationName={item.kind.relation}
                  columns={item.kind.columns}
                  parentId={bundle.record.id}
                  pulse={pulse}
                  setPulse={setPulse}
                />
              );
            }
            return null;
          })}
        </div>
      </div>
    </aside>
  );
}

interface PortalProps {
  relationName: string;
  columns: string[];
  parentId: number;
  pulse: string | null;
  setPulse: (key: string) => void;
}

function Portal({ relationName, columns, parentId, pulse, setPulse }: PortalProps) {
  const schema = useStore((state) => state.schema);
  const bundle = useStore((state) => state.bundle);
  const relation = schema?.relations.find((candidate) => candidate.name === relationName);
  const child = schema?.tables.find((candidate) => candidate.name === relation?.child_table);
  if (!relation || !child) return null;
  const visible: Column[] =
    columns.length > 0
      ? columns
          .map((name) => child.columns.find((column) => column.name === name))
          .filter((column): column is Column => column !== undefined)
      : child.columns.filter((column) => !isComputedColumn(column));
  const rows = bundle?.portals[relation.name] ?? [];

  const update = async (rowId: number, column: Column, value: Value) => {
    const key = `${child.name}:${rowId}:${column.name}`;
    const ok = await useStore.getState().updateCell(child.name, rowId, column.name, value);
    if (ok) setPulse(key);
    return ok;
  };

  const addRow = async () => {
    const values: Record<string, Value> = {
      [relation.child_column]: { Integer: parentId },
    };
    for (const column of child.columns) {
      if (column.name === relation.child_column || isComputedColumn(column)) continue;
      if (column.required) values[column.name] = fallbackValue(column);
    }
    await useStore.getState().insertChildRow(child.name, values);
  };

  return (
    <div className="form-portal">
      <div className="portal-header">
        <span>{relation.name}</span>
        <button type="button" className="text-button" onClick={() => void addRow()}>
          <Icon name="plus" size={13} /> Add line
        </button>
      </div>
      <div className="portal-table-wrap">
        <table className="portal-table">
          <thead>
            <tr>
              {visible.map((column) => (
                <th key={column.name}>
                  {column.name}
                  {isComputedColumn(column) && <span className="computed-mark"> ƒ</span>}
                </th>
              ))}
              <th className="portal-actions" aria-label="Row actions" />
            </tr>
          </thead>
          <tbody>
            {rows.length === 0 && (
              <tr>
                <td className="portal-empty" colSpan={visible.length + 1}>
                  No child rows yet.
                </td>
              </tr>
            )}
            {rows.map((row) => (
              <tr key={row.id}>
                {visible.map((column) => (
                  <td key={column.name}>
                    <FieldInput
                      compact
                      column={column}
                      value={row.values[column.name] ?? "Null"}
                      pulse={pulse === `${child.name}:${row.id}:${column.name}`}
                      onCommit={(value) => update(row.id, column, value)}
                    />
                  </td>
                ))}
                <td className="portal-actions">
                  <button
                    type="button"
                    className="icon-button tiny"
                    title="Delete line"
                    onClick={() => {
                      if (window.confirm("Delete this line?")) {
                        void useStore.getState().deleteChildRow(child.name, row.id);
                      }
                    }}
                  >
                    <Icon name="x" size={12} />
                  </button>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </div>
  );
}

function DraftRecord({ table }: { table: Table }) {
  const [values, setValues] = useState<Record<string, Value>>(() => {
    const initial: Record<string, Value> = {};
    for (const column of table.columns) {
      if (!isComputedColumn(column)) initial[column.name] = fallbackValue(column);
    }
    return initial;
  });
  const [saving, setSaving] = useState(false);
  const missing = table.columns.filter(
    (column) =>
      column.required &&
      !isComputedColumn(column) &&
      isBlankDraft(values[column.name] ?? "Null"),
  );

  const save = async () => {
    setSaving(true);
    const ok = await useStore.getState().insertRecord(values);
    setSaving(false);
    if (!ok) return;
  };

  return (
    <aside className="record-panel">
      <header className="record-header">
        <div>
          <span className="eyebrow">{table.name}</span>
          <h2>New record</h2>
        </div>
        <button
          type="button"
          className="icon-button"
          title="Cancel"
          onClick={() => useStore.getState().cancelCreate()}
        >
          <Icon name="x" size={15} />
        </button>
      </header>
      <div className="record-scroll">
        <div className="draft-grid">
          {table.columns
            .filter((column) => !isComputedColumn(column))
            .map((column) => (
              <label className="draft-field" key={column.name}>
                <DraftFieldLabel column={column} />
                <DraftField
                  column={column}
                  value={values[column.name] ?? null}
                  onChange={(value) =>
                    setValues((current) => ({ ...current, [column.name]: value }))
                  }
                />
              </label>
            ))}
        </div>
      </div>
      <footer className="record-footer">
        <button
          type="button"
          className="button ghost"
          onClick={() => useStore.getState().cancelCreate()}
        >
          Cancel
        </button>
        <span className="draft-required">
          {missing.length > 0
            ? `Required: ${missing.map((column) => column.name).join(", ")}`
            : ""}
        </span>
        <button
          type="button"
          className="button primary"
          disabled={saving || missing.length > 0}
          onClick={() => void save()}
        >
          {saving ? "Creating…" : "Create record"}
        </button>
      </footer>
    </aside>
  );
}

function isBlankDraft(value: Value): boolean {
  if (value === "Null") return true;
  if (typeof value === "string") return true;
  if ("Text" in value) return value.Text.trim() === "";
  if ("Select" in value) return value.Select.trim() === "";
  if ("AttachmentRef" in value) return value.AttachmentRef.trim() === "";
  return false;
}

function orderedItems(items: LayoutItem[]): LayoutItem[] {
  return [...items].sort((left, right) =>
    left.y === right.y ? left.x - right.x : left.y - right.y,
  );
}

function defaultItems(table: Table): LayoutItem[] {
  const items: LayoutItem[] = [
    {
      id: "auto_section",
      kind: { kind: "section", title: table.name },
      x: 0,
      y: 0,
      w: 12,
      h: 1,
    },
  ];
  let y = 1;
  table.columns.forEach((column, index) => {
    items.push({
      id: `auto_${column.id}`,
      kind: { kind: "field", column: column.name },
      x: index % 2 === 0 ? 0 : 6,
      y,
      w: 6,
      h: 2,
    });
    if (index % 2 === 1) y += 3;
  });
  return items;
}
