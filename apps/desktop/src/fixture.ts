import type { IntegrityReportDto, PageQuery, PortalFrame, PortalQuery, Record as SlabRecord, RecordBundle, RecordRevision, Value } from "./bindings";
import { currentInfo, filterRows, sortRows, tableRows, pageOf, portalFrameOf, relationByName, childRows, findRecord, searchRows } from "./fixtureQueries";
import { fixtureSchemaCall } from "./fixtureSchema";
import { createEmptyFixture, loadFixtureSample, pushHistory, restore, snapshot, stateFor, type FixtureMode } from "./fixtureState";

export { resetFixtureForTests } from "./fixtureState";

export type { FixtureMode } from "./fixtureState";

declare global {
  interface Window {
    /** Test-only one-shot latency injection for async race coverage. */
    __slabFixtureDelay?: {
      command: string;
      ms: number;
      once?: boolean;
    };
  }
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

  if (typeof window !== "undefined") {
    const delay = window.__slabFixtureDelay;
    if (delay && delay.command === command && delay.ms > 0) {
      if (delay.once) window.__slabFixtureDelay = undefined;
      await new Promise<void>((resolve) => {
        window.setTimeout(resolve, delay.ms);
      });
    }
  }

  switch (command) {
    case "workspace_info":
      return currentInfo(state) as T;
    case "get_schema":
      return state.schema as T;
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
      const rows = sortRows(filterRows(state, query), query.sort);
      return pageOf(state, query.table, rows, query) as T;
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
      if (!relation) {
        return {
          columns: [],
          rows: [],
          next_cursor: null,
          total_rows: 0,
          elapsed_ms: 0,
          index_hint: null,
        } satisfies PortalFrame as T;
      }
      const rows = childRows(state, relation, query.parent_id);
      return portalFrameOf(state, relation, rows, query.limit) as T;
    }
    case "search": {
      const term = String(payload["term"] ?? "");
      const limit = Number(payload["limit"] ?? 20);
      return searchRows(state, term, limit) as T;
    }
    case "audit_entries":
      return state.audit as T;
    case "record_history": {
      const table = String(payload["table"]);
      const rowId = Number(payload["row_id"]);
      const record = findRecord(state, table, rowId);
      const revisions: RecordRevision[] = Object.entries(record.values)
        .slice(0, 3)
        .map(([column, value], index) => ({
          auditId: index + 1,
          timestampMs: Date.now() - (index + 1) * 86_400_000,
          hlc: `000179555907591${index}-000000-fixture`,
          actor: "local",
          operation: index === 0 ? "insert" : "update",
          summary: index === 0 ? `Insert ${table} row` : `Update ${column}`,
          diffs: [
            {
              column,
              before: index === 0 ? null : "Null",
              after: value,
            },
          ],
        }));
      return revisions as T;
    }
    case "restore_record_revision": {
      const table = String(payload["table"]);
      const rowId = Number(payload["row_id"]);
      return findRecord(state, table, rowId) as T;
    }
    case "integrity_check":
      return {
        integrity: "ok",
        foreignKeyViolations: 0,
        journalMode: "wal",
        pageCount: 42,
        auditChain: true,
        auditEntries: state.audit.length,
        auditTamper: null,
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
    case "cancel_search":
      return false as T;
    case "load_sample":
      return currentInfo(loadFixtureSample(state.info.path)) as T;
    case "open_workspace":
      return currentInfo(loadFixtureSample(String(payload["path"]))) as T;
    case "create_workspace":
      return currentInfo(createEmptyFixture(String(payload["name"]), String(payload["path"]))) as T;
    case "close_workspace":
      createEmptyFixture();
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
      return fixtureSchemaCall<T>(state, command, payload);
  }
}
