//! Opening, schema management, write paths, and audit.

use std::cell::Cell;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use rusqlite::{
    Connection, InterruptHandle, OpenFlags, OptionalExtension, TransactionBehavior, params,
};
use slab_core::{
    Clock, Column, ColumnType, Command, DeleteAction, FORMAT_VERSION, Layout, PolicyHook, Relation,
    Schema, Table, TableSummary, Value,
};
use slab_formula::FormulaDag;

use crate::error::StorageError;
use crate::rollup;

mod audit;
mod export;
mod records;
mod schema;
mod write;

use audit::{ensure_audit_chain, ensure_audit_columns, set_meta, verify_audit_chain};
pub(crate) use records::read_stored_record;
use records::rebuild_fts_for_table;

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
    entry_hash TEXT,
    hash_version INTEGER NOT NULL DEFAULT 1
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

/// Identifies one registered query so it can only retire its own entry.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CancelRegistration(u64);

#[derive(Default)]
struct QueryCancelState {
    next_token: AtomicU64,
    active: Mutex<Vec<(u64, InterruptHandle)>>,
}

/// A shared handle that lets the shell interrupt superseded interactive
/// queries without taking the writer lock.
///
/// Several reads can be in flight on different reader replicas at once, so
/// registrations form a set rather than a single slot. A finishing query
/// removes exactly its own token, and an interrupt reaches every query that is
/// still registered on *this* cancel.
#[derive(Clone, Default)]
pub struct QueryCancel {
    state: Arc<QueryCancelState>,
}

impl QueryCancel {
    /// Creates an idle cancellation handle.
    pub fn new() -> Self {
        Self::default()
    }

    /// Interrupts every query currently registered on this handle.
    pub fn interrupt(&self) -> bool {
        match self.state.active.lock() {
            Ok(guard) => {
                for (_, handle) in guard.iter() {
                    handle.interrupt();
                }
                !guard.is_empty()
            }
            Err(_) => false,
        }
    }

    /// Registers a connection interrupt handle and returns its ownership token.
    ///
    /// Storage calls this around interactive reads; it is exposed so tests and
    /// alternate shells can exercise cancellation without a private hook.
    pub fn register(&self, handle: InterruptHandle) -> CancelRegistration {
        let token = self.state.next_token.fetch_add(1, Ordering::Relaxed);
        if let Ok(mut guard) = self.state.active.lock() {
            guard.push((token, handle));
        }
        CancelRegistration(token)
    }

    /// Removes one registration after its query finishes.
    pub fn clear_registration(&self, registration: CancelRegistration) {
        if let Ok(mut guard) = self.state.active.lock() {
            guard.retain(|(token, _)| *token != registration.0);
        }
    }

    /// Removes every registration. Used when a workspace is torn down.
    pub fn clear(&self) {
        if let Ok(mut guard) = self.state.active.lock() {
            guard.clear();
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
    pub(crate) search_cancel: Arc<QueryCancel>,
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
        if let Ok(metadata) = std::fs::symlink_metadata(path)
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
        validate_workspace_schema(&conn)?;
        let readers = open_readers(path);
        let workspace = Self {
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
            search_cancel: Arc::new(QueryCancel::new()),
            read_only: false,
        };
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
        if !looks_like_workspace(&conn).map_err(|error| workspace_open_error(error, path))? {
            return Err(StorageError::Workspace(format!(
                "{} is not a SlabBase workspace",
                path.display()
            )));
        }
        let name = read_meta(&conn, "name")?.unwrap_or_else(|| "Workspace".into());
        let format_version = read_meta(&conn, "format_version")?
            .and_then(|value| value.parse::<u32>().ok())
            .unwrap_or(0);
        if format_version > FORMAT_VERSION {
            return Err(StorageError::Workspace(format!(
                "workspace format {format_version} is newer than this build supports ({FORMAT_VERSION})"
            )));
        }
        // Prove the schema and formula graph are valid before changing journal
        // mode or migrating. A file with an invalid stored formula must be
        // rejected without being rewritten.
        validate_workspace_schema(&conn)?;
        // Only mutate connection settings after the file is known to be a
        // supported workspace. This keeps foreign or newer-format files
        // byte-for-byte intact.
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
            search_cancel: Arc::new(QueryCancel::new()),
            read_only: false,
        };
        // Rollup state is a derived cache. The file can be edited by an
        // external SQLite client between sessions, so a cached total is not
        // trustworthy until a mutation through this engine refreshes it.
        // Dropping the cache here is cheap: reads rebuild lazily.
        rollup::clear(&workspace.conn)?;
        workspace.repair_fts_if_pending()?;
        Ok(workspace)
    }

