//! Reversible commands applied by storage.
//!
//! A command carries enough state to build its inverse without reading the
//! database, so undo and redo do not depend on the current row contents.

use serde::{Deserialize, Serialize};

use crate::layout::Layout;
use crate::record::{Record, RecordRef, RowId};
use crate::schema::{Column, IndexSpec, Relation, Table};
use crate::value::Value;

/// A value restored into one column during an undo of a documented drop.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(
    feature = "ts",
    ts(export, export_to = "../../../apps/desktop/src/bindings/")
)]
pub struct RowValue {
    /// Row primary key.
    pub row_id: RowId,
    /// Stored value at the time of the drop.
    pub value: Value,
}

/// One reversible data or schema change.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(
    feature = "ts",
    ts(export, export_to = "../../../apps/desktop/src/bindings/")
)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Command {
    /// Insert a row.
    InsertRow {
        /// Table name.
        table: String,
        /// Complete row, including the assigned id.
        row: Record,
        /// Rows restored after the parent during an undo of a cascade delete.
        children: Vec<RecordRef>,
    },
    /// Update a row.
    UpdateRow {
        /// Table name.
        table: String,
        /// Row before the change.
        before: Record,
        /// Row after the change.
        after: Record,
    },
    /// Delete a row.
    DeleteRow {
        /// Table name.
        table: String,
        /// Row that was removed.
        row: Record,
        /// Rows removed by cascade, captured for undo.
        children: Vec<RecordRef>,
    },
    /// Create a table.
    CreateTable {
        /// Definition that was created.
        table: Table,
    },
    /// Drop a table, retaining rows and layouts for undo.
    DropTable {
        /// Definition that was removed.
        table: Table,
        /// Stored rows captured before the drop.
        rows: Vec<Record>,
        /// Layouts bound to the table before the drop.
        layouts: Vec<Layout>,
        /// User indexes bound to the table before the drop.
        indexes: Vec<IndexSpec>,
    },
    /// Recreate a table that was dropped.
    RestoreTable {
        /// Definition to recreate.
        table: Table,
        /// Stored rows to restore.
        rows: Vec<Record>,
        /// Layouts to restore.
        layouts: Vec<Layout>,
        /// User indexes to restore.
        indexes: Vec<IndexSpec>,
    },
    /// Rename a table.
    RenameTable {
        /// Existing name.
        old_name: String,
        /// New name.
        new_name: String,
    },
    /// Add a column.
    AddColumn {
        /// Table name.
        table: String,
        /// Column that was added.
        column: Column,
    },
    /// Drop a column, retaining stored values for undo.
    DropColumn {
        /// Table name.
        table: String,
        /// Column that was removed.
        column: Column,
        /// Stored values captured before the drop.
        values: Vec<RowValue>,
    },
    /// Restore a column that was dropped.
    RestoreColumn {
        /// Table name.
        table: String,
        /// Column to restore.
        column: Column,
        /// Stored values to restore.
        values: Vec<RowValue>,
    },
    /// Rename a column.
    RenameColumn {
        /// Table name.
        table: String,
        /// Existing name.
        old_name: String,
        /// New name.
        new_name: String,
    },
    /// Create a relation.
    CreateRelation {
        /// Relation that was created.
        relation: Relation,
    },
    /// Drop a relation.
    DropRelation {
        /// Relation that was removed.
        relation: Relation,
    },
    /// Create or update a layout. `before` is absent on creation.
    SaveLayout {
        /// Previous layout, when updating.
        before: Option<Layout>,
        /// Layout after the change.
        after: Layout,
    },
    /// Delete a layout.
    DeleteLayout {
        /// Layout that was removed.
        layout: Layout,
    },
    /// Create a user index.
    CreateIndex {
        /// Index definition.
        index: IndexSpec,
    },
    /// Drop a user index.
    DropIndex {
        /// Index definition.
        index: IndexSpec,
    },
}

