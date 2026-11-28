import type { AuditEntry, Layout, Record as SlabRecord, Schema, UndoState, WorkspaceInfoDto } from "./bindings";
import { DESIGN_STUDIO_AUDIT, DESIGN_STUDIO_INFO, DESIGN_STUDIO_ROWS, DESIGN_STUDIO_SCHEMA } from "./fixtures/designStudio";

export type FixtureMode = "full" | "empty" | "stress";

interface FixtureSnapshot {
  schema: Schema;
  rows: Record<string, SlabRecord[]>;
  layouts: Layout[];
  indexed: string[];
}

export interface FixtureState {
  mode: FixtureMode;
  schema: Schema;
  rows: Record<string, SlabRecord[]>;
  info: WorkspaceInfoDto;
  layouts: Layout[];
  audit: AuditEntry[];
  undo: UndoState;
  history: FixtureSnapshot[];
  future: FixtureSnapshot[];
  indexed: string[];
}

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
    indexed: [],
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
    indexed: [],
  };
}

export function snapshot(state: FixtureState): FixtureSnapshot {
  return structuredClone({
    schema: state.schema,
    rows: state.rows,
    layouts: state.layouts,
    indexed: state.indexed,
  });
}

export function pushHistory(state: FixtureState, label: string): void {
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

export function restore(state: FixtureState, snap: FixtureSnapshot): void {
  state.schema = snap.schema;
  state.rows = snap.rows;
  state.layouts = snap.layouts;
  state.indexed = snap.indexed;
  state.schema.layouts = snap.layouts;
}

export function fixtureError(code: string, message: string): never {
  throw { code, message };
}

let state: FixtureState | null = null;

export function stateFor(initialMode: FixtureMode): FixtureState {
  if (!state) state = initialMode === "stress" ? stressState() : initialState(initialMode);
  return state;
}

export function resetFixtureForTests(): void {
  state = null;
}
