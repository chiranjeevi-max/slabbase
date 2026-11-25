import type {
  AuditEntry,
  Column,
  IntegrityReportDto,
  Layout,
  Page,
  PageQuery,
  PortalQuery,
  Record as SlabRecord,
  RecordBundle,
  Relation,
  RelationDraft,
  Schema,
  SearchHit,
  SortSpec,
  UndoState,
  Value,
  WorkspaceInfoDto,
} from "./bindings";
import {
  DESIGN_STUDIO_AUDIT,
  DESIGN_STUDIO_INFO,
  DESIGN_STUDIO_ROWS,
  DESIGN_STUDIO_SCHEMA,
} from "./fixtures/designStudio";
import { valueText } from "./types";

export type FixtureMode = "full" | "empty" | "stress";

interface FixtureSnapshot {
  schema: Schema;
  rows: Record<string, SlabRecord[]>;
  layouts: Layout[];
}

interface FixtureState {
  mode: FixtureMode;
  schema: Schema;
  rows: Record<string, SlabRecord[]>;
  info: WorkspaceInfoDto;
  layouts: Layout[];
  audit: AuditEntry[];
  undo: UndoState;
  history: FixtureSnapshot[];
  future: FixtureSnapshot[];
}

const EMPTY_INFO: WorkspaceInfoDto = {
  name: "Untitled",
  path: null,
  formatVersion: 1,
  journalMode: "wal",
  tables: [],
};

function cloneRows(): Record<string, SlabRecord[]> {
  const rows: Record<string, SlabRecord[]> = {};
  for (const [table, tableRows] of Object.entries(DESIGN_STUDIO_ROWS)) {
    rows[table] = tableRows.map((row) => ({
      id: row.id,
      values: { ...row.values },
    }));
  }
  return rows;
}

function initialState(mode: FixtureMode): FixtureState {
  const schema: Schema = {
    tables: DESIGN_STUDIO_SCHEMA.tables.map((table) => ({
      ...table,
      columns: table.columns.map((column) => ({ ...column })),
    })),
    relations: DESIGN_STUDIO_SCHEMA.relations.map((relation) => ({ ...relation })),
    layouts: DESIGN_STUDIO_SCHEMA.layouts.map((layout) => ({ ...layout })),
  };
  return {
    mode,
    schema,
    rows: cloneRows(),
    info: DESIGN_STUDIO_INFO,
    layouts: schema.layouts,
    audit: DESIGN_STUDIO_AUDIT,
    undo: {
      can_undo: true,
      can_redo: false,
      undo_label: "Update row",
      redo_label: null,
      depth: 3,
    },
    history: [],
    future: [],
  };
}

function stressSchema(): Schema {
  const columns = Array.from({ length: 20 }, (_, index) => ({
    id: `c_wide_${index}`,
    table: "Wide",
    name: `Column ${String(index + 1).padStart(2, "0")}`,
    ty: { kind: "text" } as const,
    required: false,
    unique: false,
    default: null,
    position: index,
  }));
  return {
    tables: [
      {
        id: "t_wide",
        name: "Wide",
        columns,
        created_at: 0,
      },
      {
        id: "t_currencies",
        name: "Currencies",
        columns: [
          {
            id: "c_currency_label",
            table: "Currencies",
            name: "Label",
            ty: { kind: "text" },
            required: false,
            unique: false,
            default: null,
            position: 0,
          },
          {
            id: "c_currency_jpy",
            table: "Currencies",
            name: "JPY",
            ty: { kind: "currency", currency: "JPY" },
            required: false,
            unique: false,
            default: null,
            position: 1,
          },
          {
            id: "c_currency_usd",
            table: "Currencies",
            name: "USD",
            ty: { kind: "currency", currency: "USD" },
            required: false,
            unique: false,
            default: null,
            position: 2,
          },
          {
            id: "c_currency_kwd",
            table: "Currencies",
            name: "KWD",
            ty: { kind: "currency", currency: "KWD" },
            required: false,
            unique: false,
            default: null,
            position: 3,
          },
        ],
        created_at: 0,
      },
    ],
    relations: [],
    layouts: [],
  };
}

