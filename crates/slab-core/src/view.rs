//! Extension seams for later releases.

use serde::{Deserialize, Serialize};

/// Grid configuration for the table view.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(
    feature = "ts",
    ts(export, export_to = "../../../apps/desktop/src/bindings/")
)]
pub struct TableGridConfig {
    /// Visible column names, in order.
    pub columns: Vec<String>,
    /// Row height in CSS pixels.
    pub row_height: u16,
}

/// Kanban grouping configuration, introduced in v1.1.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(
    feature = "ts",
    ts(export, export_to = "../../../apps/desktop/src/bindings/")
)]
pub struct KanbanConfig {
    /// Select column used for lanes.
    pub group_by: String,
    /// Field shown on each card.
    pub title_column: String,
}

/// Gallery card configuration, introduced in v1.1.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(
    feature = "ts",
    ts(export, export_to = "../../../apps/desktop/src/bindings/")
)]
pub struct GalleryConfig {
    /// Field shown as the card title.
    pub title_column: String,
    /// Optional attachment field shown as an image.
    pub image_column: Option<String>,
    /// Cards per row.
    pub columns: u16,
}

/// Calendar configuration, introduced in v1.1.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(
    feature = "ts",
    ts(export, export_to = "../../../apps/desktop/src/bindings/")
)]
pub struct CalendarConfig {
    /// Date column used to place rows.
    pub date_column: String,
    /// Field shown on each entry.
    pub title_column: String,
}

/// A saved view of a table.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(
    feature = "ts",
    ts(export, export_to = "../../../apps/desktop/src/bindings/")
)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ViewConfig {
    /// Grid view, shipped in v1.0.
    Table(TableGridConfig),
    /// Kanban view, reserved for v1.1.
    Kanban(KanbanConfig),
    /// Gallery view, reserved for v1.1.
    Gallery(GalleryConfig),
    /// Calendar view, reserved for v1.1.
    Calendar(CalendarConfig),
}
