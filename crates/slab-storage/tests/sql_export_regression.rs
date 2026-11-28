#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod support;

use std::sync::Arc;

use slab_core::{ColumnDraft, ColumnType, PolicyHook, RowData, TableDraft, Value};
use slab_storage::Workspace;
use support::TestClock;

fn workspace_with_select() -> Workspace {
    let mut workspace =
        Workspace::open_in_memory("Export", Box::new(TestClock::new())).expect("workspace");
    workspace
        .create_table(&TableDraft {
            name: "Notes".into(),
            columns: vec![ColumnDraft {
                name: "Status".into(),
                ty: ColumnType::Select {
                    options: vec!["Open".into(), "Closed".into()],
                },
                required: true,
                unique: false,
                default: None,
            }],
        })
        .expect("create table");
    workspace
        .insert_row(
            "Notes",
            &RowData::from([("Status".into(), Value::Select("Open".into()))]),
        )
        .expect("insert row");
    workspace
}

struct RedactWith(Value);

impl PolicyHook for RedactWith {
    fn redact(&self, _table: &str, _column: &str, _value: &Value) -> Option<Value> {
        Some(self.0.clone())
    }
}

#[test]
fn invalid_select_redaction_preserves_existing_sql_export() {
    let mut workspace = workspace_with_select();
    let directory = tempfile::tempdir().expect("temporary directory");
    let target = directory.path().join("notes.sql");
    std::fs::write(&target, "previous export").expect("seed destination");
    for value in [Value::Select("Archived".into()), Value::Null] {
        workspace.set_policy(Arc::new(RedactWith(value)));
        let error = workspace
            .export_sql(&target)
            .expect_err("incompatible redaction");
        assert!(error.to_string().contains("does not fit column"));
        assert_eq!(
            std::fs::read_to_string(&target).expect("read destination"),
            "previous export"
        );
        assert_eq!(
            directory.path().read_dir().expect("read directory").count(),
            1
        );
    }
}

#[cfg(unix)]
fn scratch_in(directory: &std::path::Path) -> std::path::PathBuf {
    directory
        .read_dir()
        .expect("read export directory")
        .map(|entry| entry.expect("entry").path())
        .find(|path| path.extension().is_some_and(|extension| extension == "tmp"))
        .expect("find scratch")
}

#[cfg(unix)]
#[test]
fn a_new_sql_export_keeps_its_scratch_file_private() {
    use std::os::unix::fs::PermissionsExt;
    use std::path::PathBuf;

    struct CheckScratchMode(PathBuf);

    impl PolicyHook for CheckScratchMode {
        fn redact(&self, _table: &str, _column: &str, _value: &Value) -> Option<Value> {
            let mode = std::fs::metadata(scratch_in(&self.0))
                .expect("scratch metadata")
                .permissions()
                .mode();
            assert_eq!(mode & 0o077, 0);
            None
        }
    }

    let mut workspace = workspace_with_select();
    let directory = tempfile::tempdir().expect("temporary directory");
    let target = directory.path().join("notes.sql");
    workspace.set_policy(Arc::new(CheckScratchMode(directory.path().to_owned())));
    workspace.export_sql(&target).expect("new private export");
    let mode = target
        .metadata()
        .expect("export metadata")
        .permissions()
        .mode();
    assert_eq!(mode & 0o077, 0);
}

#[cfg(unix)]
#[test]
fn sql_export_refuses_a_replaced_scratch_file() {
    use std::path::PathBuf;

    struct ReplaceScratch(PathBuf);

    impl PolicyHook for ReplaceScratch {
        fn redact(&self, _table: &str, _column: &str, _value: &Value) -> Option<Value> {
            let scratch = scratch_in(&self.0);
            std::fs::rename(&scratch, self.0.join("detached.sql")).expect("move scratch");
            std::fs::write(scratch, "replacement").expect("replace scratch");
            None
        }
    }

    let mut workspace = workspace_with_select();
    let directory = tempfile::tempdir().expect("temporary directory");
    let target = directory.path().join("notes.sql");
    std::fs::write(&target, "previous export").expect("seed destination");
    workspace.set_policy(Arc::new(ReplaceScratch(directory.path().to_owned())));

    let error = workspace.export_sql(&target).expect_err("scratch changed");
    assert!(error.to_string().contains("scratch file was replaced"));
    assert_eq!(
        std::fs::read_to_string(&target).expect("read destination"),
        "previous export"
    );
    assert_eq!(
        directory.path().read_dir().expect("read directory").count(),
        3
    );
}
