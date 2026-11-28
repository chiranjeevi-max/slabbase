use rusqlite::{Connection, params};

use super::Workspace;
use crate::test_support::TestClock;

fn fixture_copy(directory: &tempfile::TempDir) -> std::path::PathBuf {
    let source =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/design-studio.slab");
    let path = directory.path().join("fixture.slab");
    std::fs::copy(&source, &path).expect("copy fixture");
    path
}

#[test]
fn an_invalid_stored_formula_is_rejected_before_the_format_migration() {
    let source =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/design-studio.slab");
    if !source.exists() {
        return;
    }
    let directory = tempfile::TempDir::new().expect("tempdir");
    let path = fixture_copy(&directory);
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
        let expression = format!("{}1", "1+".repeat(200));
        connection
            .execute(
                "UPDATE _slab_columns SET type_json = ?1
                 WHERE name = 'Name' AND table_name = 'Clients'",
                params![format!(
                    r#"{{"kind":"formula","expression":"{expression}"}}"#
                )],
            )
            .expect("long formula");
    }

    assert!(
        Workspace::open(&path, Box::new(TestClock::new())).is_err(),
        "an over-limit stored formula must be rejected"
    );

    let connection = Connection::open(&path).expect("raw reopen");
    let version: String = connection
        .query_row(
            "SELECT value FROM _slab_meta WHERE key = 'format_version'",
            [],
            |row| row.get(0),
        )
        .expect("format");
    assert_eq!(version, "1", "a rejected open bumped the format");
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
        "a rejected open added hash columns: {columns:?}"
    );
}

#[test]
fn a_foreign_file_with_matching_table_names_is_not_mutated() {
    let directory = tempfile::TempDir::new().expect("tempdir");
    let path = directory.path().join("foreign.slab");
    {
        let connection = Connection::open(&path).expect("raw create");
        connection
            .execute_batch(
                "CREATE TABLE _slab_meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
                 CREATE TABLE _slab_tables (x TEXT);
                 CREATE TABLE _slab_columns (x TEXT);
                 CREATE TABLE _slab_relations (x TEXT);
                 CREATE TABLE _slab_layouts (x TEXT);
                 INSERT INTO _slab_meta VALUES ('format_version', '1');",
            )
            .expect("seed");
    }
    let before = std::fs::read(&path).expect("read before");
    assert!(Workspace::open(&path, Box::new(TestClock::new())).is_err());
    let after = std::fs::read(&path).expect("read after");
    assert_eq!(before, after, "a foreign file was mutated before rejection");
}
