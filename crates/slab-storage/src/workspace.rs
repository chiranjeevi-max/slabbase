//! Opening, schema management, write paths, and audit.

use std::cell::Cell;
use std::collections::BTreeSet;
#[cfg(target_os = "macos")]
use std::ffi::CString;
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use rusqlite::{
    Connection, InterruptHandle, OpenFlags, OptionalExtension, Transaction, TransactionBehavior,
    params,
};
use slab_core::{
    AuditEntry, Clock, Column, ColumnDraft, ColumnType, Command, DeleteAction, FORMAT_VERSION,
    FieldDiff, Hlc, IndexSpec, Layout, LayoutItemKind, PolicyHook, Relation, RelationDraft,
    RowData, RowId, RowValue, Schema, Table, TableDraft, TableSummary, UndoState, Value,
    validate_identifier,
};
use slab_formula::FormulaDag;

use crate::ddl::{column_definition, create_table_sql, relation_index_sql, unique_index_sql};
use crate::error::StorageError;
use crate::rollup;
use crate::sql::{decode_value, encode_value, normalize_value, quote_ident, stored_columns};
use slab_formula::parse as parse_formula;
use slab_formula::rewrite_column_source;

const SYSTEM_DDL: &str = r#"
CREATE TABLE IF NOT EXISTS _slab_meta (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS _slab_tables (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL UNIQUE,
    created_at INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS _slab_columns (
    id TEXT PRIMARY KEY,
    table_name TEXT NOT NULL REFERENCES _slab_tables(name) ON DELETE CASCADE,
    name TEXT NOT NULL,
    type_json TEXT NOT NULL,
    required INTEGER NOT NULL,
    unique_flag INTEGER NOT NULL,
    default_json TEXT,
    position INTEGER NOT NULL,
    UNIQUE (table_name, name)
);
CREATE TABLE IF NOT EXISTS _slab_relations (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL UNIQUE,
    parent_table TEXT NOT NULL REFERENCES _slab_tables(name) ON DELETE CASCADE,
    child_table TEXT NOT NULL REFERENCES _slab_tables(name) ON DELETE CASCADE,
    child_column TEXT NOT NULL,
    on_delete TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS _slab_layouts (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    table_name TEXT NOT NULL REFERENCES _slab_tables(name) ON DELETE CASCADE,
    spec_json TEXT NOT NULL,
    updated_at INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS _slab_audit_log (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    hlc TEXT NOT NULL,
    timestamp_ms INTEGER NOT NULL,
    actor TEXT NOT NULL,
    transaction_id TEXT NOT NULL,
    table_name TEXT,
    row_id INTEGER,
    operation TEXT NOT NULL,
    summary TEXT NOT NULL,
    schema_change INTEGER NOT NULL,
    before_json TEXT,
    after_json TEXT,
    prev_hash TEXT,
    entry_hash TEXT
);
CREATE INDEX IF NOT EXISTS _slab_audit_timestamp ON _slab_audit_log(timestamp_ms);
CREATE TABLE IF NOT EXISTS _slab_rollups (
    parent_table TEXT NOT NULL,
    parent_id INTEGER NOT NULL,
    relation_name TEXT NOT NULL,
    child_column TEXT NOT NULL,
    aggregate TEXT NOT NULL,
    state_json TEXT NOT NULL,
    PRIMARY KEY (parent_table, parent_id, relation_name, child_column, aggregate)
) WITHOUT ROWID;
CREATE VIRTUAL TABLE IF NOT EXISTS _slab_fts USING fts5(
    table_name UNINDEXED,
    row_id UNINDEXED,
    body
);
"#;

const UNDO_LIMIT: usize = 200;
const READER_POOL_SIZE: usize = 4;
const GENESIS_HASH: &str = "0000000000000000000000000000000000000000000000000000000000000000";

/// A shared handle that lets the shell interrupt a superseded interactive
/// query without taking the writer lock.
#[derive(Clone, Default)]
pub struct QueryCancel {
    active: Arc<Mutex<Option<InterruptHandle>>>,
}

impl QueryCancel {
    /// Creates an idle cancellation handle.
    pub fn new() -> Self {
        Self::default()
    }

    /// Interrupts the active query if one is running.
    pub fn interrupt(&self) -> bool {
        match self.active.lock() {
            Ok(guard) => match guard.as_ref() {
                Some(handle) => {
                    handle.interrupt();
                    true
                }
                None => false,
            },
            Err(_) => false,
        }
    }

    /// Registers the active connection interrupt handle.
    ///
    /// Storage calls this around interactive reads; it is exposed so tests and
    /// alternate shells can exercise cancellation without a private hook.
    pub fn register(&self, handle: InterruptHandle) {
        if let Ok(mut guard) = self.active.lock() {
            *guard = Some(handle);
        }
    }

    /// Clears the active handle after a query finishes.
    pub fn clear(&self) {
        if let Ok(mut guard) = self.active.lock() {
            *guard = None;
        }
    }
}
/// The strategy used to produce a snapshot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SnapshotKind {
    /// APFS copy-on-write clone when available.
    ApfsClone,
    /// Transactional `VACUUM INTO` fallback.
    VacuumInto,
}

/// The result of a snapshot request.
#[derive(Clone, Debug)]
pub struct SnapshotReport {
    /// Strategy that succeeded.
    pub kind: SnapshotKind,
    /// Time spent producing the snapshot, in milliseconds.
    pub elapsed_ms: f64,
    /// Size of the resulting file, in bytes.
    pub bytes: u64,
}

/// A live SQLite workspace.
///
/// `Workspace` is not `Sync`. The storage crate keeps its connections
/// single-threaded; the desktop shell wraps a writer in a mutex and leases
/// read-only replicas behind another mutex.
pub struct Workspace {
    pub(crate) conn: Connection,
    pub(crate) readers: Vec<Connection>,
    pub(crate) reader_cursor: Cell<usize>,
    pub(crate) path: Option<PathBuf>,
    pub(crate) clock: Box<dyn Clock>,
    pub(crate) actor: String,
    pub(crate) undo: Vec<Command>,
    pub(crate) redo: Vec<Command>,
    pub(crate) pending_import: Option<Vec<Command>>,
    pub(crate) id_seed: u64,
    pub(crate) policy: Arc<dyn PolicyHook>,
    pub(crate) name: String,
    pub(crate) cancel: Arc<QueryCancel>,
    pub(crate) read_only: bool,
}

/// A summary of the open workspace.
#[derive(Clone, Debug)]
pub struct WorkspaceInfo {
    /// Workspace name stored in `_slab_meta`.
    pub name: String,
    /// Absolute or supplied path. Absent for in-memory workspaces.
    pub path: Option<String>,
    /// Format version.
    pub format_version: u32,
    /// SQLite journal mode.
    pub journal_mode: String,
    /// Tables and row counts.
    pub tables: Vec<TableSummary>,
}

/// Results of SQLite's own integrity checks plus the audit hash chain.
#[derive(Clone, Debug)]
pub struct IntegrityReport {
    /// `PRAGMA integrity_check` output.
    pub integrity: String,
    /// Number of `PRAGMA foreign_key_check` violations.
    pub foreign_key_violations: u64,
    /// `PRAGMA journal_mode` output.
    pub journal_mode: String,
    /// `PRAGMA page_count` output.
    pub page_count: u64,
    /// True when the BLAKE3 audit chain is continuous and intact.
    pub audit_chain: bool,
    /// Number of audit entries verified.
    pub audit_entries: u64,
    /// Explicit diagnostic when the chain was altered outside SlabBase.
    pub audit_tamper: Option<String>,
}

impl Workspace {
    /// Creates a new workspace at a path. Fails when a non-empty file exists.
    pub fn create(
        path: impl AsRef<Path>,
        name: &str,
        clock: Box<dyn Clock>,
    ) -> Result<Self, StorageError> {
        let path = path.as_ref();
        if let Ok(metadata) = std::fs::metadata(path)
            && metadata.len() > 0
        {
            return Err(StorageError::Workspace(format!(
                "{} already exists",
                path.display()
            )));
        }
        let conn = Connection::open(path)?;
        configure(&conn)?;
        initialize(&conn, name)?;
        migrate(&conn)?;
        let readers = open_readers(path);
        let mut workspace = Self {
            conn,
            readers,
            reader_cursor: Cell::new(0),
            path: Some(path.to_path_buf()),
            clock,
            actor: "local".into(),
            undo: Vec::new(),
            redo: Vec::new(),
            pending_import: None,
            id_seed: 0,
            policy: Arc::new(slab_core::NoopPolicy),
            name: name.to_owned(),
            cancel: Arc::new(QueryCancel::new()),
            read_only: false,
        };
        workspace.validate_schema()?;
        Ok(workspace)
    }

    /// Opens an existing workspace.
    pub fn open(path: impl AsRef<Path>, clock: Box<dyn Clock>) -> Result<Self, StorageError> {
        let path = path.as_ref();
        if !path.exists() {
            return Err(StorageError::NotFound(path.display().to_string()));
        }
        let metadata = std::fs::metadata(path)?;
        if metadata.len() == 0 {
            return Err(StorageError::Workspace(format!(
                "{} is empty; it is not a SlabBase workspace",
                path.display()
            )));
        }
        if metadata.permissions().readonly() {
            return Err(StorageError::Workspace(format!(
                "{} is read-only; SlabBase needs write access to open a workspace",
                path.display()
            )));
        }
        let conn = Connection::open(path)?;
        // Set the timeout before the first read so a concurrent writer cannot
        // turn workspace discovery into an immediate SQLITE_BUSY failure.
        conn.busy_timeout(Duration::from_millis(5_000))?;
        let has_meta: bool = conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = '_slab_meta')",
                [],
                |row| row.get(0),
            )
            .map_err(|error| workspace_open_error(error, path))?;
        if !has_meta {
            return Err(StorageError::Workspace(format!(
                "{} is not a SlabBase workspace",
                path.display()
            )));
        }
        // Only mutate connection settings after the file is known to be a
        // workspace. This keeps foreign or damaged files byte-for-byte intact.
        configure(&conn).map_err(|error| match error {
            StorageError::Sqlite(rusqlite::Error::SqliteFailure(failure, _))
                if failure.code == rusqlite::ErrorCode::ReadOnly =>
            {
                StorageError::Workspace(format!(
                    "{} is read-only; SlabBase needs write access to open a workspace",
                    path.display()
                ))
            }
            other => other,
        })?;
        let name = read_meta(&conn, "name")?.unwrap_or_else(|| "Workspace".into());
        let format_version = read_meta(&conn, "format_version")?
            .and_then(|value| value.parse::<u32>().ok())
            .unwrap_or(0);
        if format_version > FORMAT_VERSION {
            return Err(StorageError::Workspace(format!(
                "workspace format {format_version} is newer than this build supports ({FORMAT_VERSION})"
            )));
        }
        migrate(&conn)?;
        let readers = open_readers(path);
        let mut workspace = Self {
            conn,
            readers,
            reader_cursor: Cell::new(0),
            path: Some(path.to_path_buf()),
            clock,
            actor: "local".into(),
            undo: Vec::new(),
            redo: Vec::new(),
            pending_import: None,
            id_seed: 0,
            policy: Arc::new(slab_core::NoopPolicy),
            name,
            cancel: Arc::new(QueryCancel::new()),
            read_only: false,
        };
        workspace.validate_schema()?;
        Ok(workspace)
    }

    /// Opens an existing workspace in read-only mode for a concurrent reader.
    ///
    /// The caller must have opened the file for writing first so migrations and
    /// derived tables are current. This mode never checkpoints, migrates, or
    /// writes rollup cache entries.
    pub fn open_read_only(
        path: impl AsRef<Path>,
        clock: Box<dyn Clock>,
    ) -> Result<Self, StorageError> {
        let path = path.as_ref();
        if !path.exists() {
            return Err(StorageError::NotFound(path.display().to_string()));
        }
        let metadata = std::fs::metadata(path)?;
        if metadata.len() == 0 {
            return Err(StorageError::Workspace(format!(
                "{} is empty; it is not a SlabBase workspace",
                path.display()
            )));
        }
        let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
        conn.busy_timeout(Duration::from_millis(5_000))?;
        let has_meta: bool = conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = '_slab_meta')",
                [],
                |row| row.get(0),
            )
            .map_err(|error| workspace_open_error(error, path))?;
        if !has_meta {
            return Err(StorageError::Workspace(format!(
                "{} is not a SlabBase workspace",
                path.display()
            )));
        }
        configure_read_only(&conn)?;
        let name = read_meta(&conn, "name")?.unwrap_or_else(|| "Workspace".into());
        let format_version = read_meta(&conn, "format_version")?
            .and_then(|value| value.parse::<u32>().ok())
            .unwrap_or(0);
        if format_version > FORMAT_VERSION {
            return Err(StorageError::Workspace(format!(
                "workspace format {format_version} is newer than this build supports ({FORMAT_VERSION})"
            )));
        }
        let mut workspace = Self {
            conn,
            readers: Vec::new(),
            reader_cursor: Cell::new(0),
            path: Some(path.to_path_buf()),
            clock,
            actor: "local".into(),
            undo: Vec::new(),
            redo: Vec::new(),
            pending_import: None,
            id_seed: 0,
            policy: Arc::new(slab_core::NoopPolicy),
            name,
            cancel: Arc::new(QueryCancel::new()),
            read_only: true,
        };
        workspace.validate_schema()?;
        Ok(workspace)
    }

    /// Creates an in-memory workspace for tests and previews.
    pub fn open_in_memory(name: &str, clock: Box<dyn Clock>) -> Result<Self, StorageError> {
        let conn = Connection::open_in_memory()?;
        configure(&conn)?;
        initialize(&conn, name)?;
        migrate(&conn)?;
        Ok(Self {
            conn,
            readers: Vec::new(),
            reader_cursor: Cell::new(0),
            path: None,
            clock,
            actor: "local".into(),
            undo: Vec::new(),
            redo: Vec::new(),
            pending_import: None,
            id_seed: 0,
            policy: Arc::new(slab_core::NoopPolicy),
            name: name.to_owned(),
            cancel: Arc::new(QueryCancel::new()),
            read_only: false,
        })
    }

    /// The workspace name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The workspace path, absent for in-memory workspaces.
    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    /// Sets the actor recorded in the audit log.
    pub fn set_actor(&mut self, actor: impl Into<String>) {
        self.actor = actor.into();
    }

    /// Replaces the read policy hook.
    pub fn set_policy(&mut self, policy: Arc<dyn PolicyHook>) {
        self.policy = policy;
    }

    /// Shares the cancellation handle exposed to the shell.
    pub fn set_cancel_handle(&mut self, cancel: Arc<QueryCancel>) {
        self.cancel = cancel;
    }

    /// Interrupts the active interactive query, if any.
    pub fn cancel_query(&self) -> bool {
        self.cancel.interrupt()
    }

    /// Returns a pooled read-only connection, or the writer for an in-memory
    /// workspace where separate connections cannot share the database.
    pub(crate) fn read_conn(&self) -> &Connection {
        if self.readers.is_empty() {
            return &self.conn;
        }
        let index = self.reader_cursor.get() % self.readers.len();
        self.reader_cursor.set((index + 1) % self.readers.len());
        &self.readers[index]
    }

    /// Returns the writer connection.
    pub(crate) fn write_conn(&self) -> &Connection {
        &self.conn
    }

    fn refresh_readers(&mut self) {
        if self.path.is_none() || self.read_only {
            return;
        }
        self.readers.clear();
        if let Some(path) = self.path.clone() {
            self.readers = open_readers(&path);
        }
        self.reader_cursor.set(0);
    }

    /// Loads the schema from metadata tables and validates it.
    pub fn schema(&self) -> Result<Schema, StorageError> {
        let mut tables: Vec<Table> = Vec::new();
        {
            let mut stmt = self
                .conn
                .prepare("SELECT id, name, created_at FROM _slab_tables ORDER BY name")?;
            let rows = stmt.query_map([], |row| {
                Ok(Table {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    columns: Vec::new(),
                    created_at: row.get(2)?,
                })
            })?;
            for row in rows {
                tables.push(row?);
            }
        }
        {
            let mut stmt = self.conn.prepare(
                "SELECT id, table_name, name, type_json, required, unique_flag, default_json, position
                 FROM _slab_columns ORDER BY table_name, position, name",
            )?;
            let rows = stmt.query_map([], |row| {
                let type_json: String = row.get(3)?;
                let default_json: Option<String> = row.get(6)?;
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    type_json,
                    row.get::<_, i64>(4)? != 0,
                    row.get::<_, i64>(5)? != 0,
                    default_json,
                    row.get::<_, i64>(7)?,
                ))
            })?;
            for row in rows {
                let (id, table_name, name, type_json, required, unique, default_json, position) =
                    row?;
                let ty: ColumnType = serde_json::from_str(&type_json)?;
                let default = default_json
                    .map(|json| serde_json::from_str::<Value>(&json))
                    .transpose()?;
                if let Some(table) = tables.iter_mut().find(|table| table.name == table_name) {
                    table.columns.push(Column {
                        id,
                        table: table_name,
                        name,
                        ty,
                        required,
                        unique,
                        default,
                        position: u32::try_from(position).unwrap_or(0),
                    });
                }
            }
        }
        let mut relations: Vec<Relation> = Vec::new();
        {
            let mut stmt = self.conn.prepare(
                "SELECT id, name, parent_table, child_table, child_column, on_delete
                 FROM _slab_relations ORDER BY name",
            )?;
            let rows = stmt.query_map([], |row| {
                let action: String = row.get(5)?;
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    action,
                ))
            })?;
            for row in rows {
                let (id, name, parent_table, child_table, child_column, action) = row?;
                relations.push(Relation {
                    id,
                    name,
                    parent_table,
                    child_table,
                    child_column,
                    on_delete: match action.as_str() {
                        "cascade" => DeleteAction::Cascade,
                        "setNull" => DeleteAction::SetNull,
                        _ => DeleteAction::Restrict,
                    },
                });
            }
        }
        let mut layouts: Vec<Layout> = Vec::new();
        {
            let mut stmt = self
                .conn
                .prepare("SELECT spec_json FROM _slab_layouts ORDER BY name")?;
            let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
            for row in rows {
                layouts.push(serde_json::from_str(&row?)?);
            }
        }
        Ok(Schema {
            tables,
            relations,
            layouts,
        })
    }

    fn validate_schema(&mut self) -> Result<(), StorageError> {
        let schema = self.schema()?;
        schema.validate()?;
        FormulaDag::build(&schema)?;
        Ok(())
    }

    /// Workspace metadata and row counts.
    pub fn info(&self) -> Result<WorkspaceInfo, StorageError> {
        let journal_mode: String = self
            .conn
            .query_row("PRAGMA journal_mode", [], |row| row.get(0))?;
        let format_version = read_meta(&self.conn, "format_version")?
            .and_then(|value| value.parse::<u32>().ok())
            .unwrap_or(FORMAT_VERSION);
        Ok(WorkspaceInfo {
            name: self.name.clone(),
            path: self.path.as_ref().map(|path| path.display().to_string()),
            format_version,
            journal_mode,
            tables: self.table_summaries()?,
        })
    }

    /// Runs SQLite integrity and foreign key checks plus audit chain
    /// verification.
    pub fn integrity_check(&self) -> Result<IntegrityReport, StorageError> {
        let integrity: String = self
            .conn
            .query_row("PRAGMA integrity_check", [], |row| row.get(0))?;
        let foreign_key_violations: i64 =
            self.conn
                .query_row("SELECT COUNT(*) FROM pragma_foreign_key_check", [], |row| {
                    row.get(0)
                })?;
        let journal_mode: String = self
            .conn
            .query_row("PRAGMA journal_mode", [], |row| row.get(0))?;
        let page_count: i64 = self
            .conn
            .query_row("PRAGMA page_count", [], |row| row.get(0))?;
        let (audit_chain, audit_entries, audit_tamper) = verify_audit_chain(&self.conn)?;
        Ok(IntegrityReport {
            integrity,
            foreign_key_violations: u64::try_from(foreign_key_violations).unwrap_or(0),
            journal_mode,
            page_count: u64::try_from(page_count).unwrap_or(0),
            audit_chain,
            audit_entries,
            audit_tamper,
        })
    }

    /// Writes a portable SQL script containing user tables and SlabBase
    /// metadata. The full-text index and audit log are excluded.
    pub fn export_sql(&self, path: impl AsRef<Path>) -> Result<(), StorageError> {
        let schema = self.schema()?;
        let file = File::create(path)?;
        let mut out = BufWriter::new(file);
        writeln!(out, "-- SlabBase SQL export (format {FORMAT_VERSION})")?;
        writeln!(out, "PRAGMA foreign_keys = OFF;")?;
        writeln!(out, "BEGIN TRANSACTION;")?;
        let mut table_names: Vec<String> = vec![
            "_slab_meta".into(),
            "_slab_tables".into(),
            "_slab_columns".into(),
            "_slab_relations".into(),
            "_slab_layouts".into(),
        ];
        table_names.extend(schema.tables.iter().map(|table| table.name.clone()));
        for table_name in &table_names {
            let create: Option<String> = self
                .conn
                .query_row(
                    "SELECT sql FROM sqlite_master WHERE type = 'table' AND name = ?1",
                    params![table_name],
                    |row| row.get(0),
                )
                .optional()?;
            let Some(create) = create else {
                continue;
            };
            writeln!(out, "DROP TABLE IF EXISTS {};", quote_ident(table_name))?;
            writeln!(out, "{create};")?;
            let sql = format!("SELECT * FROM {}", quote_ident(table_name));
            let mut stmt = self.conn.prepare(&sql)?;
            let column_count = stmt.column_count();
            let columns: Vec<String> = stmt.column_names().map(quote_ident).collect();
            let mut rows = stmt.query([])?;
            while let Some(row) = rows.next()? {
                let mut literals = Vec::with_capacity(column_count);
                for index in 0..column_count {
                    let value = row.get_ref(index)?;
                    literals.push(sql_value_literal(value));
                }
                writeln!(
                    out,
                    "INSERT INTO {} ({}) VALUES ({});",
                    quote_ident(table_name),
                    columns.join(", "),
                    literals.join(", ")
                )?;
            }
        }
        let mut indexes = self.conn.prepare(
            "SELECT sql FROM sqlite_master
             WHERE type = 'index' AND name LIKE 'slab_idx_%' AND sql IS NOT NULL
             ORDER BY name",
        )?;
        let index_sql = indexes.query_map([], |row| row.get::<_, String>(0))?;
        for statement in index_sql {
            writeln!(out, "{};", statement?)?;
        }
        writeln!(out, "COMMIT;")?;
        out.flush()?;
        Ok(())
    }

    /// Writes a consistent snapshot with `VACUUM INTO` after checkpointing.
    pub fn backup_to(&self, path: impl AsRef<Path>) -> Result<(), StorageError> {
        let path = path.as_ref();
        if path.exists() {
            return Err(StorageError::Workspace(format!(
                "{} already exists",
                path.display()
            )));
        }
        self.checkpoint_for_snapshot()?;
        let target = path
            .to_str()
            .ok_or_else(|| StorageError::Workspace("backup path is not valid UTF-8".into()))?;
        self.conn.execute("VACUUM INTO ?1", params![target])?;
        Ok(())
    }

    /// Produces a snapshot, preferring an atomic APFS copy-on-write clone on
    /// macOS and falling back to transactional `VACUUM INTO`.
    pub fn snapshot_to(&self, path: impl AsRef<Path>) -> Result<SnapshotReport, StorageError> {
        self.snapshot_with(path.as_ref(), true)
    }

    /// Forces the transactional `VACUUM INTO` path. Used by tests and by
    /// callers that need a portable destination.
    pub fn snapshot_vacuum_to(
        &self,
        path: impl AsRef<Path>,
    ) -> Result<SnapshotReport, StorageError> {
        self.snapshot_with(path.as_ref(), false)
    }

    fn snapshot_with(
        &self,
        path: &Path,
        prefer_clone: bool,
    ) -> Result<SnapshotReport, StorageError> {
        if path.exists() {
            return Err(StorageError::Workspace(format!(
                "{} already exists",
                path.display()
            )));
        }
        let started = std::time::Instant::now();
        let clean = self.checkpoint_for_snapshot()?;
        if prefer_clone
            && clean
            && let Some(source) = &self.path
            && clone_file(source, path)?
        {
            return snapshot_report(SnapshotKind::ApfsClone, path, started);
        }
        let target = path
            .to_str()
            .ok_or_else(|| StorageError::Workspace("snapshot path is not valid UTF-8".into()))?;
        self.conn.execute("VACUUM INTO ?1", params![target])?;
        snapshot_report(SnapshotKind::VacuumInto, path, started)
    }

    fn checkpoint_for_snapshot(&self) -> Result<bool, StorageError> {
        if self.path.is_none() {
            return Ok(true);
        }
        self.conn.execute_batch("PRAGMA optimize;")?;
        let (busy, _log, _checkpointed): (i64, i64, i64) =
            self.conn
                .query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |row| {
                    Ok((row.get(0)?, row.get(1)?, row.get(2)?))
                })?;
        Ok(busy == 0)
    }

    /// Checkpoints the WAL and consumes the workspace, leaving one
    /// self-contained `.slab` file.
    pub fn close(mut self) -> Result<(), StorageError> {
        if self.path.is_some() && !self.read_only {
            // Reader connections must close before the final checkpoint so
            // SQLite can truncate the WAL and delete both sidecars.
            self.readers.clear();
            self.conn.execute_batch("PRAGMA optimize;")?;
            self.conn
                .execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")?;
        }
        Ok(())
    }

    /// Tables and row counts for the sidebar.
    pub fn table_summaries(&self) -> Result<Vec<TableSummary>, StorageError> {
        let schema = self.schema()?;
        let mut summaries = Vec::with_capacity(schema.tables.len());
        for table in &schema.tables {
            let sql = format!("SELECT COUNT(*) FROM {}", quote_ident(&table.name));
            let count: i64 = self.conn.query_row(&sql, [], |row| row.get(0))?;
            summaries.push(TableSummary {
                name: table.name.clone(),
                row_count: u64::try_from(count).unwrap_or(0),
            });
        }
        Ok(summaries)
    }

    /// Creates a table from a draft, rejecting invalid formulas before any DDL.
    pub fn create_table(&mut self, draft: &TableDraft) -> Result<Table, StorageError> {
        validate_identifier(&draft.name)?;
        let mut table = Table {
            id: self.next_id("t"),
            name: draft.name.clone(),
            columns: Vec::with_capacity(draft.columns.len()),
            created_at: now_ms(),
        };
        for (index, column) in draft.columns.iter().enumerate() {
            table.columns.push(Column {
                id: self.next_id("c"),
                table: draft.name.clone(),
                name: column.name.clone(),
                ty: column.ty.clone(),
                required: column.required,
                unique: column.unique,
                default: column.default.clone(),
                position: u32::try_from(index).unwrap_or(0),
            });
        }
        table.validate()?;
        let mut candidate = self.schema()?;
        if candidate.table(&table.name).is_some() {
            return Err(StorageError::Unique(format!(
                "table {} already exists",
                table.name
            )));
        }
        candidate.tables.push(table.clone());
        candidate.validate()?;
        FormulaDag::build(&candidate)?;
        let command = Command::CreateTable {
            table: table.clone(),
        };
        self.apply_command(&command)?;
        Ok(table)
    }

    /// Adds a column, rebuilding no tables.
    pub fn add_column(
        &mut self,
        table_name: &str,
        draft: &ColumnDraft,
    ) -> Result<Column, StorageError> {
        let schema = self.schema()?;
        let table = schema
            .table(table_name)
            .ok_or_else(|| StorageError::NotFound(table_name.to_owned()))?;
        let position = u32::try_from(table.columns.len()).unwrap_or(0);
        let column = Column {
            id: self.next_id("c"),
            table: table_name.to_owned(),
            name: draft.name.clone(),
            ty: draft.ty.clone(),
            required: draft.required,
            unique: draft.unique,
            default: draft.default.clone(),
            position,
        };
        column.validate()?;
        let mut candidate = schema.clone();
        let table = candidate
            .table_mut(table_name)
            .ok_or_else(|| StorageError::NotFound(table_name.to_owned()))?;
        if table.column(&column.name).is_some() {
            return Err(StorageError::Unique(format!(
                "column {}.{} already exists",
                table_name, column.name
            )));
        }
        table.columns.push(column.clone());
        candidate.validate()?;
        FormulaDag::build(&candidate)?;
        let command = Command::AddColumn {
            table: table_name.to_owned(),
            column: column.clone(),
        };
        self.apply_command(&command)?;
        Ok(column)
    }

    /// Creates a parent-to-child relation, rebuilding the child table with a
    /// real foreign key.
    pub fn create_relation(&mut self, draft: &RelationDraft) -> Result<Relation, StorageError> {
        if draft.on_delete == DeleteAction::SetNull {
            return Err(StorageError::Unsupported(
                "SET NULL relations are reserved for a later release".into(),
            ));
        }
        let relation = Relation {
            id: self.next_id("r"),
            name: draft.name.clone(),
            parent_table: draft.parent_table.clone(),
            child_table: draft.child_table.clone(),
            child_column: draft.child_column.clone(),
            on_delete: draft.on_delete,
        };
        relation.validate()?;
        let mut candidate = self.schema()?;
        if candidate.relation(&relation.name).is_some() {
            return Err(StorageError::Unique(format!(
                "relation {} already exists",
                relation.name
            )));
        }
        let child = candidate
            .table(&relation.child_table)
            .ok_or_else(|| StorageError::NotFound(relation.child_table.clone()))?;
        let column = child.column(&relation.child_column).ok_or_else(|| {
            StorageError::Query(format!(
                "missing child column {}.{}",
                relation.child_table, relation.child_column
            ))
        })?;
        if !matches!(column.ty, ColumnType::Integer) {
            return Err(StorageError::Command(
                "relation columns must be Integer".into(),
            ));
        }
        if !column.ty.is_stored() {
            return Err(StorageError::Command(
                "relation columns must be stored".into(),
            ));
        }
        if candidate.relations.iter().any(|existing| {
            existing.child_table == relation.child_table
                && existing.child_column == relation.child_column
        }) {
            return Err(StorageError::Command(format!(
                "{} already has a relation on {}",
                relation.child_table, relation.child_column
            )));
        }
        candidate.relations.push(relation.clone());
        candidate.validate()?;
        let command = Command::CreateRelation {
            relation: relation.clone(),
        };
        self.apply_command(&command)?;
        Ok(relation)
    }

    /// Removes a relation and its foreign key constraint.
    pub fn drop_relation(&mut self, name: &str) -> Result<(), StorageError> {
        let schema = self.schema()?;
        let relation = schema
            .relation(name)
            .ok_or_else(|| StorageError::NotFound(name.to_owned()))?
            .clone();
        let command = Command::DropRelation { relation };
        self.apply_command(&command)?;
        Ok(())
    }

    /// Renames a table and every metadata reference to it.
    pub fn rename_table(&mut self, old_name: &str, new_name: &str) -> Result<(), StorageError> {
        validate_identifier(new_name)?;
        let schema = self.schema()?;
        if schema.table(old_name).is_none() {
            return Err(StorageError::NotFound(old_name.to_owned()));
        }
        if schema.table(new_name).is_some() {
            return Err(StorageError::Unique(format!(
                "table {new_name} already exists"
            )));
        }
        let mut candidate = schema.clone();
        let table = candidate
            .table_mut(old_name)
            .ok_or_else(|| StorageError::NotFound(old_name.to_owned()))?;
        table.name = new_name.to_owned();
        for column in &mut table.columns {
            column.table = new_name.to_owned();
        }
        for relation in &mut candidate.relations {
            if relation.parent_table == old_name {
                relation.parent_table = new_name.to_owned();
            }
            if relation.child_table == old_name {
                relation.child_table = new_name.to_owned();
            }
        }
        for layout in &mut candidate.layouts {
            if layout.table == old_name {
                layout.table = new_name.to_owned();
            }
        }
        candidate.validate()?;
        FormulaDag::build(&candidate)?;
        let command = Command::RenameTable {
            old_name: old_name.to_owned(),
            new_name: new_name.to_owned(),
        };
        self.apply_command(&command)?;
        Ok(())
    }

    /// Renames a column and rewrites formulas, lookups, layouts, and relations.
    pub fn rename_column(
        &mut self,
        table: &str,
        old_name: &str,
        new_name: &str,
    ) -> Result<(), StorageError> {
        validate_identifier(new_name)?;
        let schema = self.schema()?;
        let definition = schema
            .table(table)
            .ok_or_else(|| StorageError::NotFound(table.to_owned()))?;
        if definition.column(old_name).is_none() {
            return Err(StorageError::NotFound(format!("{table}.{old_name}")));
        }
        if definition.column(new_name).is_some() {
            return Err(StorageError::Unique(format!(
                "{table}.{new_name} already exists"
            )));
        }
        rename_column_in_schema(&schema, table, old_name, new_name)?;
        let command = Command::RenameColumn {
            table: table.to_owned(),
            old_name: old_name.to_owned(),
            new_name: new_name.to_owned(),
        };
        self.apply_command(&command)?;
        Ok(())
    }

    /// Drops a table and retains its rows and layouts for undo.
    pub fn drop_table(&mut self, name: &str) -> Result<(), StorageError> {
        let schema = self.schema()?;
        let table = schema
            .table(name)
            .ok_or_else(|| StorageError::NotFound(name.to_owned()))?
            .clone();
        if let Some(relation) = schema
            .relations
            .iter()
            .find(|relation| relation.parent_table == name || relation.child_table == name)
        {
            return Err(StorageError::Command(format!(
                "table {name} is used by relation {}; drop the relation first",
                relation.name
            )));
        }
        let rows = stored_records(&self.conn, &table)?;
        let layouts = schema
            .layouts
            .iter()
            .filter(|layout| layout.table == name)
            .cloned()
            .collect();
        let indexes = list_user_indexes(&self.conn, name)?;
        let command = Command::DropTable {
            table,
            rows,
            layouts,
            indexes,
        };
        self.apply_command(&command)?;
        Ok(())
    }

    /// Drops a column after blocking formula, relation, and layout references.
    pub fn drop_column(&mut self, table: &str, column: &str) -> Result<(), StorageError> {
        let schema = self.schema()?;
        let definition = schema
            .table(table)
            .ok_or_else(|| StorageError::NotFound(table.to_owned()))?;
        let column_definition = definition
            .column(column)
            .ok_or_else(|| StorageError::NotFound(format!("{table}.{column}")))?
            .clone();
        if let Some(relation) = schema
            .relations
            .iter()
            .find(|relation| relation.child_table == table && relation.child_column == column)
        {
            return Err(StorageError::Reference(format!(
                "column {table}.{column} is used by relation {}; drop the relation first",
                relation.name
            )));
        }
        if let Some(reference) = formula_reference(&schema, table, column)? {
            return Err(StorageError::Reference(format!(
                "column {table}.{column} is referenced by {reference}"
            )));
        }
        if let Some(layout) = layout_reference(&schema, table, column) {
            return Err(StorageError::Reference(format!(
                "column {table}.{column} is used by form {layout}"
            )));
        }
        let values = stored_values(&self.conn, definition, &column_definition)?;
        let command = Command::DropColumn {
            table: table.to_owned(),
            column: column_definition,
            values,
        };
        self.apply_command(&command)?;
        Ok(())
    }

    /// Lists user-created single-column indexes for a table.
    pub fn list_indexes(&self, table: &str) -> Result<Vec<IndexSpec>, StorageError> {
        let schema = self.schema()?;
        if schema.table(table).is_none() {
            return Err(StorageError::NotFound(table.to_owned()));
        }
        list_user_indexes(&self.conn, table)
    }

    /// Creates a single-column index if it does not already exist.
    pub fn create_index(&mut self, table: &str, column: &str) -> Result<IndexSpec, StorageError> {
        let schema = self.schema()?;
        let definition = schema
            .table(table)
            .ok_or_else(|| StorageError::NotFound(table.to_owned()))?;
        let column_definition = definition
            .column(column)
            .ok_or_else(|| StorageError::NotFound(format!("{table}.{column}")))?;
        if !column_definition.ty.is_stored() {
            return Err(StorageError::Command(
                "computed columns cannot be indexed".into(),
            ));
        }
        let existing = list_user_indexes(&self.conn, table)?;
        if let Some(index) = existing
            .into_iter()
            .find(|index| index.columns == vec![column.to_owned()])
        {
            return Ok(index);
        }
        let index = IndexSpec {
            name: format!("slab_idx_{table}_{column}").replace(' ', "_"),
            table: table.to_owned(),
            columns: vec![column.to_owned()],
            unique: false,
        };
        let command = Command::CreateIndex {
            index: index.clone(),
        };
        self.apply_command(&command)?;
        Ok(index)
    }

    /// Drops a user-created index.
    pub fn drop_index(&mut self, name: &str) -> Result<(), StorageError> {
        let schema = self.schema()?;
        let mut found = None;
        for table in &schema.tables {
            if let Some(index) = list_user_indexes(&self.conn, &table.name)?
                .into_iter()
                .find(|index| index.name == name)
            {
                found = Some(index);
                break;
            }
        }
        let index = found.ok_or_else(|| StorageError::NotFound(format!("index {name}")))?;
        let command = Command::DropIndex { index };
        self.apply_command(&command)?;
        Ok(())
    }

    /// Inserts or updates a form layout.
    pub fn save_layout(&mut self, mut layout: Layout) -> Result<Layout, StorageError> {
        let schema = self.schema()?;
        if layout.id.is_empty() {
            layout.id = self.next_id("l");
        }
        layout.updated_at = now_ms();
        layout.validate(&schema)?;
        let before = schema
            .layouts
            .iter()
            .find(|existing| existing.id == layout.id)
            .cloned();
        let command = Command::SaveLayout {
            before,
            after: layout.clone(),
        };
        self.apply_command(&command)?;
        Ok(layout)
    }

    /// Deletes a form layout.
    pub fn delete_layout(&mut self, id: &str) -> Result<(), StorageError> {
        let schema = self.schema()?;
        let layout = schema
            .layouts
            .iter()
            .find(|layout| layout.id == id)
            .cloned()
            .ok_or_else(|| StorageError::NotFound(format!("layout {id}")))?;
        let command = Command::DeleteLayout { layout };
        self.apply_command(&command)?;
        Ok(())
    }

    /// Inserts a row and returns the stored record.
    pub fn insert_row(
        &mut self,
        table: &str,
        values: &RowData,
    ) -> Result<slab_core::Record, StorageError> {
        let schema = self.schema()?;
        let definition = schema
            .table(table)
            .ok_or_else(|| StorageError::NotFound(table.to_owned()))?;
        let prepared = prepare_row(definition, values, None, true)?;
        let command = Command::InsertRow {
            table: table.to_owned(),
            row: slab_core::Record {
                id: 0,
                values: prepared,
            },
            children: Vec::new(),
        };
        let applied = self.apply_command(&command)?;
        let id = applied.row_id().ok_or_else(|| {
            StorageError::Internal("insert command did not carry a row id".into())
        })?;
        self.read_record(table, id)
    }

    /// Inserts several rows in one transaction. Every row still receives its
    /// own audit entry and undo command, so undo semantics match single inserts.
    pub fn insert_rows(
        &mut self,
        table: &str,
        rows: &[RowData],
    ) -> Result<Vec<slab_core::Record>, StorageError> {
        if rows.is_empty() {
            return Ok(Vec::new());
        }
        let schema = self.schema()?;
        let definition = schema
            .table(table)
            .ok_or_else(|| StorageError::NotFound(table.to_owned()))?;
        let mut prepared = Vec::with_capacity(rows.len());
        for values in rows {
            prepared.push(prepare_row(definition, values, None, true)?);
        }
        let hlc = self.clock.now();
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let mut commands = Vec::with_capacity(prepared.len());
        for values in prepared {
            let row = slab_core::Record { id: 0, values };
            let id = insert_record_tx(&tx, definition, &row, false)?;
            let applied = slab_core::Record {
                id,
                values: row.values,
            };
            index_fts(&tx, definition, id, &applied.values)?;
            rollup::maintain_insert(&tx, &schema, table, &applied)?;
            let command = Command::InsertRow {
                table: table.to_owned(),
                row: applied,
                children: Vec::new(),
            };
            append_audit(&tx, &command, &hlc, &self.actor, "insert", &command.label())?;
            commands.push(command);
        }
        tx.commit()?;
        for command in &commands {
            self.push_undo(command.clone());
        }
        let mut records = Vec::with_capacity(commands.len());
        for command in commands {
            if let Command::InsertRow { row, .. } = command {
                records.push(self.read_record(table, row.id)?);
            }
        }
        Ok(records)
    }

    /// Starts an all-or-nothing import transaction.
    ///
    /// Rows written through [`Self::insert_import_rows`] share one SQLite
    /// transaction. Nothing is visible to other connections, and no undo entry
    /// is published, until [`Self::commit_import`] succeeds. Call
    /// [`Self::rollback_import`] on every error path so a failed import leaves
    /// zero half-committed rows.
    pub fn begin_import(&mut self) -> Result<(), StorageError> {
        if self.pending_import.is_some() || !self.conn.is_autocommit() {
            return Err(StorageError::Command(
                "an import transaction is already active".into(),
            ));
        }
        self.conn.execute_batch("BEGIN IMMEDIATE")?;
        self.pending_import = Some(Vec::new());
        Ok(())
    }

    /// Inserts one validated batch inside the active import transaction.
    ///
    /// Every row receives its own audit entry, matching a normal insert.
    pub fn insert_import_rows(
        &mut self,
        table: &str,
        rows: &[RowData],
    ) -> Result<Vec<RowId>, StorageError> {
        if self.pending_import.is_none() {
            return Err(StorageError::Command(
                "no import transaction is active".into(),
            ));
        }
        if rows.is_empty() {
            return Ok(Vec::new());
        }
        let schema = self.schema()?;
        let definition = schema
            .table(table)
            .ok_or_else(|| StorageError::NotFound(table.to_owned()))?;
        let mut prepared = Vec::with_capacity(rows.len());
        for values in rows {
            prepared.push(prepare_row(definition, values, None, true)?);
        }
        let hlc = self.clock.now();
        let mut commands = Vec::with_capacity(prepared.len());
        let mut ids = Vec::with_capacity(prepared.len());
        for values in prepared {
            let row = slab_core::Record { id: 0, values };
            let id = insert_record_tx(&self.conn, definition, &row, false)?;
            let applied = slab_core::Record {
                id,
                values: row.values,
            };
            index_fts(&self.conn, definition, id, &applied.values)?;
            rollup::maintain_insert(&self.conn, &schema, table, &applied)?;
            let command = Command::InsertRow {
                table: table.to_owned(),
                row: applied,
                children: Vec::new(),
            };
            append_audit(
                &self.conn,
                &command,
                &hlc,
                &self.actor,
                "insert",
                &command.label(),
            )?;
            commands.push(command);
            ids.push(id);
        }
        if let Some(pending) = self.pending_import.as_mut() {
            pending.extend(commands);
        }
        Ok(ids)
    }

    /// Commits the active import transaction and publishes its undo entries.
    pub fn commit_import(&mut self) -> Result<(), StorageError> {
        let Some(commands) = self.pending_import.take() else {
            return Err(StorageError::Command(
                "no import transaction is active".into(),
            ));
        };
        match self.conn.execute_batch("COMMIT") {
            Ok(()) => {
                for command in commands {
                    self.push_undo(command);
                }
                Ok(())
            }
            Err(error) => {
                let _ = self.conn.execute_batch("ROLLBACK");
                Err(error.into())
            }
        }
    }

    /// Rolls back the active import transaction, discarding every row.
    pub fn rollback_import(&mut self) -> Result<(), StorageError> {
        if self.pending_import.take().is_none() {
            return Err(StorageError::Command(
                "no import transaction is active".into(),
            ));
        }
        self.conn.execute_batch("ROLLBACK")?;
        Ok(())
    }

    /// Updates a row from a partial column map and returns the stored record.
    pub fn update_row(
        &mut self,
        table: &str,
        id: RowId,
        changes: &RowData,
    ) -> Result<slab_core::Record, StorageError> {
        let schema = self.schema()?;
        let definition = schema
            .table(table)
            .ok_or_else(|| StorageError::NotFound(table.to_owned()))?;
        let before = read_stored_record(&self.conn, definition, id)?
            .ok_or_else(|| StorageError::NotFound(format!("{table} #{id}")))?;
        let prepared = prepare_row(definition, changes, Some(&before.values), false)?;
        let after = slab_core::Record {
            id,
            values: prepared,
        };
        let command = Command::UpdateRow {
            table: table.to_owned(),
            before: before.clone(),
            after,
        };
        self.apply_command(&command)?;
        self.read_record(table, id)
    }

    /// Deletes a row and captures cascade-deleted children for undo.
    pub fn delete_row(&mut self, table: &str, id: RowId) -> Result<(), StorageError> {
        let schema = self.schema()?;
        let definition = schema
            .table(table)
            .ok_or_else(|| StorageError::NotFound(table.to_owned()))?;
        let row = read_stored_record(&self.conn, definition, id)?
            .ok_or_else(|| StorageError::NotFound(format!("{table} #{id}")))?;
        let hlc = self.clock.now();
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let mut children = Vec::new();
        capture_cascades(&tx, &schema, table, id, &mut children)?;
        delete_row_tx(&tx, table, id)?;
        delete_fts(&tx, table, id)?;
        rollup::maintain_delete(&tx, &schema, table, &row)?;
        rollup::forget_parent(&tx, table, id)?;
        for child in &children {
            delete_fts(&tx, &child.table, child.row.id)?;
            rollup::maintain_delete(&tx, &schema, &child.table, &child.row)?;
            rollup::forget_parent(&tx, &child.table, child.row.id)?;
        }
        let command = Command::DeleteRow {
            table: table.to_owned(),
            row,
            children,
        };
        append_audit(&tx, &command, &hlc, &self.actor, "delete", &command.label())?;
        tx.commit()?;
        self.push_undo(command);
        Ok(())
    }

    /// Clears the in-memory undo and redo history.
    pub fn clear_history(&mut self) {
        self.undo.clear();
        self.redo.clear();
    }

    /// Undo and redo availability.
    pub fn undo_state(&self) -> UndoState {
        UndoState {
            can_undo: !self.undo.is_empty(),
            can_redo: !self.redo.is_empty(),
            undo_label: self.undo.last().map(Command::label),
            redo_label: self.redo.last().map(Command::label),
            depth: u32::try_from(self.undo.len()).unwrap_or(u32::MAX),
        }
    }

    /// Reverses the most recent command.
    pub fn undo(&mut self) -> Result<UndoState, StorageError> {
        let Some(command) = self.undo.pop() else {
            return Err(StorageError::NothingToUndo);
        };
        let inverse = command.inverse();
        let summary = format!("Undo {}", command.label());
        match self.apply_command_with_audit(&inverse, "undo", &summary) {
            Ok(_) => {
                self.redo.push(command);
                Ok(self.undo_state())
            }
            Err(error) => {
                self.undo.push(command);
                Err(error)
            }
        }
    }

    /// Reapplies the most recently undone command.
    pub fn redo(&mut self) -> Result<UndoState, StorageError> {
        let Some(command) = self.redo.pop() else {
            return Err(StorageError::NothingToRedo);
        };
        let summary = format!("Redo {}", command.label());
        match self.apply_command_with_audit(&command, "redo", &summary) {
            Ok(_) => {
                // Keep the remaining redo stack intact. `push_undo` clears it
                // for new edits, but a redo is a replay, not a new edit.
                self.undo.push(command);
                if self.undo.len() > UNDO_LIMIT {
                    self.undo.remove(0);
                }
                Ok(self.undo_state())
            }
            Err(error) => {
                self.redo.push(command);
                Err(error)
            }
        }
    }

    /// Recent audit entries, newest first.
    pub fn audit_entries(&self, limit: u32, offset: u32) -> Result<Vec<AuditEntry>, StorageError> {
        let sql = "SELECT id, hlc, timestamp_ms, actor, transaction_id, table_name, row_id,
                          operation, summary, schema_change, entry_hash
                   FROM _slab_audit_log ORDER BY id DESC LIMIT ?1 OFFSET ?2";
        let mut stmt = self.read_conn().prepare(sql)?;
        let rows = stmt.query_map(
            params![i64::from(limit.min(1000)), i64::from(offset)],
            |row| {
                Ok(AuditEntry {
                    id: row.get(0)?,
                    hlc: row.get(1)?,
                    timestamp_ms: row.get(2)?,
                    actor: row.get(3)?,
                    transaction: row.get(4)?,
                    table: row.get(5)?,
                    row_id: row.get(6)?,
                    operation: row.get(7)?,
                    summary: row.get(8)?,
                    schema_change: row.get::<_, i64>(9)? != 0,
                    entry_hash: row.get(10)?,
                })
            },
        )?;
        let mut entries = Vec::new();
        for row in rows {
            entries.push(row?);
        }
        Ok(entries)
    }

    /// Chronological field-level revisions for one record.
    pub fn record_history(
        &self,
        table: &str,
        row_id: RowId,
    ) -> Result<Vec<slab_core::RecordRevision>, StorageError> {
        let sql = "SELECT id, hlc, timestamp_ms, actor, operation, summary, before_json, after_json
                   FROM _slab_audit_log
                   WHERE table_name = ?1 AND row_id = ?2
                   ORDER BY id ASC";
        let mut stmt = self.read_conn().prepare(sql)?;
        let mapped = stmt.query_map(params![table, row_id], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, String>(5)?,
                row.get::<_, Option<String>>(6)?,
                row.get::<_, Option<String>>(7)?,
            ))
        })?;
        let mut revisions = Vec::new();
        for row in mapped {
            let (audit_id, hlc, timestamp_ms, actor, operation, summary, before, after) = row?;
            let before = parse_payload_values(before.as_deref())?;
            let after = parse_payload_values(after.as_deref())?;
            let diffs = field_diffs(before.as_ref(), after.as_ref());
            revisions.push(slab_core::RecordRevision {
                audit_id,
                timestamp_ms,
                hlc,
                actor,
                operation,
                summary,
                diffs,
            });
        }
        Ok(revisions)
    }

    /// Restores one historical revision as a new, audited, undoable mutation.
    pub fn restore_record_revision(
        &mut self,
        table: &str,
        row_id: RowId,
        audit_id: i64,
    ) -> Result<slab_core::Record, StorageError> {
        let schema = self.schema()?;
        FormulaDag::build(&schema)?;
        let definition = schema
            .table(table)
            .ok_or_else(|| StorageError::NotFound(table.to_owned()))?;
        let (before, after): (Option<String>, Option<String>) = self
            .read_conn()
            .query_row(
                "SELECT before_json, after_json FROM _slab_audit_log
                 WHERE id = ?1 AND table_name = ?2 AND row_id = ?3",
                params![audit_id, table, row_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?
            .ok_or_else(|| {
                StorageError::NotFound(format!("audit revision {audit_id} for {table} #{row_id}"))
            })?;
        let target = parse_payload_values(after.as_deref())?
            .or(parse_payload_values(before.as_deref())?)
            .ok_or_else(|| {
                StorageError::Query(format!(
                    "audit revision {audit_id} does not contain a restorable row"
                ))
            })?;
        let current = read_stored_record(self.read_conn(), definition, row_id)?;
        let command = match current {
            Some(before) => {
                let prepared = prepare_row(definition, &target, Some(&before.values), false)?;
                Command::UpdateRow {
                    table: table.to_owned(),
                    before,
                    after: slab_core::Record {
                        id: row_id,
                        values: prepared,
                    },
                }
            }
            None => {
                let prepared = prepare_row(definition, &target, None, true)?;
                Command::InsertRow {
                    table: table.to_owned(),
                    row: slab_core::Record {
                        id: row_id,
                        values: prepared,
                    },
                    children: Vec::new(),
                }
            }
        };
        let summary = format!("Restore {table} #{row_id} revision {audit_id}");
        let applied = self.apply_command_with_audit(&command, "restore", &summary)?;
        self.push_undo(applied);
        self.read_record(table, row_id)
    }

    fn push_undo(&mut self, command: Command) {
        self.undo.push(command);
        if self.undo.len() > UNDO_LIMIT {
            self.undo.remove(0);
        }
        self.redo.clear();
    }

    fn next_id(&mut self, prefix: &str) -> String {
        self.id_seed = self.id_seed.wrapping_add(1);
        format!("{prefix}_{:x}_{:x}", now_ms(), self.id_seed)
    }

    fn apply_command(&mut self, command: &Command) -> Result<Command, StorageError> {
        let applied =
            self.apply_command_with_audit(command, command.operation(), &command.label())?;
        self.push_undo(applied.clone());
        Ok(applied)
    }

    fn apply_command_with_audit(
        &mut self,
        command: &Command,
        operation: &str,
        summary: &str,
    ) -> Result<Command, StorageError> {
        match command {
            Command::CreateRelation { .. } | Command::DropRelation { .. } => self
                .with_foreign_keys_off(|workspace| {
                    workspace.apply_command_tx(command, operation, summary)
                }),
            _ => self.apply_command_tx(command, operation, summary),
        }
    }

    fn with_foreign_keys_off<T>(
        &mut self,
        work: impl FnOnce(&mut Self) -> Result<T, StorageError>,
    ) -> Result<T, StorageError> {
        self.conn.execute_batch("PRAGMA foreign_keys = OFF;")?;
        let outcome = work(self);
        let reenabled = self.conn.execute_batch("PRAGMA foreign_keys = ON;");
        match (outcome, reenabled) {
            (Ok(value), Ok(())) => Ok(value),
            (Err(error), _) => Err(error),
            (Ok(_), Err(error)) => Err(error.into()),
        }
    }

    fn apply_command_tx(
        &mut self,
        command: &Command,
        operation: &str,
        summary: &str,
    ) -> Result<Command, StorageError> {
        let schema = self.schema()?;
        let hlc = self.clock.now();
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let applied = match command {
            Command::InsertRow {
                table,
                row,
                children,
            } => {
                let definition = schema
                    .table(table)
                    .ok_or_else(|| StorageError::NotFound(table.clone()))?;
                let id = insert_record_tx(&tx, definition, row, row.id > 0)?;
                let mut applied_row = row.clone();
                applied_row.id = id;
                index_fts(&tx, definition, id, &applied_row.values)?;
                rollup::maintain_insert(&tx, &schema, table, &applied_row)?;
                for child in children {
                    let child_table = schema
                        .table(&child.table)
                        .ok_or_else(|| StorageError::NotFound(child.table.clone()))?;
                    insert_record_tx(&tx, child_table, &child.row, true)?;
                    index_fts(&tx, child_table, child.row.id, &child.row.values)?;
                    rollup::maintain_insert(&tx, &schema, &child.table, &child.row)?;
                }
                Command::InsertRow {
                    table: table.clone(),
                    row: applied_row,
                    children: children.clone(),
                }
            }
            Command::UpdateRow {
                table,
                before,
                after,
            } => {
                let definition = schema
                    .table(table)
                    .ok_or_else(|| StorageError::NotFound(table.clone()))?;
                update_record_tx(&tx, definition, after)?;
                refresh_fts(&tx, definition, after.id, &after.values)?;
                rollup::maintain_update(&tx, &schema, table, before, after)?;
                let _ = before;
                command.clone()
            }
            Command::DeleteRow {
                table,
                row,
                children,
            } => {
                let _ = schema
                    .table(table)
                    .ok_or_else(|| StorageError::NotFound(table.clone()))?;
                let mut cascaded = children.clone();
                if cascaded.is_empty() {
                    capture_cascades(&tx, &schema, table, row.id, &mut cascaded)?;
                }
                delete_row_tx(&tx, table, row.id)?;
                delete_fts(&tx, table, row.id)?;
                rollup::maintain_delete(&tx, &schema, table, row)?;
                rollup::forget_parent(&tx, table, row.id)?;
                for child in &cascaded {
                    delete_fts(&tx, &child.table, child.row.id)?;
                    rollup::maintain_delete(&tx, &schema, &child.table, &child.row)?;
                    rollup::forget_parent(&tx, &child.table, child.row.id)?;
                }
                command.clone()
            }
            Command::CreateTable { table } => {
                let sql = create_table_sql(table, &table.name, &[])?;
                tx.execute_batch(&sql)?;
                insert_table_meta(&tx, table)?;
                command.clone()
            }
            Command::DropTable { table, .. } => {
                tx.execute_batch(&format!("DROP TABLE {}", quote_ident(&table.name)))?;
                tx.execute(
                    "DELETE FROM _slab_tables WHERE name = ?1",
                    params![table.name],
                )?;
                tx.execute(
                    "DELETE FROM _slab_fts WHERE table_name = ?1",
                    params![table.name],
                )?;
                command.clone()
            }
            Command::RestoreTable {
                table,
                rows,
                layouts,
                indexes,
            } => {
                let sql = create_table_sql(table, &table.name, &[])?;
                tx.execute_batch(&sql)?;
                insert_table_meta(&tx, table)?;
                for layout in layouts {
                    insert_layout_meta(&tx, layout)?;
                }
                for row in rows {
                    insert_record_tx(&tx, table, row, true)?;
                    index_fts(&tx, table, row.id, &row.values)?;
                }
                for index in indexes {
                    tx.execute_batch(&create_index_sql(index))?;
                }
                command.clone()
            }
            Command::RenameTable { old_name, new_name } => {
                tx.execute_batch("PRAGMA defer_foreign_keys = ON;")?;
                tx.execute_batch(&format!(
                    "ALTER TABLE {} RENAME TO {}",
                    quote_ident(old_name),
                    quote_ident(new_name)
                ))?;
                tx.execute(
                    "UPDATE _slab_tables SET name = ?1 WHERE name = ?2",
                    params![new_name, old_name],
                )?;
                tx.execute(
                    "UPDATE _slab_columns SET table_name = ?1 WHERE table_name = ?2",
                    params![new_name, old_name],
                )?;
                tx.execute(
                    "UPDATE _slab_relations SET parent_table = ?1 WHERE parent_table = ?2",
                    params![new_name, old_name],
                )?;
                tx.execute(
                    "UPDATE _slab_relations SET child_table = ?1 WHERE child_table = ?2",
                    params![new_name, old_name],
                )?;
                for layout in &schema.layouts {
                    if layout.table == *old_name {
                        let mut updated = layout.clone();
                        updated.table = new_name.clone();
                        // A rename changes identity, not form content, so the
                        // original timestamp survives undo/redo exactly.
                        tx.execute(
                            "UPDATE _slab_layouts SET table_name = ?1, spec_json = ?2
                                WHERE id = ?3",
                            params![new_name, serde_json::to_string(&updated)?, updated.id],
                        )?;
                    }
                }
                tx.execute(
                    "UPDATE _slab_fts SET table_name = ?1 WHERE table_name = ?2",
                    params![new_name, old_name],
                )?;
                let violations: i64 =
                    tx.query_row("SELECT COUNT(*) FROM pragma_foreign_key_check", [], |row| {
                        row.get(0)
                    })?;
                if violations > 0 {
                    return Err(StorageError::ForeignKey(format!(
                        "{violations} rows violate foreign keys after renaming {old_name}"
                    )));
                }
                command.clone()
            }
            Command::RenameColumn {
                table,
                old_name,
                new_name,
            } => {
                let candidate = rename_column_in_schema(&schema, table, old_name, new_name)?;
                let definition = schema
                    .table(table)
                    .ok_or_else(|| StorageError::NotFound(table.clone()))?;
                if definition
                    .column(old_name)
                    .is_some_and(|column| column.ty.is_stored())
                {
                    tx.execute_batch(&format!(
                        "ALTER TABLE {} RENAME COLUMN {} TO {}",
                        quote_ident(table),
                        quote_ident(old_name),
                        quote_ident(new_name)
                    ))?;
                }
                for definition in &candidate.tables {
                    for column in &definition.columns {
                        tx.execute(
                            "UPDATE _slab_columns SET name = ?1, type_json = ?2 WHERE id = ?3",
                            params![column.name, serde_json::to_string(&column.ty)?, column.id],
                        )?;
                    }
                }
                for relation in &candidate.relations {
                    tx.execute(
                        "UPDATE _slab_relations SET parent_table = ?1, child_table = ?2,
                            child_column = ?3, on_delete = ?4 WHERE id = ?5",
                        params![
                            relation.parent_table,
                            relation.child_table,
                            relation.child_column,
                            match relation.on_delete {
                                DeleteAction::Cascade => "cascade",
                                DeleteAction::SetNull => "setNull",
                                DeleteAction::Restrict => "restrict",
                            },
                            relation.id,
                        ],
                    )?;
                }
                for layout in &candidate.layouts {
                    tx.execute(
                        "UPDATE _slab_layouts SET name = ?1, table_name = ?2, spec_json = ?3,
                            updated_at = ?4 WHERE id = ?5",
                        params![
                            layout.name,
                            layout.table,
                            serde_json::to_string(layout)?,
                            layout.updated_at,
                            layout.id
                        ],
                    )?;
                }
                command.clone()
            }
            Command::AddColumn { table, column } => {
                let definition = schema
                    .table(table)
                    .ok_or_else(|| StorageError::NotFound(table.clone()))?;
                if column.ty.is_stored() {
                    let exists: i64 = tx.query_row(
                        &format!("SELECT COUNT(*) FROM {}", quote_ident(table)),
                        [],
                        |row| row.get(0),
                    )?;
                    if column.required && column.default.is_none() && exists > 0 {
                        return Err(StorageError::Command(format!(
                            "{table}.{} cannot be required without a default on a non-empty table",
                            column.name
                        )));
                    }
                    let ddl = column_definition_alter(column)?;
                    tx.execute_batch(&format!(
                        "ALTER TABLE {} ADD COLUMN {ddl}",
                        quote_ident(table)
                    ))?;
                    if column.unique {
                        tx.execute_batch(&unique_index_sql(table, &column.name))?;
                    }
                    let mut updated = definition.clone();
                    updated.columns.push(column.clone());
                    rebuild_fts_for_table(&tx, &updated)?;
                }
                insert_column_meta(&tx, column)?;
                command.clone()
            }
            Command::DropColumn {
                table,
                column,
                values: _,
            } => {
                let definition = schema
                    .table(table)
                    .ok_or_else(|| StorageError::NotFound(table.clone()))?;
                if schema.relations.iter().any(|relation| {
                    relation.child_table == *table && relation.child_column == column.name
                }) {
                    return Err(StorageError::Command(format!(
                        "column {table}.{} is used by a relation",
                        column.name
                    )));
                }
                let relations = schema.relations.clone();
                let mut reduced = definition.clone();
                reduced
                    .columns
                    .retain(|existing| existing.name != column.name);
                rebuild_table(&tx, &reduced, &relations)?;
                tx.execute(
                    "DELETE FROM _slab_columns WHERE table_name = ?1 AND name = ?2",
                    params![table, column.name],
                )?;
                rebuild_fts_for_table(&tx, &reduced)?;
                command.clone()
            }
            Command::RestoreColumn {
                table,
                column,
                values,
            } => {
                let definition = schema
                    .table(table)
                    .ok_or_else(|| StorageError::NotFound(table.clone()))?;
                let mut expanded = definition.clone();
                expanded.columns.push(column.clone());
                expanded.columns.sort_by_key(|candidate| candidate.position);
                let mut staged = expanded.clone();
                if let Some(staged_column) = staged.column_mut(&column.name) {
                    staged_column.required = false;
                }
                rebuild_table(&tx, &staged, &schema.relations)?;
                insert_column_meta(&tx, column)?;
                for row_value in values {
                    tx.execute(
                        &format!(
                            "UPDATE {} SET {} = ?1 WHERE {} = ?2",
                            quote_ident(table),
                            quote_ident(&column.name),
                            quote_ident("id")
                        ),
                        params![
                            encode_value(&row_value.value, &column.ty)?,
                            row_value.row_id
                        ],
                    )?;
                }
                if column.required && column.ty.is_stored() {
                    rebuild_table(&tx, &expanded, &schema.relations)?;
                }
                rebuild_fts_for_table(&tx, &expanded)?;
                command.clone()
            }
            Command::CreateIndex { index } => {
                tx.execute_batch(&create_index_sql(index))?;
                command.clone()
            }
            Command::DropIndex { index } => {
                tx.execute_batch(&format!("DROP INDEX {}", quote_ident(&index.name)))?;
                command.clone()
            }
            Command::CreateRelation { relation } => {
                let child = schema
                    .table(&relation.child_table)
                    .ok_or_else(|| StorageError::NotFound(relation.child_table.clone()))?;
                let mut relations = schema.relations.clone();
                relations.push(relation.clone());
                ensure_relation_values_valid(
                    &tx,
                    &relation.child_table,
                    &relation.child_column,
                    &relation.parent_table,
                )?;
                rebuild_table(&tx, child, &relations)?;
                insert_relation_meta(&tx, relation)?;
                command.clone()
            }
            Command::DropRelation { relation } => {
                let child = schema
                    .table(&relation.child_table)
                    .ok_or_else(|| StorageError::NotFound(relation.child_table.clone()))?;
                tx.execute(
                    "DELETE FROM _slab_relations WHERE id = ?1",
                    params![relation.id],
                )?;
                let relations: Vec<Relation> = schema
                    .relations
                    .iter()
                    .filter(|existing| existing.id != relation.id)
                    .cloned()
                    .collect();
                rebuild_table(&tx, child, &relations)?;
                command.clone()
            }
            Command::SaveLayout { before, after } => {
                let before = before.clone();
                tx.execute(
                    "INSERT INTO _slab_layouts (id, name, table_name, spec_json, updated_at)
                     VALUES (?1, ?2, ?3, ?4, ?5)
                     ON CONFLICT(id) DO UPDATE SET
                        name = excluded.name,
                        table_name = excluded.table_name,
                        spec_json = excluded.spec_json,
                        updated_at = excluded.updated_at",
                    params![
                        after.id,
                        after.name,
                        after.table,
                        serde_json::to_string(after)?,
                        after.updated_at
                    ],
                )?;
                Command::SaveLayout {
                    before,
                    after: after.clone(),
                }
            }
            Command::DeleteLayout { layout } => {
                tx.execute(
                    "DELETE FROM _slab_layouts WHERE id = ?1",
                    params![layout.id],
                )?;
                command.clone()
            }
        };
        append_audit(&tx, &applied, &hlc, &self.actor, operation, summary)?;
        let schema_change = command.is_schema_change();
        if schema_change {
            rollup::clear(&tx)?;
        }
        tx.commit()?;
        if schema_change {
            self.refresh_readers();
        }
        Ok(applied)
    }
}

fn configure(conn: &Connection) -> Result<(), StorageError> {
    conn.busy_timeout(Duration::from_millis(5_000))?;
    conn.execute_batch(
        "PRAGMA foreign_keys = ON;
         PRAGMA journal_mode = WAL;
         PRAGMA synchronous = NORMAL;
         PRAGMA temp_store = MEMORY;
         PRAGMA cache_size = -64000;
         PRAGMA mmap_size = 268435456;",
    )?;
    Ok(())
}

fn configure_read_only(conn: &Connection) -> Result<(), StorageError> {
    conn.execute_batch(
        "PRAGMA query_only = ON;
         PRAGMA temp_store = MEMORY;
         PRAGMA cache_size = -64000;
         PRAGMA mmap_size = 268435456;",
    )?;
    Ok(())
}

fn open_readers(path: &Path) -> Vec<Connection> {
    let mut readers = Vec::with_capacity(READER_POOL_SIZE);
    for _ in 0..READER_POOL_SIZE {
        let Ok(conn) = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY) else {
            break;
        };
        if conn.busy_timeout(Duration::from_millis(5_000)).is_err() {
            continue;
        }
        if conn
            .execute_batch(
                "PRAGMA query_only = ON;
                 PRAGMA temp_store = MEMORY;
                 PRAGMA cache_size = -64000;
                 PRAGMA mmap_size = 268435456;",
            )
            .is_err()
        {
            continue;
        }
        readers.push(conn);
    }
    readers
}

