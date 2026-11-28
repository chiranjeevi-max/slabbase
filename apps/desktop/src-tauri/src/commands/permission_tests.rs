use super::*;
use slab_core::{ColumnDraft, ColumnType, TableDraft};
use slab_sync::HybridLogicalClock;
use std::fs;
use std::os::unix::fs::PermissionsExt;

#[test]
fn export_publish_preserves_private_destination_permissions() {
    let mut workspace =
        Workspace::open_in_memory("Export", Box::new(HybridLogicalClock::new("test")))
            .expect("workspace");
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

    let directory = tempfile::TempDir::new().expect("tempdir");
    let path = directory.path().join("private.json");
    fs::write(&path, "old").expect("seed");
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).expect("private mode");

    export_json_to(
        &workspace,
        path.to_str().expect("path"),
        PageQuery {
            table: "Notes".into(),
            ..PageQuery::default()
        },
    )
    .expect("export");

    let mode = fs::metadata(&path).expect("metadata").permissions().mode() & 0o777;
    assert_eq!(
        mode, 0o600,
        "publishing widened the destination permissions"
    );
}

#[test]
fn export_scratch_is_private_before_rows_are_written() {
    let directory = tempfile::TempDir::new().expect("tempdir");
    let path = directory.path().join("private.csv");
    fs::write(&path, "old").expect("seed");
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).expect("private mode");

    let (temporary, _file) = ExportTemp::create(path.to_str().expect("path")).expect("scratch");

    let mode = fs::metadata(&temporary.path)
        .expect("scratch metadata")
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(
        mode & 0o077,
        0,
        "the scratch file for a private export is readable by others: {mode:o}"
    );
}

#[test]
fn sql_export_does_not_truncate_when_the_scratch_cannot_be_created() {
    let workspace = Workspace::open_in_memory("Export", Box::new(HybridLogicalClock::new("test")))
        .expect("workspace");
    let directory = tempfile::TempDir::new().expect("tempdir");
    let parent = directory.path().join("locked");
    fs::create_dir(&parent).expect("locked dir");
    let target = parent.join("export.sql");
    fs::write(&target, "keep me").expect("seed export");
    fs::set_permissions(&parent, fs::Permissions::from_mode(0o500)).expect("lock dir");

    let outcome = export_sql_to(&workspace, target.to_str().expect("path"));
    // Make the directory removable again before any assertion can panic.
    fs::set_permissions(&parent, fs::Permissions::from_mode(0o700)).expect("unlock dir");

    assert!(
        outcome.is_err(),
        "a scratch file in a read-only directory must fail"
    );
    assert_eq!(
        fs::read_to_string(&target).expect("read export"),
        "keep me",
        "a failed SQL export truncated the previous file"
    );
}
