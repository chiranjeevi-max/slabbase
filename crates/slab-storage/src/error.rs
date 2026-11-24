//! Storage errors with stable codes.

use slab_core::error::codes;
use slab_core::{CoreError, ErrorCode, SlabError};
use slab_formula::FormulaError;

/// Errors produced by the SQLite workspace.
#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    /// SQLite returned an error.
    #[error("database error: {0}")]
    Sqlite(rusqlite::Error),
    /// A foreign key constraint failed.
    #[error("foreign key violation: {0}")]
    ForeignKey(String),
    /// A unique constraint failed.
    #[error("unique constraint violation: {0}")]
    Unique(String),
    /// A check or not-null constraint failed.
    #[error("constraint violation: {0}")]
    Constraint(String),
    /// JSON serialization failed.
    #[error("serialization error: {0}")]
    Json(serde_json::Error),
    /// The filesystem rejected an operation.
    #[error("filesystem error: {0}")]
    Io(std::io::Error),
    /// A domain operation failed.
    #[error(transparent)]
    Core(CoreError),
    /// A formula failed.
    #[error(transparent)]
    Formula(FormulaError),
    /// The requested row, table, or layout does not exist.
    #[error("not found: {0}")]
    NotFound(String),
    /// A query or filter was malformed.
    #[error("invalid query: {0}")]
    Query(String),
    /// A command was rejected.
    #[error("invalid command: {0}")]
    Command(String),
    /// The file is not a supported workspace.
    #[error("workspace error: {0}")]
    Workspace(String),
    /// An operation is reserved for a later release.
    #[error("unsupported operation: {0}")]
    Unsupported(String),
    /// The undo stack is empty.
    #[error("nothing to undo")]
    NothingToUndo,
    /// The redo stack is empty.
    #[error("nothing to redo")]
    NothingToRedo,
    /// An internal invariant did not hold.
    #[error("internal error: {0}")]
    Internal(String),
}

impl From<rusqlite::Error> for StorageError {
    fn from(error: rusqlite::Error) -> Self {
        if let rusqlite::Error::SqliteFailure(failure, message) = &error {
            let detail = message
                .clone()
                .unwrap_or_else(|| format!("{:?}", failure.code));
            match failure.extended_code {
                787 | 1811 => return Self::ForeignKey(detail),
                1555 | 2067 => return Self::Unique(detail),
                1299 => return Self::Constraint(format!("not null: {detail}")),
                275 => return Self::Constraint(format!("check: {detail}")),
                _ => {}
            }
        }
        Self::Sqlite(error)
    }
}

impl From<serde_json::Error> for StorageError {
    fn from(error: serde_json::Error) -> Self {
        Self::Json(error)
    }
}

impl From<std::io::Error> for StorageError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<CoreError> for StorageError {
    fn from(error: CoreError) -> Self {
        Self::Core(error)
    }
}

impl From<FormulaError> for StorageError {
    fn from(error: FormulaError) -> Self {
        Self::Formula(error)
    }
}

impl SlabError for StorageError {
    fn code(&self) -> ErrorCode {
        match self {
            Self::Sqlite(_) | Self::Json(_) | Self::Internal(_) => codes::STORAGE,
            Self::ForeignKey(_) => codes::FK_VIOLATION,
            Self::Unique(_) => codes::UNIQUE,
            Self::Constraint(_) => codes::TYPE,
            Self::Io(_) | Self::Workspace(_) => codes::WORKSPACE,
            Self::Core(error) => error.code(),
            Self::Formula(error) => error.code(),
            Self::NotFound(_) => codes::NOT_FOUND,
            Self::Query(_) => codes::QUERY,
            Self::Command(_) | Self::Unsupported(_) => codes::COMMAND,
            Self::NothingToUndo | Self::NothingToRedo => codes::UNDO_EMPTY,
        }
    }
}

impl StorageError {
    /// The stable code string.
    pub fn code_str(&self) -> &'static str {
        self.code().as_str()
    }
}