fn migrate(conn: &Connection) -> Result<(), StorageError> {
    conn.execute_batch(SYSTEM_DDL)?;
    ensure_audit_columns(conn)?;
    ensure_audit_chain(conn)?;
    Ok(())
}

fn ensure_audit_columns(conn: &Connection) -> Result<(), StorageError> {
    let mut columns = Vec::new();
    {
        let mut stmt = conn.prepare("PRAGMA table_info(_slab_audit_log)")?;
        let rows = stmt.query_map([], |row| row.get::<_, String>(1))?;
        for row in rows {
            columns.push(row?);
        }
    }
    if !columns.iter().any(|name| name == "prev_hash") {
        conn.execute_batch("ALTER TABLE _slab_audit_log ADD COLUMN prev_hash TEXT;")?;
    }
    if !columns.iter().any(|name| name == "entry_hash") {
        conn.execute_batch("ALTER TABLE _slab_audit_log ADD COLUMN entry_hash TEXT;")?;
    }
    Ok(())
}

fn ensure_audit_chain(conn: &Connection) -> Result<(), StorageError> {
    let missing: i64 = conn.query_row(
        "SELECT COUNT(*) FROM _slab_audit_log
         WHERE prev_hash IS NULL OR prev_hash = '' OR entry_hash IS NULL OR entry_hash = ''",
        [],
        |row| row.get(0),
    )?;
    if missing > 0 {
        return backfill_audit_chain(conn);
    }
    if read_meta(conn, "audit_head_hash")?.is_none() {
        let last: Option<(i64, String)> = conn
            .query_row(
                "SELECT id, entry_hash FROM _slab_audit_log ORDER BY id DESC LIMIT 1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        match last {
            Some((id, hash)) => {
                set_meta(conn, "audit_head_hash", &hash)?;
                set_meta(conn, "audit_head_id", &id.to_string())?;
            }
            None => {
                set_meta(conn, "audit_head_hash", GENESIS_HASH)?;
                set_meta(conn, "audit_head_id", "0")?;
            }
        }
    }
    Ok(())
}