function stressState(): FixtureState {
  const schema = stressSchema();
  const long = "x".repeat(500);
  const rows: Record<string, SlabRecord[]> = { Wide: [], Currencies: [] };
  for (let index = 0; index < 120; index += 1) {
    const values: SlabRecord["values"] = {};
    for (let column = 1; column <= 20; column += 1) {
      values[`Column ${String(column).padStart(2, "0")}`] = {
        Text: `Row ${index + 1} · ${long}`,
      };
    }
    rows["Wide"]?.push({ id: index + 1, values });
  }
  rows["Currencies"]?.push({
    id: 1,
    values: {
      Label: { Text: "Three scales" },
      JPY: { Currency: { currency: "JPY", amount: "1500" } },
      USD: { Currency: { currency: "USD", amount: "19.99" } },
      KWD: { Currency: { currency: "KWD", amount: "1.234" } },
    },
  });
  return {
    mode: "stress",
    schema,
    rows,
    info: {
      name: "Stress",
      path: null,
      formatVersion: 1,
      journalMode: "wal",
      tables: schema.tables.map((table) => ({
        name: table.name,
        rowCount: rows[table.name]?.length ?? 0,
      })),
    },
    layouts: [],
    audit: [],
    undo: {
      can_undo: false,
      can_redo: false,
      undo_label: null,
      redo_label: null,
      depth: 0,
    },
    history: [],
    future: [],
  };
}

function snapshot(state: FixtureState): FixtureSnapshot {
  return structuredClone({
    schema: state.schema,
    rows: state.rows,
    layouts: state.layouts,
  });
}

function pushHistory(state: FixtureState, label: string): void {
  state.history.push(snapshot(state));
  if (state.history.length > 50) state.history.shift();
  state.future = [];
  state.undo = {
    can_undo: true,
    can_redo: false,
    undo_label: label,
    redo_label: null,
    depth: state.history.length,
  };
}

function restore(state: FixtureState, snap: FixtureSnapshot): void {
  state.schema = snap.schema;
  state.rows = snap.rows;
  state.layouts = snap.layouts;
  state.schema.layouts = snap.layouts;
}

function fixtureError(code: string, message: string): never {
  throw { code, message };
}

function currentInfo(state: FixtureState): WorkspaceInfoDto {
  if (state.mode === "empty") return EMPTY_INFO;
  return {
    ...DESIGN_STUDIO_INFO,
    tables: state.schema.tables.map((table) => ({
      name: table.name,
      rowCount: state.rows[table.name]?.length ?? 0,
    })),
  };
}

function tableRows(state: FixtureState, table: string): SlabRecord[] {
  return state.rows[table] ?? [];
}

function sortRows(rows: SlabRecord[], sort: SortSpec[]): SlabRecord[] {
  if (sort.length === 0) return [...rows].sort((left, right) => left.id - right.id);
  return [...rows].sort((left, right) => {
    for (const key of sort) {
      const a = key.column === "id" ? left.id : left.values[key.column];
      const b = key.column === "id" ? right.id : right.values[key.column];
      const comparison = compareValues(a, b);
      if (comparison !== 0) return key.direction === "asc" ? comparison : -comparison;
    }
    return left.id - right.id;
  });
}

function displayValue(value: Value | number): string {
  return typeof value === "number" ? String(value) : valueText(value);
}

function compareValues(left: Value | number | undefined, right: Value | number | undefined): number {
  if (left === undefined || left === "Null" || left === null) {
    return right === undefined || right === "Null" || right === null ? 0 : 1;
  }
  if (right === undefined || right === "Null" || right === null) return -1;
  if (typeof left === "number" || typeof right === "number") {
    const a = typeof left === "number" ? left : Number(displayValue(left));
    const b = typeof right === "number" ? right : Number(displayValue(right));
    if (Number.isFinite(a) && Number.isFinite(b)) return a - b;
  }
  return displayValue(left).localeCompare(displayValue(right), undefined, {
    numeric: true,
    sensitivity: "base",
  });
}

function filterRows(rows: SlabRecord[], query: PageQuery): SlabRecord[] {
  let next = rows;
  if (query.search) {
    const term = query.search.toLowerCase();
    next = next.filter((row) =>
      Object.values(row.values).some((value) => valueText(value).toLowerCase().includes(term)),
    );
  }
  return next;
}

