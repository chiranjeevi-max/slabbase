import { useEffect, useRef, useState } from "react";
import type {
  Aggregate,
  ColumnDraft,
  ColumnType,
  DeleteAction,
  RelationDraft,
  Schema,
  TableDraft,
} from "../bindings";
import { useFocusTrap } from "../focus";
import { usePresence } from "../presence";
import { useStore } from "../store";
import { Icon } from "./Icons";

type Tab = "table" | "column" | "relation";
type Kind =
  | "text"
  | "integer"
  | "decimal"
  | "currency"
  | "boolean"
  | "date"
  | "timestamp"
  | "select"
  | "attachmentRef"
  | "formula"
  | "lookup";

interface ColumnForm {
  name: string;
  kind: Kind;
  scale: string;
  currency: string;
  options: string;
  expression: string;
  relation: string;
  lookupColumn: string;
  aggregate: Aggregate;
  required: boolean;
  unique: boolean;
}

const blankColumn = (): ColumnForm => ({
  name: "",
  kind: "text",
  scale: "2",
  currency: "USD",
  options: "",
  expression: "",
  relation: "",
  lookupColumn: "",
  aggregate: "sum",
  required: false,
  unique: false,
});

export function SchemaDialog() {
  const open = useStore((state) => state.schemaOpen);
  const initialTab = useStore((state) => state.schemaTab);
  const schema = useStore((state) => state.schema);
  const activeTable = useStore((state) => state.activeTable);
  const targetTable = useStore((state) => state.schemaTargetTable);
  const setOpen = useStore((state) => state.setSchemaOpen);
  const createTable = useStore((state) => state.createTable);
  const addColumn = useStore((state) => state.addColumn);
  const createRelation = useStore((state) => state.createRelation);

  const [tab, setTab] = useState<Tab>(initialTab);
  const [busy, setBusy] = useState(false);

  const [tableName, setTableName] = useState("");
  const [tableColumns, setTableColumns] = useState<ColumnForm[]>([blankColumn()]);

  const [columnTable, setColumnTable] = useState(activeTable ?? "");
  const [columnForm, setColumnForm] = useState<ColumnForm>(blankColumn());

  const [relationName, setRelationName] = useState("");
  const [parentTable, setParentTable] = useState("");
  const [childTable, setChildTable] = useState("");
  const [childColumn, setChildColumn] = useState("");
  const [onDelete, setOnDelete] = useState<DeleteAction>("cascade");

  useEffect(() => {
    if (!open) return;
    setTab(initialTab);
    const parent = targetTable || activeTable || schema?.tables[0]?.name || "";
    setColumnTable(parent);
    if (schema && schema.tables.length >= 2) {
      setParentTable(parent);
      setChildTable(
        schema.tables.find((table) => table.name !== parent)?.name ??
          schema.tables[1]?.name ??
          "",
      );
      setChildColumn("");
    }
  }, [open, initialTab, activeTable, schema, targetTable]);

  const modalRef = useRef<HTMLDivElement>(null);
  const presence = usePresence(open, 110);
  useFocusTrap(open, modalRef, presence.mounted);

  if (!presence.mounted) return null;

  const tables = schema?.tables ?? [];
  const childDefinition = tables.find((table) => table.name === childTable);
  const integerColumns =
    childDefinition?.columns.filter((column) => column.ty.kind === "integer") ?? [];

  const submitTable = async () => {
    if (!tableName.trim()) return;
    const draft: TableDraft = {
      name: tableName.trim(),
      columns: tableColumns
        .filter((column) => column.name.trim())
        .map(columnDraftFromForm),
    };
    setBusy(true);
    const ok = await createTable(draft);
    setBusy(false);
    if (ok) {
      setTableName("");
      setTableColumns([blankColumn()]);
      setOpen(false);
    }
  };

  const submitColumn = async () => {
    if (!columnTable || !columnForm.name.trim()) return;
    setBusy(true);
    const ok = await addColumn(columnTable, columnDraftFromForm(columnForm));
    setBusy(false);
    if (ok) {
      setColumnForm(blankColumn());
      setOpen(false);
    }
  };

  const submitRelation = async () => {
    if (!parentTable || !childTable || !childColumn) return;
    const draft: RelationDraft = {
      name: relationName.trim() || `${parentTable} ${childTable}`,
      parent_table: parentTable,
      child_table: childTable,
      child_column: childColumn,
      on_delete: onDelete,
    };
    setBusy(true);
    const ok = await createRelation(draft);
    setBusy(false);
    if (ok) {
      setRelationName("");
      setOpen(false);
    }
  };

  return (
    <div
      className={`modal-backdrop ${presence.closing ? "is-closing" : ""}`}
      onMouseDown={(event) => {
        if (event.target === event.currentTarget) setOpen(false);
      }}
    >
      <div
        ref={modalRef}
        className={`modal schema-modal ${presence.className}`}
        role="dialog"
        aria-modal="true"
        aria-label="Schema"
        data-testid="schema-dialog"
        tabIndex={-1}
      >
        <header className="modal-header">
          <div className="modal-title">
            <span className="eyebrow">Schema</span>
            <h2>{tab === "table" ? "New Table" : tab === "column" ? "Add Column" : "New Relation"}</h2>
          </div>
          <button
            type="button"
            className="icon-control"
            onClick={() => setOpen(false)}
            aria-label="Close"
          >
            <Icon name="x" size={14} strokeWidth={1.5} />
          </button>
        </header>
        <div className="tabs" role="tablist" aria-label="Schema">
          {(
            [
              ["table", "New Table"],
              ["column", "Add Column"],
              ["relation", "New Relation"],
            ] as const
          ).map(([value, label]) => (
            <button
              type="button"
              role="tab"
              aria-selected={tab === value}
              key={value}
              className={`tab ${tab === value ? "active" : ""}`}
              onClick={() => setTab(value)}
            >
              {label}
            </button>
          ))}
        </div>
        <div className="modal-body">
          {tab === "table" && (
            <form
              className="schema-form"
              onSubmit={(event) => {
                event.preventDefault();
                void submitTable();
              }}
            >
              <label className="field-label">
                Table name
                <input
                  className="field-input"
                  value={tableName}
                  placeholder="Invoice Line Items"
                  onChange={(event) => setTableName(event.target.value)}
                  autoFocus
                />
              </label>
              <div className="column-builder">
                <div className="column-builder-head">
                  <span>Columns</span>
                  <button
                    type="button"
                    className="text-button"
                    onClick={() => setTableColumns((current) => [...current, blankColumn()])}
                  >
                    <Icon name="plus" size={13} /> Add column
                  </button>
                </div>
                {tableColumns.map((column, index) => (
                  <div className="column-row" key={`${index}-${column.kind}`}>
                    <input
                      className="field-input"
                      value={column.name}
                      placeholder="Column name"
                      onChange={(event) =>
                        setTableColumns((current) =>
                          current.map((candidate, position) =>
                            position === index
                              ? { ...candidate, name: event.target.value }
                              : candidate,
                          ),
                        )
                      }
                    />
                    <select
                      className="select type-select"
                      value={column.kind}
                      onChange={(event) =>
                        setTableColumns((current) =>
                          current.map((candidate, position) =>
                            position === index
                              ? { ...candidate, kind: event.target.value as Kind }
                              : candidate,
                          ),
                        )
                      }
                    >
                      {TYPE_OPTIONS}
                    </select>
                    <ColumnOptions
                      form={column}
                      schema={schema}
                      parentTable={tableName}
                      onChange={(patch) =>
                        setTableColumns((current) =>
                          current.map((candidate, position) =>
                            position === index ? { ...candidate, ...patch } : candidate,
                          ),
                        )
                      }
                    />
                    <label className="inline-check">
                      <input
                        type="checkbox"
                        checked={column.required}
                        onChange={(event) =>
                          setTableColumns((current) =>
                            current.map((candidate, position) =>
                              position === index
                                ? { ...candidate, required: event.target.checked }
                                : candidate,
                            ),
                          )
                        }
                      />
                      Required
                    </label>
                    <label className="inline-check">
                      <input
                        type="checkbox"
                        checked={column.unique}
                        onChange={(event) =>
                          setTableColumns((current) =>
                            current.map((candidate, position) =>
                              position === index
                                ? { ...candidate, unique: event.target.checked }
                                : candidate,
                            ),
                          )
                        }
                      />
                      Unique
                    </label>
                    {tableColumns.length > 1 && (
                      <button
                        type="button"
                        className="icon-button tiny"
                        title="Remove column"
                        aria-label="Remove column"
                        onClick={() =>
                          setTableColumns((current) =>
                            current.filter((_, position) => position !== index),
                          )
                        }
                      >
                        <Icon name="x" size={12} />
                      </button>
                    )}
                  </div>
                ))}
              </div>
              <footer className="modal-footer">
                <button type="button" className="button" onClick={() => setOpen(false)}>
                  Cancel
                </button>
                <button type="submit" className="button primary" disabled={busy}>
                  {busy ? "Creating…" : "Create table"}
                </button>
              </footer>
            </form>
          )}

          {tab === "column" && (
            <form
              className="schema-form"
              onSubmit={(event) => {
                event.preventDefault();
                void submitColumn();
              }}
            >
              <label className="field-label">
                Table
                <select
                  className="select"
                  value={columnTable}
                  onChange={(event) => setColumnTable(event.target.value)}
                >
                  {tables.map((table) => (
                    <option key={table.name} value={table.name}>
                      {table.name}
                    </option>
                  ))}
                </select>
              </label>
              <label className="field-label">
                Column name
                <input
                  className="field-input"
                  value={columnForm.name}
                  onChange={(event) =>
                    setColumnForm({ ...columnForm, name: event.target.value })
                  }
                  autoFocus
                />
              </label>
              <div className="schema-inline">
                <label className="field-label">
                  Type
                  <select
                    className="select"
                    value={columnForm.kind}
                    onChange={(event) =>
                      setColumnForm({ ...columnForm, kind: event.target.value as Kind })
                    }
                  >
                    {TYPE_OPTIONS}
                  </select>
                </label>
                <ColumnOptions
                  form={columnForm}
                  schema={schema}
                  parentTable={columnTable}
                  onChange={(patch) => setColumnForm({ ...columnForm, ...patch })}
                />
              </div>
              <div className="schema-inline">
                <label className="inline-check">
                  <input
                    type="checkbox"
                    checked={columnForm.required}
                    onChange={(event) =>
                      setColumnForm({ ...columnForm, required: event.target.checked })
                    }
                  />
                  Required
                </label>
                <label className="inline-check">
                  <input
                    type="checkbox"
                    checked={columnForm.unique}
                    onChange={(event) =>
                      setColumnForm({ ...columnForm, unique: event.target.checked })
                    }
                  />
                  Unique
                </label>
              </div>
              <footer className="modal-footer">
                <button type="button" className="button" onClick={() => setOpen(false)}>
                  Cancel
                </button>
                <button type="submit" className="button primary" disabled={busy}>
                  {busy ? "Adding…" : "Add column"}
                </button>
              </footer>
            </form>
          )}

          {tab === "relation" && (
            <form
              className="schema-form"
              onSubmit={(event) => {
                event.preventDefault();
                void submitRelation();
              }}
            >
              <p className="form-note">
                A relation adds a real SQLite foreign key on the child column. The column must
                already exist and hold integers.
              </p>
              <label className="field-label">
                Name
                <input
                  className="field-input"
                  value={relationName}
                  placeholder={`${parentTable || "Parent"} ${childTable || "Children"}`}
                  onChange={(event) => setRelationName(event.target.value)}
                />
              </label>
              <div className="schema-inline">
                <label className="field-label">
                  Parent
                  <select
                    className="select"
                    value={parentTable}
                    onChange={(event) => setParentTable(event.target.value)}
                  >
                    {tables.map((table) => (
                      <option key={table.name} value={table.name}>
                        {table.name}
                      </option>
                    ))}
                  </select>
                </label>
                <label className="field-label">
                  Child
                  <select
                    className="select"
                    value={childTable}
                    onChange={(event) => {
                      setChildTable(event.target.value);
                      setChildColumn("");
                    }}
                  >
                    {tables.map((table) => (
                      <option key={table.name} value={table.name}>
                        {table.name}
                      </option>
                    ))}
                  </select>
                </label>
              </div>
              <label className="field-label">
                Child column
                <select
                  className="select"
                  value={childColumn}
                  onChange={(event) => setChildColumn(event.target.value)}
                >
                  <option value="">Choose an integer column</option>
                  {integerColumns.map((column) => (
                    <option key={column.name} value={column.name}>
                      {column.name}
                    </option>
                  ))}
                </select>
              </label>
              <label className="field-label">
                On delete
                <select
                  className="select"
                  value={onDelete}
                  onChange={(event) => setOnDelete(event.target.value as DeleteAction)}
                >
                  <option value="cascade">Cascade child rows</option>
                  <option value="restrict">Restrict parent deletion</option>
                </select>
              </label>
              <footer className="modal-footer">
                <button type="button" className="button" onClick={() => setOpen(false)}>
                  Cancel
                </button>
                <button
                  type="submit"
                  className="button primary"
                  disabled={busy || !childColumn}
                >
                  {busy ? "Creating…" : "Create relation"}
                </button>
              </footer>
            </form>
          )}
        </div>
      </div>
    </div>
  );
}

