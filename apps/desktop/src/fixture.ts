import type {
  AuditEntry,
  IntegrityReportDto,
  Layout,
  Page,
  PageQuery,
  PortalQuery,
  Record as SlabRecord,
  RecordBundle,
  Relation,
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

export type FixtureMode = "full" | "empty";

interface FixtureState {
  mode: FixtureMode;
  schema: Schema;
  rows: Record<string, SlabRecord[]>;
  info: WorkspaceInfoDto;
  layouts: Layout[];
  audit: AuditEntry[];
  undo: UndoState;
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
  };
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
    case "undo":
    case "redo":
      return state.undo as T;
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
      record.values = { ...record.values, ...values };
      state.undo = { ...state.undo, can_undo: true, undo_label: "Update row" };
      return record as T;
    }
    case "insert_row": {
      const table = String(payload["table"]);
      const values = payload["values"] as Record<string, Value>;
      const rows = tableRows(state, table);
      const id = rows.reduce((max, row) => Math.max(max, row.id), 0) + 1;
      const record: SlabRecord = { id, values: { ...values } };
      rows.push(record);
      state.undo = { ...state.undo, can_undo: true, undo_label: "Insert row" };
      return record as T;
    }
    case "delete_row": {
      const table = String(payload["table"]);
      const id = Number(payload["id"]);
      state.rows[table] = tableRows(state, table).filter((row) => row.id !== id);
      state.undo = { ...state.undo, can_undo: true, undo_label: "Delete row" };
      return undefined as T;
    }
    case "save_layout": {
      const layout = payload["layout"] as Layout;
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
  if (!state) state = initialState(initialMode);
  return state;
}

export function resetFixtureForTests(): void {
  state = null;
}

