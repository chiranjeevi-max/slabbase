import type { Dispatch, SetStateAction } from "react";
import type { Aggregate, DeleteAction, Schema } from "../bindings";
import { Icon } from "./Icons";

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

export interface ColumnForm {
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

export function CreateTableForm({
  schema,
  tableName,
  setTableName,
  tableColumns,
  blankColumn,
  setTableColumns,
  busy,
  setOpen,
  submitTable,
}: {
  schema: Schema | null;
  tableName: string;
  setTableName: (name: string) => void;
  tableColumns: ColumnForm[];
  blankColumn: () => ColumnForm;
  setTableColumns: Dispatch<SetStateAction<ColumnForm[]>>;
  busy: boolean;
  setOpen: (open: boolean) => void;
  submitTable: () => Promise<void>;
}) {
  const patchColumn = (index: number, patch: Partial<ColumnForm>) => {
    setTableColumns((current) =>
      current.map((column, position) => (position === index ? { ...column, ...patch } : column)),
    );
  };

  return (
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
                patchColumn(index, { name: event.target.value })
              }
            />
            <select
              className="select type-select"
              value={column.kind}
              onChange={(event) =>
                patchColumn(index, { kind: event.target.value as Kind })
              }
            >
              {TYPE_OPTIONS}
            </select>
            <ColumnOptions
              form={column}
              schema={schema}
              parentTable={tableName}
              onChange={(patch) =>
                patchColumn(index, patch)
              }
            />
            <label className="inline-check">
              <input
                type="checkbox"
                checked={column.required}
                onChange={(event) =>
                  patchColumn(index, { required: event.target.checked })
                }
              />
              Required
            </label>
            <label className="inline-check">
              <input
                type="checkbox"
                checked={column.unique}
                onChange={(event) =>
                  patchColumn(index, { unique: event.target.checked })
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
      <SchemaFormFooter
        onCancel={() => setOpen(false)}
        disabled={busy}
        label={busy ? "Creating…" : "Create table"}
      />
    </form>
  );
}

export function AddColumnForm({
  schema,
  tables,
  columnTable,
  setColumnTable,
  columnForm,
  setColumnForm,
  busy,
  setOpen,
  submitColumn,
}: {
  schema: Schema | null;
  tables: Schema["tables"];
  columnTable: string;
  setColumnTable: (name: string) => void;
  columnForm: ColumnForm;
  setColumnForm: (form: ColumnForm) => void;
  busy: boolean;
  setOpen: (open: boolean) => void;
  submitColumn: () => Promise<void>;
}) {
  return (
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
      <SchemaFormFooter
        onCancel={() => setOpen(false)}
        disabled={busy}
        label={busy ? "Adding…" : "Add column"}
      />
    </form>
  );
}

export function CreateRelationForm({
  tables,
  relationName,
  setRelationName,
  parentTable,
  setParentTable,
  childTable,
  setChildTable,
  childColumn,
  setChildColumn,
  onDelete,
  setOnDelete,
  integerColumns,
  busy,
  setOpen,
  submitRelation,
}: {
  tables: Schema["tables"];
  relationName: string;
  setRelationName: (name: string) => void;
  parentTable: string;
  setParentTable: (name: string) => void;
  childTable: string;
  setChildTable: (name: string) => void;
  childColumn: string;
  setChildColumn: (name: string) => void;
  onDelete: DeleteAction;
  setOnDelete: (action: DeleteAction) => void;
  integerColumns: Schema["tables"][number]["columns"];
  busy: boolean;
  setOpen: (open: boolean) => void;
  submitRelation: () => Promise<void>;
}) {
  return (
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
      <SchemaFormFooter
        onCancel={() => setOpen(false)}
        disabled={busy || !childColumn}
        label={busy ? "Creating…" : "Create relation"}
      />
    </form>
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

function SchemaFormFooter({
  onCancel,
  disabled,
  label,
}: {
  onCancel: () => void;
  disabled: boolean;
  label: string;
}) {
  return (
    <footer className="modal-footer">
      <button type="button" className="button" onClick={onCancel}>
        Cancel
      </button>
      <button type="submit" className="button primary" disabled={disabled}>
        {label}
      </button>
    </footer>
  );
}
