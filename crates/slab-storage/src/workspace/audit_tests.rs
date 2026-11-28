use rusqlite::{Connection, params};
use slab_core::{ColumnDraft, ColumnType, RowData, TableDraft, Value};

use super::Workspace;
use super::audit::{
    AUDIT_ROW_COLUMNS, AuditRow, GENESIS_HASH, audit_hash_legacy, migrate_audit_hash_current,
    set_meta, verify_audit_chain,
};
use crate::test_support::TestClock;

fn note_workspace() -> Workspace {
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
    for index in 0..3 {
        workspace
            .insert_row(
                "Notes",
                &RowData::from([("Body".to_owned(), Value::Text(format!("note {index}")))]),
            )
            .expect("row");
    }
    workspace
}

#[test]
fn audit_pages_count_only_authorized_and_tableless_entries() {
    struct DenyNotes;
    impl slab_core::PolicyHook for DenyNotes {
        fn can_read_table(&self, table: &str) -> bool {
            table != "Notes"
        }
    }
    let mut workspace = note_workspace();
    workspace
        .create_table(&TableDraft {
            name: "Public".into(),
            columns: vec![ColumnDraft {
                name: "Body".into(),
                ty: ColumnType::Text,
                required: false,
                unique: false,
                default: None,
            }],
        })
        .expect("public table");
    workspace
        .conn
        .execute(
            "UPDATE _slab_audit_log SET table_name = NULL WHERE id = 3",
            [],
        )
        .expect("represent a tableless entry");
    for table in ["Notes", "Public", "Notes", "Notes", "Notes"] {
        workspace.insert_row(table, &RowData::new()).expect("row");
    }
    workspace.set_policy(std::sync::Arc::new(DenyNotes));

    let table_names = |limit, offset| {
        workspace
            .audit_entries(limit, offset)
            .expect("audit page")
            .into_iter()
            .map(|entry| entry.table)
            .collect::<Vec<_>>()
    };
    assert_eq!(
        table_names(2, 0),
        vec![Some("Public".into()), Some("Public".into())]
    );
    assert_eq!(table_names(2, 2), vec![None]);
    assert!(table_names(2, 3).is_empty());
    assert!(table_names(0, 0).is_empty());
}

pub(super) fn downgrade_to_legacy_scheme(workspace: &Workspace) {
    let rows = {
        let mut stmt = workspace
            .conn
            .prepare(&format!(
                "SELECT {AUDIT_ROW_COLUMNS} FROM _slab_audit_log ORDER BY id ASC"
            ))
            .expect("select");
        let mapped = stmt.query_map([], AuditRow::from_row).expect("map");
        let mut collected = Vec::new();
        for row in mapped {
            collected.push(row.expect("row"));
        }
        collected
    };
    let mut previous = GENESIS_HASH.to_owned();
    let mut last_id = 0;
    for audit in rows {
        let diff = audit.diff().expect("diff");
        let hash = audit_hash_legacy(
            &previous,
            &audit.hlc,
            audit.table.as_deref(),
            audit.row_id,
            &diff,
        );
        workspace
            .conn
            .execute(
                "UPDATE _slab_audit_log
                 SET prev_hash = ?1, entry_hash = ?2, hash_version = 1 WHERE id = ?3",
                params![previous, hash, audit.id],
            )
            .expect("downgrade");
        previous.clone_from(&hash);
        last_id = audit.id;
    }
    set_meta(&workspace.conn, "audit_head_hash", &previous).expect("head");
    set_meta(&workspace.conn, "audit_head_id", &last_id.to_string()).expect("head id");
}

pub(super) fn run_migration(workspace: &Workspace) {
    let tx = workspace.conn.unchecked_transaction().expect("tx");
    migrate_audit_hash_current(&tx).expect("migrate");
    tx.commit().expect("commit");
}

pub(super) fn versions(workspace: &Workspace) -> Vec<i64> {
    let mut stmt = workspace
        .conn
        .prepare("SELECT hash_version FROM _slab_audit_log ORDER BY id ASC")
        .expect("select versions");
    let mapped = stmt
        .query_map([], |row| row.get::<_, i64>(0))
        .expect("map versions");
    mapped.map(|row| row.expect("version")).collect()
}

