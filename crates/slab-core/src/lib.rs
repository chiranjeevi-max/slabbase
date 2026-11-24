#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

//! Pure domain types shared by every SlabBase layer.
//!
//! This crate performs no I/O. Values, exact decimal arithmetic, schema
//! definitions, form layouts, commands, and query shapes live here so that the
//! storage engine, the formula engine, the desktop shell, and future shells
//! share one representation.

pub mod audit;
pub mod clock;
pub mod command;
pub mod date;
pub mod decimal;
pub mod error;
pub mod layout;
pub mod policy;
pub mod query;
pub mod record;
pub mod schema;
pub mod value;
pub mod view;

pub use audit::{AuditEntry, UndoState};
pub use clock::{Clock, Hlc};
pub use command::Command;
pub use date::{SlabDate, SlabTimestamp};
pub use decimal::{CurrencyCode, Decimal, MAX_SCALE, Money, RoundingMode};
pub use error::{CoreError, ErrorCode, SlabError, validate_identifier};
pub use layout::{GridSpec, Layout, LayoutItem, LayoutItemKind};
pub use policy::{NoopPolicy, PolicyHook};
pub use query::{
    Cursor, Filter, FilterOp, Page, PageQuery, SearchHit, SortDirection, SortSpec, TableSummary,
};
pub use record::{Record, RecordBundle, RecordRef, RowData, RowId};
pub use schema::{
    Aggregate, Column, ColumnDraft, ColumnType, DeleteAction, IndexSpec, Relation, RelationDraft,
    Schema, Table, TableDraft,
};
pub use value::Value;
pub use view::{CalendarConfig, GalleryConfig, KanbanConfig, TableGridConfig, ViewConfig};

/// The metadata prefix reserved for SlabBase system tables.
pub const SYSTEM_PREFIX: &str = "_slab_";

/// The physical primary key column present on every user table.
pub const ROW_ID_COLUMN: &str = "id";

/// Maximum rows returned by one page query.
pub const MAX_PAGE_SIZE: u32 = 500;

/// Maximum nesting depth accepted by the formula parser.
pub const MAX_EXPRESSION_DEPTH: u32 = 128;

/// Workspace format version written to `_slab_meta`.
pub const FORMAT_VERSION: u32 = 1;
