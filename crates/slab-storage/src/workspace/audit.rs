use std::collections::BTreeSet;

use rusqlite::{Connection, OptionalExtension, params};
use slab_core::{
    AuditEntry, ColumnType, Command, FieldDiff, Hlc, RowData, RowId, UndoState, Value,
};
use slab_formula::FormulaDag;

use crate::error::StorageError;

use super::records::{prepare_row, read_stored_record};
use super::{UNDO_LIMIT, Workspace, read_meta};

pub(super) const GENESIS_HASH: &str =
    "0000000000000000000000000000000000000000000000000000000000000000";

impl Workspace {
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
        self.ensure_writer_available()?;
        let Some(command) = self.undo.pop() else {
            return Err(StorageError::NothingToUndo);
        };
        let inverse = command.inverse();
        let summary = format!("Undo {}", command.label());
        let outcome = self
            .check_command_policy(&inverse)
            .and_then(|()| self.apply_command_with_audit(&inverse, "undo", &summary));
        match outcome {
            Ok(applied) => {
                // An external writer may have added descendants since the
                // original insert. Restore the cascade undo actually removed.
                self.redo
                    .push(if matches!(applied, Command::DeleteRow { .. }) {
                        applied.inverse()
                    } else {
                        command
                    });
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
        self.ensure_writer_available()?;
        let Some(command) = self.redo.pop() else {
            return Err(StorageError::NothingToRedo);
        };
        let summary = format!("Redo {}", command.label());
        let outcome = self
            .check_command_policy(&command)
            .and_then(|()| self.apply_command_with_audit(&command, "redo", &summary));
        match outcome {
            Ok(applied) => {
                // Keep the remaining redo stack intact. `push_undo` clears it
                // for new edits, but a redo is a replay, not a new edit.
                self.undo
                    .push(if matches!(applied, Command::DeleteRow { .. }) {
                        applied
                    } else {
                        command
                    });
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

    /// Recent audit entries, newest first. The offset counts visible entries.
    pub fn audit_entries(&self, limit: u32, offset: u32) -> Result<Vec<AuditEntry>, StorageError> {
        let sql = "SELECT id, hlc, timestamp_ms, actor, transaction_id, table_name, row_id,
                          operation, summary, schema_change, entry_hash
                   FROM _slab_audit_log ORDER BY id DESC";
        let mut stmt = self.read_conn().prepare_cached(sql)?;
        let mut rows = stmt.query([])?;
        let mut remaining = limit.min(1000);
        let mut skip = offset;
        let mut entries = Vec::new();
        // Stream a single snapshot and decode a full entry only if it is
        // authorized and belongs to this page.
        while remaining > 0
            && let Some(row) = rows.next()?
        {
            let table: Option<String> = row.get(5)?;
            if table
                .as_deref()
                .is_some_and(|table| !self.policy.can_read_table(table))
            {
                continue;
            }
            if skip > 0 {
                skip -= 1;
                continue;
            }
            entries.push(AuditEntry {
                id: row.get(0)?,
                hlc: row.get(1)?,
                timestamp_ms: row.get(2)?,
                actor: row.get(3)?,
                transaction: row.get(4)?,
                table,
                row_id: row.get(6)?,
                operation: row.get(7)?,
                summary: row.get(8)?,
                schema_change: row.get::<_, i64>(9)? != 0,
                entry_hash: row.get(10)?,
            });
            remaining -= 1;
        }
        Ok(entries)
    }

    /// Chronological field-level revisions for one record.
    pub fn record_history(
        &self,
        table: &str,
        row_id: RowId,
    ) -> Result<Vec<slab_core::RecordRevision>, StorageError> {
        self.check_policy(table)?;
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
            let mut diffs = field_diffs(before.as_ref(), after.as_ref());
            for diff in &mut diffs {
                if let Some(value) = &diff.before
                    && let Some(replacement) = self.policy.redact(table, &diff.column, value)
                {
                    diff.before = Some(replacement);
                }
                if let Some(value) = &diff.after
                    && let Some(replacement) = self.policy.redact(table, &diff.column, value)
                {
                    diff.after = Some(replacement);
                }
            }
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
        self.check_policy(table)?;
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
        let mut target = parse_payload_values(after.as_deref())?
            .or(parse_payload_values(before.as_deref())?)
            .ok_or_else(|| {
                StorageError::Query(format!(
                    "audit revision {audit_id} does not contain a restorable row"
                ))
            })?;
        // Revisions written before select/attachment decode fidelity stored
        // those values as Text. Coerce only when the value is valid for the
        // column so a restore does not fail on its own history.
        for (name, value) in target.iter_mut() {
            let Some(column) = definition.column(name) else {
                continue;
            };
            match (&column.ty, value.clone()) {
                (ColumnType::Select { options }, Value::Text(text))
                    if options.iter().any(|option| option == &text) =>
                {
                    *value = Value::Select(text);
                }
                (ColumnType::AttachmentRef, Value::Text(text)) => {
                    *value = Value::AttachmentRef(text);
                }
                _ => {}
            }
        }
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
        self.apply_row_command_with_readback(&command, "restore", &summary)
    }
}

/// Adds the hash columns when they are missing.
///
/// Returns true only when the file predates the whole hash schema. A modern
/// file whose hashes were NULLed keeps its `prev_hash`/`entry_hash` columns,
/// so `ensure_audit_chain` can tell tampering from a genuine legacy log.
pub(super) fn ensure_audit_columns(conn: &Connection) -> Result<bool, StorageError> {
    let mut columns = Vec::new();
    {
        let mut stmt = conn.prepare("PRAGMA table_info(_slab_audit_log)")?;
        let rows = stmt.query_map([], |row| row.get::<_, String>(1))?;
        for row in rows {
            columns.push(row?);
        }
    }
    let predates_hash_columns = !columns.iter().any(|name| name == "prev_hash")
        && !columns.iter().any(|name| name == "entry_hash")
        && !columns.iter().any(|name| name == "hash_version");
    if !columns.iter().any(|name| name == "prev_hash") {
        conn.execute_batch("ALTER TABLE _slab_audit_log ADD COLUMN prev_hash TEXT;")?;
    }
    if !columns.iter().any(|name| name == "entry_hash") {
        conn.execute_batch("ALTER TABLE _slab_audit_log ADD COLUMN entry_hash TEXT;")?;
    }
    if !columns.iter().any(|name| name == "hash_version") {
        conn.execute_batch(
            "ALTER TABLE _slab_audit_log ADD COLUMN hash_version INTEGER NOT NULL DEFAULT 1;",
        )?;
    }
    Ok(predates_hash_columns)
}

pub(super) fn ensure_audit_chain(
    conn: &Connection,
    predates_hash_columns: bool,
    format_version: u32,
) -> Result<(), StorageError> {
    let total: i64 =
        conn.query_row("SELECT COUNT(*) FROM _slab_audit_log", [], |row| row.get(0))?;
    let missing: i64 = conn.query_row(
        "SELECT COUNT(*) FROM _slab_audit_log
         WHERE prev_hash IS NULL OR prev_hash = '' OR entry_hash IS NULL OR entry_hash = ''",
        [],
        |row| row.get(0),
    )?;
    if missing > 0 {
        // Only a file whose schema predates the hash columns can be a genuine
        // legacy log. NULLing every hash in a modern file (and deleting the
        // head anchor) must not let the next open mint a fresh chain over
        // altered history.
        if predates_hash_columns
            && format_version < 2
            && missing == total
            && read_meta(conn, "audit_head_hash")?.is_none()
        {
            return rewrite_audit_chain(conn);
        }
        // Leave every other missing hash for `verify_audit_chain` to report.
        return Ok(());
    }
    // A missing head anchor on a modern log is tamper evidence, not something
    // to repair by re-anchoring the remaining tail. A fresh empty log verifies
    // without an anchor.
    migrate_audit_hash_current(conn)?;
    Ok(())
}

/// Rows rewritten per pass. The before/after payloads can be large, so the
/// migration must not materialize the whole log.
const AUDIT_REWRITE_CHUNK: i64 = 256;

/// One decoded audit row, shared by verification, migration, and tests so the
/// column order has one definition.
pub(super) struct AuditRow {
    pub(super) id: i64,
    pub(super) hlc: String,
    timestamp_ms: i64,
    actor: String,
    transaction_id: String,
    pub(super) table: Option<String>,
    pub(super) row_id: Option<i64>,
    operation: String,
    summary: String,
    schema_change: bool,
    before: Option<String>,
    after: Option<String>,
    prev_hash: Option<String>,
    entry_hash: Option<String>,
    hash_version: i64,
}

pub(super) const AUDIT_ROW_COLUMNS: &str =
    "id, hlc, timestamp_ms, actor, transaction_id, table_name,
    row_id, operation, summary, schema_change, before_json, after_json, prev_hash, entry_hash,
    hash_version";

impl AuditRow {
    pub(super) fn from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            id: row.get(0)?,
            hlc: row.get(1)?,
            timestamp_ms: row.get(2)?,
            actor: row.get(3)?,
            transaction_id: row.get(4)?,
            table: row.get(5)?,
            row_id: row.get(6)?,
            operation: row.get(7)?,
            summary: row.get(8)?,
            schema_change: row.get::<_, i64>(9)? != 0,
            before: row.get(10)?,
            after: row.get(11)?,
            prev_hash: row.get(12)?,
            entry_hash: row.get(13)?,
            hash_version: row.get(14)?,
        })
    }

    pub(super) fn diff(&self) -> Result<String, StorageError> {
        audit_diff_json(
            &self.operation,
            &self.summary,
            self.schema_change,
            self.before.as_deref(),
            self.after.as_deref(),
        )
    }
}

/// Rewrites every audit entry with the current scheme in one transaction.
///
/// The caller must have verified the current chain first (when it already has
/// hashes) so tampered content is never blessed with a fresh chain. Rows are
/// read and updated in bounded chunks; a big legacy log cannot balloon memory.
fn rewrite_audit_chain(conn: &Connection) -> Result<(), StorageError> {
    // The caller owns the transaction so the legacy column adds and the
    // rewrite commit (or roll back) as one unit.
    let mut previous = GENESIS_HASH.to_owned();
    let mut last_id = 0_i64;
    loop {
        let rows = {
            let mut stmt = conn.prepare(&format!(
                "SELECT {AUDIT_ROW_COLUMNS} FROM _slab_audit_log
                 WHERE id > ?1 ORDER BY id ASC LIMIT ?2"
            ))?;
            let mapped =
                stmt.query_map(params![last_id, AUDIT_REWRITE_CHUNK], AuditRow::from_row)?;
            let mut collected = Vec::new();
            for row in mapped {
                collected.push(row?);
            }
            collected
        };
        if rows.is_empty() {
            break;
        }
        for audit in rows {
            let diff = audit.diff()?;
            let hash = audit_hash_current(
                &previous,
                &audit.hlc,
                audit.timestamp_ms,
                &audit.actor,
                &audit.transaction_id,
                audit.table.as_deref(),
                audit.row_id,
                &diff,
            );
            conn.execute(
                "UPDATE _slab_audit_log
                 SET prev_hash = ?1, entry_hash = ?2, hash_version = ?3 WHERE id = ?4",
                params![previous, hash, AUDIT_HASH_CURRENT, audit.id],
            )?;
            previous.clone_from(&hash);
            last_id = audit.id;
        }
    }
    set_meta(conn, "audit_head_hash", &previous)?;
    set_meta(conn, "audit_head_id", &last_id.to_string())?;
    Ok(())
}

/// Upgrades a valid legacy scheme-1 chain to cover the actor and transaction id.
///
/// A log that does not verify is left byte-for-byte alone; `integrity_check`
/// reports the tamper instead of the migration laundering it.
pub(super) fn migrate_audit_hash_current(conn: &Connection) -> Result<(), StorageError> {
    // The last row decides. New entries are appended with the current scheme and the rewrite is
    // atomic, so a scheme-1 last row means a legacy chain to migrate. A scheme-2 last row
    // means migration already ran (a mixed tail can only follow a tampered,
    // verification-failing log).
    let last: Option<i64> = conn
        .query_row(
            "SELECT hash_version FROM _slab_audit_log ORDER BY id DESC LIMIT 1",
            [],
            |row| row.get(0),
        )
        .optional()?;
    if last.is_none_or(|version| version >= AUDIT_HASH_CURRENT) {
        return Ok(());
    }
    let (valid, _, _) = verify_audit_chain(conn)?;
    if !valid {
        return Ok(());
    }
    rewrite_audit_chain(conn)
}

pub(super) fn set_meta(conn: &Connection, key: &str, value: &str) -> Result<(), StorageError> {
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

/// Legacy chain: covers the previous hash, clock, row identity, and diff.
const AUDIT_HASH_LEGACY: i64 = 1;
/// Current chain: additionally covers the recorded actor and transaction id.
const AUDIT_HASH_CURRENT: i64 = 2;

pub(super) fn audit_hash_legacy(
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

/// A length-prefixed field. Lengths make the scheme-2 preimage unambiguous even
/// when a user-controlled field contains the separator byte.
fn hash_field(hasher: &mut blake3::Hasher, value: &[u8]) {
    hasher.update(&(value.len() as u64).to_be_bytes());
    hasher.update(value);
}

#[expect(
    clippy::too_many_arguments,
    reason = "the hash preimage fields are inherently positional"
)]
fn audit_hash_current(
    previous: &str,
    hlc: &str,
    timestamp_ms: i64,
    actor: &str,
    transaction_id: &str,
    table: Option<&str>,
    row_id: Option<i64>,
    diff: &str,
) -> String {
    let mut hasher = blake3::Hasher::new();
    hash_field(&mut hasher, previous.as_bytes());
    hash_field(&mut hasher, hlc.as_bytes());
    hash_field(&mut hasher, &timestamp_ms.to_be_bytes());
    hash_field(&mut hasher, actor.as_bytes());
    hash_field(&mut hasher, transaction_id.as_bytes());
    hash_field(&mut hasher, table.unwrap_or_default().as_bytes());
    hash_field(
        &mut hasher,
        row_id
            .map(|id| id.to_string())
            .unwrap_or_default()
            .as_bytes(),
    );
    hash_field(&mut hasher, diff.as_bytes());
    hasher.finalize().to_hex().to_string()
}

pub(super) fn verify_audit_chain(
    conn: &Connection,
) -> Result<(bool, u64, Option<String>), StorageError> {
    let mut expected = GENESIS_HASH.to_owned();
    let mut count = 0_u64;
    let mut last_id = 0_i64;
    let mut last_hash = expected.clone();
    {
        let mut stmt = conn.prepare(&format!(
            "SELECT {AUDIT_ROW_COLUMNS} FROM _slab_audit_log ORDER BY id ASC"
        ))?;
        let mut rows = stmt.query([])?;
        while let Some(row) = rows.next()? {
            let audit = AuditRow::from_row(row)?;
            let id = audit.id;
            if audit.prev_hash.as_deref() != Some(expected.as_str()) {
                return Ok((
                    false,
                    count,
                    Some(format!(
                        "audit entry {id} has a broken prev_hash link; entries were altered or removed"
                    )),
                ));
            }
            let diff = audit.diff()?;
            let computed = match audit.hash_version {
                AUDIT_HASH_LEGACY => audit_hash_legacy(
                    &expected,
                    &audit.hlc,
                    audit.table.as_deref(),
                    audit.row_id,
                    &diff,
                ),
                AUDIT_HASH_CURRENT => audit_hash_current(
                    &expected,
                    &audit.hlc,
                    audit.timestamp_ms,
                    &audit.actor,
                    &audit.transaction_id,
                    audit.table.as_deref(),
                    audit.row_id,
                    &diff,
                ),
                other => {
                    return Ok((
                        false,
                        count,
                        Some(format!(
                            "audit entry {id} uses unsupported hash version {other}"
                        )),
                    ));
                }
            };
            if audit.entry_hash.as_deref() != Some(computed.as_str()) {
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

pub(super) fn append_audit(
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
    let hlc_text = hlc.to_string();
    let transaction_id = format!("tx-{hlc}");
    let entry = audit_hash_current(
        &previous,
        &hlc_text,
        hlc.wall_ms,
        actor,
        &transaction_id,
        command.table_name(),
        command.row_id(),
        &diff,
    );
    tx.execute(
        "INSERT INTO _slab_audit_log
            (hlc, timestamp_ms, actor, transaction_id, table_name, row_id, operation, summary,
             schema_change, before_json, after_json, prev_hash, entry_hash, hash_version)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
        params![
            hlc_text,
            hlc.wall_ms,
            actor,
            transaction_id,
            command.table_name(),
            command.row_id(),
            operation,
            summary,
            i64::from(command.is_schema_change()),
            before,
            after,
            previous,
            entry,
            AUDIT_HASH_CURRENT,
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