impl Command {
    /// The inverse command, sufficient to undo this one.
    pub fn inverse(&self) -> Self {
        match self {
            Self::InsertRow {
                table,
                row,
                children,
            } => Self::DeleteRow {
                table: table.clone(),
                row: row.clone(),
                children: children.clone(),
            },
            Self::UpdateRow {
                table,
                before,
                after,
            } => Self::UpdateRow {
                table: table.clone(),
                before: after.clone(),
                after: before.clone(),
            },
            Self::DeleteRow {
                table,
                row,
                children,
            } => Self::InsertRow {
                table: table.clone(),
                row: row.clone(),
                children: children.clone(),
            },
            Self::CreateTable { table } => Self::DropTable {
                table: table.clone(),
                rows: Vec::new(),
                layouts: Vec::new(),
                indexes: Vec::new(),
            },
            Self::DropTable {
                table,
                rows,
                layouts,
                indexes,
            } => Self::RestoreTable {
                table: table.clone(),
                rows: rows.clone(),
                layouts: layouts.clone(),
                indexes: indexes.clone(),
            },
            Self::RestoreTable {
                table,
                rows,
                layouts,
                indexes,
            } => Self::DropTable {
                table: table.clone(),
                rows: rows.clone(),
                layouts: layouts.clone(),
                indexes: indexes.clone(),
            },
            Self::RenameTable { old_name, new_name } => Self::RenameTable {
                old_name: new_name.clone(),
                new_name: old_name.clone(),
            },
            Self::AddColumn { table, column } => Self::DropColumn {
                table: table.clone(),
                column: column.clone(),
                values: Vec::new(),
            },
            Self::DropColumn {
                table,
                column,
                values,
            } => Self::RestoreColumn {
                table: table.clone(),
                column: column.clone(),
                values: values.clone(),
            },
            Self::RestoreColumn {
                table,
                column,
                values,
            } => Self::DropColumn {
                table: table.clone(),
                column: column.clone(),
                values: values.clone(),
            },
            Self::RenameColumn {
                table,
                old_name,
                new_name,
            } => Self::RenameColumn {
                table: table.clone(),
                old_name: new_name.clone(),
                new_name: old_name.clone(),
            },
            Self::CreateRelation { relation } => Self::DropRelation {
                relation: relation.clone(),
            },
            Self::DropRelation { relation } => Self::CreateRelation {
                relation: relation.clone(),
            },
            Self::SaveLayout {
                before: Some(before),
                after,
            } => Self::SaveLayout {
                before: Some(after.clone()),
                after: before.clone(),
            },
            Self::SaveLayout {
                before: None,
                after,
            } => Self::DeleteLayout {
                layout: after.clone(),
            },
            Self::DeleteLayout { layout } => Self::SaveLayout {
                before: None,
                after: layout.clone(),
            },
            Self::CreateIndex { index } => Self::DropIndex {
                index: index.clone(),
            },
            Self::DropIndex { index } => Self::CreateIndex {
                index: index.clone(),
            },
        }
    }

    /// The table this command affects, when it affects one.
    pub fn table_name(&self) -> Option<&str> {
        match self {
            Self::InsertRow { table, .. }
            | Self::UpdateRow { table, .. }
            | Self::DeleteRow { table, .. }
            | Self::AddColumn { table, .. }
            | Self::DropColumn { table, .. }
            | Self::RestoreColumn { table, .. }
            | Self::RenameColumn { table, .. } => Some(table),
            Self::CreateTable { table }
            | Self::DropTable { table, .. }
            | Self::RestoreTable { table, .. } => Some(&table.name),
            Self::RenameTable { old_name, .. } => Some(old_name),
            Self::CreateIndex { index } | Self::DropIndex { index } => Some(&index.table),
            Self::CreateRelation { relation } | Self::DropRelation { relation } => {
                Some(&relation.child_table)
            }
            Self::SaveLayout { after, .. } => Some(&after.table),
            Self::DeleteLayout { layout } => Some(&layout.table),
        }
    }

