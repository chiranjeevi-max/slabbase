use rusqlite::Connection;

use super::Workspace;
use crate::test_support::TestClock;

#[test]
fn a_failed_legacy_migration_rolls_the_new_columns_back() {
    let source =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/design-studio.slab");
    if !source.exists() {
        return;
    }
    let directory = tempfile::TempDir::new().expect("tempdir");
    let path = directory.path().join("fixture.slab");
    std::fs::copy(&source, &path).expect("copy fixture");
    {
        let connection = Connection::open(&path).expect("raw open");
        let version: String = connection
            .query_row(
                "SELECT value FROM _slab_meta WHERE key = 'format_version'",
                [],
                |row| row.get(0),
            )
            .expect("format");
        assert_eq!(version, "1", "the fixture must predate format 2");
        connection
            .execute_batch(
                "CREATE TRIGGER fail_audit_rewrite BEFORE UPDATE ON _slab_audit_log
                 BEGIN SELECT RAISE(ABORT, 'forced migration failure'); END;",
            )
            .expect("trigger");
    }
    assert!(
        Workspace::open(&path, Box::new(TestClock::new())).is_err(),
        "the forced rewrite failure must abort the open"
    );
    {
        let connection = Connection::open(&path).expect("raw reopen");
        let mut stmt = connection
            .prepare("PRAGMA table_info(_slab_audit_log)")
            .expect("table info");
        let columns: Vec<String> = stmt
            .query_map([], |row| row.get::<_, String>(1))
            .expect("columns")
            .map(|row| row.expect("column"))
            .collect();
        assert!(
            !columns.iter().any(|name| name == "prev_hash"),
            "a failed migration left the new columns behind: {columns:?}"
        );
        connection
            .execute_batch("DROP TRIGGER fail_audit_rewrite")
            .expect("drop trigger");
    }
    let workspace = Workspace::open(&path, Box::new(TestClock::new())).expect("retry");
    assert!(workspace.integrity_check().expect("check").audit_chain);
}
