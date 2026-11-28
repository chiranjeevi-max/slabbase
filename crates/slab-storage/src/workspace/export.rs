use std::collections::BTreeSet;
#[cfg(target_os = "macos")]
use std::ffi::CString;
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};

use rusqlite::{OptionalExtension, params};
use slab_core::FORMAT_VERSION;

use crate::error::StorageError;
use crate::sql::{decode_value, ensure_value_fits, quote_ident, sql_literal};

use super::{SnapshotKind, SnapshotReport, Workspace};

impl Workspace {
    /// Writes a portable SQL script containing user tables and SlabBase
    /// metadata.
    ///
    /// The audit log is excluded, so its `_slab_meta` chain anchors are
    /// excluded too; a restored file starts a fresh genesis chain instead of
    /// claiming a head that has no entries. The derived full-text index is
    /// omitted as well: a plain `sqlite3` must be able to restore the script
    /// without an FTS5 build. A pending marker rebuilds the index once on the
    /// next SlabBase open.
    pub fn export_sql(&self, path: impl AsRef<Path>) -> Result<(), StorageError> {
        let path = path.as_ref();
        self.check_export_policy()?;
        let parent = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        let (temporary, file) = create_scratch_sibling(parent)?;
        let outcome = (|| {
            // Apply the destination's mode before the first row is written.
            if let Ok(metadata) = std::fs::metadata(path) {
                std::fs::set_permissions(&temporary, metadata.permissions())?;
            }
            self.export_sql_to(file)
        })()
        .and_then(|()| {
            std::fs::rename(&temporary, path)?;
            Ok(())
        });
        if outcome.is_err() {
            let _ = std::fs::remove_file(&temporary);
        }
        outcome
    }

    fn check_export_policy(&self) -> Result<(), StorageError> {
        let schema = self.schema()?;
        for table in &schema.tables {
            self.check_policy(&table.name)?;
        }
        Ok(())
    }

