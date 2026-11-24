//! Errors returned across the Tauri boundary.

use serde::Serialize;
use slab_core::{ErrorCode, SlabError};
use slab_import::ImportError;
use slab_print::PrintError;
use slab_storage::StorageError;
use ts_rs::TS;

/// A serializable error with a stable code.
#[derive(Clone, Debug, Serialize, TS)]
#[ts(export, export_to = "../../src/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct ApiError {
    /// Stable `SLAB_E_*` code.
    pub code: String,
    /// Human-readable message.
    pub message: String,
}

impl ApiError {
    /// Builds an error from a code and message.
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
        }
    }

    /// The no-workspace error.
    pub fn no_workspace() -> Self {
        Self::new(
            "SLAB_E_WORKSPACE",
            "Open or create a workspace before using this command.",
        )
    }

    /// An internal error.
    pub fn internal(message: impl Into<String>) -> Self {
        Self::new("SLAB_E_INTERNAL", message)
    }

    fn from_parts(code: ErrorCode, message: String) -> Self {
        Self {
            code: code.as_str().to_owned(),
            message,
        }
    }
}

impl std::fmt::Display for ApiError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for ApiError {}

impl From<StorageError> for ApiError {
    fn from(error: StorageError) -> Self {
        Self::from_parts(error.code(), error.to_string())
    }
}

impl From<ImportError> for ApiError {
    fn from(error: ImportError) -> Self {
        Self::from_parts(error.code(), error.to_string())
    }
}

impl From<PrintError> for ApiError {
    fn from(error: PrintError) -> Self {
        Self::from_parts(error.code(), error.to_string())
    }
}

impl From<slab_formula::FormulaError> for ApiError {
    fn from(error: slab_formula::FormulaError) -> Self {
        Self::from_parts(error.code(), error.to_string())
    }
}

impl From<std::io::Error> for ApiError {
    fn from(error: std::io::Error) -> Self {
        Self::new("SLAB_E_WORKSPACE", error.to_string())
    }
}

impl From<serde_json::Error> for ApiError {
    fn from(error: serde_json::Error) -> Self {
        Self::new("SLAB_E_INTERNAL", error.to_string())
    }
}

impl From<csv::Error> for ApiError {
    fn from(error: csv::Error) -> Self {
        Self::new("SLAB_E_IMPORT", error.to_string())
    }
}

impl From<tauri::Error> for ApiError {
    fn from(error: tauri::Error) -> Self {
        Self::new("SLAB_E_INTERNAL", error.to_string())
    }
}
