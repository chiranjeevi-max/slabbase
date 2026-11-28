import type { IndexHint, PageFrame, PageQuery, PortalFrame, Record as SlabRecord, Relation, SearchHit, SortSpec, Value, WorkspaceInfoDto } from "./bindings";
import { DESIGN_STUDIO_INFO } from "./fixtures/designStudio";
import { type FixtureState } from "./fixtureState";
import { valueText } from "./types";

const EMPTY_INFO: WorkspaceInfoDto = {
  name: "Untitled",
  path: null,
  formatVersion: 1,
  journalMode: "wal",
  tables: [],
};

export function currentInfo(state: FixtureState): WorkspaceInfoDto {
  if (state.mode === "empty") return EMPTY_INFO;
  return {
    ...DESIGN_STUDIO_INFO,
    tables: state.schema.tables.map((table) => ({
      name: table.name,
      rowCount: state.rows[table.name]?.length ?? 0,
    })),
  };
}

export function tableRows(state: FixtureState, table: string): SlabRecord[] {
  return state.rows[table] ?? [];
}

export function sortRows(rows: SlabRecord[], sort: SortSpec[]): SlabRecord[] {
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

export function filterRows(rows: SlabRecord[], query: PageQuery): SlabRecord[] {
  let next = rows;
  if (query.search) {
    const term = query.search.toLowerCase();
    next = next.filter((row) =>
      Object.values(row.values).some((value) => valueText(value).toLowerCase().includes(term)),
    );
  }
  return next;
}

function indexHintFor(
  state: FixtureState,
  table: string,
  sort: SortSpec[],
  filters: PageQuery["filters"],
): IndexHint | null {
  if (state.mode !== "stress") return null;
  const column = sort[0]?.column ?? filters[0]?.column;
  if (!column || column === "id") return null;
  if (state.indexed.includes(`${table}:${column}`)) return null;
  return {
    table,
    column,
    suggestedIndex: `slab_idx_${table}_${column}`.replaceAll(" ", "_"),
    elapsedMs: 38.2,
    scannedRows: tableRows(state, table).length,
    detail: `SCAN ${table}`,
  };
}

export function pageOf(
  state: FixtureState,
  table: string,
  rows: SlabRecord[],
  query: PageQuery,
): PageFrame {
  const definition = state.schema.tables.find((candidate) => candidate.name === table);
  const columns =
    definition?.columns.map((column) => column.name) ??
    Object.keys(rows[0]?.values ?? {});
  return {
    columns,
    rows: rows.slice(0, query.limit).map((row) => ({
      id: row.id,
      values: columns.map((column) => row.values[column] ?? "Null"),
    })),
    next_cursor: null,
    elapsed_ms: 0.8,
    total_rows: rows.length,
    index_hint: indexHintFor(state, table, query.sort, query.filters),
  };
}

export function portalFrameOf(
  state: FixtureState,
  relation: Relation,
  rows: SlabRecord[],
  limit: number,
): PortalFrame {
  const definition = state.schema.tables.find(
    (candidate) => candidate.name === relation.child_table,
  );
  const columns = definition?.columns.map((column) => column.name) ?? [];
  return {
    columns,
    rows: rows.slice(0, limit).map((row) => ({
      id: row.id,
      values: columns.map((column) => row.values[column] ?? "Null"),
    })),
    next_cursor: null,
    total_rows: rows.length,
    elapsed_ms: 0.5,
    index_hint: null,
  };
}

export function relationByName(state: FixtureState, name: string): Relation | undefined {
  return state.schema.relations.find((relation) => relation.name === name);
}

export function childRows(state: FixtureState, relation: Relation, parentId: number): SlabRecord[] {
  const childColumn = relation.child_column;
  return tableRows(state, relation.child_table).filter((row) => {
    const value = row.values[childColumn];
    return value !== undefined && typeof value === "object" && "Integer" in value
      ? value.Integer === parentId
      : false;
  });
}

export function findRecord(state: FixtureState, table: string, id: number): SlabRecord {
  const record = tableRows(state, table).find((row) => row.id === id);
  if (!record) throw new Error(`Fixture record ${table}#${id} not found`);
  return record;
}

export function searchRows(state: FixtureState, term: string, limit: number): SearchHit[] {
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
