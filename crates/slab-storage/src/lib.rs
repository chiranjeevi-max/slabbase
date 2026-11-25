#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

//! SQLite workspace engine for SlabBase.
//!
//! The crate owns one serialized writer connection plus a bounded pool of
//! read-only connections. It applies open SQLite pragmas, generates DDL for
//! user tables, validates formulas before schema changes, maintains full-text
//! and incremental rollup indexes, and writes a hash-chained before/after
//! audit entry in the same transaction as every mutation.

mod ddl;
mod error;
mod read;
mod rollup;
mod seed;
mod sql;
#[cfg(test)]
mod test_support;
mod workspace;

pub use error::StorageError;
pub use workspace::{
    CancelRegistration, IntegrityReport, QueryCancel, SnapshotKind, SnapshotReport, Workspace,
    WorkspaceInfo,
};

/// The crate version exposed to the shell.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
