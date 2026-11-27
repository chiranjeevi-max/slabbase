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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn record_lookup_distinguishes_null_from_a_missing_column() {
        let mut record = Record::new(42);
        record.values.insert("units".into(), Value::Integer(1_375));
        record.values.insert("notes".into(), Value::Null);

        assert_eq!(record.get("units"), Some(&Value::Integer(1_375)));
        assert_eq!(record.get("notes"), Some(&Value::Null));
        assert_eq!(record.get("missing"), None);
        assert_eq!(record.display("units"), "1375");
        assert_eq!(record.display("notes"), "");
        assert_eq!(record.display("missing"), "");
    }

    #[test]
    fn portal_lookup_returns_children_for_the_requested_relation() {
        let mut bundle = RecordBundle::new(Record::new(42));
        let line_items = vec![Record::new(3), Record::new(1)];
        bundle.portals.insert("items".into(), line_items.clone());
        bundle
            .portals
            .insert("payments".into(), vec![Record::new(2)]);

        assert_eq!(bundle.portal("items"), line_items.as_slice());
        assert!(bundle.portal("missing").is_empty());
    }
}