function pageOf(rows: SlabRecord[], limit: number): Page {
  return {
    rows: rows.slice(0, limit),
    next_cursor: null,
    elapsed_ms: 0.8,
    total_rows: rows.length,
  };
}

function relationByName(state: FixtureState, name: string): Relation | undefined {
  return state.schema.relations.find((relation) => relation.name === name);
}

function childRows(state: FixtureState, relation: Relation, parentId: number): SlabRecord[] {
  const childColumn = relation.child_column;
  return tableRows(state, relation.child_table).filter((row) => {
    const value = row.values[childColumn];
    return value !== undefined && typeof value === "object" && "Integer" in value
      ? value.Integer === parentId
      : false;
  });
}

function findRecord(state: FixtureState, table: string, id: number): SlabRecord {
  const record = tableRows(state, table).find((row) => row.id === id);
  if (!record) throw new Error(`Fixture record ${table}#${id} not found`);
  return record;
}

function searchRows(state: FixtureState, term: string, limit: number): SearchHit[] {
  if (state.mode === "empty") return [];
  const needle = term.trim().toLowerCase();
  if (!needle) return [];
  const hits: SearchHit[] = [];
  for (const table of state.schema.tables) {
    for (const row of tableRows(state, table.name)) {
      for (const column of table.columns) {
        const text = valueText(row.values[column.name]);
        const index = text.toLowerCase().indexOf(needle);
        if (index < 0) continue;
        const start = Math.max(0, index - 24);
        const end = Math.min(text.length, index + needle.length + 32);
        const before = text.slice(start, index);
        const match = text.slice(index, index + needle.length);
        const after = text.slice(index + needle.length, end);
        hits.push({
          table: table.name,
          row_id: row.id,
          snippet: `${before}<mark>${match}</mark>${after}`,
          rank: hits.length + 1,
        });
        break;
      }
    }
  }
  return hits.sort((left, right) => left.rank - right.rank).slice(0, limit);
}

function argsOf(value: unknown): Record<string, unknown> {
  return typeof value === "object" && value !== null ? (value as Record<string, unknown>) : {};
}

