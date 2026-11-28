use rusqlite::{Connection, params};
use slab_core::{ColumnDraft, ColumnType, RowData, TableDraft, Value};

use super::Workspace;
use crate::test_support::TestClock;

#[test]
fn partial_update_uses_the_row_committed_by_the_previous_writer() {
    let directory = tempfile::TempDir::new().expect("tempdir");
    let path = directory.path().join("concurrent-update.slab");
    let mut workspace =
        Workspace::create(&path, "Notes", Box::new(TestClock::new())).expect("create");
    workspace
        .create_table(&TableDraft {
            name: "Notes".into(),
            columns: ["Title", "Tag"]
                .into_iter()
                .map(|name| ColumnDraft {
                    name: name.into(),
                    ty: ColumnType::Text,
                    required: false,
                    unique: false,
                    default: None,
                })
                .collect(),
        })
        .expect("table");
    let id = workspace
        .insert_row(
            "Notes",
            &RowData::from([
                ("Title".into(), Value::from("Original")),
                ("Tag".into(), Value::from("stale")),
            ]),
        )
        .expect("seed row")
        .id;

    let other_writer = Connection::open(&path).expect("open second writer");
    other_writer
        .execute_batch("BEGIN IMMEDIATE")
        .expect("begin");
    other_writer
        .execute(
            "UPDATE Notes SET Tag = 'concurrent' WHERE id = ?1",
            params![id],
        )
        .expect("stage unrelated update");
    // The pending edit commits only when update_row tries to acquire its
    // write lock, so reading the row before BEGIN observes the stale value.
    workspace
        .conn
        .busy_handler(Some(move |attempt| {
            attempt == 0 && other_writer.execute_batch("COMMIT").is_ok()
        }))
        .expect("install busy handler");

    let record = workspace
        .update_row(
            "Notes",
            id,
            &RowData::from([("Title".into(), Value::from("Revised"))]),
        )
        .expect("partial update");
    assert_eq!(record.values.get("Title"), Some(&Value::from("Revised")));
    assert_eq!(record.values.get("Tag"), Some(&Value::from("concurrent")));
    assert_eq!(
        workspace.search("concurrent", 10).expect("find tag").len(),
        1
    );
    let history = workspace.record_history("Notes", id).expect("history");
    let diffs = &history.last().expect("update revision").diffs;
    assert_eq!(diffs.len(), 1);
    assert_eq!(diffs[0].column, "Title");
    assert_eq!(diffs[0].before, Some(Value::from("Original")));
    assert_eq!(diffs[0].after, Some(Value::from("Revised")));

    workspace.undo().expect("undo partial update");
    let record = workspace
        .read_record("Notes", id)
        .expect("read restored row");
    assert_eq!(record.values.get("Title"), Some(&Value::from("Original")));
    assert_eq!(record.values.get("Tag"), Some(&Value::from("concurrent")));
}