fn backfill_audit_chain(conn: &Connection) -> Result<(), StorageError> {
    let rows = {
        let mut stmt = conn.prepare(
            "SELECT id, hlc, table_name, row_id, operation, summary, schema_change,
                    before_json, after_json
             FROM _slab_audit_log ORDER BY id ASC",
        )?;
        let mapped = stmt.query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<String>>(2)?,
                row.get::<_, Option<i64>>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, String>(5)?,
                row.get::<_, i64>(6)? != 0,
                row.get::<_, Option<String>>(7)?,
                row.get::<_, Option<String>>(8)?,
            ))
        })?;
        let mut collected = Vec::new();
        for row in mapped {
            collected.push(row?);
        }
        collected
    };
    let mut previous = GENESIS_HASH.to_owned();
    let mut last_id = 0_i64;
    for (id, hlc, table, row_id, operation, summary, schema_change, before, after) in rows {
        let diff = audit_diff_json(
            &operation,
            &summary,
            schema_change,
            before.as_deref(),
            after.as_deref(),
        )?;
        let hash = audit_hash(&previous, &hlc, table.as_deref(), row_id, &diff);
        conn.execute(
            "UPDATE _slab_audit_log SET prev_hash = ?1, entry_hash = ?2 WHERE id = ?3",
            params![previous, hash, id],
        )?;
        previous.clone_from(&hash);
        last_id = id;
    }
    set_meta(conn, "audit_head_hash", &previous)?;
    set_meta(conn, "audit_head_id", &last_id.to_string())?;
    Ok(())
}