    /// The audit operation label.
    pub const fn operation(&self) -> &'static str {
        match self {
            Self::InsertRow { .. } => "insert",
            Self::UpdateRow { .. } => "update",
            Self::DeleteRow { .. } => "delete",
            Self::CreateTable { .. } => "create_table",
            Self::DropTable { .. } => "drop_table",
            Self::RestoreTable { .. } => "restore_table",
            Self::RenameTable { .. } => "rename_table",
            Self::AddColumn { .. } => "add_column",
            Self::DropColumn { .. } => "drop_column",
            Self::RestoreColumn { .. } => "restore_column",
            Self::RenameColumn { .. } => "rename_column",
            Self::CreateRelation { .. } => "create_relation",
            Self::DropRelation { .. } => "drop_relation",
            Self::SaveLayout { .. } => "save_layout",
            Self::DeleteLayout { .. } => "delete_layout",
            Self::CreateIndex { .. } => "create_index",
            Self::DropIndex { .. } => "drop_index",
        }
    }

    /// True for commands that change schema, layout, or relation metadata.
    pub const fn is_schema_change(&self) -> bool {
        !matches!(
            self,
            Self::InsertRow { .. } | Self::UpdateRow { .. } | Self::DeleteRow { .. }
        )
    }

    /// A short human-readable label for undo UI and the audit list.
    pub fn label(&self) -> String {
        match self {
            Self::InsertRow { table, row, .. } => format!("Insert {table} #{}", row.id),
            Self::UpdateRow { table, after, .. } => format!("Update {table} #{}", after.id),
            Self::DeleteRow { table, row, .. } => format!("Delete {table} #{}", row.id),
            Self::CreateTable { table } => format!("Create table {}", table.name),
            Self::DropTable { table, .. } => format!("Drop table {}", table.name),
            Self::RestoreTable { table, .. } => format!("Restore table {}", table.name),
            Self::RenameTable { new_name, .. } => format!("Rename table to {new_name}"),
            Self::AddColumn { table, column } => {
                format!("Add {}.{}", table, column.name)
            }
            Self::DropColumn { table, column, .. } => {
                format!("Drop {}.{}", table, column.name)
            }
            Self::RestoreColumn { table, column, .. } => {
                format!("Restore {}.{}", table, column.name)
            }
            Self::RenameColumn {
                table, new_name, ..
            } => {
                format!("Rename {table}.{new_name}")
            }
            Self::CreateRelation { relation } => format!("Create relation {}", relation.name),
            Self::DropRelation { relation } => format!("Drop relation {}", relation.name),
            Self::SaveLayout { after, .. } => format!("Save form {}", after.name),
            Self::DeleteLayout { layout } => format!("Delete form {}", layout.name),
            Self::CreateIndex { index } => format!("Index {}", index.name),
            Self::DropIndex { index } => format!("Drop index {}", index.name),
        }
    }

    /// The row id, when the command affects a single row.
    pub const fn row_id(&self) -> Option<i64> {
        match self {
            Self::InsertRow { row, .. }
            | Self::DeleteRow { row, .. }
            | Self::UpdateRow { after: row, .. } => Some(row.id),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn row_command_inverse_round_trips() {
        let row = Record::new(7);
        let command = Command::InsertRow {
            table: "Clients".into(),
            row: row.clone(),
            children: Vec::new(),
        };
        let inverse = command.inverse();
        assert_eq!(
            inverse,
            Command::DeleteRow {
                table: "Clients".into(),
                row,
                children: Vec::new(),
            }
        );
        assert_eq!(inverse.inverse(), command);
    }

    #[test]
    fn audit_metadata_identifies_row_and_schema_commands() {
        let update = Command::UpdateRow {
            table: "Clients".into(),
            before: Record::new(7),
            after: Record::new(7),
        };
        assert_eq!(update.table_name(), Some("Clients"));
        assert_eq!(update.row_id(), Some(7));
        assert_eq!(update.operation(), "update");
        assert_eq!(update.label(), "Update Clients #7");
        assert!(!update.is_schema_change());

        let rename = Command::RenameTable {
            old_name: "Clients".into(),
            new_name: "Customers".into(),
        };
        assert_eq!(rename.table_name(), Some("Clients"));
        assert_eq!(rename.row_id(), None);
        assert_eq!(rename.operation(), "rename_table");
        assert_eq!(rename.label(), "Rename table to Customers");
        assert!(rename.is_schema_change());
    }

    #[test]
    fn layout_inverse_matches_creation() {
        let layout = Layout {
            id: "l".into(),
            name: "Client".into(),
            table: "Clients".into(),
            grid: crate::layout::GridSpec::default(),
            items: Vec::new(),
            updated_at: 0,
        };
        let command = Command::SaveLayout {
            before: None,
            after: layout.clone(),
        };
        assert_eq!(
            command.inverse(),
            Command::DeleteLayout {
                layout: layout.clone()
            }
        );
        assert_eq!(
            command.inverse().inverse(),
            Command::SaveLayout {
                before: None,
                after: layout
            }
        );
    }
}
