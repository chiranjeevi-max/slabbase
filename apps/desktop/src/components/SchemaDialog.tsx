import { useEffect, useRef, useState } from "react";
import type {
  ColumnDraft,
  ColumnType,
  DeleteAction,
  RelationDraft,
  TableDraft,
} from "../bindings";
import { useFocusTrap } from "../focus";
import { usePresence } from "../presence";
import { useStore } from "../store";
import { Icon } from "./Icons";

import { AddColumnForm, CreateRelationForm, CreateTableForm, type ColumnForm } from "./SchemaForms";

type Tab = "table" | "column" | "relation";
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
        <SchemaDialogHeader
          tab={tab}
          setTab={setTab}
          setOpen={setOpen}
        />
        <div className="modal-body">
          {tab === "table" && (
            <CreateTableForm
              schema={schema}
              tableName={tableName}
              setTableName={setTableName}
              tableColumns={tableColumns}
              blankColumn={blankColumn}
              setTableColumns={setTableColumns}
              busy={busy}
              setOpen={setOpen}
              submitTable={submitTable}
            />
          )}

          {tab === "column" && (
            <AddColumnForm
              schema={schema}
              tables={tables}
              columnTable={columnTable}
              setColumnTable={setColumnTable}
              columnForm={columnForm}
              setColumnForm={setColumnForm}
              busy={busy}
              setOpen={setOpen}
              submitColumn={submitColumn}
            />
          )}

          {tab === "relation" && (
            <CreateRelationForm
              tables={tables}
              relationName={relationName}
              setRelationName={setRelationName}
              parentTable={parentTable}
              setParentTable={setParentTable}
              childTable={childTable}
              setChildTable={setChildTable}
              childColumn={childColumn}
              setChildColumn={setChildColumn}
              onDelete={onDelete}
              setOnDelete={setOnDelete}
              integerColumns={integerColumns}
              busy={busy}
              setOpen={setOpen}
              submitRelation={submitRelation}
            />
          )}
        </div>
      </div>
    </div>
  );
}

function SchemaDialogHeader({
  tab,
  setTab,
  setOpen,
}: {
  tab: Tab;
  setTab: (tab: Tab) => void;
  setOpen: (open: boolean) => void;
}) {
  return (
    <>
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
    </>
  );
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
