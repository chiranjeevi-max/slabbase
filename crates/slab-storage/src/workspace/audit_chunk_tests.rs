use slab_core::{ColumnDraft, ColumnType, RowData, TableDraft, Value};

use super::audit_tests::run_migration;
use super::{Workspace, verify_audit_chain};
use crate::test_support::TestClock;

#[test]
fn a_legacy_chain_migrates_across_chunk_boundaries() {
    let mut workspace =
        Workspace::open_in_memory("Audit", Box::new(TestClock::new())).expect("workspace");
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
    let rows: Vec<RowData> = (0..300)
        .map(|index| RowData::from([("Body".to_owned(), Value::Text(format!("note {index}")))]))
        .collect();
    workspace.insert_rows("Notes", &rows).expect("rows");

    // 301 entries (create table + 300 rows) cross the 256-row chunk.
    super::audit_tests::downgrade_to_legacy_scheme(&workspace);
    assert!(verify_audit_chain(&workspace.conn).expect("scheme 1").0);

    run_migration(&workspace);
    let versions = super::audit_tests::versions(&workspace);
    assert_eq!(versions.len(), 301);
    assert!(versions.iter().all(|version| *version == 2));
    assert!(verify_audit_chain(&workspace.conn).expect("scheme 2").0);
}