    /// Writes the SQL dump to an already-open writer.
    ///
    /// The shell opens a private scratch file with `create_new` and passes the
    /// handle here, so publishing cannot be raced through a predictable path.
    pub fn export_sql_to<W: Write>(&self, writer: W) -> Result<(), StorageError> {
        self.check_export_policy()?;
        let schema = self.schema()?;
        let mut out = BufWriter::new(writer);
        writeln!(out, "-- SlabBase SQL export (format {FORMAT_VERSION})")?;
        writeln!(out, "PRAGMA user_version = {FORMAT_VERSION};")?;
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
            let definition = schema.table(table_name);
            let mut stored = None;
            let sql = if table_name == "_slab_meta" {
                // The audit log is not part of this dump. Carrying its head
                // anchors would make every restored file look tampered.
                format!(
                    "SELECT * FROM {} WHERE key NOT IN
                        ('audit_head_hash', 'audit_head_id', 'fts_pending')",
                    quote_ident(table_name)
                )
            } else if let Some(definition) = definition {
                let mut names = vec![quote_ident("id")];
                let mut columns = Vec::new();
                for column in definition.stored_columns() {
                    names.push(quote_ident(&column.name));
                    columns.push(column);
                }
                stored = Some(columns);
                format!(
                    "SELECT {} FROM {}",
                    names.join(", "),
                    quote_ident(table_name)
                )
            } else {
                format!("SELECT * FROM {}", quote_ident(table_name))
            };
            let mut stmt = self.conn.prepare(&sql)?;
            let column_count = stmt.column_count();
            let columns: Vec<String> = stmt.column_names().map(quote_ident).collect();
            let mut rows = stmt.query([])?;
            while let Some(row) = rows.next()? {
                let mut literals = Vec::with_capacity(column_count);
                for index in 0..column_count {
                    let value = row.get_ref(index)?;
                    let column = stored
                        .as_ref()
                        .and_then(|columns| index.checked_sub(1).and_then(|i| columns.get(i)));
                    let Some(column) = column else {
                        literals.push(sql_value_literal(value));
                        continue;
                    };
                    let decoded = decode_value(row, index, &column.ty)?;
                    match self.policy.redact(table_name, &column.name, &decoded) {
                        Some(replacement) => {
                            ensure_value_fits(&replacement, &column.ty)?;
                            literals.push(sql_literal(&replacement, &column.ty)?);
                        }
                        None => literals.push(sql_value_literal(value)),
                    }
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
        writeln!(
            out,
            "INSERT INTO {} ({}, {}) VALUES ('fts_pending', '1');",
            quote_ident("_slab_meta"),
            quote_ident("key"),
            quote_ident("value")
        )?;
        // Emit every explicit index on an exported user table: advisor indexes
        // (`slab_idx_*`), relation indexes (`slab_ix_*`), and the unique index
        // that `add_column` creates for a late `UNIQUE` column (`slab_uq_*`).
        let exported: BTreeSet<&str> = schema
            .tables
            .iter()
            .map(|table| table.name.as_str())
            .collect();
        let mut indexes = self.conn.prepare(
            "SELECT sql, tbl_name FROM sqlite_master
             WHERE type = 'index' AND sql IS NOT NULL AND name NOT LIKE 'sqlite_%'
             ORDER BY name",
        )?;
        let index_sql = indexes.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;
        for statement in index_sql {
            let (statement, table) = statement?;
            if exported.contains(table.as_str()) {
                writeln!(out, "{statement};")?;
            }
        }
        writeln!(out, "COMMIT;")?;
        out.flush()?;
        Ok(())
    }

    /// Writes a consistent snapshot with `VACUUM INTO` after checkpointing.
    pub fn backup_to(&self, path: impl AsRef<Path>) -> Result<(), StorageError> {
        let path = path.as_ref();
        ensure_absent(path)?;
        self.checkpoint_for_snapshot()?;
        self.vacuum_into_atomic(path)
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
        ensure_absent(path)?;
        let started = std::time::Instant::now();
        let clean = self.checkpoint_for_snapshot()?;
        if prefer_clone
            && clean
            && let Some(source) = &self.path
            && clone_file(source, path)?
        {
            return snapshot_report(SnapshotKind::ApfsClone, path, started);
        }
        self.vacuum_into_atomic(path)?;
        snapshot_report(SnapshotKind::VacuumInto, path, started)
    }

    /// Runs `VACUUM INTO` against a private sibling and publishes the finished
    /// file with an atomic no-replace link.
    ///
    /// Writing directly to the caller's path would leave a partial database if
    /// SQLite or the process failed mid-copy, and every later attempt would
    /// then see a non-empty file and refuse. The temporary file is removed on
    /// every error path.
    fn vacuum_into_atomic(&self, path: &Path) -> Result<(), StorageError> {
        let parent = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        let (temporary, file) = create_scratch_sibling(parent)?;
        drop(file);
        let Some(target) = temporary.to_str() else {
            // A non-UTF-8 parent produces a valid Unix path that `VACUUM INTO`
            // cannot receive as a Rust string. Remove the scratch file before
            // returning so failed snapshots do not accumulate residue.
            let _ = std::fs::remove_file(&temporary);
            return Err(StorageError::Workspace(
                "snapshot path is not valid UTF-8".into(),
            ));
        };
        let outcome = self.conn.execute("VACUUM INTO ?1", params![target]);
        if let Err(error) = outcome {
            let _ = std::fs::remove_file(&temporary);
            return Err(error.into());
        }
        // `hard_link` refuses to replace an existing entry, so a concurrent
        // writer can never be clobbered by the publish step.
        let published = std::fs::hard_link(&temporary, path);
        let _ = std::fs::remove_file(&temporary);
        match published {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => Err(
                StorageError::Workspace(format!("{} already exists", path.display())),
            ),
            Err(error) => Err(error.into()),
        }
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
        // A caller that abandoned an import must not leave an open transaction
        // behind. Roll it back before the close checkpoint so the file that
        // lands on disk contains committed state only.
        if self.pending_import.take().is_some() || !self.conn.is_autocommit() {
            let _ = self.conn.execute_batch("ROLLBACK");
        }
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
}

/// Rejects any existing directory entry, including a dangling symlink that
/// `Path::exists` would report as absent.
fn ensure_absent(path: &Path) -> Result<(), StorageError> {
    match std::fs::symlink_metadata(path) {
        Ok(_) => Err(StorageError::Workspace(format!(
            "{} already exists",
            path.display()
        ))),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}

/// Creates a uniquely named empty scratch file beside the destination.
///
/// `create_new` never follows a symlink and never reuses an entry another
/// process owns.
fn create_scratch_sibling(parent: &Path) -> Result<(PathBuf, File), StorageError> {
    use std::sync::atomic::{AtomicU64, Ordering};

    static COUNTER: AtomicU64 = AtomicU64::new(0);
    for _ in 0..64 {
        let serial = COUNTER.fetch_add(1, Ordering::Relaxed);
        let path = parent.join(format!(
            ".slabbase-scratch-{}-{serial}.tmp",
            std::process::id()
        ));
        match File::create_new(&path) {
            Ok(file) => return Ok((path, file)),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error.into()),
        }
    }
    Err(StorageError::Internal(
        "could not allocate a scratch file".into(),
    ))
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
    // Never unlink here. `clonefile` is atomic and does not create the target
    // on failure, while an unconditional `remove_file` would let a racing
    // writer's file (or a symlink) be deleted based on an unrelated error.
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
        rusqlite::types::ValueRef::Text(bytes) => match std::str::from_utf8(bytes) {
            Ok(text) if !text.contains('\0') => {
                format!("'{}'", text.replace('\'', "''"))
            }
            // A raw NUL terminates SQLite's parser and invalid UTF-8 cannot be
            // represented in a Rust string literal. A hex blob cast back to
            // TEXT preserves the exact bytes through a restore.
            _ => format!("CAST(X'{}' AS TEXT)", hex_bytes(bytes)),
        },
        rusqlite::types::ValueRef::Blob(bytes) => format!("X'{}'", hex_bytes(bytes)),
    }
}

fn hex_bytes(bytes: &[u8]) -> String {
    let mut hex = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        hex.push_str(&format!("{byte:02X}"));
    }
    hex
}
