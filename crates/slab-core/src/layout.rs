//! Form layouts: a snap grid of fields, labels, sections, and child portals.

use serde::{Deserialize, Serialize};

use crate::error::CoreError;
use crate::schema::Schema;

/// The grid geometry of a form.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(
    feature = "ts",
    ts(export, export_to = "../../../apps/desktop/src/bindings/")
)]
pub struct GridSpec {
    /// Number of columns in the snap grid.
    pub columns: u16,
    /// Height of one grid row in CSS pixels.
    pub row_height: u16,
}

impl Default for GridSpec {
    fn default() -> Self {
        Self {
            columns: 12,
            row_height: 28,
        }
    }
}

/// What a layout item renders.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(
    feature = "ts",
    ts(export, export_to = "../../../apps/desktop/src/bindings/")
)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum LayoutItemKind {
    /// A bound table column.
    Field {
        /// Column name.
        column: String,
    },
    /// An editable child table.
    Portal {
        /// Relation name.
        relation: String,
        /// Child columns to show; empty means all stored child columns.
        columns: Vec<String>,
        /// Number of visible child rows before scrolling.
        rows: u16,
    },
    /// Static text.
    Label {
        /// Display text.
        text: String,
    },
    /// A titled section divider.
    Section {
        /// Section title.
        title: String,
    },
}

/// One placed item on a form.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(
    feature = "ts",
    ts(export, export_to = "../../../apps/desktop/src/bindings/")
)]
pub struct LayoutItem {
    /// Stable identifier.
    pub id: String,
    /// Rendered content.
    pub kind: LayoutItemKind,
    /// Column position in grid units.
    pub x: u16,
    /// Row position in grid units.
    pub y: u16,
    /// Width in grid units.
    pub w: u16,
    /// Height in grid units.
    pub h: u16,
}

/// A saved form layout for one table.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(
    feature = "ts",
    ts(export, export_to = "../../../apps/desktop/src/bindings/")
)]
pub struct Layout {
    /// Stable identifier.
    pub id: String,
    /// Display name.
    pub name: String,
    /// Bound table name.
    pub table: String,
    /// Grid geometry.
    pub grid: GridSpec,
    /// Placed items.
    pub items: Vec<LayoutItem>,
    /// Last update in Unix milliseconds.
    pub updated_at: i64,
}

impl Layout {
    /// Validates that every reference resolves and every item fits the grid.
    pub fn validate(&self, schema: &Schema) -> Result<(), CoreError> {
        if self.name.trim().is_empty() {
            return Err(CoreError::InvalidLayout {
                reason: "layout name is empty".into(),
            });
        }
        if self.grid.columns == 0 || self.grid.row_height == 0 {
            return Err(CoreError::InvalidLayout {
                reason: "grid must have at least one column and one row unit".into(),
            });
        }
        let table = schema
            .table(&self.table)
            .ok_or_else(|| CoreError::InvalidLayout {
                reason: format!("missing table {}", self.table),
            })?;
        let mut ids = std::collections::BTreeSet::new();
        for item in &self.items {
            if !ids.insert(item.id.as_str()) {
                return Err(CoreError::InvalidLayout {
                    reason: format!("duplicate item id {}", item.id),
                });
            }
            if item.w == 0 || item.h == 0 {
                return Err(CoreError::InvalidLayout {
                    reason: format!("item {} has zero size", item.id),
                });
            }
            if u32::from(item.x) + u32::from(item.w) > u32::from(self.grid.columns) {
                return Err(CoreError::InvalidLayout {
                    reason: format!(
                        "item {} exceeds {} grid columns",
                        item.id, self.grid.columns
                    ),
                });
            }
            match &item.kind {
                LayoutItemKind::Field { column } => {
                    if table.column(column).is_none() {
                        return Err(CoreError::InvalidLayout {
                            reason: format!("item {} references missing column {column}", item.id),
                        });
                    }
                }
                LayoutItemKind::Portal {
                    relation, columns, ..
                } => {
                    let relation =
                        schema
                            .relation(relation)
                            .ok_or_else(|| CoreError::InvalidLayout {
                                reason: format!("item {} references missing relation", item.id),
                            })?;
                    if relation.parent_table != self.table {
                        return Err(CoreError::InvalidLayout {
                            reason: format!(
                                "item {}: relation {} does not belong to {}",
                                item.id, relation.name, self.table
                            ),
                        });
                    }
                    let child = schema.table(&relation.child_table).ok_or_else(|| {
                        CoreError::InvalidLayout {
                            reason: format!("item {} references missing child table", item.id),
                        }
                    })?;
                    for column in columns {
                        if child.column(column).is_none() {
                            return Err(CoreError::InvalidLayout {
                                reason: format!(
                                    "item {} references missing child column {column}",
                                    item.id
                                ),
                            });
                        }
                    }
                }
                LayoutItemKind::Label { text } if text.trim().is_empty() => {
                    return Err(CoreError::InvalidLayout {
                        reason: format!("item {} has an empty label", item.id),
                    });
                }
                LayoutItemKind::Section { title } if title.trim().is_empty() => {
                    return Err(CoreError::InvalidLayout {
                        reason: format!("item {} has an empty section title", item.id),
                    });
                }
                LayoutItemKind::Label { .. } | LayoutItemKind::Section { .. } => {}
            }
        }
        Ok(())
    }