fn set_meta(conn: &Connection, key: &str, value: &str) -> Result<(), StorageError> {
    conn.execute(
        "INSERT INTO _slab_meta (key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![key, value],
    )?;
    Ok(())
}

fn audit_diff_json(
    operation: &str,
    summary: &str,
    schema_change: bool,
    before: Option<&str>,
    after: Option<&str>,
) -> Result<String, StorageError> {
    Ok(serde_json::to_string(&serde_json::json!({
        "operation": operation,
        "summary": summary,
        "schemaChange": schema_change,
        "before": before,
        "after": after,
    }))?)
}

fn audit_hash(
    previous: &str,
    hlc: &str,
    table: Option<&str>,
    row_id: Option<i64>,
    diff: &str,
) -> String {
    let mut hasher = blake3::Hasher::new();
    hasher.update(previous.as_bytes());
    hasher.update(b"\x1f");
    hasher.update(hlc.as_bytes());
    hasher.update(b"\x1f");
    hasher.update(table.unwrap_or_default().as_bytes());
    hasher.update(b"\x1f");
    hasher.update(
        row_id
            .map(|id| id.to_string())
            .unwrap_or_default()
            .as_bytes(),
    );
    hasher.update(b"\x1f");
    hasher.update(diff.as_bytes());
    hasher.finalize().to_hex().to_string()
}