    /// Rebuilds the derived full-text index when a restored SQL export asks
    /// for it.
    ///
    /// The export omits `_slab_fts` so a plain `sqlite3` can restore the
    /// script without an FTS5 build. The marker is consumed here, once, in a
    /// single transaction.
    fn repair_fts_if_pending(&mut self) -> Result<(), StorageError> {
        if self.read_only || self.path.is_none() {
            return Ok(());
        }
        if read_meta(&self.conn, "fts_pending")?.as_deref() != Some("1") {
            return Ok(());
        }
        let schema = self.schema()?;
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute("DELETE FROM _slab_fts", [])?;
        for table in &schema.tables {
            rebuild_fts_for_table(&tx, table)?;
        }
        tx.execute("DELETE FROM _slab_meta WHERE key = 'fts_pending'", [])?;
        tx.commit()?;
        Ok(())
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
        if !looks_like_workspace(&conn).map_err(|error| workspace_open_error(error, path))? {
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
        validate_workspace_schema(&conn)?;
        let workspace = Self {
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
            search_cancel: Arc::new(QueryCancel::new()),
            read_only: true,
        };
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
            search_cancel: Arc::new(QueryCancel::new()),
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

    /// Shares the page cancellation handle exposed to the shell.
    pub fn set_cancel_handle(&mut self, cancel: Arc<QueryCancel>) {
        self.cancel = cancel;
    }

    /// Shares the search cancellation handle exposed to the shell.
    pub fn set_search_cancel_handle(&mut self, cancel: Arc<QueryCancel>) {
        self.search_cancel = cancel;
    }

    /// Interrupts the active page query, if any.
    pub fn cancel_query(&self) -> bool {
        self.cancel.interrupt()
    }

    /// Interrupts the active full-text search, if any.
    ///
    /// Search has its own handle so superseding a palette query cannot
    /// interrupt a grid or portal page that is still loading.
    pub fn cancel_search_query(&self) -> bool {
        self.search_cancel.interrupt()
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

    /// The connection allowed to store derived rollup state, or `None` for a
    /// read-only replica.
    pub(crate) fn cache_conn(&self) -> Option<&Connection> {
        (!self.read_only).then(|| self.write_conn())
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

    /// Loads the schema from metadata tables.
    pub fn schema(&self) -> Result<Schema, StorageError> {
        load_schema(&self.conn)
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

fn load_schema(conn: &Connection) -> Result<Schema, StorageError> {
    let mut tables: Vec<Table> = Vec::new();
    {
        let mut stmt =
            conn.prepare("SELECT id, name, created_at FROM _slab_tables ORDER BY name")?;
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
        let mut stmt = conn.prepare(
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
            let (id, table_name, name, type_json, required, unique, default_json, position) = row?;
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
        let mut stmt = conn.prepare(
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
        let mut stmt = conn.prepare("SELECT spec_json FROM _slab_layouts ORDER BY name")?;
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

fn validate_workspace_schema(conn: &Connection) -> Result<(), StorageError> {
    let schema = load_schema(conn)?;
    schema.validate()?;
    FormulaDag::build(&schema)?;
    Ok(())
}

fn migrate(conn: &Connection) -> Result<(), StorageError> {
    // Schema addition and hash backfill must commit together. SQLite DDL is
    // transactional; a crash leaves the file untouched instead of with hash
    // columns but no hashes (which would look like tampering forever).
    let tx = conn.unchecked_transaction()?;
    tx.execute_batch(SYSTEM_DDL)?;
    let predates_hash_columns = ensure_audit_columns(&tx)?;
    let format_version = read_meta(&tx, "format_version")?
        .and_then(|value| value.parse::<u32>().ok())
        .unwrap_or(0);
    ensure_audit_chain(&tx, predates_hash_columns, format_version)?;
    // Record the format bump in the same transaction as the hash schema.
    let version = FORMAT_VERSION.to_string();
    if read_meta(&tx, "format_version")?.as_deref() != Some(version.as_str()) {
        set_meta(&tx, "format_version", &version)?;
        tx.execute_batch(&format!("PRAGMA user_version = {FORMAT_VERSION};"))?;
    }
    tx.commit()?;
    Ok(())
}

/// True when the file carries the SlabBase metadata signature.
///
/// The probe performs reads only, so a foreign database is rejected before
/// `configure` changes its journal mode or `migrate` creates metadata tables.
/// `_slab_fts`, `_slab_audit_log`, and `_slab_rollups` are deliberately not
/// required: a SQL export omits them and `migrate` recreates them.
fn looks_like_workspace(conn: &Connection) -> Result<bool, rusqlite::Error> {
    let tables: i64 = conn.query_row(
        "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name IN
            ('_slab_meta', '_slab_tables', '_slab_columns', '_slab_relations', '_slab_layouts')",
        [],
        |row| row.get(0),
    )?;
    if tables != 5 {
        return Ok(false);
    }
    let version: Result<String, rusqlite::Error> = conn.query_row(
        "SELECT value FROM _slab_meta WHERE key = 'format_version'",
        [],
        |row| row.get(0),
    );
    Ok(version.is_ok_and(|value| value.parse::<u32>().is_ok()))
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

#[cfg(test)]
mod audit_tests;

#[cfg(test)]
mod audit_chunk_tests;

#[cfg(test)]
mod audit_provenance_tests;

#[cfg(test)]
mod audit_atomic_tests;

#[cfg(test)]
mod open_validation_tests;

#[cfg(test)]
mod audit_foreign_columns_tests;

#[cfg(test)]
mod restore_compat_tests;
