#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

//! SQLite workspace engine for SlabBase.
//!
//! The crate owns the only database connection in the system. It applies open
//! SQLite pragmas, generates DDL for user tables, validates formulas before
//! schema changes, maintains a full-text index, and writes a before/after audit
//! entry in the same transaction as every mutation.

mod ddl;
mod error;
mod read;
mod seed;
mod sql;
#[cfg(test)]
mod test_support;
mod workspace;

pub use error::StorageError;
pub use workspace::{IntegrityReport, Workspace, WorkspaceInfo};

/// The crate version exposed to the shell.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