fn verify_audit_chain(conn: &Connection) -> Result<(bool, u64, Option<String>), StorageError> {
    let mut expected = GENESIS_HASH.to_owned();
    let mut count = 0_u64;
    let mut last_id = 0_i64;
    let mut last_hash = expected.clone();
    {
        let mut stmt = conn.prepare(
            "SELECT id, hlc, table_name, row_id, operation, summary, schema_change,
                    before_json, after_json, prev_hash, entry_hash
             FROM _slab_audit_log ORDER BY id ASC",
        )?;
        let mut rows = stmt.query([])?;
        while let Some(row) = rows.next()? {
            let id: i64 = row.get(0)?;
            let hlc: String = row.get(1)?;
            let table: Option<String> = row.get(2)?;
            let row_id: Option<i64> = row.get(3)?;
            let operation: String = row.get(4)?;
            let summary: String = row.get(5)?;
            let schema_change: bool = row.get::<_, i64>(6)? != 0;
            let before: Option<String> = row.get(7)?;
            let after: Option<String> = row.get(8)?;
            let prev: Option<String> = row.get(9)?;
            let entry: Option<String> = row.get(10)?;
            if prev.as_deref() != Some(expected.as_str()) {
                return Ok((
                    false,
                    count,
                    Some(format!(
                        "audit entry {id} has a broken prev_hash link; entries were altered or removed"
                    )),
                ));
            }
            let diff = audit_diff_json(
                &operation,
                &summary,
                schema_change,
                before.as_deref(),
                after.as_deref(),
            )?;
            let computed = audit_hash(&expected, &hlc, table.as_deref(), row_id, &diff);
            if entry.as_deref() != Some(computed.as_str()) {
                return Ok((
                    false,
                    count,
                    Some(format!(
                        "audit entry {id} failed BLAKE3 verification; its content was altered outside SlabBase"
                    )),
                ));
            }
            expected = computed;
            count = count.saturating_add(1);
            last_id = id;
            last_hash.clone_from(&expected);
        }
    }
    let head_hash = read_meta(conn, "audit_head_hash")?;
    let head_id = read_meta(conn, "audit_head_id")?.and_then(|value| value.parse::<i64>().ok());
    match (head_hash, head_id) {
        (Some(hash), Some(id)) => {
            if count == 0 {
                if hash != GENESIS_HASH || id != 0 {
                    return Ok((
                        false,
                        0,
                        Some("audit chain head points at a missing entry".into()),
                    ));
                }
            } else if hash != last_hash || id != last_id {
                return Ok((
                    false,
                    count,
                    Some(
                        "audit chain head does not match the last entry; the tail was removed"
                            .into(),
                    ),
                ));
            }
        }
        (None, None) if count == 0 => {}
        _ => {
            return Ok((
                false,
                count,
                Some("audit chain head metadata is missing or inconsistent".into()),
            ));
        }
    }
    Ok((true, count, None))
}

