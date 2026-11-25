//! Audit entries and undo state exposed to the shell.

use serde::{Deserialize, Serialize};

use crate::record::RowId;
use crate::value::Value;

/// One row from `_slab_audit_log`, without the full before/after payloads.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(
    feature = "ts",
    ts(export, export_to = "../../../apps/desktop/src/bindings/")
)]
pub struct AuditEntry {
    /// Monotonic audit id.
    pub id: i64,
    /// Hybrid logical clock reading.
    pub hlc: String,
    /// Physical time in Unix milliseconds.
    pub timestamp_ms: i64,
    /// Actor recorded with the write.
    pub actor: String,
    /// Groups entries written by one transaction.
    pub transaction: String,
    /// Table affected, when applicable.
    pub table: Option<String>,
    /// Row affected, when applicable.
    pub row_id: Option<RowId>,
    /// Operation label such as `insert`, `update`, `undo`.
    pub operation: String,
    /// Short description suitable for a list.
    pub summary: String,
    /// Whether this entry changed schema rather than data.
    pub schema_change: bool,
    /// BLAKE3 hash chaining this entry to its predecessor.
    pub entry_hash: Option<String>,
}

/// One field-level change inside a record revision.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(
    feature = "ts",
    ts(export, export_to = "../../../apps/desktop/src/bindings/")
)]
#[serde(rename_all = "camelCase")]
pub struct FieldDiff {
    /// Column name.
    pub column: String,
    /// Value before the revision, or absent when the field did not exist.
    pub before: Option<Value>,
    /// Value after the revision, or absent when the field was removed.
    pub after: Option<Value>,
}

/// One chronological revision of a single record.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(
    feature = "ts",
    ts(export, export_to = "../../../apps/desktop/src/bindings/")
)]
#[serde(rename_all = "camelCase")]
pub struct RecordRevision {
    /// Audit entry that produced the revision.
    pub audit_id: i64,
    /// Physical time in Unix milliseconds.
    pub timestamp_ms: i64,
    /// Hybrid logical clock reading.
    pub hlc: String,
    /// Actor recorded with the write.
    pub actor: String,
    /// Operation label such as `insert`, `update`, or `restore`.
    pub operation: String,
    /// Short description.
    pub summary: String,
    /// Field-level differences for this revision.
    pub diffs: Vec<FieldDiff>,
}

/// The current undo and redo availability.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(
    feature = "ts",
    ts(export, export_to = "../../../apps/desktop/src/bindings/")
)]
pub struct UndoState {
    /// True when a command can be undone.
    pub can_undo: bool,
    /// True when a command can be redone.
    pub can_redo: bool,
    /// Label of the next undo.
    pub undo_label: Option<String>,
    /// Label of the next redo.
    pub redo_label: Option<String>,
    /// Number of commands currently undoable.
    pub depth: u32,
}
