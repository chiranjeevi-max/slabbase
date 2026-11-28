import type { Column, Layout, Relation, RelationDraft, Value } from "./bindings";
import { tableRows } from "./fixtureQueries";
import { fixtureError, pushHistory, type FixtureState } from "./fixtureState";

export function fixtureSchemaCall<T>(
  state: FixtureState,
  command: string,
  payload: Record<string, unknown>,
): T {
  switch (command) {
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
    case "create_index": {
      const table = String(payload["table"]);
      const column = String(payload["column"]);
      state.indexed.push(`${table}:${column}`);
      return {
        name: `slab_idx_${table}_${column}`.replaceAll(" ", "_"),
        table,
        columns: [column],
        unique: false,
      } as T;
    }
    case "toggle_index": {
      const table = String(payload["table"]);
      const column = String(payload["column"]);
      const key = `${table}:${column}`;
      if (payload["enabled"] === false) {
        state.indexed = state.indexed.filter((candidate) => candidate !== key);
      } else if (!state.indexed.includes(key)) {
        state.indexed.push(key);
      }
      return [] as T;
    }
    default:
      throw new Error(`Fixture mode does not implement ${command}`);
  }
}