fn snapshot_report(
    kind: SnapshotKind,
    path: &Path,
    started: std::time::Instant,
) -> Result<SnapshotReport, StorageError> {
    Ok(SnapshotReport {
        kind,
        elapsed_ms: started.elapsed().as_secs_f64() * 1_000.0,
        bytes: std::fs::metadata(path)?.len(),
    })
}

#[cfg(target_os = "macos")]
#[allow(unsafe_code)]
fn clone_file(source: &Path, target: &Path) -> Result<bool, StorageError> {
    use std::os::unix::ffi::OsStrExt;

    let source_path = CString::new(source.as_os_str().as_bytes())
        .map_err(|_| StorageError::Workspace("snapshot source contains a NUL byte".into()))?;
    let target_path = CString::new(target.as_os_str().as_bytes())
        .map_err(|_| StorageError::Workspace("snapshot target contains a NUL byte".into()))?;
    // SAFETY: both pointers reference NUL-terminated paths that outlive the
    // call, and clonefile does not retain them after returning.
    let result = unsafe { libc::clonefile(source_path.as_ptr(), target_path.as_ptr(), 0) };
    if result == 0 {
        return Ok(true);
    }
    let error = std::io::Error::last_os_error();
    let code = error.raw_os_error().unwrap_or_default();
    if code == libc::EEXIST {
        return Err(StorageError::Workspace(format!(
            "{} already exists",
            target.display()
        )));
    }
    let _ = std::fs::remove_file(target);
    if matches!(code, libc::EXDEV | libc::ENOTSUP | libc::EINVAL) {
        return Ok(false);
    }
    // Any other clone failure is transparently retried with VACUUM INTO.
    Ok(false)
}

#[cfg(not(target_os = "macos"))]
fn clone_file(_source: &Path, _target: &Path) -> Result<bool, StorageError> {
    Ok(false)
}

fn workspace_open_error(error: rusqlite::Error, path: &Path) -> StorageError {
    if let rusqlite::Error::SqliteFailure(failure, _) = &error
        && matches!(
            failure.code,
            rusqlite::ErrorCode::NotADatabase | rusqlite::ErrorCode::DatabaseCorrupt
        )
    {
        return StorageError::Workspace(format!(
            "{} is not a valid SQLite database",
            path.display()
        ));
    }
    error.into()
}

fn initialize(conn: &Connection, name: &str) -> Result<(), StorageError> {
    conn.execute_batch(SYSTEM_DDL)?;
    conn.execute(
        "INSERT OR IGNORE INTO _slab_meta (key, value) VALUES ('format_version', ?1)",
        params![FORMAT_VERSION.to_string()],
    )?;
    conn.execute(
        "INSERT OR IGNORE INTO _slab_meta (key, value) VALUES ('name', ?1)",
        params![name],
    )?;
    conn.execute(
        "INSERT OR IGNORE INTO _slab_meta (key, value) VALUES ('created_at', ?1)",
        params![now_ms().to_string()],
    )?;
    conn.execute_batch(&format!("PRAGMA user_version = {FORMAT_VERSION};"))?;
    Ok(())
}

fn read_meta(conn: &Connection, key: &str) -> Result<Option<String>, StorageError> {
    Ok(conn
        .query_row(
            "SELECT value FROM _slab_meta WHERE key = ?1",
            params![key],
            |row| row.get(0),
        )
        .optional()?)
}

pub(crate) fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| i64::try_from(duration.as_millis()).unwrap_or(i64::MAX))
        .unwrap_or(0)
}

