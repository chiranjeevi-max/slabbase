import type { RowValues } from "./api";
import type { AppearancePrefs, ResolvedTheme } from "./appearance";
import type {
  AuditEntry,
  ColumnDraft,
  Filter,
  IndexHint,
  IndexSpec,
  IntegrityReportDto,
  Layout,
  PageQuery,
  Record as SlabRecord,
  RecordBundle,
  RecordRevision,
  RelationDraft,
  Schema,
  SortSpec,
  TableDraft,
  UndoState,
  Value,
  WorkspaceInfoDto,
} from "./bindings";

export type Tone = "info" | "success" | "error";

export interface Toast {
  id: number;
  tone: Tone;
  message: string;
  code?: string;
}

export interface FormStatus {
  name: string;
  items: number;
}

export interface AppStore {
  info: WorkspaceInfoDto | null;
  schema: Schema | null;
  booting: boolean;
  busy: boolean;
  appearance: AppearancePrefs;
  resolvedTheme: ResolvedTheme;
  appearanceOpen: boolean;
  formStatus: FormStatus | null;
  toast: Toast | null;

  activeTable: string | null;
  activeLayoutId: string | null;
  view: "table" | "form" | "schema";
  inspectorOpen: boolean;
  rows: SlabRecord[];
  nextCursor: PageQuery["cursor"];
  hasMore: boolean;
  elapsedMs: number;
  loadingRows: boolean;
  sort: SortSpec[];
  filters: Filter[];
  filterColumn: string;
  filterText: string;
  indexHint: IndexHint | null;

  selectedId: number | null;
  bundle: RecordBundle | null;
  bundleTable: string | null;
  creating: boolean;
  undoState: UndoState;
  historyOpen: boolean;
  recordHistory: RecordRevision[];
  historyLoading: boolean;

  paletteOpen: boolean;
  schemaOpen: boolean;
  schemaTab: "table" | "column" | "relation";
  schemaTargetTable: string | null;
  importOpen: boolean;
  printOpen: boolean;
  auditOpen: boolean;
  audit: AuditEntry[];
  saveTick: number;
  historyTick: number;
  integrity: string;
  integrityReport: IntegrityReportDto | null;
  integrityCheckedAt: number | null;

  initialize: () => Promise<void>;
  openWorkspace: (path: string) => Promise<void>;
  createWorkspace: (path: string, name: string) => Promise<void>;
  loadSample: () => Promise<void>;
  closeWorkspace: () => Promise<void>;

  notify: (tone: Tone, message: string, code?: string) => void;
  clearToast: () => void;
  setAppearance: (patch: Partial<Omit<AppearancePrefs, "version">>) => void;
  setAppearanceOpen: (open: boolean) => void;
  setFormStatus: (status: FormStatus | null) => void;

  selectTable: (table: string) => Promise<void>;
  selectLayout: (id: string) => void;
  setView: (view: "table" | "form" | "schema") => void;
  toggleInspector: () => void;
  setInspectorOpen: (open: boolean) => void;
  setSort: (column: string) => Promise<void>;
  setFilterColumn: (column: string) => void;
  applyFilter: (text: string) => Promise<void>;
  clearFilter: () => Promise<void>;
  loadFirstPage: () => Promise<void>;
  loadMore: () => Promise<void>;

  applyIndexHint: () => Promise<void>;
  dismissIndexHint: () => void;
  openHistory: () => Promise<void>;
  closeHistory: () => void;
  restoreRevision: (auditId: number) => Promise<boolean>;

  selectRow: (id: number) => Promise<void>;
  updatePortalRow: (
    table: string,
    id: number,
    column: string,
    value: Value,
  ) => Promise<SlabRecord | null>;
  insertPortalRow: (table: string, values: RowValues) => Promise<SlabRecord | null>;
  deletePortalRow: (table: string, id: number) => Promise<boolean>;
  jumpToRecord: (table: string, id: number) => Promise<void>;
  closeRecord: () => void;
  refreshBundle: () => Promise<void>;
  beginCreate: () => void;
  cancelCreate: () => void;

  updateCell: (
    table: string,
    id: number,
    column: string,
    value: Value,
  ) => Promise<boolean>;
  insertRecord: (values: RowValues) => Promise<boolean>;
  deleteRecord: (id: number) => Promise<void>;

  undo: () => Promise<void>;
  redo: () => Promise<void>;

  createTable: (draft: TableDraft) => Promise<boolean>;
  addColumn: (table: string, draft: ColumnDraft) => Promise<boolean>;
  createRelation: (draft: RelationDraft) => Promise<boolean>;
  renameTable: (oldName: string, newName: string) => Promise<boolean>;
  renameColumn: (table: string, oldName: string, newName: string) => Promise<boolean>;
  dropTable: (name: string) => Promise<boolean>;
  dropColumn: (table: string, column: string) => Promise<boolean>;
  dropRelation: (name: string) => Promise<boolean>;
  toggleIndex: (table: string, column: string, enabled: boolean) => Promise<IndexSpec[] | null>;
  saveLayout: (layout: Layout) => Promise<Layout | null>;

  openAudit: () => Promise<void>;
  closeAudit: () => void;
  backup: (path: string) => Promise<boolean>;
  checkIntegrity: () => Promise<void>;
  refreshAfterImport: () => Promise<void>;

  setPaletteOpen: (open: boolean) => void;
  setSchemaOpen: (open: boolean) => void;
  openSchema: (tab: "table" | "column" | "relation", table?: string) => void;
  setImportOpen: (open: boolean) => void;
  setPrintOpen: (open: boolean) => void;
}