const TYPE_OPTIONS = (
  <>
    <option value="text">Text</option>
    <option value="integer">Integer</option>
    <option value="decimal">Decimal</option>
    <option value="currency">Currency</option>
    <option value="boolean">Boolean</option>
    <option value="date">Date</option>
    <option value="timestamp">Timestamp</option>
    <option value="select">Select</option>
    <option value="attachmentRef">Attachment</option>
    <option value="formula">Formula</option>
    <option value="lookup">Rollup</option>
  </>
);

function ColumnOptions({
  form,
  schema,
  parentTable,
  onChange,
}: {
  form: ColumnForm;
  schema: Schema | null;
  parentTable: string;
  onChange: (patch: Partial<ColumnForm>) => void;
}) {
  if (form.kind === "decimal") {
    return (
      <label className="compact-field">
        <span>Scale</span>
        <input
          className="field-input"
          type="number"
          min={0}
          max={9}
          value={form.scale}
          onChange={(event) => onChange({ scale: event.target.value })}
        />
      </label>
    );
  }
  if (form.kind === "currency") {
    return (
      <label className="compact-field">
        <span>Code</span>
        <input
          className="field-input"
          maxLength={3}
          value={form.currency}
          onChange={(event) => onChange({ currency: event.target.value.toUpperCase() })}
        />
      </label>
    );
  }
  if (form.kind === "select") {
    return (
      <label className="compact-field grow">
        <span>Options</span>
        <input
          className="field-input"
          placeholder="Draft, Sent, Paid"
          value={form.options}
          onChange={(event) => onChange({ options: event.target.value })}
        />
      </label>
    );
  }
  if (form.kind === "formula") {
    return (
      <label className="compact-field grow">
        <span>Expression</span>
        <input
          className="field-input"
          placeholder="ROUND([Quantity] * [Unit Price], 2)"
          value={form.expression}
          onChange={(event) => onChange({ expression: event.target.value })}
        />
      </label>
    );
  }
  if (form.kind === "lookup") {
    const availableRelations =
      schema?.relations.filter((candidate) => candidate.parent_table === parentTable) ?? [];
    const relation = availableRelations.find((candidate) => candidate.name === form.relation);
    const child = schema?.tables.find((candidate) => candidate.name === relation?.child_table);
    return (
      <>
        <label className="compact-field">
          <span>Relation</span>
          <select
            className="select"
            value={form.relation}
            onChange={(event) => onChange({ relation: event.target.value, lookupColumn: "" })}
          >
            <option value="">Choose relation</option>
            {availableRelations.map((candidate) => (
              <option key={candidate.name} value={candidate.name}>
                {candidate.name}
              </option>
            ))}
          </select>
        </label>
        <label className="compact-field">
          <span>Column</span>
          <select
            className="select"
            value={form.lookupColumn}
            onChange={(event) => onChange({ lookupColumn: event.target.value })}
          >
            <option value="">Choose column</option>
            {child?.columns
              .filter((column) => column.ty.kind !== "formula")
              .map((column) => (
                <option key={column.name} value={column.name}>
                  {column.name}
                </option>
              ))}
          </select>
        </label>
        <label className="compact-field">
          <span>Aggregate</span>
          <select
            className="select"
            value={form.aggregate}
            onChange={(event) => onChange({ aggregate: event.target.value as Aggregate })}
          >
            <option value="sum">Sum</option>
            <option value="count">Count</option>
            <option value="avg">Average</option>
            <option value="min">Minimum</option>
            <option value="max">Maximum</option>
          </select>
        </label>
      </>
    );
  }
  return null;
}

function columnDraftFromForm(form: ColumnForm): ColumnDraft {
  return {
    name: form.name.trim(),
    ty: columnTypeFromForm(form),
    required: form.required,
    unique: form.unique,
    default: null,
  };
}

function columnTypeFromForm(form: ColumnForm): ColumnType {
  switch (form.kind) {
    case "text":
      return { kind: "text" };
    case "integer":
      return { kind: "integer" };
    case "decimal":
      return { kind: "decimal", scale: Number(form.scale) };
    case "currency":
      return { kind: "currency", currency: form.currency || "USD" };
    case "boolean":
      return { kind: "boolean" };
    case "date":
      return { kind: "date" };
    case "timestamp":
      return { kind: "timestamp" };
    case "select":
      return {
        kind: "select",
        options: form.options
          .split(",")
          .map((option) => option.trim())
          .filter(Boolean),
      };
    case "attachmentRef":
      return { kind: "attachmentRef" };
    case "formula":
      return { kind: "formula", expression: form.expression };
    case "lookup":
      return {
        kind: "lookup",
        relation: form.relation,
        column: form.lookupColumn,
        aggregate: form.aggregate,
      };
  }
}
