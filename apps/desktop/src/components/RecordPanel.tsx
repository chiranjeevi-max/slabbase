import { useEffect, useState, type CSSProperties } from "react";
import type { Column, LayoutItem, Table, Value } from "../bindings";
import { useStore } from "../store";
import {
  isComputedColumn,
  isLongField,
  isMultilineField,
  recordTitle,
} from "../types";
import { confirmDestructive } from "../native";
import { FieldInput } from "./FieldInput";
import { PortalGrid } from "./PortalGrid";
import { DraftField, DraftFieldLabel, fallbackValue } from "./DraftFields";
import { Icon } from "./Icons";

export function RecordPanel({
  width,
  closing = false,
}: {
  width: number;
  closing?: boolean;
}) {
  const schema = useStore((state) => state.schema);
  const activeTable = useStore((state) => state.activeTable);
  const selectedId = useStore((state) => state.selectedId);
  const bundle = useStore((state) => state.bundle);
  const creating = useStore((state) => state.creating);
  const activeLayoutId = useStore((state) => state.activeLayoutId);
  const [pulse, setPulse] = useState<string | null>(null);
  const panelStyle = { "--inspector-w": `${width}px` } as CSSProperties;

  const table = schema?.tables.find((candidate) => candidate.name === activeTable);

  useEffect(() => {
    if (!pulse) return;
    const timer = window.setTimeout(() => setPulse(null), 480);
    return () => window.clearTimeout(timer);
  }, [pulse]);

  if (!table) {
    return (
      <aside className={`record-panel ${closing ? "is-closing" : ""}`} data-testid="inspector" style={panelStyle}>
        <header className="record-header">
          <div>
            <span className="eyebrow">Inspector</span>
            <h2>No table</h2>
          </div>
          <button
            type="button"
            className="icon-control"
            title="Close inspector"
            aria-label="Close inspector"
            onClick={() => useStore.getState().setInspectorOpen(false)}
          >
            <Icon name="x" size={14} strokeWidth={1.5} />
          </button>
        </header>
        <p className="inspector-hint">Select a table to inspect its records.</p>
      </aside>
    );
  }

  if (creating) {
    return <DraftRecord table={table} width={width} closing={closing} />;
  }

  if (!bundle || selectedId === null) {
    return (
      <aside className={`record-panel ${closing ? "is-closing" : ""}`} data-testid="inspector" style={panelStyle}>
        <header className="record-header">
          <div>
            <span className="eyebrow">{table.name}</span>
            <h2>Inspector</h2>
          </div>
          <button
            type="button"
            className="icon-control"
            title="Close inspector"
            aria-label="Close inspector"
            onClick={() => useStore.getState().setInspectorOpen(false)}
          >
            <Icon name="x" size={14} strokeWidth={1.5} />
          </button>
        </header>
        <p className="inspector-hint">Select a record to inspect its fields and portals.</p>
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
  const inspector = inspectorLayout(items, table);
  const narrow = width < 340;

  const commit = async (column: string, value: Value): Promise<boolean> => {
    const ok = await useStore
      .getState()
      .updateCell(table.name, bundle.record.id, column, value);
    if (ok) setPulse(column);
    return ok;
  };

  return (
    <aside className={`record-panel ${closing ? "is-closing" : ""}`} data-testid="inspector" style={panelStyle} aria-label="Record inspector">
      <header className="record-header">
        <div className="record-heading">
          <span className="eyebrow">
            {table.name}
            <span className="record-id">#{bundle.record.id}</span>
          </span>
          <h2>{recordTitle(table, bundle.record)}</h2>
        </div>
        <div className="record-header-actions">
          <button
            type="button"
            className="icon-control danger"
            title="Delete record"
            aria-label="Delete record"
            onClick={() => {
              void (async () => {
                const ok = await confirmDestructive(
                  "Delete this record and its child rows? Undo can restore it.",
                  "Delete Record",
                  "Delete",
                );
                if (ok) await useStore.getState().deleteRecord(bundle.record.id);
              })();
            }}
          >
            <Icon name="trash" size={14} strokeWidth={1.5} />
          </button>
          <button
            type="button"
            className="icon-control"
            title="Close inspector"
            aria-label="Close inspector"
            onClick={() => useStore.getState().setInspectorOpen(false)}
          >
            <Icon name="x" size={14} strokeWidth={1.5} />
          </button>
        </div>
      </header>

      <div className="record-scroll">
        <div className={`record-form ${narrow ? "narrow" : ""}`}>
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
                <div className="form-item label-item" key={item.id}>
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
              const hideLabel = inspector.hiddenLabels.has(item.id);
              return (
                <div
                  className={`form-item ${
                    inspector.fullWidth.has(item.id) ? "long-field" : ""
                  } ${hideLabel ? "no-label" : ""}`}
                  key={item.id}
                >
                  {!hideLabel && (
                    <span className="field-label">
                      {column.name}
                      {isComputedColumn(column) && (
                        <span className="field-type">computed</span>
                      )}
                    </span>
                  )}
                  <FieldInput
                    column={column}
                    value={value}
                    multiline={isMultilineField(column)}
                    pulse={pulse === column.name}
                    onCommit={(next) => commit(column.name, next)}
                  />
                </div>
              );
            }
            if (item.kind.kind === "portal") {
              return (
                <PortalGrid
                  key={item.id}
                  relationName={item.kind.relation}
                  columns={item.kind.columns}
                  parentId={bundle.record.id}
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

function DraftRecord({
  table,
  width,
  closing,
}: {
  table: Table;
  width: number;
  closing: boolean;
}) {
  const [values, setValues] = useState<Record<string, Value>>(() => {
    const initial: Record<string, Value> = {};
    for (const column of table.columns) {
      if (!isComputedColumn(column)) initial[column.name] = fallbackValue(column);
    }
    return initial;
  });
  const [saving, setSaving] = useState(false);
  const panelStyle = { "--inspector-w": `${width}px` } as CSSProperties;
  const columns = table.columns.filter((column) => !isComputedColumn(column));
  const fullWidth = draftFullWidth(columns);
  const narrow = width < 340;
  const missing = table.columns.filter(
    (column) =>
      column.required &&
      !isComputedColumn(column) &&
      isBlankDraft(values[column.name] ?? "Null"),
  );

  const save = async () => {
    setSaving(true);
    await useStore.getState().insertRecord(values);
    setSaving(false);
  };

  return (
    <aside className={`record-panel ${closing ? "is-closing" : ""}`} data-testid="inspector" style={panelStyle} aria-label="New record">
      <header className="record-header">
        <div className="record-heading">
          <span className="eyebrow">{table.name}</span>
          <h2>New Record</h2>
        </div>
        <button
          type="button"
          className="icon-control"
          title="Cancel"
          aria-label="Cancel"
          onClick={() => useStore.getState().cancelCreate()}
        >
          <Icon name="x" size={14} strokeWidth={1.5} />
        </button>
      </header>
      <div className="record-scroll">
        <div className={`draft-grid ${narrow ? "narrow" : ""}`}>
          {columns.map((column) => (
            <label
              className={`draft-field ${fullWidth.has(column.id) ? "long-field" : ""}`}
              key={column.name}
            >
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
          className="button compact"
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
          className="button primary compact"
          disabled={saving || missing.length > 0}
          onClick={() => void save()}
        >
          {saving ? "Creating…" : "Create Record"}
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

function inspectorLayout(
  items: LayoutItem[],
  table: Table,
): { fullWidth: Set<string>; hiddenLabels: Set<string> } {
  const fullWidth = new Set<string>();
  const hiddenLabels = new Set<string>();
  let section: LayoutItem | null = null;
  let fieldItems: LayoutItem[] = [];
  let shortRun: LayoutItem[] = [];

  const flushRun = () => {
    if (shortRun.length % 2 === 1) {
      const last = shortRun[shortRun.length - 1];
      if (last) fullWidth.add(last.id);
    }
    shortRun = [];
  };

  const closeSection = () => {
    if (section?.kind.kind === "section" && fieldItems.length === 1) {
      const field = fieldItems[0];
      if (
        field?.kind.kind === "field" &&
        field.kind.column.trim().toLowerCase() ===
          section.kind.title.trim().toLowerCase()
      ) {
        hiddenLabels.add(field.id);
      }
    }
    flushRun();
    fieldItems = [];
  };

  for (const item of items) {
    if (item.kind.kind === "section") {
      closeSection();
      section = item;
      continue;
    }
    if (item.kind.kind === "field") {
      fieldItems.push(item);
      const columnName = item.kind.column;
      const column = table.columns.find((candidate) => candidate.name === columnName);
      if (!column) continue;
      if (isLongField(column)) {
        flushRun();
        fullWidth.add(item.id);
      } else {
        shortRun.push(item);
      }
      continue;
    }
    flushRun();
  }
  closeSection();
  return { fullWidth, hiddenLabels };
}

function draftFullWidth(columns: Column[]): Set<string> {
  const fullWidth = new Set<string>();
  let shortRun: Column[] = [];
  const flushRun = () => {
    if (shortRun.length % 2 === 1) {
      const last = shortRun[shortRun.length - 1];
      if (last) fullWidth.add(last.id);
    }
    shortRun = [];
  };
  for (const column of columns) {
    if (isLongField(column)) {
      flushRun();
      fullWidth.add(column.id);
    } else {
      shortRun.push(column);
    }
  }
  flushRun();
  return fullWidth;
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