#[test]
fn a_format_one_scheme_one_file_upgrades_on_open() {
    let directory = tempfile::TempDir::new().expect("tempdir");
    let path = directory.path().join("scheme-one.slab");
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
    downgrade_to_legacy_scheme(&workspace);
    set_meta(&workspace.conn, "format_version", "1").expect("format");
    workspace.close().expect("close");
    {
        let connection = Connection::open(&path).expect("raw open");
        connection
            .execute_batch("ALTER TABLE _slab_audit_log DROP COLUMN hash_version;")
            .expect("drop version column");
    }

    let workspace = Workspace::open(&path, Box::new(TestClock::new())).expect("open");
    assert!(
        workspace.integrity_check().expect("check").audit_chain,
        "scheme-1 file did not upgrade and verify"
    );
    assert!(versions(&workspace).iter().all(|version| *version == 2));
    assert_eq!(
        workspace.info().expect("info").format_version,
        slab_core::FORMAT_VERSION
    );
}

#[test]
fn new_entries_hash_the_actor_and_transaction_id() {
    let workspace = note_workspace();
    assert!(versions(&workspace).iter().all(|version| *version == 2));
    assert!(verify_audit_chain(&workspace.conn).expect("verify").0);

    workspace
        .conn
        .execute(
            "UPDATE _slab_audit_log SET actor = 'attacker' WHERE id = 1",
            [],
        )
        .expect("tamper actor");
    let (valid, _, reason) = verify_audit_chain(&workspace.conn).expect("verify");
    assert!(!valid, "actor tampering was not detected");
    assert!(reason.is_some_and(|text| text.contains("BLAKE3")));

    workspace
        .conn
        .execute(
            "UPDATE _slab_audit_log SET actor = 'local', transaction_id = 'forged' WHERE id = 1",
            [],
        )
        .expect("tamper transaction");
    let (valid, _, _) = verify_audit_chain(&workspace.conn).expect("verify");
    assert!(!valid, "transaction-id tampering was not detected");
}

#[test]
fn timestamp_tampering_is_detected() {
    let workspace = note_workspace();
    workspace
        .conn
        .execute(
            "UPDATE _slab_audit_log SET timestamp_ms = timestamp_ms + 1 WHERE id = 1",
            [],
        )
        .expect("tamper timestamp");
    let (valid, _, _) = verify_audit_chain(&workspace.conn).expect("verify");
    assert!(!valid, "timestamp tampering was not detected");
}

#[test]
fn deleting_the_head_or_all_hashes_does_not_launder_a_modern_log() {
    let directory = tempfile::TempDir::new().expect("tempdir");
    for (name, statement) in [
        (
            "head-only",
            "DELETE FROM _slab_meta WHERE key IN ('audit_head_hash', 'audit_head_id')",
        ),
        (
            "nulled-and-head",
            "UPDATE _slab_audit_log
                SET prev_hash = NULL, entry_hash = NULL, actor = 'mallory';
             DELETE FROM _slab_meta WHERE key IN ('audit_head_hash', 'audit_head_id')",
        ),
    ] {
        let path = directory.path().join(format!("{name}.slab"));
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
            connection.execute_batch(statement).expect("tamper");
        }
        let workspace = Workspace::open(&path, Box::new(TestClock::new())).expect("reopen");
        let report = workspace.integrity_check().expect("check");
        assert!(
            !report.audit_chain,
            "{name}: a deleted anchor or NULLed modern log was laundered"
        );
        assert!(report.audit_tamper.is_some(), "{name}: no diagnostic");
    }
}

#[test]
fn a_legacy_chain_migrates_to_the_current_scheme() {
    let workspace = note_workspace();
    downgrade_to_legacy_scheme(&workspace);
    assert!(versions(&workspace).iter().all(|version| *version == 1));
    assert!(
        verify_audit_chain(&workspace.conn)
            .expect("scheme 1 verify")
            .0
    );

    run_migration(&workspace);
    assert!(versions(&workspace).iter().all(|version| *version == 2));
    assert!(
        verify_audit_chain(&workspace.conn)
            .expect("scheme 2 verify")
            .0
    );

    workspace
        .conn
        .execute(
            "UPDATE _slab_audit_log SET actor = 'attacker' WHERE id = 2",
            [],
        )
        .expect("tamper");
    assert!(!verify_audit_chain(&workspace.conn).expect("verify").0);
}

#[test]
fn a_tampered_legacy_chain_is_not_migrated() {
    let workspace = note_workspace();
    downgrade_to_legacy_scheme(&workspace);
    workspace
        .conn
        .execute(
            "UPDATE _slab_audit_log SET summary = 'silently rewritten' WHERE id = 2",
            [],
        )
        .expect("tamper");

    run_migration(&workspace);
    assert!(versions(&workspace).iter().all(|version| *version == 1));
    assert!(
        !verify_audit_chain(&workspace.conn).expect("verify").0,
        "a tampered legacy chain was laundered"
    );
}