fn prepare_row(
    table: &Table,
    values: &RowData,
    existing: Option<&RowData>,
    inserting: bool,
) -> Result<RowData, StorageError> {
    for key in values.keys() {
        if key == "id" {
            continue;
        }
        let column = table
            .column(key)
            .ok_or_else(|| StorageError::Query(format!("unknown column {}.{key}", table.name)))?;
        if column.ty.is_computed() {
            continue;
        }
    }
    let mut prepared = RowData::new();
    for column in table.stored_columns() {
        let value = if let Some(value) = values.get(&column.name) {
            normalize_value(value, &column.ty)?
        } else if let Some(existing) = existing {
            existing.get(&column.name).cloned().unwrap_or(Value::Null)
        } else if let Some(default) = &column.default {
            normalize_value(default, &column.ty)?
        } else {
            Value::Null
        };
        if column.required && value.is_null() {
            return Err(StorageError::Constraint(format!(
                "{}.{} is required",
                table.name, column.name
            )));
        }
        if !column.ty.accepts(&value) {
            return Err(StorageError::Query(format!(
                "{}.{} does not accept {}",
                table.name,
                column.name,
                value.type_name()
            )));
        }
        prepared.insert(column.name.clone(), value);
    }
    if inserting {
        for key in values.keys() {
            if key != "id" && table.column(key).is_none() {
                return Err(StorageError::Query(format!(
                    "unknown column {}.{key}",
                    table.name
                )));
            }
        }
    }
    Ok(prepared)
}

pub(crate) fn read_stored_record(
    conn: &Connection,
    table: &Table,
    id: RowId,
) -> Result<Option<slab_core::Record>, StorageError> {
    let columns = stored_columns(table);
    let mut projections = vec![quote_ident("id")];
    projections.extend(columns.iter().map(|column| quote_ident(&column.name)));
    let sql = format!(
        "SELECT {} FROM {} WHERE {} = ?1",
        projections.join(", "),
        quote_ident(&table.name),
        quote_ident("id")
    );
    let mut stmt = conn.prepare_cached(&sql)?;
    let mut rows = stmt.query(params![id])?;
    let Some(row) = rows.next()? else {
        return Ok(None);
    };
    let mut values = RowData::new();
    for (index, column) in columns.iter().enumerate() {
        values.insert(
            column.name.clone(),
            decode_value(row, index + 1, &column.ty)?,
        );
    }
    Ok(Some(slab_core::Record { id, values }))
}

fn insert_record_tx(
    tx: &Connection,
    table: &Table,
    record: &slab_core::Record,
    explicit_id: bool,
) -> Result<RowId, StorageError> {
    let columns = stored_columns(table);
    let mut names: Vec<String> = Vec::new();
    let mut placeholders: Vec<String> = Vec::new();
    let mut bound: Vec<rusqlite::types::Value> = Vec::new();
    if explicit_id {
        names.push(quote_ident("id"));
        placeholders.push("?1".into());
        bound.push(rusqlite::types::Value::Integer(record.id));
    }
    for column in &columns {
        let value = record
            .values
            .get(&column.name)
            .cloned()
            .unwrap_or(Value::Null);
        if column.required && value.is_null() {
            return Err(StorageError::Constraint(format!(
                "{}.{} is required",
                table.name, column.name
            )));
        }
        names.push(quote_ident(&column.name));
        placeholders.push(format!("?{}", bound.len() + 1));
        bound.push(encode_value(&value, &column.ty)?);
    }
    let sql = if names.is_empty() {
        format!("INSERT INTO {} DEFAULT VALUES", quote_ident(&table.name))
    } else {
        format!(
            "INSERT INTO {} ({}) VALUES ({})",
            quote_ident(&table.name),
            names.join(", "),
            placeholders.join(", ")
        )
    };
    tx.execute(&sql, rusqlite::params_from_iter(bound))?;
    if explicit_id {
        Ok(record.id)
    } else {
        Ok(tx.last_insert_rowid())
    }
}

fn update_record_tx(
    tx: &Transaction<'_>,
    table: &Table,
    record: &slab_core::Record,
) -> Result<(), StorageError> {
    let columns = stored_columns(table);
    let mut assignments: Vec<String> = Vec::new();
    let mut bound: Vec<rusqlite::types::Value> = Vec::new();
    for column in &columns {
        let value = record
            .values
            .get(&column.name)
            .cloned()
            .unwrap_or(Value::Null);
        if column.required && value.is_null() {
            return Err(StorageError::Constraint(format!(
                "{}.{} is required",
                table.name, column.name
            )));
        }
        bound.push(encode_value(&value, &column.ty)?);
        assignments.push(format!("{} = ?{}", quote_ident(&column.name), bound.len()));
    }
    if assignments.is_empty() {
        return Ok(());
    }
    bound.push(rusqlite::types::Value::Integer(record.id));
    let sql = format!(
        "UPDATE {} SET {} WHERE {} = ?{}",
        quote_ident(&table.name),
        assignments.join(", "),
        quote_ident("id"),
        bound.len()
    );
    let changed = tx.execute(&sql, rusqlite::params_from_iter(bound))?;
    if changed == 0 {
        return Err(StorageError::NotFound(format!(
            "{} #{}",
            table.name, record.id
        )));
    }
    Ok(())
}

fn delete_row_tx(tx: &Transaction<'_>, table: &str, id: RowId) -> Result<(), StorageError> {
    let changed = tx.execute(
        &format!(
            "DELETE FROM {} WHERE {} = ?1",
            quote_ident(table),
            quote_ident("id")
        ),
        params![id],
    )?;
    if changed == 0 {
        return Err(StorageError::NotFound(format!("{table} #{id}")));
    }
    Ok(())
}

fn capture_cascades(
    conn: &Connection,
    schema: &Schema,
    table: &str,
    id: RowId,
    out: &mut Vec<slab_core::RecordRef>,
) -> Result<(), StorageError> {
    for relation in schema.relations.iter().filter(|relation| {
        relation.parent_table == table && relation.on_delete == DeleteAction::Cascade
    }) {
        let Some(child) = schema.table(&relation.child_table) else {
            continue;
        };
        let sql = format!(
            "SELECT {} FROM {} WHERE {} = ?1 ORDER BY {}",
            quote_ident("id"),
            quote_ident(&child.name),
            quote_ident(&relation.child_column),
            quote_ident("id")
        );
        let mut stmt = conn.prepare(&sql)?;
        let ids = stmt.query_map(params![id], |row| row.get::<_, RowId>(0))?;
        let mut child_ids = Vec::new();
        for child_id in ids {
            child_ids.push(child_id?);
        }
        drop(stmt);
        for child_id in child_ids {
            let Some(row) = read_stored_record(conn, child, child_id)? else {
                continue;
            };
            out.push(slab_core::RecordRef {
                table: child.name.clone(),
                row,
            });
            capture_cascades(conn, schema, &child.name, child_id, out)?;
        }
    }
    Ok(())
}

fn rebuild_fts_for_table(tx: &Transaction<'_>, table: &Table) -> Result<(), StorageError> {
    tx.execute(
        "DELETE FROM _slab_fts WHERE table_name = ?1",
        params![table.name],
    )?;
    let columns = stored_columns(table);
    let mut projections = vec![quote_ident("id")];
    projections.extend(columns.iter().map(|column| quote_ident(&column.name)));
    let sql = format!(
        "SELECT {} FROM {}",
        projections.join(", "),
        quote_ident(&table.name)
    );
    let mut stmt = tx.prepare(&sql)?;
    let mut rows = stmt.query([])?;
    while let Some(row) = rows.next()? {
        let id: RowId = row.get(0)?;
        let mut values = RowData::new();
        for (index, column) in columns.iter().enumerate() {
            values.insert(
                column.name.clone(),
                decode_value(row, index + 1, &column.ty)?,
            );
        }
        index_fts(tx, table, id, &values)?;
    }
    Ok(())
}

fn index_fts(
    tx: &Connection,
    table: &Table,
    id: RowId,
    values: &RowData,
) -> Result<(), StorageError> {
    let mut body = String::new();
    for column in table.stored_columns() {
        if let Some(value) = values.get(&column.name) {
            if value.is_null() {
                continue;
            }
            if !body.is_empty() {
                body.push('\n');
            }
            body.push_str(&value.display_string());
        }
    }
    if body.trim().is_empty() {
        return Ok(());
    }
    tx.execute(
        "INSERT INTO _slab_fts (table_name, row_id, body) VALUES (?1, ?2, ?3)",
        params![table.name, id, body],
    )?;
    Ok(())
}

fn refresh_fts(
    tx: &Connection,
    table: &Table,
    id: RowId,
    values: &RowData,
) -> Result<(), StorageError> {
    delete_fts(tx, &table.name, id)?;
    index_fts(tx, table, id, values)
}

fn delete_fts(tx: &Connection, table: &str, id: RowId) -> Result<(), StorageError> {
    tx.execute(
        "DELETE FROM _slab_fts WHERE table_name = ?1 AND CAST(row_id AS INTEGER) = ?2",
        params![table, id],
    )?;
    Ok(())
}

fn insert_table_meta(tx: &Transaction<'_>, table: &Table) -> Result<(), StorageError> {
    tx.execute(
        "INSERT INTO _slab_tables (id, name, created_at) VALUES (?1, ?2, ?3)",
        params![table.id, table.name, table.created_at],
    )?;
    for column in &table.columns {
        insert_column_meta(tx, column)?;
    }
    Ok(())
}

fn insert_column_meta(tx: &Transaction<'_>, column: &Column) -> Result<(), StorageError> {
    tx.execute(
        "INSERT INTO _slab_columns
            (id, table_name, name, type_json, required, unique_flag, default_json, position)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            column.id,
            column.table,
            column.name,
            serde_json::to_string(&column.ty)?,
            i64::from(column.required),
            i64::from(column.unique),
            column
                .default
                .as_ref()
                .map(serde_json::to_string)
                .transpose()?,
            i64::from(column.position),
        ],
    )?;
    Ok(())
}

fn insert_layout_meta(tx: &Transaction<'_>, layout: &Layout) -> Result<(), StorageError> {
    tx.execute(
        "INSERT INTO _slab_layouts (id, name, table_name, spec_json, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![
            layout.id,
            layout.name,
            layout.table,
            serde_json::to_string(layout)?,
            layout.updated_at
        ],
    )?;
    Ok(())
}

fn create_index_sql(index: &IndexSpec) -> String {
    format!(
        "CREATE {}INDEX IF NOT EXISTS {} ON {} ({})",
        if index.unique { "UNIQUE " } else { "" },
        quote_ident(&index.name),
        quote_ident(&index.table),
        index
            .columns
            .iter()
            .map(|column| quote_ident(column))
            .collect::<Vec<_>>()
            .join(", ")
    )
}

fn insert_relation_meta(tx: &Transaction<'_>, relation: &Relation) -> Result<(), StorageError> {
    tx.execute(
        "INSERT INTO _slab_relations (id, name, parent_table, child_table, child_column, on_delete)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            relation.id,
            relation.name,
            relation.parent_table,
            relation.child_table,
            relation.child_column,
            match relation.on_delete {
                DeleteAction::Cascade => "cascade",
                DeleteAction::SetNull => "setNull",
                DeleteAction::Restrict => "restrict",
            },
        ],
    )?;
    Ok(())
}

fn column_definition_alter(column: &Column) -> Result<String, StorageError> {
    let definition = column_definition(column)?;
    // ALTER TABLE cannot add a UNIQUE column; a unique index is created after.
    Ok(definition.replace(" UNIQUE", ""))
}

fn ensure_relation_values_valid(
    tx: &Transaction<'_>,
    child_table: &str,
    child_column: &str,
    parent_table: &str,
) -> Result<(), StorageError> {
    let sql = format!(
        "SELECT COUNT(*) FROM {} AS child
         LEFT JOIN {} AS parent ON child.{} = parent.{}
         WHERE child.{} IS NOT NULL AND parent.{} IS NULL",
        quote_ident(child_table),
        quote_ident(parent_table),
        quote_ident(child_column),
        quote_ident("id"),
        quote_ident(child_column),
        quote_ident("id")
    );
    let violations: i64 = tx.query_row(&sql, [], |row| row.get(0))?;
    if violations > 0 {
        return Err(StorageError::ForeignKey(format!(
            "{violations} child rows in {child_table}.{child_column} have no matching parent"
        )));
    }
    Ok(())
}

fn rebuild_table(
    tx: &Transaction<'_>,
    table: &Table,
    relations: &[Relation],
) -> Result<(), StorageError> {
    let existing_indexes = list_user_indexes(tx, &table.name)?;
    let temporary = format!("{}__slab_rebuild", table.name);
    tx.execute_batch(&format!("DROP TABLE IF EXISTS {}", quote_ident(&temporary)))?;
    let create = create_table_sql(table, &temporary, relations)?;
    tx.execute_batch(&create)?;
    let columns = stored_columns(table);
    let column_list = {
        let mut names = vec![quote_ident("id")];
        names.extend(columns.iter().map(|column| quote_ident(&column.name)));
        names.join(", ")
    };
    tx.execute_batch(&format!(
        "INSERT INTO {} ({}) SELECT {} FROM {}",
        quote_ident(&temporary),
        column_list,
        column_list,
        quote_ident(&table.name)
    ))?;
    tx.execute_batch(&format!("DROP TABLE {}", quote_ident(&table.name)))?;
    tx.execute_batch(&format!(
        "ALTER TABLE {} RENAME TO {}",
        quote_ident(&temporary),
        quote_ident(&table.name)
    ))?;
    for relation in relations
        .iter()
        .filter(|relation| relation.child_table == table.name)
    {
        tx.execute_batch(&relation_index_sql(&table.name, &relation.child_column))?;
    }
    for index in existing_indexes {
        if index
            .columns
            .iter()
            .all(|name| table.column(name).is_some())
        {
            tx.execute_batch(&create_index_sql(&index))?;
        }
    }
    let violations: i64 =
        tx.query_row("SELECT COUNT(*) FROM pragma_foreign_key_check", [], |row| {
            row.get(0)
        })?;
    if violations > 0 {
        return Err(StorageError::ForeignKey(format!(
            "{violations} rows violate foreign keys after rebuilding {}",
            table.name
        )));
    }
    Ok(())
}

