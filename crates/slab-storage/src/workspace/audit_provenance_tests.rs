use rusqlite::Connection;
use slab_core::{ColumnDraft, ColumnType, RowData, TableDraft, Value};

use super::Workspace;
use crate::test_support::TestClock;

#[test]
fn dropping_only_the_hash_columns_does_not_reenable_backfill() {
    let directory = tempfile::TempDir::new().expect("tempdir");
    let path = directory.path().join("audit-half-migrated.slab");
    let mut workspace =
        Workspace::create(&path, "Audit", Box::new(TestClock::new())).expect("create");
    workspace
        .create_table(&TableDraft {
            name: "Notes".into(),
            columns: vec![ColumnDraft {
                name: "Body".into(),
                ty: ColumnType::Text,
                required: false,
                unique: false,
                default: None,
            }],
        })
        .expect("table");
    workspace
        .insert_row(
            "Notes",
            &RowData::from([("Body".to_owned(), Value::Text("hello".into()))]),
        )
        .expect("row");
    workspace.close().expect("close");

    {
        let connection = Connection::open(&path).expect("raw open");
        // A modern log keeps `hash_version`; dropping only the two hash
        // columns must not make it look like a pre-hash legacy file.
        connection
            .execute_batch(
                "ALTER TABLE _slab_audit_log DROP COLUMN prev_hash;
                 ALTER TABLE _slab_audit_log DROP COLUMN entry_hash;
                 UPDATE _slab_audit_log SET summary = 'silently rewritten';
                 DELETE FROM _slab_meta WHERE key IN ('audit_head_hash', 'audit_head_id');",
            )
            .expect("tamper");
    }

    let workspace = Workspace::open(&path, Box::new(TestClock::new())).expect("reopen");
    let report = workspace.integrity_check().expect("check");
    assert!(
        !report.audit_chain,
        "a half-migrated modern log was backfilled and laundered"
    );
}
