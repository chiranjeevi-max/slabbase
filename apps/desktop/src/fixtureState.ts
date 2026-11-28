import type { AuditEntry, IndexSpec, Record as SlabRecord, Schema, UndoState, WorkspaceInfoDto } from "./bindings";
import { DESIGN_STUDIO_AUDIT, DESIGN_STUDIO_INFO, DESIGN_STUDIO_ROWS, DESIGN_STUDIO_SCHEMA } from "./fixtures/designStudio";

export type FixtureMode = "full" | "empty" | "stress";

interface FixtureSnapshot {
  schema: Schema;
  rows: Record<string, SlabRecord[]>;
  indexes: IndexSpec[];
}

export interface FixtureState extends FixtureSnapshot {
  mode: FixtureMode;
  info: WorkspaceInfoDto;
  audit: AuditEntry[];
  undo: UndoState;
  history: FixtureSnapshot[];
  future: FixtureSnapshot[];
}

const EMPTY_UNDO: UndoState = {
  can_undo: false,
  can_redo: false,
  undo_label: null,
  redo_label: null,
  depth: 0,
};

function emptyState(name = "Untitled", path: string | null = null): FixtureState {
  return {
    mode: "empty",
    schema: { tables: [], relations: [], layouts: [] },
    rows: {},
    info: { name, path, formatVersion: 1, journalMode: "wal", tables: [] },
    audit: [],
    undo: { ...EMPTY_UNDO },
    history: [],
    future: [],
    indexes: [],
  };
}

function sampleState(path: string | null = DESIGN_STUDIO_INFO.path): FixtureState {
  return {
    ...emptyState(DESIGN_STUDIO_INFO.name, path),
    mode: "full",
    schema: structuredClone(DESIGN_STUDIO_SCHEMA),
    rows: structuredClone(DESIGN_STUDIO_ROWS),
    info: { ...DESIGN_STUDIO_INFO, path },
    audit: structuredClone(DESIGN_STUDIO_AUDIT),
    undo: {
      can_undo: true,
      can_redo: false,
      undo_label: "Update row",
      redo_label: null,
      depth: 3,
    },
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
    ...emptyState("Stress"),
    mode: "stress",
    schema,
    rows,
  };
}

export function snapshot(state: FixtureState): FixtureSnapshot {
  return structuredClone({
    schema: state.schema,
    rows: state.rows,
    indexes: state.indexes,
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
  state.indexes = snap.indexes;
}

export function fixtureError(code: string, message: string): never {
  throw { code, message };
}

let state: FixtureState | null = null;

export function stateFor(initialMode: FixtureMode): FixtureState {
  if (!state) {
    switch (initialMode) {
      case "full":
        state = sampleState();
        break;
      case "empty":
        state = emptyState();
        break;
      case "stress":
        state = stressState();
        break;
    }
  }
  return state;
}

export function loadFixtureSample(path?: string | null): FixtureState {
  state = sampleState(path);
  return state;
}

export function createEmptyFixture(name?: string, path?: string | null): FixtureState {
  state = emptyState(name, path);
  return state;
}

export function resetFixtureForTests(): void {
  state = null;
}
