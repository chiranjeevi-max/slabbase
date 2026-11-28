use rusqlite::params;
use slab_core::{ColumnDraft, ColumnType, RowData, TableDraft, Value};

use super::Workspace;
use crate::test_support::TestClock;

#[test]
fn restores_a_text_tagged_select_revision() {
    let mut workspace =
        Workspace::open_in_memory("Restore", Box::new(TestClock::new())).expect("workspace");
    workspace
        .create_table(&TableDraft {
            name: "Deals".into(),
            columns: vec![
                ColumnDraft {
                    name: "Name".into(),
                    ty: ColumnType::Text,
                    required: false,
                    unique: false,
                    default: None,
                },
                ColumnDraft {
                    name: "Stage".into(),
                    ty: ColumnType::Select {
                        options: vec!["Open".into(), "Won".into()],
                    },
                    required: false,
                    unique: false,
                    default: None,
                },
            ],
        })
        .expect("table");
    let deal = workspace
        .insert_row(
            "Deals",
            &RowData::from([
                ("Name".to_owned(), Value::Text("Northlight".into())),
                ("Stage".to_owned(), Value::Select("Open".into())),
            ]),
        )
        .expect("insert");
    let insert_audit = workspace
        .audit_entries(10, 0)
        .expect("audit")
        .into_iter()
        .find(|entry| entry.operation == "insert" && entry.table.as_deref() == Some("Deals"))
        .expect("insert audit")
        .id;
    workspace
        .update_row(
            "Deals",
            deal.id,
            &RowData::from([("Stage".to_owned(), Value::Select("Won".into()))]),
        )
        .expect("update");

    // Simulate a revision written before decode fidelity.
    workspace
        .conn
        .execute(
            "UPDATE _slab_audit_log
             SET after_json = replace(after_json, '\"Select\":\"Open\"', '\"Text\":\"Open\"')
             WHERE id = ?1",
            params![insert_audit],
        )
        .expect("legacy payload");
    let payload: String = workspace
        .conn
        .query_row(
            "SELECT after_json FROM _slab_audit_log WHERE id = ?1",
            params![insert_audit],
            |row| row.get(0),
        )
        .expect("payload");
    assert!(
        payload.contains("\"Text\":\"Open\""),
        "the legacy payload rewrite must match: {payload}"
    );

    let restored = workspace
        .restore_record_revision("Deals", deal.id, insert_audit)
        .expect("restore legacy select revision");
    assert_eq!(restored.get("Stage"), Some(&Value::Select("Open".into())));
}

#[test]
fn restores_a_text_tagged_attachment_revision() {
    let mut workspace =
        Workspace::open_in_memory("Restore", Box::new(TestClock::new())).expect("workspace");
    workspace
        .create_table(&TableDraft {
            name: "Files".into(),
            columns: vec![
                ColumnDraft {
                    name: "Label".into(),
                    ty: ColumnType::Text,
                    required: false,
                    unique: false,
                    default: None,
                },
                ColumnDraft {
                    name: "Path".into(),
                    ty: ColumnType::AttachmentRef,
                    required: false,
                    unique: false,
                    default: None,
                },
            ],
        })
        .expect("table");
    let file = workspace
        .insert_row(
            "Files",
            &RowData::from([
                ("Label".to_owned(), Value::Text("Spec".into())),
                (
                    "Path".to_owned(),
                    Value::AttachmentRef("/tmp/spec.pdf".into()),
                ),
            ]),
        )
        .expect("insert");
    let insert_audit = workspace
        .audit_entries(10, 0)
        .expect("audit")
        .into_iter()
        .find(|entry| entry.operation == "insert" && entry.table.as_deref() == Some("Files"))
        .expect("insert audit")
        .id;
    workspace
        .update_row(
            "Files",
            file.id,
            &RowData::from([(
                "Path".to_owned(),
                Value::AttachmentRef("/tmp/other.pdf".into()),
            )]),
        )
        .expect("update");

    workspace
        .conn
        .execute(
            "UPDATE _slab_audit_log
             SET after_json = replace(after_json, '\"AttachmentRef\":\"/tmp/spec.pdf\"', '\"Text\":\"/tmp/spec.pdf\"')
             WHERE id = ?1",
            params![insert_audit],
        )
        .expect("legacy payload");
    let payload: String = workspace
        .conn
        .query_row(
            "SELECT after_json FROM _slab_audit_log WHERE id = ?1",
            params![insert_audit],
            |row| row.get(0),
        )
        .expect("payload");
    assert!(
        payload.contains("\"Text\":\"/tmp/spec.pdf\""),
        "the legacy payload rewrite must match: {payload}"
    );

    let restored = workspace
        .restore_record_revision("Files", file.id, insert_audit)
        .expect("restore legacy attachment revision");
    assert_eq!(
        restored.get("Path"),
        Some(&Value::AttachmentRef("/tmp/spec.pdf".into()))
    );
}
