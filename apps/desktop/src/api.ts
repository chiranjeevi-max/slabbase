import { invoke } from "@tauri-apps/api/core";
import type {
  AuditEntry,
  Column,
  ColumnDraft,
  ImportOptionsDto,
  ImportPreviewDto,
  ImportSummaryDto,
  ImportValidationDto,
  IndexSpec,
  IntegrityReportDto,
  Layout,
  Page,
  PortalPage,
  PortalQuery,
  PageQuery,
  PrintOptionsDto,
  Record as SlabRecord,
  RecordBundle,
  Relation,
  RelationDraft,
  Schema,
  SearchHit,
  Table,
  TableDraft,
  TableHtmlResultDto,
  TablePdfResultDto,
  UndoState,
  Value,
  WorkspaceInfoDto,
} from "./bindings";

export interface ApiErrorShape {
  code: string;
  message: string;
}

export class SlabError extends Error {
  readonly code: string;

  constructor(code: string, message: string) {
    super(message);
    this.name = "SlabError";
    this.code = code;
  }
}

function isApiError(value: unknown): value is ApiErrorShape {
  if (typeof value !== "object" || value === null) return false;
  const candidate = value as Record<string, unknown>;
  return typeof candidate["code"] === "string" && typeof candidate["message"] === "string";
}

function normalizeError(error: unknown): SlabError {
  if (error instanceof SlabError) return error;
  if (isApiError(error)) return new SlabError(error.code, error.message);
  if (typeof error === "string") {
    try {
      const parsed: unknown = JSON.parse(error);
      if (isApiError(parsed)) return new SlabError(parsed.code, parsed.message);
    } catch {
      // Not JSON; use the text as-is.
    }
    return new SlabError("SLAB_E_INTERNAL", error);
  }
  if (error instanceof Error) return new SlabError("SLAB_E_INTERNAL", error.message);
  return new SlabError("SLAB_E_INTERNAL", "Unexpected failure");
}

async function call<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  try {
    return await invoke<T>(command, args);
  } catch (error) {
    throw normalizeError(error);
  }
}

export type RowValues = Record<string, Value>;

export const api = {
  openWorkspace: (path: string) =>
    call<WorkspaceInfoDto>("open_workspace", { path }),
  createWorkspace: (path: string, name: string) =>
    call<WorkspaceInfoDto>("create_workspace", { path, name }),
  closeWorkspace: () => call<void>("close_workspace"),
  workspaceInfo: () => call<WorkspaceInfoDto | null>("workspace_info"),
  loadSample: () => call<WorkspaceInfoDto>("load_sample"),

  getSchema: () => call<Schema>("get_schema"),
  createTable: (draft: TableDraft) =>
    call<Table>("create_table", { draft }),
  addColumn: (table: string, draft: ColumnDraft) =>
    call<Column>("add_column", { table, draft }),
  createRelation: (draft: RelationDraft) =>
    call<Relation>("create_relation", { draft }),
  dropRelation: (name: string) => call<void>("drop_relation", { name }),
  renameTable: (oldName: string, newName: string) =>
    call<void>("rename_table", { old_name: oldName, new_name: newName }),
  renameColumn: (table: string, oldName: string, newName: string) =>
    call<void>("rename_column", {
      table,
      old_name: oldName,
      new_name: newName,
    }),
  dropTable: (name: string) => call<void>("drop_table", { name }),
  dropColumn: (table: string, column: string) =>
    call<void>("drop_column", { table, column }),
  listIndexes: (table: string) =>
    call<IndexSpec[]>("list_indexes", { table }),
  toggleIndex: (table: string, column: string, enabled: boolean) =>
    call<IndexSpec[]>("toggle_index", { table, column, enabled }),
  saveLayout: (layout: Layout) => call<Layout>("save_layout", { layout }),
  deleteLayout: (id: string) => call<void>("delete_layout", { id }),

  pageRows: (query: PageQuery) => call<Page>("page_rows", { query }),
  pagePortal: (query: PortalQuery) =>
    call<PortalPage>("page_portal", { query }),
  readRecord: (table: string, id: number) =>
    call<SlabRecord>("read_record", { table, id }),
  readBundle: (table: string, id: number, portalLimit = 500) =>
    call<RecordBundle>("read_bundle", {
      table,
      id,
      portal_limit: portalLimit,
    }),
  insertRow: (table: string, values: RowValues) =>
    call<SlabRecord>("insert_row", { table, values }),
  updateRow: (table: string, id: number, values: RowValues) =>
    call<SlabRecord>("update_row", { table, id, values }),
  deleteRow: (table: string, id: number) =>
    call<void>("delete_row", { table, id }),

  undo: () => call<UndoState>("undo"),
  redo: () => call<UndoState>("redo"),
  undoState: () => call<UndoState>("undo_state"),
  search: (term: string, limit = 20) =>
    call<SearchHit[]>("search", { term, limit }),
  auditEntries: (limit = 100, offset = 0) =>
    call<AuditEntry[]>("audit_entries", { limit, offset }),

  backup: (path: string) => call<void>("backup_workspace", { path }),
  integrityCheck: () => call<IntegrityReportDto>("integrity_check"),

  previewImport: (path: string, format: string, options: ImportOptionsDto) =>
    call<ImportPreviewDto>("preview_import", { path, format, options }),
  validateImport: (
    path: string,
    format: string,
    targetTable: string,
    mapping: Record<string, string>,
    options: ImportOptionsDto,
  ) =>
    call<ImportValidationDto>("validate_import", {
      path,
      format,
      target_table: targetTable,
      mapping,
      options,
    }),
  applyImport: (
    path: string,
    format: string,
    targetTable: string,
    mapping: Record<string, string>,
    options: ImportOptionsDto,
  ) =>
    call<ImportSummaryDto>("apply_import", {
      path,
      format,
      target_table: targetTable,
      mapping,
      options,
    }),

  exportCsv: (path: string, query: PageQuery) =>
    call<void>("export_csv", { path, query }),
  exportJson: (path: string, query: PageQuery) =>
    call<void>("export_json", { path, query }),
  exportSql: (path: string) => call<void>("export_sql", { path }),

  renderFormHtml: (
    table: string,
    id: number,
    layoutId: string | null,
    options: PrintOptionsDto,
  ) =>
    call<string>("render_form_html", {
      table,
      id,
      layout_id: layoutId,
      options,
    }),
  renderTableHtml: (
    table: string,
    query: PageQuery,
    options: PrintOptionsDto,
  ) => call<TableHtmlResultDto>("render_table_html", { table, query, options }),
  exportFormPdf: (
    path: string,
    table: string,
    id: number,
    layoutId: string | null,
    options: PrintOptionsDto,
  ) =>
    call<void>("export_form_pdf", {
      path,
      table,
      id,
      layout_id: layoutId,
      options,
    }),
  exportTablePdf: (
    path: string,
    table: string,
    query: PageQuery,
    options: PrintOptionsDto,
  ) => call<TablePdfResultDto>("export_table_pdf", { path, table, query, options }),
};