    /// Items in visual order: top to bottom, then left to right.
    pub fn ordered_items(&self) -> Vec<&LayoutItem> {
        let mut items: Vec<&LayoutItem> = self.items.iter().collect();
        items.sort_by_key(|item| (item.y, item.x, item.id.as_str()));
        items
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::{Column, ColumnType, DeleteAction, Relation, Table};

    fn table(name: &str, column: &str, ty: ColumnType) -> Table {
        Table {
            id: format!("t_{name}"),
            name: name.into(),
            columns: vec![Column {
                id: format!("c_{column}"),
                table: name.into(),
                name: column.into(),
                ty,
                required: false,
                unique: false,
                default: None,
                position: 0,
            }],
            created_at: 0,
        }
    }

    fn schema() -> Schema {
        Schema {
            tables: vec![
                table("Clients", "Name", ColumnType::Text),
                table("Projects", "Client", ColumnType::Integer),
            ],
            relations: vec![Relation {
                id: "r".into(),
                name: "Client Projects".into(),
                parent_table: "Clients".into(),
                child_table: "Projects".into(),
                child_column: "Client".into(),
                on_delete: DeleteAction::Cascade,
            }],
            layouts: Vec::new(),
        }
    }

    fn layout() -> Layout {
        Layout {
            id: "l".into(),
            name: "Client".into(),
            table: "Clients".into(),
            grid: GridSpec::default(),
            items: vec![LayoutItem {
                id: "i".into(),
                kind: LayoutItemKind::Field {
                    column: "Name".into(),
                },
                x: 10,
                y: 0,
                w: 2,
                h: 1,
            }],
            updated_at: 0,
        }
    }

    fn assert_invalid(result: Result<(), CoreError>, expected: &str) {
        match result.expect_err(expected) {
            CoreError::InvalidLayout { reason } => assert_eq!(reason, expected),
            other => panic!("expected invalid layout for {expected}, got {other}"),
        }
    }

    #[test]
    fn rejects_a_zero_grid_dimension() {
        let schema = schema();
        for (columns, row_height) in [(0, 28), (12, 0)] {
            let mut layout = layout();
            layout.grid = GridSpec {
                columns,
                row_height,
            };
            assert_invalid(
                layout.validate(&schema),
                "grid must have at least one column and one row unit",
            );
        }
    }

    #[test]
    fn validates_item_size_and_right_edge() {
        let schema = schema();
        layout()
            .validate(&schema)
            .expect("item may end at the grid edge");

        for (x, w, h, reason) in [
            (10, 0, 1, "item i has zero size"),
            (10, 2, 0, "item i has zero size"),
            (10, 4, 1, "item i exceeds 12 grid columns"),
            (0, 13, 1, "item i exceeds 12 grid columns"),
        ] {
            let mut layout = layout();
            let item = &mut layout.items[0];
            item.x = x;
            item.w = w;
            item.h = h;
            assert_invalid(layout.validate(&schema), reason);
        }
    }

    #[test]
    fn rejects_duplicate_item_ids() {
        let mut layout = layout();
        layout.items.push(layout.items[0].clone());
        assert_invalid(layout.validate(&schema()), "duplicate item id i");
    }

    #[test]
    fn validates_static_text() {
        let schema = schema();
        for (kind, reason) in [
            (
                LayoutItemKind::Label {
                    text: "Clients".into(),
                },
                None,
            ),
            (
                LayoutItemKind::Section {
                    title: "Details".into(),
                },
                None,
            ),
            (
                LayoutItemKind::Label { text: " \t".into() },
                Some("item i has an empty label"),
            ),
            (
                LayoutItemKind::Section {
                    title: " \t".into(),
                },
                Some("item i has an empty section title"),
            ),
        ] {
            let mut layout = layout();
            layout.items[0].kind = kind;
            match reason {
                None => layout.validate(&schema).expect("nonblank static text"),
                Some(reason) => assert_invalid(layout.validate(&schema), reason),
            }
        }
    }

    #[test]
    fn portal_must_use_a_relation_owned_by_the_forms_table() {
        let schema = schema();
        let mut layout = layout();
        layout.items[0].kind = LayoutItemKind::Portal {
            relation: "Client Projects".into(),
            columns: vec!["Client".into()],
            rows: 3,
        };
        layout.validate(&schema).expect("portal on parent table");

        layout.table = "Projects".into();
        assert_invalid(
            layout.validate(&schema),
            "item i: relation Client Projects does not belong to Projects",
        );
    }

    #[test]
    fn orders_items_by_row_column_and_id() {
        let mut layout = layout();
        let field = layout.items[0].clone();
        layout.items = [("bottom", 1, 0), ("right", 0, 2), ("b", 0, 0), ("a", 0, 0)]
            .into_iter()
            .map(|(id, y, x)| LayoutItem {
                id: id.into(),
                y,
                x,
                ..field.clone()
            })
            .collect();
        let ordered: Vec<_> = layout
            .ordered_items()
            .into_iter()
            .map(|item| item.id.as_str())
            .collect();
        assert_eq!(ordered, ["a", "b", "right", "bottom"]);
    }
}
