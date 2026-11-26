//! Regression tests for defects found during the failure-mode audit.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use slab_core::{ColumnDraft, ColumnType, TableDraft};
use slabbase_desktop::ApiError;
use slabbase_desktop::worker::DbHandle;

#[test]
fn failed_create_keeps_the_current_workspace_open() {
    let directory = tempfile::TempDir::new().expect("tempdir");
    let workspace = directory.path().join("open.slab");
    let db = DbHandle::new().expect("handle");
    db.create(workspace.to_str().expect("path").to_owned(), "Open".into())
        .expect("create");
    assert!(db.info().expect("info").is_some());

    // A create that fails must not tear down the workspace already on screen.
    let blocked = directory.path().join("blocked.slab");
    std::fs::write(&blocked, "not an empty workspace").expect("blocker");
    let error = db.create(blocked.to_str().expect("path").to_owned(), "Blocked".into());
    assert!(error.is_err(), "create over a non-empty file must fail");
    assert!(
        db.info().expect("info").is_some(),
        "a failed create closed the active workspace"
    );

    // The surviving writer is still usable, and a later create can switch it.
    db.write(|workspace| {
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
            .map_err(ApiError::from)?;
        Ok(())
    })
    .expect("write after a failed create");

    let second = directory.path().join("second.slab");
    db.create(second.to_str().expect("path").to_owned(), "Second".into())
        .expect("second create");
    assert_eq!(db.info().expect("info").expect("open").name, "Second");
}