fn append_audit(
    tx: &Connection,
    command: &Command,
    hlc: &Hlc,
    actor: &str,
    operation: &str,
    summary: &str,
) -> Result<(), StorageError> {
    let (before, after) = audit_payload(command)?;
    let before = before
        .map(|value| serde_json::to_string(&value))
        .transpose()?;
    let after = after
        .map(|value| serde_json::to_string(&value))
        .transpose()?;
    let diff = audit_diff_json(
        operation,
        summary,
        command.is_schema_change(),
        before.as_deref(),
        after.as_deref(),
    )?;
    let previous = read_meta(tx, "audit_head_hash")?
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| GENESIS_HASH.to_owned());
    let entry = audit_hash(
        &previous,
        &hlc.to_string(),
        command.table_name(),
        command.row_id(),
        &diff,
    );
    tx.execute(
        "INSERT INTO _slab_audit_log
            (hlc, timestamp_ms, actor, transaction_id, table_name, row_id, operation, summary,
             schema_change, before_json, after_json, prev_hash, entry_hash)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
        params![
            hlc.to_string(),
            hlc.wall_ms,
            actor,
            format!("tx-{hlc}"),
            command.table_name(),
            command.row_id(),
            operation,
            summary,
            i64::from(command.is_schema_change()),
            before,
            after,
            previous,
            entry,
        ],
    )?;
    let id = tx.last_insert_rowid();
    set_meta(tx, "audit_head_hash", &entry)?;
    set_meta(tx, "audit_head_id", &id.to_string())?;
    Ok(())
}

fn audit_payload(
    command: &Command,
) -> Result<(Option<serde_json::Value>, Option<serde_json::Value>), StorageError> {
    let row_value = |row: &slab_core::Record, children: &[slab_core::RecordRef]| {
        if children.is_empty() {
            serde_json::to_value(row)
        } else {
            serde_json::to_value(serde_json::json!({
                "row": row,
                "children": children,
            }))
        }
    };
    Ok(match command {
        Command::InsertRow { row, children, .. } => (None, Some(row_value(row, children)?)),
        Command::UpdateRow { before, after, .. } => (
            Some(serde_json::to_value(before)?),
            Some(serde_json::to_value(after)?),
        ),
        Command::DeleteRow { row, children, .. } => (Some(row_value(row, children)?), None),
        Command::CreateTable { table } => (None, Some(serde_json::to_value(table)?)),
        Command::DropTable {
            table,
            layouts,
            indexes,
            ..
        } => (
            Some(serde_json::json!({
                "table": table,
                "layouts": layouts,
                "indexes": indexes,
            })),
            None,
        ),
        Command::RestoreTable {
            table,
            layouts,
            indexes,
            ..
        } => (
            None,
            Some(serde_json::json!({
                "table": table,
                "layouts": layouts,
                "indexes": indexes,
            })),
        ),
        Command::RenameTable { old_name, new_name } => (
            Some(serde_json::json!({"name": old_name})),
            Some(serde_json::json!({"name": new_name})),
        ),
        Command::AddColumn { column, .. } => (None, Some(serde_json::to_value(column)?)),
        Command::DropColumn { column, .. } => (Some(serde_json::to_value(column)?), None),
        Command::RestoreColumn { column, .. } => (None, Some(serde_json::to_value(column)?)),
        Command::RenameColumn {
            table,
            old_name,
            new_name,
        } => (
            Some(serde_json::json!({"table": table, "name": old_name})),
            Some(serde_json::json!({"table": table, "name": new_name})),
        ),
        Command::CreateRelation { relation } => (None, Some(serde_json::to_value(relation)?)),
        Command::DropRelation { relation } => (Some(serde_json::to_value(relation)?), None),
        Command::SaveLayout { before, after } => (
            before.as_ref().map(serde_json::to_value).transpose()?,
            Some(serde_json::to_value(after)?),
        ),
        Command::DeleteLayout { layout } => (Some(serde_json::to_value(layout)?), None),
        Command::CreateIndex { index } => (None, Some(serde_json::to_value(index)?)),
        Command::DropIndex { index } => (Some(serde_json::to_value(index)?), None),
    })
}

fn parse_payload_values(json: Option<&str>) -> Result<Option<RowData>, StorageError> {
    let Some(json) = json else {
        return Ok(None);
    };
    let value: serde_json::Value = serde_json::from_str(json)?;
    let record = value.get("row").unwrap_or(&value);
    let Some(values) = record.get("values") else {
        return Ok(None);
    };
    let parsed: RowData = serde_json::from_value(values.clone())?;
    Ok(Some(parsed))
}

fn field_diffs(before: Option<&RowData>, after: Option<&RowData>) -> Vec<FieldDiff> {
    let mut names: BTreeSet<&String> = BTreeSet::new();
    if let Some(before) = before {
        names.extend(before.keys());
    }
    if let Some(after) = after {
        names.extend(after.keys());
    }
    let mut diffs = Vec::new();
    for name in names {
        let old = before.and_then(|values| values.get(name));
        let new = after.and_then(|values| values.get(name));
        if old != new {
            diffs.push(FieldDiff {
                column: name.clone(),
                before: old.cloned(),
                after: new.cloned(),
            });
        }
    }
    diffs
}

fn rename_column_in_schema(
    schema: &Schema,
    table: &str,
    old_name: &str,
    new_name: &str,
) -> Result<Schema, StorageError> {
    let mut candidate = schema.clone();
    let child_relations: Vec<String> = schema
        .relations
        .iter()
        .filter(|relation| relation.child_table == table)
        .map(|relation| relation.name.clone())
        .collect();
    for definition in &mut candidate.tables {
        for column in &mut definition.columns {
            match &mut column.ty {
                ColumnType::Formula { expression } => {
                    let parsed = parse_formula(expression)?;
                    let bare = parsed.bare_columns();
                    let qualified = parsed.qualified_columns();
                    if definition.name == table && bare.iter().any(|name| name == old_name) {
                        *expression = rewrite_column_source(expression, None, old_name, new_name)?;
                    }
                    for relation in &child_relations {
                        if qualified
                            .iter()
                            .any(|(name, column)| name == relation && column == old_name)
                        {
                            *expression = rewrite_column_source(
                                expression,
                                Some(relation),
                                old_name,
                                new_name,
                            )?;
                        }
                    }
                }
                ColumnType::Lookup {
                    relation, column, ..
                } if schema
                    .relation(relation)
                    .is_some_and(|relation| relation.child_table == table)
                    && column == old_name =>
                {
                    *column = new_name.to_owned();
                }
                _ => {}
            }
            if definition.name == table && column.name == old_name {
                column.name = new_name.to_owned();
            }
        }
    }
    for relation in &mut candidate.relations {
        if relation.child_table == table && relation.child_column == old_name {
            relation.child_column = new_name.to_owned();
        }
    }
    for layout in &mut candidate.layouts {
        let mut changed = false;
        if layout.table == table {
            for item in &mut layout.items {
                if let LayoutItemKind::Field { column } = &mut item.kind
                    && column == old_name
                {
                    *column = new_name.to_owned();
                    changed = true;
                }
            }
        }
        for item in &mut layout.items {
            if let LayoutItemKind::Portal {
                relation, columns, ..
            } = &mut item.kind
            {
                let is_renamed_table = schema
                    .relation(relation)
                    .is_some_and(|relation| relation.child_table == table);
                if is_renamed_table {
                    for column in columns {
                        if column == old_name {
                            *column = new_name.to_owned();
                            changed = true;
                        }
                    }
                }
            }
        }
        if changed {
            layout.updated_at = now_ms();
        }
    }
    candidate.validate()?;
    FormulaDag::build(&candidate)?;
    Ok(candidate)
}

fn formula_reference(
    schema: &Schema,
    table: &str,
    column: &str,
) -> Result<Option<String>, StorageError> {
    for definition in &schema.tables {
        for formula in &definition.columns {
            match &formula.ty {
                ColumnType::Formula { expression } => {
                    let parsed = parse_formula(expression)?;
                    let bare = parsed.bare_columns();
                    if definition.name == table && bare.iter().any(|name| name == column) {
                        return Ok(Some(format!(
                            "formula {}.{}",
                            definition.name, formula.name
                        )));
                    }
                    for (relation_name, child_column) in parsed.qualified_columns() {
                        if schema
                            .relation(&relation_name)
                            .is_some_and(|relation| relation.child_table == table)
                            && child_column == column
                        {
                            return Ok(Some(format!(
                                "formula {}.{}",
                                definition.name, formula.name
                            )));
                        }
                    }
                }
                ColumnType::Lookup {
                    relation,
                    column: child_column,
                    ..
                } if schema
                    .relation(relation)
                    .is_some_and(|relation| relation.child_table == table)
                    && child_column == column =>
                {
                    return Ok(Some(format!("rollup {}.{}", definition.name, formula.name)));
                }
                _ => {}
            }
        }
    }
    Ok(None)
}

fn layout_reference(schema: &Schema, table: &str, column: &str) -> Option<String> {
    for layout in &schema.layouts {
        for item in &layout.items {
            match &item.kind {
                LayoutItemKind::Field {
                    column: item_column,
                } if layout.table == table && item_column == column => {
                    return Some(layout.name.clone());
                }
                LayoutItemKind::Portal {
                    relation, columns, ..
                } if schema
                    .relation(relation)
                    .is_some_and(|relation| relation.child_table == table)
                    && columns.iter().any(|name| name == column) =>
                {
                    return Some(layout.name.clone());
                }
                _ => {}
            }
        }
    }
    None
}

fn stored_records(
    conn: &Connection,
    table: &Table,
) -> Result<Vec<slab_core::Record>, StorageError> {
    let sql = format!(
        "SELECT {} FROM {} ORDER BY {}",
        quote_ident("id"),
        quote_ident(&table.name),
        quote_ident("id")
    );
    let mut stmt = conn.prepare(&sql)?;
    let ids = stmt.query_map([], |row| row.get::<_, RowId>(0))?;
    let mut output = Vec::new();
    for id in ids {
        let id = id?;
        if let Some(record) = read_stored_record(conn, table, id)? {
            output.push(record);
        }
    }
    Ok(output)
}

fn stored_values(
    conn: &Connection,
    table: &Table,
    column: &Column,
) -> Result<Vec<RowValue>, StorageError> {
    if !column.ty.is_stored() {
        return Ok(Vec::new());
    }
    let sql = format!(
        "SELECT {}, {} FROM {} ORDER BY {}",
        quote_ident("id"),
        quote_ident(&column.name),
        quote_ident(&table.name),
        quote_ident("id")
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map([], |row| {
        Ok((row.get::<_, RowId>(0)?, decode_value(row, 1, &column.ty)))
    })?;
    let mut output = Vec::new();
    for row in rows {
        let (row_id, value) = row?;
        output.push(RowValue {
            row_id,
            value: value?,
        });
    }
    Ok(output)
}

fn list_user_indexes(conn: &Connection, table: &str) -> Result<Vec<IndexSpec>, StorageError> {
    let sql = format!("PRAGMA index_list({})", quote_ident(table));
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>(1)?,
            row.get::<_, i64>(2)?,
            row.get::<_, String>(3)?,
        ))
    })?;
    let mut indexes = Vec::new();
    for row in rows {
        let (name, unique, origin) = row?;
        if origin != "c" || !name.starts_with("slab_idx_") {
            continue;
        }
        let info_sql = format!("PRAGMA index_info({})", quote_ident(&name));
        let mut info = conn.prepare(&info_sql)?;
        let columns = info.query_map([], |row| row.get::<_, String>(2))?;
        let mut names = Vec::new();
        for column in columns {
            names.push(column?);
        }
        indexes.push(IndexSpec {
            name,
            table: table.to_owned(),
            columns: names,
            unique: unique != 0,
        });
    }
    Ok(indexes)
}

fn sql_value_literal(value: rusqlite::types::ValueRef<'_>) -> String {
    match value {
        rusqlite::types::ValueRef::Null => "NULL".to_owned(),
        rusqlite::types::ValueRef::Integer(value) => value.to_string(),
        rusqlite::types::ValueRef::Real(value) => {
            if value.is_finite() {
                format!("{value}")
            } else {
                "NULL".to_owned()
            }
        }
        rusqlite::types::ValueRef::Text(bytes) => {
            format!("'{}'", String::from_utf8_lossy(bytes).replace('\'', "''"))
        }
        rusqlite::types::ValueRef::Blob(bytes) => {
            let mut hex = String::with_capacity(bytes.len() * 2);
            for byte in bytes {
                hex.push_str(&format!("{byte:02X}"));
            }
            format!("X'{hex}'")
        }
    }
}