export async function fixtureCall<T>(
  command: string,
  args: unknown,
  initialMode: FixtureMode,
): Promise<T> {
  const state = stateFor(initialMode);
  const payload = argsOf(args);

  switch (command) {
    case "workspace_info":
      return currentInfo(state) as T;
    case "get_schema":
      return (state.mode === "empty" ? { tables: [], relations: [], layouts: [] } : state.schema) as T;
    case "undo_state":
      return state.undo as T;
    case "undo": {
      const snap = state.history.pop();
      if (!snap) return state.undo as T;
      state.future.push(snapshot(state));
      restore(state, snap);
      state.undo = {
        can_undo: state.history.length > 0,
        can_redo: true,
        undo_label: state.history.at(-1) ? "Restore" : null,
        redo_label: "Restore",
        depth: state.history.length,
      };
      return state.undo as T;
    }
    case "redo": {
      const snap = state.future.pop();
      if (!snap) return state.undo as T;
      state.history.push(snapshot(state));
      restore(state, snap);
      state.undo = {
        can_undo: true,
        can_redo: state.future.length > 0,
        undo_label: "Restore",
        redo_label: null,
        depth: state.history.length,
      };
      return state.undo as T;
    }
    case "page_rows": {
      const query = payload["query"] as PageQuery;
      const rows = filterRows(sortRows(tableRows(state, query.table), query.sort), query);
      return pageOf(rows, query.limit) as T;
    }
    case "read_record": {
      const table = String(payload["table"]);
      const id = Number(payload["id"]);
      return findRecord(state, table, id) as T;
    }
    case "read_bundle": {
      const table = String(payload["table"]);
      const id = Number(payload["id"]);
      const record = findRecord(state, table, id);
      const portals: Record<string, SlabRecord[]> = {};
      for (const relation of state.schema.relations.filter(
        (candidate) => candidate.parent_table === table,
      )) {
        portals[relation.name] = childRows(state, relation, id);
      }
      return { record, portals } satisfies RecordBundle as T;
    }
    case "page_portal": {
      const query = payload["query"] as PortalQuery;
      const relation = relationByName(state, query.relation);
      if (!relation) return { rows: [], next_cursor: null, total_rows: 0, elapsed_ms: 0 } as T;
      const rows = childRows(state, relation, query.parent_id);
      return {
        rows: rows.slice(0, query.limit),
        next_cursor: null,
        total_rows: rows.length,
        elapsed_ms: 0.5,
      } as T;
    }
    case "search": {
      const term = String(payload["term"] ?? "");
      const limit = Number(payload["limit"] ?? 20);
      return searchRows(state, term, limit) as T;
    }
    case "audit_entries":
      return state.audit as T;
    case "integrity_check":
      return {
        integrity: "ok",
        foreignKeyViolations: 0,
        journalMode: "wal",
        pageCount: 42,
      } satisfies IntegrityReportDto as T;
    case "update_row": {
      const table = String(payload["table"]);
      const id = Number(payload["id"]);
      const values = payload["values"] as Record<string, Value>;
      const record = findRecord(state, table, id);
      pushHistory(state, "Update row");
      record.values = { ...record.values, ...values };
      return record as T;
    }
    case "insert_row": {
      const table = String(payload["table"]);
      const values = payload["values"] as Record<string, Value>;
      const rows = tableRows(state, table);
      const id = rows.reduce((max, row) => Math.max(max, row.id), 0) + 1;
      const record: SlabRecord = { id, values: { ...values } };
      pushHistory(state, "Insert row");
      rows.push(record);
      return record as T;
    }
    case "delete_row": {
      const table = String(payload["table"]);
      const id = Number(payload["id"]);
      pushHistory(state, "Delete row");
      state.rows[table] = tableRows(state, table).filter((row) => row.id !== id);
      return undefined as T;
    }
    case "create_table": {
      const draft = payload["draft"] as {
        name: string;
        columns: Array<{
          name: string;
          ty: Column["ty"];
          required: boolean;
          unique: boolean;
          default: Value | null;
        }>;
      };
      if (state.schema.tables.some((table) => table.name === draft.name)) {
        fixtureError("SLAB_E_UNIQUE", `table ${draft.name} already exists`);
      }
      const table = {
        id: `t_${crypto.randomUUID()}`,
        name: draft.name,
        columns: draft.columns.map((column, index) => ({
          id: `c_${crypto.randomUUID()}`,
          table: draft.name,
          name: column.name,
          ty: column.ty,
          required: column.required,
          unique: column.unique,
          default: column.default,
          position: index,
        })),
        created_at: Date.now(),
      };
      pushHistory(state, `Create table ${draft.name}`);
      state.schema.tables.push(table);
      state.rows[draft.name] = [];
      state.mode = "full";
      return table as T;
    }
    case "add_column": {
      const tableName = String(payload["table"]);
      const draft = payload["draft"] as {
        name: string;
        ty: Column["ty"];
        required: boolean;
        unique: boolean;
        default: Value | null;
      };
      const table = state.schema.tables.find((candidate) => candidate.name === tableName);
      if (!table) fixtureError("SLAB_E_NOT_FOUND", `table ${tableName} is missing`);
      if (table.columns.some((column) => column.name === draft.name)) {
        fixtureError("SLAB_E_UNIQUE", `${tableName}.${draft.name} already exists`);
      }
      if (draft.ty.kind === "formula") {
        const referenced = draft.ty.expression.match(/\[([^\]]+)\]/g) ?? [];
        for (const token of referenced) {
          const name = token.slice(1, -1);
          if (!table.columns.some((column) => column.name === name)) {
            fixtureError("SLAB_E_FORMULA_REF", `formula references missing column ${name}`);
          }
        }
      }
      if (draft.ty.kind === "lookup") {
        const lookup = draft.ty;
        if (!state.schema.relations.some((relation) => relation.name === lookup.relation)) {
          fixtureError("SLAB_E_FORMULA_REF", `lookup relation ${lookup.relation} is missing`);
        }
      }
      pushHistory(state, `Add ${tableName}.${draft.name}`);
      const column = {
        id: `c_${crypto.randomUUID()}`,
        table: tableName,
        name: draft.name,
        ty: draft.ty,
        required: draft.required,
        unique: draft.unique,
        default: draft.default,
        position: table.columns.length,
      };
      table.columns.push(column);
      return column as T;
    }
    case "create_relation": {
      const draft = payload["draft"] as RelationDraft;
      if (state.schema.relations.some((relation) => relation.name === draft.name)) {
        fixtureError("SLAB_E_UNIQUE", `relation ${draft.name} already exists`);
      }
      pushHistory(state, `Create relation ${draft.name}`);
      const relation: Relation = {
        id: `r_${crypto.randomUUID()}`,
        name: draft.name,
        parent_table: draft.parent_table,
        child_table: draft.child_table,
        child_column: draft.child_column,
        on_delete: draft.on_delete,
      };
      state.schema.relations.push(relation);
      return relation as T;
    }
    case "rename_table": {
      const oldName = String(payload["old_name"]);
      const newName = String(payload["new_name"]);
      const table = state.schema.tables.find((candidate) => candidate.name === oldName);
      if (!table) fixtureError("SLAB_E_NOT_FOUND", `table ${oldName} is missing`);
      if (state.schema.tables.some((candidate) => candidate.name === newName)) {
        fixtureError("SLAB_E_UNIQUE", `table ${newName} already exists`);
      }
      pushHistory(state, `Rename table to ${newName}`);
      table.name = newName;
      for (const column of table.columns) column.table = newName;
      for (const relation of state.schema.relations) {
        if (relation.parent_table === oldName) relation.parent_table = newName;
        if (relation.child_table === oldName) relation.child_table = newName;
      }
      for (const layout of state.layouts) {
        if (layout.table === oldName) layout.table = newName;
      }
      state.rows[newName] = state.rows[oldName] ?? [];
      delete state.rows[oldName];
      return undefined as T;
    }
    case "rename_column": {
      const tableName = String(payload["table"]);
      const oldName = String(payload["old_name"]);
      const newName = String(payload["new_name"]);
      const table = state.schema.tables.find((candidate) => candidate.name === tableName);
      if (!table) fixtureError("SLAB_E_NOT_FOUND", `table ${tableName} is missing`);
      const column = table.columns.find((candidate) => candidate.name === oldName);
      if (!column) fixtureError("SLAB_E_NOT_FOUND", `${tableName}.${oldName} is missing`);
      if (table.columns.some((candidate) => candidate.name === newName)) {
        fixtureError("SLAB_E_UNIQUE", `${tableName}.${newName} already exists`);
      }
      pushHistory(state, `Rename ${tableName}.${newName}`);
      const pattern = `[${oldName}]`;
      for (const definition of state.schema.tables) {
        for (const candidate of definition.columns) {
          if (candidate.ty.kind === "formula" && definition.name === tableName) {
            candidate.ty.expression = candidate.ty.expression.split(pattern).join(`[${newName}]`);
          }
          if (candidate.ty.kind === "lookup") {
            const lookup = candidate.ty;
            if (
              lookup.column === oldName &&
              state.schema.relations.some(
                (relation) =>
                  relation.name === lookup.relation && relation.child_table === tableName,
              )
            ) {
              lookup.column = newName;
            }
          }
        }
      }
      for (const relation of state.schema.relations) {
        if (relation.child_table === tableName && relation.child_column === oldName) {
          relation.child_column = newName;
        }
      }
      for (const layout of state.layouts) {
        if (layout.table !== tableName) continue;
        for (const item of layout.items) {
          if (item.kind.kind === "field" && item.kind.column === oldName) {
            item.kind.column = newName;
          }
        }
      }
      column.name = newName;
      for (const row of tableRows(state, tableName)) {
        if (oldName in row.values) {
          row.values[newName] = row.values[oldName] as Value;
          delete row.values[oldName];
        }
      }
      return undefined as T;
    }
    case "drop_table": {
      const name = String(payload["name"]);
      const table = state.schema.tables.find((candidate) => candidate.name === name);
      if (!table) fixtureError("SLAB_E_NOT_FOUND", `table ${name} is missing`);
      const relation = state.schema.relations.find(
        (candidate) => candidate.parent_table === name || candidate.child_table === name,
      );
      if (relation) {
        fixtureError(
          "SLAB_E_COMMAND",
          `table ${name} is used by relation ${relation.name}; drop the relation first`,
        );
      }
      pushHistory(state, `Drop table ${name}`);
      state.schema.tables = state.schema.tables.filter((candidate) => candidate.name !== name);
      state.layouts = state.layouts.filter((layout) => layout.table !== name);
      state.schema.layouts = state.layouts;
      delete state.rows[name];
      return undefined as T;
    }
    case "drop_column": {
      const tableName = String(payload["table"]);
      const name = String(payload["column"]);
      const table = state.schema.tables.find((candidate) => candidate.name === tableName);
      if (!table) fixtureError("SLAB_E_NOT_FOUND", `table ${tableName} is missing`);
      if (!table.columns.some((column) => column.name === name)) {
        fixtureError("SLAB_E_NOT_FOUND", `${tableName}.${name} is missing`);
      }
      const relation = state.schema.relations.find(
        (candidate) => candidate.child_table === tableName && candidate.child_column === name,
      );
      if (relation) {
        fixtureError(
          "SLAB_E_FORMULA_REF",
          `column ${tableName}.${name} is used by relation ${relation.name}; drop the relation first`,
        );
      }
      const pattern = `[${name}]`;
      const formula = table.columns.find(
        (column) => column.ty.kind === "formula" && column.ty.expression.includes(pattern),
      );
      if (formula) {
        fixtureError(
          "SLAB_E_FORMULA_REF",
          `column ${tableName}.${name} is referenced by formula ${tableName}.${formula.name}`,
        );
      }
      const layout = state.layouts.find(
        (candidate) =>
          candidate.table === tableName &&
          candidate.items.some((item) => item.kind.kind === "field" && item.kind.column === name),
      );
      if (layout) {
        fixtureError(
          "SLAB_E_FORMULA_REF",
          `column ${tableName}.${name} is used by form ${layout.name}`,
        );
      }
      pushHistory(state, `Drop ${tableName}.${name}`);
      table.columns = table.columns.filter((column) => column.name !== name);
      for (const row of tableRows(state, tableName)) delete row.values[name];
      return undefined as T;
    }
    case "drop_relation": {
      const name = String(payload["name"]);
      if (!state.schema.relations.some((relation) => relation.name === name)) {
        fixtureError("SLAB_E_NOT_FOUND", `relation ${name} is missing`);
      }
      pushHistory(state, `Drop relation ${name}`);
      state.schema.relations = state.schema.relations.filter((relation) => relation.name !== name);
      return undefined as T;
    }
    case "save_layout": {
      const layout = payload["layout"] as Layout;
      pushHistory(state, `Save form ${layout.name}`);
      const index = state.layouts.findIndex((candidate) => candidate.id === layout.id);
      if (index >= 0) state.layouts[index] = layout;
      else state.layouts.push(layout);
      state.schema.layouts = state.layouts;
      return layout as T;
    }
    case "delete_layout": {
      const id = String(payload["id"]);
      state.layouts = state.layouts.filter((layout) => layout.id !== id);
      state.schema.layouts = state.layouts;
      return undefined as T;
    }
    case "list_indexes":
      return [] as T;
    case "toggle_index":
      return [] as T;
    case "load_sample":
    case "open_workspace":
    case "create_workspace":
      state.mode = "full";
      return currentInfo(state) as T;
    case "close_workspace":
      state.mode = "empty";
      return undefined as T;
    case "backup_workspace":
    case "preview_import":
    case "validate_import":
    case "apply_import":
    case "export_csv":
    case "export_json":
    case "export_sql":
    case "export_form_pdf":
    case "export_table_pdf":
      return undefined as T;
    case "render_form_html":
    case "render_table_html":
      return "<!doctype html><html><body>Fixture preview</body></html>" as T;
    default:
      throw new Error(`Fixture mode does not implement ${command}`);
  }
}

let state: FixtureState | null = null;

function stateFor(initialMode: FixtureMode): FixtureState {
  if (!state) state = initialMode === "stress" ? stressState() : initialState(initialMode);
  return state;
}

export function resetFixtureForTests(): void {
  state = null;
}

