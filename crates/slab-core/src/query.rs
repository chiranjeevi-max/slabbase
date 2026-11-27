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
    /// Optional advisor result for an unindexed sort or filter.
    pub index_hint: Option<IndexHint>,
}

impl Page {
    /// Converts the named-record payload to compact positional tuples.
    ///
    /// Column names travel once in `columns`; each row is a value array in the
    /// same order. A value absent from a record is transmitted as `Null`.
    pub fn into_frame(self, columns: Vec<String>) -> PageFrame {
        let rows = self
            .rows
            .into_iter()
            .map(|record| RowTuple {
                id: record.id,
                values: columns
                    .iter()
                    .map(|column| record.values.get(column).cloned().unwrap_or(Value::Null))
                    .collect(),
            })
            .collect();
        PageFrame {
            columns,
            rows,
            next_cursor: self.next_cursor,
            elapsed_ms: self.elapsed_ms,
            total_rows: self.total_rows,
            index_hint: self.index_hint,
        }
    }
}

/// A stable identifier for a column in a tuple-framed page.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(
    feature = "ts",
    ts(export, export_to = "../../../apps/desktop/src/bindings/")
)]
pub struct RowTuple {
    /// Primary key.
    pub id: RowId,
    /// Values aligned with `PageFrame::columns`.
    pub values: Vec<Value>,
}

/// A page whose column metadata is sent once and whose rows are positional.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(
    feature = "ts",
    ts(export, export_to = "../../../apps/desktop/src/bindings/")
)]
pub struct PageFrame {
    /// Column names, in row-value order.
    pub columns: Vec<String>,
    /// Positional rows.
    pub rows: Vec<RowTuple>,
    /// Cursor for the next page, absent on the final page.
    pub next_cursor: Option<Cursor>,
    /// Time spent in storage, in milliseconds.
    pub elapsed_ms: f64,
    /// Total rows matching filters, when requested.
    pub total_rows: Option<u64>,
    /// Optional advisor result for an unindexed sort or filter.
    pub index_hint: Option<IndexHint>,
}

impl PageFrame {
    /// Rebuilds named records for consumers that still need them.
    pub fn records(&self) -> Vec<Record> {
        self.rows
            .iter()
            .map(|tuple| {
                let values = self
                    .columns
                    .iter()
                    .cloned()
                    .zip(tuple.values.iter().cloned())
                    .collect();
                Record {
                    id: tuple.id,
                    values,
                }
            })
            .collect()
    }
}

/// A keyset-paginated request for one child relation.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(
    feature = "ts",
    ts(export, export_to = "../../../apps/desktop/src/bindings/")
)]
pub struct PortalQuery {
    /// Relation name.
    pub relation: String,
    /// Parent row id.
    pub parent_id: RowId,
    /// Maximum child rows to return.
    pub limit: u32,
    /// Cursor from a prior portal page.
    pub cursor: Option<Cursor>,
}

/// One keyset page of child rows plus the exact relation row count.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(
    feature = "ts",
    ts(export, export_to = "../../../apps/desktop/src/bindings/")
)]
pub struct PortalPage {
    /// Child rows for this page.
    pub rows: Vec<Record>,
    /// Cursor for the next page, absent on the final page.
    pub next_cursor: Option<Cursor>,
    /// Exact number of child rows for the parent.
    pub total_rows: u64,
    /// Time spent in storage, in milliseconds.
    pub elapsed_ms: f64,
    /// Optional advisor result for an unindexed sort or filter.
    pub index_hint: Option<IndexHint>,
}

impl PortalPage {
    /// Converts the portal payload to compact positional tuples.
    pub fn into_frame(self, columns: Vec<String>) -> PortalFrame {
        let rows = self
            .rows
            .into_iter()
            .map(|record| RowTuple {
                id: record.id,
                values: columns
                    .iter()
                    .map(|column| record.values.get(column).cloned().unwrap_or(Value::Null))
                    .collect(),
            })
            .collect();
        PortalFrame {
            columns,
            rows,
            next_cursor: self.next_cursor,
            total_rows: self.total_rows,
            elapsed_ms: self.elapsed_ms,
            index_hint: self.index_hint,
        }
    }
}

/// A portal page whose column metadata is sent once and whose rows are positional.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(
    feature = "ts",
    ts(export, export_to = "../../../apps/desktop/src/bindings/")
)]
pub struct PortalFrame {
    /// Column names, in row-value order.
    pub columns: Vec<String>,
    /// Positional rows.
    pub rows: Vec<RowTuple>,
    /// Cursor for the next page, absent on the final page.
    pub next_cursor: Option<Cursor>,
    /// Exact number of child rows for the parent.
    pub total_rows: u64,
    /// Time spent in storage, in milliseconds.
    pub elapsed_ms: f64,
    /// Optional advisor result for an unindexed sort or filter.
    pub index_hint: Option<IndexHint>,
}

impl PortalFrame {
    /// Rebuilds named records for consumers that still need them.
    pub fn records(&self) -> Vec<Record> {
        self.rows
            .iter()
            .map(|tuple| {
                let values = self
                    .columns
                    .iter()
                    .cloned()
                    .zip(tuple.values.iter().cloned())
                    .collect();
                Record {
                    id: tuple.id,
                    values,
                }
            })
            .collect()
    }
}

/// A quiet, dismissible suggestion emitted when a page performed a full scan.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(
    feature = "ts",
    ts(export, export_to = "../../../apps/desktop/src/bindings/")
)]
#[serde(rename_all = "camelCase")]
pub struct IndexHint {
    /// Table that was scanned.
    pub table: String,
    /// Sort or filter column that triggered the scan.
    pub column: String,
    /// Index name the shell should create on one click.
    pub suggested_index: String,
    /// Page latency that exposed the scan.
    pub elapsed_ms: f64,
    /// Rows present in the table when the hint was produced.
    pub scanned_rows: u64,
    /// Raw `EXPLAIN QUERY PLAN` detail line.
    pub detail: String,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_page_uses_requested_table_and_limit() {
        assert_eq!(
            PageQuery::first("invoices", 25),
            PageQuery {
                table: "invoices".into(),
                limit: 25,
                cursor: None,
                sort: Vec::new(),
                filters: Vec::new(),
                search: None,
            }
        );
    }

    #[test]
    fn portal_frame_rebuilds_named_records_in_tuple_order() {
        let frame = PortalFrame {
            columns: vec!["units".into(), "description".into()],
            rows: vec![
                RowTuple {
                    id: 9,
                    values: vec![Value::Integer(3), Value::from("Paper")],
                },
                RowTuple {
                    id: 7,
                    values: vec![Value::Integer(1), Value::Null],
                },
            ],
            next_cursor: None,
            total_rows: 2,
            elapsed_ms: 0.0,
            index_hint: None,
        };

        assert_eq!(
            frame.records(),
            vec![
                Record {
                    id: 9,
                    values: [
                        ("description".into(), Value::from("Paper")),
                        ("units".into(), Value::Integer(3)),
                    ]
                    .into(),
                },
                Record {
                    id: 7,
                    values: [
                        ("description".into(), Value::Null),
                        ("units".into(), Value::Integer(1)),
                    ]
                    .into(),
                },
            ]
        );
    }
}
