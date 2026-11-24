//! Table queries, keyset cursors, and search results.

use serde::{Deserialize, Serialize};

use crate::record::{Record, RowId};
use crate::value::Value;

/// Sort direction for one column.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(
    feature = "ts",
    ts(export, export_to = "../../../apps/desktop/src/bindings/")
)]
#[serde(rename_all = "camelCase")]
pub enum SortDirection {
    /// Ascending, nulls last.
    Asc,
    /// Descending, nulls last.
    Desc,
}

/// One sort key.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(
    feature = "ts",
    ts(export, export_to = "../../../apps/desktop/src/bindings/")
)]
pub struct SortSpec {
    /// Column name.
    pub column: String,
    /// Direction.
    pub direction: SortDirection,
}

/// Comparison used by a filter.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(
    feature = "ts",
    ts(export, export_to = "../../../apps/desktop/src/bindings/")
)]
#[serde(rename_all = "camelCase")]
pub enum FilterOp {
    /// Equal.
    Eq,
    /// Not equal.
    NotEq,
    /// Case-insensitive substring.
    Contains,
    /// Case-insensitive prefix.
    StartsWith,
    /// Case-insensitive suffix.
    EndsWith,
    /// Greater than.
    Gt,
    /// Greater than or equal.
    Gte,
    /// Less than.
    Lt,
    /// Less than or equal.
    Lte,
    /// Null test.
    IsNull,
    /// Not-null test.
    IsNotNull,
}

/// One filter predicate.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(
    feature = "ts",
    ts(export, export_to = "../../../apps/desktop/src/bindings/")
)]
pub struct Filter {
    /// Column name.
    pub column: String,
    /// Comparison.
    pub op: FilterOp,
    /// Right-hand value. Ignored by null tests.
    pub value: Option<Value>,
}

/// A keyset cursor: the last sort value and primary key from the previous page.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(
    feature = "ts",
    ts(export, export_to = "../../../apps/desktop/src/bindings/")
)]
pub struct Cursor {
    /// Sorted column. `id` means the primary key.
    pub column: String,
    /// Sort value of the last row.
    pub value: Value,
    /// Primary key of the last row.
    pub row_id: RowId,
}

/// A bounded page request. Limits above 500 are clamped by storage.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(
    feature = "ts",
    ts(export, export_to = "../../../apps/desktop/src/bindings/")
)]
pub struct PageQuery {
    /// Table to read.
    pub table: String,
    /// Maximum rows to return.
    pub limit: u32,
    /// Cursor from a prior page.
    pub cursor: Option<Cursor>,
    /// Sort keys. Empty means primary key ascending.
    pub sort: Vec<SortSpec>,
    /// Filters applied before pagination.
    pub filters: Vec<Filter>,
    /// Optional full-text term combined with filters.
    pub search: Option<String>,
}

impl PageQuery {
    /// A first page request with the given limit.
    pub fn first(table: impl Into<String>, limit: u32) -> Self {
        Self {
            table: table.into(),
            limit,
            ..Self::default()
        }
    }
}

/// One page of rows plus a cursor for the next page.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(
    feature = "ts",
    ts(export, export_to = "../../../apps/desktop/src/bindings/")
)]
pub struct Page {
    /// Rows in sort order.
    pub rows: Vec<Record>,
    /// Cursor for the next page, absent on the final page.
    pub next_cursor: Option<Cursor>,
    /// Time spent in storage, in milliseconds.
    pub elapsed_ms: f64,
    /// Total rows matching filters, when the caller requested it.
    pub total_rows: Option<u64>,
}

/// One full-text hit.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(
    feature = "ts",
    ts(export, export_to = "../../../apps/desktop/src/bindings/")
)]
pub struct SearchHit {
    /// Table containing the row.
    pub table: String,
    /// Primary key.
    pub row_id: RowId,
    /// Highlighted excerpt with `<mark>` around matches.
    pub snippet: String,
    /// Lower is better.
    pub rank: f64,
}

/// A table name and row count for the sidebar.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(
    feature = "ts",
    ts(export, export_to = "../../../apps/desktop/src/bindings/")
)]
pub struct TableSummary {
    /// Table name.
    pub name: String,
    /// Approximate row count from the storage engine.
    pub row_count: u64,
}
