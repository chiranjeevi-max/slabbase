//! Rows and records.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::value::Value;

/// The physical primary key type for every user table.
pub type RowId = i64;

/// Column values keyed by column name.
pub type RowData = BTreeMap<String, Value>;

/// One stored row with its primary key.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(
    feature = "ts",
    ts(export, export_to = "../../../apps/desktop/src/bindings/")
)]
pub struct Record {
    /// Primary key.
    pub id: RowId,
    /// Column values, including computed columns.
    pub values: RowData,
}

impl Record {
    /// An empty record bound to a new id.
    pub fn new(id: RowId) -> Self {
        Self {
            id,
            values: RowData::new(),
        }
    }

    /// Looks up a value.
    pub fn get(&self, column: &str) -> Option<&Value> {
        self.values.get(column)
    }

    /// Returns a display string for a column, or an empty string when absent.
    pub fn display(&self, column: &str) -> String {
        self.values
            .get(column)
            .map(Value::display_string)
            .unwrap_or_default()
    }
}

/// A row in a table other than the command's primary table.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(
    feature = "ts",
    ts(export, export_to = "../../../apps/desktop/src/bindings/")
)]
pub struct RecordRef {
    /// Table name.
    pub table: String,
    /// The row.
    pub row: Record,
}

/// A record together with its child rows, keyed by relation name.
///
/// The print renderer and the record form both consume this shape, so neither
/// has to reach into storage.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(
    feature = "ts",
    ts(export, export_to = "../../../apps/desktop/src/bindings/")
)]
pub struct RecordBundle {
    /// The parent record.
    pub record: Record,
    /// Child rows by relation name.
    pub portals: BTreeMap<String, Vec<Record>>,
}

impl RecordBundle {
    /// A bundle with no child rows.
    pub fn new(record: Record) -> Self {
        Self {
            record,
            portals: BTreeMap::new(),
        }
    }

    /// Child rows for a relation, or an empty slice.
    pub fn portal(&self, relation: &str) -> &[Record] {
        self.portals.get(relation).map(Vec::as_slice).unwrap_or(&[])
    }
}
