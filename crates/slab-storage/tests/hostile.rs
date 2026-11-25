#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

//! Adversarial regression tests for the storage engine.
//!
//! Every test in this file encodes a failure mode that was reproduced before
//! the matching fix. Keep the assertions behavioral: they must fail on the
//! defect, not on an incidental implementation detail.

mod support;

use std::fs;
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::Path;

use rusqlite::Connection;
use slab_core::{
    Aggregate, ColumnDraft, ColumnType, DeleteAction, RelationDraft, RowData, TableDraft, Value,
};
use slab_storage::{QueryCancel, StorageError, Workspace};
use support::TestClock;

fn text(name: &str) -> ColumnDraft {
    ColumnDraft {
        name: name.into(),
        ty: ColumnType::Text,
        required: false,
        unique: false,
        default: None,
    }
}

fn integer(name: &str) -> ColumnDraft {
    ColumnDraft {
        name: name.into(),
        ty: ColumnType::Integer,
        required: false,
        unique: false,
        default: None,
    }
}

fn lookup(name: &str, relation: &str, column: &str, aggregate: Aggregate) -> ColumnDraft {
    ColumnDraft {
        name: name.into(),
        ty: ColumnType::Lookup {
            relation: relation.into(),
            column: column.into(),
            aggregate,
        },
        required: false,
        unique: false,
        default: None,
    }
}

fn row(values: &[(&str, Value)]) -> RowData {
    values
        .iter()
        .map(|(name, value)| ((*name).to_owned(), value.clone()))
        .collect()
}

fn raw_count(path: &Path, table: &str) -> i64 {
    let connection = Connection::open(path).expect("raw open");
    connection
        .query_row(&format!("SELECT COUNT(*) FROM \"{table}\""), [], |row| {
            row.get(0)
        })
        .expect("raw count")
}

fn raw_scalar(path: &Path, sql: &str) -> i64 {
    let connection = Connection::open(path).expect("raw open");
    connection
        .query_row(sql, [], |row| row.get(0))
        .expect("raw scalar")
}

fn stored_text(path: &Path, table: &str, column: &str) -> String {
    let connection = Connection::open(path).expect("raw open");
    connection
        .query_row(
            &format!("SELECT \"{column}\" FROM \"{table}\" ORDER BY id LIMIT 1"),
            [],
            |row| row.get(0),
        )
        .expect("raw text")
}

/// A rebuild of a parent table must never fire `ON DELETE CASCADE` against its
/// children. `DROP TABLE` performs an implicit `DELETE` when foreign keys are
/// enabled, so the rebuild path has to suspend enforcement first.
#[test]
fn dropping_a_column_preserves_cascade_children() {
    let directory = tempfile::TempDir::new().expect("tempdir");
    let path = directory.path().join("cascade-drop.slab");
    let mut workspace =
        Workspace::create(&path, "Cascade", Box::new(TestClock::new())).expect("create");
    workspace
        .create_table(&TableDraft {
            name: "Clients".into(),
            columns: vec![text("Name"), text("Notes")],
        })
        .expect("clients");
    workspace
        .create_table(&TableDraft {
            name: "Projects".into(),
            columns: vec![text("Name"), integer("Client")],
        })
        .expect("projects");
    workspace
        .create_relation(&RelationDraft {
            name: "Client Projects".into(),
            parent_table: "Clients".into(),
            child_table: "Projects".into(),
            child_column: "Client".into(),
            on_delete: DeleteAction::Cascade,
        })
        .expect("relation");
    let client = workspace
        .insert_row(
            "Clients",
            &row(&[
                ("Name", Value::from("Northlight")),
                ("Notes", Value::from("priority client")),
            ]),
        )
        .expect("client");
    for project in ["Identity", "Website"] {
        workspace
            .insert_row(
                "Projects",
                &row(&[
                    ("Name", Value::from(project)),
                    ("Client", Value::Integer(client.id)),
                ]),
            )
            .expect("project");
    }

    workspace.clear_history();
    workspace
        .drop_column("Clients", "Notes")
        .expect("drop unused column");

    assert_eq!(
        raw_count(&path, "Projects"),
        2,
        "dropping a parent column cascaded its children away"
    );
    assert_eq!(raw_count(&path, "Clients"), 1);
    assert_eq!(
        workspace
            .integrity_check()
            .expect("integrity")
            .foreign_key_violations,
        0
    );

    // Undo rebuilds the table again through `RestoreColumn`; it must preserve
    // both the restored values and the child rows.
    workspace.undo().expect("undo the drop");
    assert_eq!(raw_count(&path, "Projects"), 2);
    let restored = workspace.read_record("Clients", client.id).expect("client");
    assert_eq!(restored.get("Notes"), Some(&Value::from("priority client")));
}

/// `export_sql` intentionally omits the audit log. It must therefore not carry
/// the old chain anchors into the restored file, or `Integrity Check` reports
/// tamper on a perfectly legitimate restore.
#[test]
fn sql_export_round_trip_keeps_the_audit_chain_verifiable() {
    let directory = tempfile::TempDir::new().expect("tempdir");
    let path = directory.path().join("source.slab");
    let dump = directory.path().join("source.sql");
    let mut workspace =
        Workspace::create(&path, "Portable", Box::new(TestClock::new())).expect("create");
    workspace
        .create_table(&TableDraft {
            name: "Notes".into(),
            columns: vec![text("Body")],
        })
        .expect("table");
    for index in 0..3 {
        workspace
            .insert_row(
                "Notes",
                &row(&[("Body", Value::from(format!("note {index}")))]),
            )
            .expect("row");
    }
    workspace.export_sql(&dump).expect("export");
    workspace.close().expect("close");

    let restored_path = directory.path().join("restored.slab");
    let script = fs::read_to_string(&dump).expect("dump");
    let connection = Connection::open(&restored_path).expect("restore open");
    connection.execute_batch(&script).expect("restore script");
    drop(connection);

    let mut restored = Workspace::open(&restored_path, Box::new(TestClock::new())).expect("open");
    let report = restored.integrity_check().expect("integrity");
    assert!(
        report.audit_chain,
        "restored workspace reported a false tamper: {:?}",
        report.audit_tamper
    );
    assert_eq!(report.audit_entries, 0);

    restored
        .insert_row("Notes", &row(&[("Body", Value::from("after restore"))]))
        .expect("write after restore");
    let report = restored.integrity_check().expect("integrity");
    assert!(
        report.audit_chain,
        "first write after restore broke the chain: {:?}",
        report.audit_tamper
    );
}

/// The rollup cache lives in the database. An external `sqlite3` write that
/// bypasses SlabBase must not leave a stale aggregate behind after reopen.
#[test]
fn external_child_write_invalidates_stale_rollups_on_reopen() {
    let directory = tempfile::TempDir::new().expect("tempdir");
    let path = directory.path().join("external-rollup.slab");
    let mut workspace =
        Workspace::create(&path, "External", Box::new(TestClock::new())).expect("create");
    workspace
        .create_table(&TableDraft {
            name: "Invoices".into(),
            columns: vec![text("Number")],
        })
        .expect("invoices");
    workspace
        .create_table(&TableDraft {
            name: "Items".into(),
            columns: vec![integer("Invoice"), integer("Amount")],
        })
        .expect("items");
    workspace
        .create_relation(&RelationDraft {
            name: "Invoice Items".into(),
            parent_table: "Invoices".into(),
            child_table: "Items".into(),
            child_column: "Invoice".into(),
            on_delete: DeleteAction::Cascade,
        })
        .expect("relation");
    workspace
        .add_column(
            "Invoices",
            &lookup("Total", "Invoice Items", "Amount", Aggregate::Sum),
        )
        .expect("lookup");
    let invoice = workspace
        .insert_row("Invoices", &row(&[("Number", Value::from("INV-1"))]))
        .expect("invoice")
        .id;
    workspace
        .insert_row(
            "Items",
            &row(&[
                ("Invoice", Value::Integer(invoice)),
                ("Amount", Value::Integer(100)),
            ]),
        )
        .expect("item");

    // Warm the cache with a normal read, then close so the value is persisted.
    let before = workspace
        .read_record("Invoices", invoice)
        .expect("warm read");
    assert_eq!(before.get("Total"), Some(&Value::Integer(100)));
    workspace.close().expect("close");

    let connection = Connection::open(&path).expect("external open");
    connection
        .busy_timeout(std::time::Duration::from_millis(5_000))
        .expect("timeout");
    connection
        .execute(
            "INSERT INTO \"Items\" (\"Invoice\", \"Amount\") VALUES (?1, ?2)",
            rusqlite::params![invoice, 50_i64],
        )
        .expect("external insert");
    drop(connection);

    let workspace = Workspace::open(&path, Box::new(TestClock::new())).expect("reopen");
    let after = workspace
        .read_record("Invoices", invoice)
        .expect("read after reopen");
    assert_eq!(
        after.get("Total"),
        Some(&Value::Integer(150)),
        "stale rollup served after an external child insert"
    );
}

/// `SUM` delta math must use a wide intermediate. The final total fits in
/// `i64`, but `old_sum - old_value` momentarily falls below `i64::MIN`.
#[test]
fn rollup_update_uses_wide_intermediate_arithmetic() {
    let directory = tempfile::TempDir::new().expect("tempdir");
    let path = directory.path().join("rollup-overflow.slab");
    let mut workspace =
        Workspace::create(&path, "Overflow", Box::new(TestClock::new())).expect("create");
    workspace
        .create_table(&TableDraft {
            name: "Invoices".into(),
            columns: vec![text("Number")],
        })
        .expect("invoices");
    workspace
        .create_table(&TableDraft {
            name: "Items".into(),
            columns: vec![integer("Invoice"), integer("Amount")],
        })
        .expect("items");
    workspace
        .create_relation(&RelationDraft {
            name: "Items".into(),
            parent_table: "Invoices".into(),
            child_table: "Items".into(),
            child_column: "Invoice".into(),
            on_delete: DeleteAction::Cascade,
        })
        .expect("relation");
    workspace
        .add_column(
            "Invoices",
            &lookup("Total", "Items", "Amount", Aggregate::Sum),
        )
        .expect("lookup");
    let invoice = workspace
        .insert_row("Invoices", &row(&[("Number", Value::from("INV-2"))]))
        .expect("invoice")
        .id;

    // Insertion order builds the running sum MIN -> -1 -> -2 in checked i64.
    let mut ids = Vec::new();
    for amount in [i64::MIN, i64::MAX, -1] {
        ids.push(
            workspace
                .insert_row(
                    "Items",
                    &row(&[
                        ("Invoice", Value::Integer(invoice)),
                        ("Amount", Value::Integer(amount)),
                    ]),
                )
                .expect("child")
                .id,
        );
    }
    assert_eq!(
        workspace
            .read_record("Invoices", invoice)
            .expect("read")
            .get("Total"),
        Some(&Value::Integer(-2))
    );

    // Removing MAX from a running sum of -2 underflows i64, but replacing it
    // with 1 yields MIN, which fits.
    let updated = workspace.update_row("Items", ids[1], &row(&[("Amount", Value::Integer(1))]));
    let record = updated.expect("update with a wide intermediate must succeed");
    assert_eq!(record.get("Amount"), Some(&Value::Integer(1)));
    assert_eq!(
        workspace
            .read_record("Invoices", invoice)
            .expect("read")
            .get("Total"),
        Some(&Value::Integer(i64::MIN))
    );
}

/// A cascade delete must not resurrect rollup rows for a parent that was just
/// deleted. The old single-pass loop processed the grandchild after forgetting
/// the child, which recreated the child's cache entry.
#[test]
fn cascade_delete_leaves_no_orphan_rollup_rows() {
    let directory = tempfile::TempDir::new().expect("tempdir");
    let path = directory.path().join("cascade-rollup.slab");
    let mut workspace =
        Workspace::create(&path, "Cascade", Box::new(TestClock::new())).expect("create");
    workspace
        .create_table(&TableDraft {
            name: "Companies".into(),
            columns: vec![text("Name")],
        })
        .expect("companies");
    workspace
        .create_table(&TableDraft {
            name: "Clients".into(),
            columns: vec![text("Name"), integer("Company")],
        })
        .expect("clients");
    workspace
        .create_table(&TableDraft {
            name: "Projects".into(),
            columns: vec![text("Label"), integer("Client")],
        })
        .expect("projects");
    workspace
        .create_relation(&RelationDraft {
            name: "Company Clients".into(),
            parent_table: "Companies".into(),
            child_table: "Clients".into(),
            child_column: "Company".into(),
            on_delete: DeleteAction::Cascade,
        })
        .expect("company relation");
    workspace
        .create_relation(&RelationDraft {
            name: "Client Projects".into(),
            parent_table: "Clients".into(),
            child_table: "Projects".into(),
            child_column: "Client".into(),
            on_delete: DeleteAction::Cascade,
        })
        .expect("client relation");
    workspace
        .add_column(
            "Companies",
            &lookup("Client Count", "Company Clients", "Name", Aggregate::Count),
        )
        .expect("company lookup");
    workspace
        .add_column(
            "Clients",
            &lookup("Line Count", "Client Projects", "Label", Aggregate::Count),
        )
        .expect("client lookup");

    let company = workspace
        .insert_row("Companies", &row(&[("Name", Value::from("Acme"))]))
        .expect("company")
        .id;
    let client = workspace
        .insert_row(
            "Clients",
            &row(&[
                ("Name", Value::from("Northlight")),
                ("Company", Value::Integer(company)),
            ]),
        )
        .expect("client")
        .id;
    workspace
        .insert_row(
            "Projects",
            &row(&[
                ("Label", Value::from("Identity")),
                ("Client", Value::Integer(client)),
            ]),
        )
        .expect("project");

    // Warm both parent caches.
    workspace
        .read_record("Companies", company)
        .expect("company read");
    workspace
        .read_record("Clients", client)
        .expect("client read");
    assert!(
        raw_scalar(
            &path,
            "SELECT COUNT(*) FROM _slab_rollups WHERE parent_table = 'Clients'",
        ) > 0
    );

    workspace
        .delete_row("Companies", company)
        .expect("cascade delete");

    assert_eq!(
        raw_scalar(
            &path,
            "SELECT COUNT(*) FROM _slab_rollups WHERE parent_table = 'Clients'",
        ),
        0,
        "cascade delete left rollup rows for a deleted client"
    );
    assert_eq!(raw_count(&path, "Clients"), 0);
    assert_eq!(raw_count(&path, "Projects"), 0);
}

/// A `.slab` paired with a WAL whose header declares a different page size must
/// be rejected or ignored safely. It must never panic and must never surface
/// frames that were not part of a valid checkpoint.
#[test]
fn hostile_wal_page_size_is_handled_without_panic() {
    let directory = tempfile::TempDir::new().expect("tempdir");
    let source = directory.path().join("source.slab");
    {
        let mut workspace =
            Workspace::create(&source, "Wal", Box::new(TestClock::new())).expect("create");
        workspace
            .create_table(&TableDraft {
                name: "Notes".into(),
                columns: vec![text("Body")],
            })
            .expect("table");
        workspace.close().expect("close");
    }

    let connection = Connection::open(&source).expect("raw open");
    connection
        .busy_timeout(std::time::Duration::from_millis(5_000))
        .expect("timeout");
    connection
        .execute_batch("PRAGMA journal_mode = WAL; PRAGMA wal_autocheckpoint = 0;")
        .expect("wal");
    connection
        .execute(
            "INSERT INTO \"Notes\" (\"Body\") VALUES ('hostile page size')",
            [],
        )
        .expect("insert");
    let source_wal = source.with_extension("slab-wal");
    assert!(source_wal.exists(), "WAL was not created");
    let hostile = directory.path().join("hostile.slab");
    let hostile_wal = hostile.with_extension("slab-wal");
    fs::copy(&source, &hostile).expect("copy database");
    fs::copy(&source_wal, &hostile_wal).expect("copy wal");
    drop(connection);

    let mut header = fs::read(&hostile_wal).expect("read wal");
    assert!(header.len() >= 12);
    header[8..12].copy_from_slice(&8192_u32.to_be_bytes());
    fs::write(&hostile_wal, &header).expect("write wal");

    match Workspace::open(&hostile, Box::new(TestClock::new())) {
        Ok(workspace) => {
            let report = workspace.integrity_check().expect("integrity");
            assert_eq!(report.integrity, "ok");
            assert_eq!(
                raw_count(&hostile, "Notes"),
                0,
                "hostile WAL frames became visible"
            );
        }
        Err(error) => {
            assert!(
                matches!(error.code_str(), "SLAB_E_WORKSPACE" | "SLAB_E_STORAGE"),
                "unexpected error code {}: {error}",
                error.code_str()
            );
        }
    }
}

/// Snapshots must never follow a hostile link, clobber an existing file, or
/// leave a partial target behind.
#[test]
fn snapshots_reject_hostile_targets_without_clobbering() {
    let directory = tempfile::TempDir::new().expect("tempdir");
    let path = directory.path().join("live.slab");
    let mut workspace =
        Workspace::create(&path, "Snapshot", Box::new(TestClock::new())).expect("create");
    workspace
        .create_table(&TableDraft {
            name: "Notes".into(),
            columns: vec![text("Body")],
        })
        .expect("table");
    workspace
        .insert_row("Notes", &row(&[("Body", Value::from("snapshot"))]))
        .expect("row");

    let existing = directory.path().join("existing.slab");
    fs::write(&existing, b"do not clobber").expect("seed target");
    let error = workspace
        .snapshot_to(&existing)
        .expect_err("existing target");
    assert_eq!(error.code_str(), "SLAB_E_WORKSPACE");
    assert_eq!(
        fs::read(&existing).expect("existing read"),
        b"do not clobber"
    );

    let link = directory.path().join("link.slab");
    symlink(&existing, &link).expect("symlink");
    let error = workspace.snapshot_to(&link).expect_err("symlink target");
    assert_eq!(error.code_str(), "SLAB_E_WORKSPACE");
    assert_eq!(
        fs::read(&existing).expect("link target read"),
        b"do not clobber"
    );

    let missing_target = directory.path().join("never-created.slab");
    let dangling = directory.path().join("dangling.slab");
    symlink(&missing_target, &dangling).expect("dangling symlink");
    let error = workspace
        .snapshot_vacuum_to(&dangling)
        .expect_err("dangling symlink must be rejected");
    assert_eq!(error.code_str(), "SLAB_E_WORKSPACE");
    assert!(
        fs::symlink_metadata(&dangling)
            .expect("link metadata")
            .file_type()
            .is_symlink(),
        "dangling symlink was replaced"
    );
    assert!(
        !missing_target.exists(),
        "snapshot wrote through a dangling symlink"
    );

    let missing_parent = directory.path().join("missing").join("snapshot.slab");
    let error = workspace
        .snapshot_to(&missing_parent)
        .expect_err("missing parent");
    assert!(!matches!(error, StorageError::Cancelled));
    assert!(!missing_parent.exists());

    let readonly = directory.path().join("readonly");
    fs::create_dir(&readonly).expect("readonly dir");
    fs::set_permissions(&readonly, fs::Permissions::from_mode(0o555)).expect("chmod");
    let readonly_target = readonly.join("snapshot.slab");
    let result = workspace.snapshot_to(&readonly_target);
    fs::set_permissions(&readonly, fs::Permissions::from_mode(0o755)).expect("restore chmod");
    assert!(result.is_err(), "snapshot into a read-only directory");
    assert!(!readonly_target.exists(), "partial snapshot residue");

    let long_name = directory.path().join("x".repeat(300));
    let error = workspace
        .snapshot_to(&long_name)
        .expect_err("over-long filename");
    assert!(!matches!(error, StorageError::Cancelled));
    assert!(!long_name.exists());
}

/// A caller that abandons an import must not wedge the single writer forever.
/// The next ordinary write has to roll the stale transaction back and leave the
/// workspace usable.
#[test]
fn abandoned_import_is_rolled_back_before_the_next_write() {
    let directory = tempfile::TempDir::new().expect("tempdir");
    let path = directory.path().join("import-abort.slab");
    let mut workspace =
        Workspace::create(&path, "Import", Box::new(TestClock::new())).expect("create");
    workspace
        .create_table(&TableDraft {
            name: "Notes".into(),
            columns: vec![text("Body")],
        })
        .expect("table");

    workspace.begin_import().expect("begin import");
    let ids = workspace
        .insert_import_rows("Notes", &[row(&[("Body", Value::from("uncommitted"))])])
        .expect("import row");
    assert_eq!(ids.len(), 1);

    let error = workspace
        .insert_row("Notes", &row(&[("Body", Value::from("during import"))]))
        .expect_err("a normal write during an import must be rejected");
    assert_eq!(
        error.code_str(),
        "SLAB_E_COMMAND",
        "unexpected error while recovering an abandoned import: {error}"
    );

    let recovered = workspace
        .insert_row("Notes", &row(&[("Body", Value::from("after abort"))]))
        .expect("writer recovered after the abandoned import");
    assert_eq!(recovered.get("Body"), Some(&Value::from("after abort")));
    let summaries = workspace.table_summaries().expect("summaries");
    assert_eq!(summaries[0].row_count, 1);
    assert_eq!(stored_text(&path, "Notes", "Body"), "after abort");
}

/// SQL export must preserve text bytes exactly, including an embedded NUL. A
/// raw NUL terminates the SQLite parser's statement and silently destroys the
/// whole transactional restore.
#[test]
fn sql_export_round_trips_text_with_nul_bytes() {
    let directory = tempfile::TempDir::new().expect("tempdir");
    let path = directory.path().join("nul-source.slab");
    let dump = directory.path().join("nul-source.sql");
    let mut workspace =
        Workspace::create(&path, "Nul", Box::new(TestClock::new())).expect("create");
    workspace
        .create_table(&TableDraft {
            name: "Notes".into(),
            columns: vec![text("Body")],
        })
        .expect("table");
    let body = "before\0after";
    workspace
        .insert_row("Notes", &row(&[("Body", Value::from(body))]))
        .expect("row");
    workspace.export_sql(&dump).expect("export");
    workspace.close().expect("close");

    let restored_path = directory.path().join("nul-restored.slab");
    let script = fs::read_to_string(&dump).expect("dump");
    let connection = Connection::open(&restored_path).expect("restore open");
    connection
        .execute_batch(&script)
        .expect("restore script with NUL text");
    drop(connection);

    let restored = Workspace::open(&restored_path, Box::new(TestClock::new())).expect("open");
    let page = restored
        .page(&slab_core::PageQuery {
            table: "Notes".into(),
            limit: 10,
            ..slab_core::PageQuery::default()
        })
        .expect("page");
    assert_eq!(
        page.rows[0].get("Body"),
        Some(&Value::from(body)),
        "export did not preserve the embedded NUL"
    );
}

/// The SQL export carries the derived full-text index, so search keeps working
/// after a restore instead of silently returning nothing.
#[test]
fn sql_export_restores_the_search_index() {
    let directory = tempfile::TempDir::new().expect("tempdir");
    let path = directory.path().join("search-source.slab");
    let dump = directory.path().join("search-source.sql");
    let mut workspace =
        Workspace::create(&path, "Search", Box::new(TestClock::new())).expect("create");
    workspace
        .create_table(&TableDraft {
            name: "Notes".into(),
            columns: vec![text("Body")],
        })
        .expect("table");
    workspace
        .insert_row(
            "Notes",
            &row(&[("Body", Value::from("needle in a haystack"))]),
        )
        .expect("row");
    workspace.export_sql(&dump).expect("export");
    workspace.close().expect("close");

    let restored_path = directory.path().join("search-restored.slab");
    let script = fs::read_to_string(&dump).expect("dump");
    assert!(
        !script.contains("fts5"),
        "the SQL export must restore with a plain sqlite3 build"
    );
    let connection = Connection::open(&restored_path).expect("restore open");
    connection.execute_batch(&script).expect("restore script");
    drop(connection);

    let restored = Workspace::open(&restored_path, Box::new(TestClock::new())).expect("open");
    let hits = restored.search("needle", 10).expect("search");
    assert_eq!(hits.len(), 1, "restored workspace lost its search index");
    assert_eq!(hits[0].table, "Notes");

    let pending: i64 = Connection::open(&restored_path)
        .expect("raw open")
        .query_row(
            "SELECT COUNT(*) FROM _slab_meta WHERE key = 'fts_pending'",
            [],
            |row| row.get(0),
        )
        .expect("pending marker");
    assert_eq!(pending, 0, "the restore marker was not consumed");
}

/// A revision recorded before a column rename cannot be mapped by name. The
/// engine must refuse with a typed error and leave the row, the audit chain,
/// and the rollups untouched instead of writing a partial record.
#[test]
fn restore_across_a_column_rename_is_a_typed_refusal() {
    let directory = tempfile::TempDir::new().expect("tempdir");
    let path = directory.path().join("rename-restore.slab");
    let mut workspace =
        Workspace::create(&path, "Rename", Box::new(TestClock::new())).expect("create");
    workspace
        .create_table(&TableDraft {
            name: "Invoices".into(),
            columns: vec![text("Status")],
        })
        .expect("table");
    let invoice = workspace
        .insert_row("Invoices", &row(&[("Status", Value::from("Draft"))]))
        .expect("insert")
        .id;
    workspace
        .update_row(
            "Invoices",
            invoice,
            &row(&[("Status", Value::from("Sent"))]),
        )
        .expect("update");
    let update_revision = workspace
        .record_history("Invoices", invoice)
        .expect("history")
        .into_iter()
        .find(|revision| revision.operation == "update")
        .expect("update revision");
    workspace
        .rename_column("Invoices", "Status", "Invoice Status")
        .expect("rename");

    let error = workspace
        .restore_record_revision("Invoices", invoice, update_revision.audit_id)
        .expect_err("a pre-rename revision cannot be restored by name");
    assert_eq!(error.code_str(), "SLAB_E_QUERY");
    let record = workspace.read_record("Invoices", invoice).expect("record");
    assert_eq!(record.get("Invoice Status"), Some(&Value::from("Sent")));
    assert!(workspace.integrity_check().expect("integrity").audit_chain);
}

/// Dropping an unrelated column also invalidates a pre-drop revision by name.
/// The refusal must be typed, not a silent write that drops the stored value.
#[test]
fn restore_across_a_dropped_column_is_a_typed_refusal() {
    let directory = tempfile::TempDir::new().expect("tempdir");
    let path = directory.path().join("drop-restore.slab");
    let mut workspace =
        Workspace::create(&path, "Drop", Box::new(TestClock::new())).expect("create");
    workspace
        .create_table(&TableDraft {
            name: "Notes".into(),
            columns: vec![text("Body"), text("Scratch")],
        })
        .expect("table");
    let note = workspace
        .insert_row(
            "Notes",
            &row(&[
                ("Body", Value::from("keep me")),
                ("Scratch", Value::from("temporary")),
            ]),
        )
        .expect("insert")
        .id;
    let update_revision = workspace
        .record_history("Notes", note)
        .expect("history")
        .into_iter()
        .find(|revision| revision.operation == "insert")
        .expect("insert revision");
    workspace
        .drop_column("Notes", "Scratch")
        .expect("drop column");

    let error = workspace
        .restore_record_revision("Notes", note, update_revision.audit_id)
        .expect_err("a pre-drop revision cannot be restored by name");
    assert_eq!(error.code_str(), "SLAB_E_QUERY");
    let record = workspace.read_record("Notes", note).expect("record");
    assert_eq!(record.get("Body"), Some(&Value::from("keep me")));
    assert!(workspace.integrity_check().expect("integrity").audit_chain);
}

/// A new required column added after a revision is taken keeps the current
/// value; restoring an older revision must still satisfy the live schema.
#[test]
fn restore_keeps_a_required_column_added_after_the_revision() {
    let directory = tempfile::TempDir::new().expect("tempdir");
    let path = directory.path().join("required-restore.slab");
    let mut workspace =
        Workspace::create(&path, "Required", Box::new(TestClock::new())).expect("create");
    workspace
        .create_table(&TableDraft {
            name: "Clients".into(),
            columns: vec![text("Name")],
        })
        .expect("table");
    let client = workspace
        .insert_row("Clients", &row(&[("Name", Value::from("Northlight"))]))
        .expect("insert")
        .id;
    let insert_revision = workspace
        .record_history("Clients", client)
        .expect("history")
        .into_iter()
        .next()
        .expect("insert revision");
    workspace
        .add_column(
            "Clients",
            &ColumnDraft {
                name: "Region".into(),
                ty: ColumnType::Text,
                required: true,
                unique: false,
                default: Some(Value::from("EU")),
            },
        )
        .expect("required column");
    workspace
        .update_row("Clients", client, &row(&[("Region", Value::from("US"))]))
        .expect("set region");

    let restored = workspace
        .restore_record_revision("Clients", client, insert_revision.audit_id)
        .expect("restore");
    assert_eq!(restored.get("Name"), Some(&Value::from("Northlight")));
    assert_eq!(
        restored.get("Region"),
        Some(&Value::from("US")),
        "restore overwrote a live column that the revision did not contain"
    );
    assert!(workspace.integrity_check().expect("integrity").audit_chain);
}

/// Restoring a cascade-deleted child whose parent is also gone must fail at the
/// foreign key, roll back the whole command, and leave the audit chain intact.
#[test]
fn restore_without_its_parent_is_a_foreign_key_error() {
    let directory = tempfile::TempDir::new().expect("tempdir");
    let path = directory.path().join("fk-restore.slab");
    let mut workspace =
        Workspace::create(&path, "Foreign", Box::new(TestClock::new())).expect("create");
    workspace
        .create_table(&TableDraft {
            name: "Clients".into(),
            columns: vec![text("Name")],
        })
        .expect("clients");
    workspace
        .create_table(&TableDraft {
            name: "Projects".into(),
            columns: vec![text("Name"), integer("Client")],
        })
        .expect("projects");
    workspace
        .create_relation(&RelationDraft {
            name: "Client Projects".into(),
            parent_table: "Clients".into(),
            child_table: "Projects".into(),
            child_column: "Client".into(),
            on_delete: DeleteAction::Cascade,
        })
        .expect("relation");
    let client = workspace
        .insert_row("Clients", &row(&[("Name", Value::from("Northlight"))]))
        .expect("client")
        .id;
    let project = workspace
        .insert_row(
            "Projects",
            &row(&[
                ("Name", Value::from("Identity")),
                ("Client", Value::Integer(client)),
            ]),
        )
        .expect("project")
        .id;
    let project_revision = workspace
        .record_history("Projects", project)
        .expect("history")
        .into_iter()
        .next()
        .expect("insert revision")
        .audit_id;
    let audit_before = workspace.audit_entries(1_000, 0).expect("audit").len();
    workspace
        .delete_row("Clients", client)
        .expect("cascade delete");
    assert_eq!(raw_count(&path, "Projects"), 0);

    let error = workspace
        .restore_record_revision("Projects", project, project_revision)
        .expect_err("the parent is gone");
    assert_eq!(error.code_str(), "SLAB_E_FK_VIOLATION");
    assert_eq!(raw_count(&path, "Projects"), 0);
    let report = workspace.integrity_check().expect("integrity");
    assert!(report.audit_chain);
    assert_eq!(
        workspace.audit_entries(1_000, 0).expect("audit").len(),
        audit_before + 1,
        "a failed restore must not append an audit entry"
    );
}

/// Interleaving undo, redo, delete/restore, and a snapshot must never produce a
/// false tamper alarm.
#[test]
fn audit_chain_survives_interleaved_rewrites_and_snapshots() {
    let directory = tempfile::TempDir::new().expect("tempdir");
    let path = directory.path().join("chain.slab");
    let mut workspace =
        Workspace::create(&path, "Chain", Box::new(TestClock::new())).expect("create");
    workspace
        .create_table(&TableDraft {
            name: "Notes".into(),
            columns: vec![text("Body")],
        })
        .expect("table");
    let note = workspace
        .insert_row("Notes", &row(&[("Body", Value::from("first"))]))
        .expect("insert")
        .id;
    workspace
        .update_row("Notes", note, &row(&[("Body", Value::from("second"))]))
        .expect("update");
    workspace.undo().expect("undo update");
    workspace.redo().expect("redo update");
    workspace.delete_row("Notes", note).expect("delete");
    let insert_revision = workspace
        .record_history("Notes", note)
        .expect("history")
        .into_iter()
        .next()
        .expect("insert revision");
    workspace
        .restore_record_revision("Notes", note, insert_revision.audit_id)
        .expect("restore");
    workspace.undo().expect("undo restore");
    workspace.redo().expect("redo restore");

    for index in 0..2 {
        let snapshot = directory.path().join(format!("chain-{index}.slab"));
        workspace.snapshot_to(&snapshot).expect("snapshot");
        let report = workspace.integrity_check().expect("integrity");
        assert!(
            report.audit_chain,
            "false tamper: {:?}",
            report.audit_tamper
        );
    }
}

/// Retiring one query registration must not erase another query's ability to
/// be cancelled.
#[test]
fn retiring_one_query_keeps_other_registrations_cancellable() {
    let cancel = QueryCancel::new();
    let first = Connection::open_in_memory().expect("first");
    let second = Connection::open_in_memory().expect("second");
    let first_registration = cancel.register(first.get_interrupt_handle());
    let _second_registration = cancel.register(second.get_interrupt_handle());
    cancel.clear_registration(first_registration);

    let worker_cancel = cancel.clone();
    let worker = std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(25));
        assert!(worker_cancel.interrupt());
    });
    let result: Result<i64, rusqlite::Error> = second.query_row(
        "WITH RECURSIVE cnt(x) AS (
            SELECT 1 UNION ALL SELECT x + 1 FROM cnt WHERE x < 100000000
         ) SELECT SUM(x) FROM cnt",
        [],
        |row| row.get(0),
    );
    worker.join().expect("join");
    let error = StorageError::from(result.expect_err("the second query must be interrupted"));
    assert!(matches!(error, StorageError::Cancelled));
}

/// An abandoned import must roll back cleanly when the workspace is closed, and
/// closing must still produce one self-contained file.
#[test]
fn closing_with_an_abandoned_import_rolls_it_back() {
    let directory = tempfile::TempDir::new().expect("tempdir");
    let path = directory.path().join("close-import.slab");
    let mut workspace =
        Workspace::create(&path, "Close", Box::new(TestClock::new())).expect("create");
    workspace
        .create_table(&TableDraft {
            name: "Notes".into(),
            columns: vec![text("Body")],
        })
        .expect("table");
    workspace.begin_import().expect("begin");
    workspace
        .insert_import_rows("Notes", &[row(&[("Body", Value::from("uncommitted"))])])
        .expect("row");
    workspace.close().expect("close with an open import");

    assert!(!path.with_extension("slab-wal").exists());
    assert!(!path.with_extension("slab-shm").exists());
    assert_eq!(raw_count(&path, "Notes"), 0);
    let reopened = Workspace::open(&path, Box::new(TestClock::new())).expect("reopen");
    assert!(reopened.integrity_check().expect("integrity").audit_chain);
}

/// A unique index added after table creation is part of the schema and must
/// survive the SQL round trip.
#[test]
fn sql_export_preserves_late_unique_constraints() {
    let directory = tempfile::TempDir::new().expect("tempdir");
    let path = directory.path().join("unique-source.slab");
    let dump = directory.path().join("unique-source.sql");
    let mut workspace =
        Workspace::create(&path, "Unique", Box::new(TestClock::new())).expect("create");
    workspace
        .create_table(&TableDraft {
            name: "Clients".into(),
            columns: vec![text("Name")],
        })
        .expect("table");
    workspace
        .add_column(
            "Clients",
            &ColumnDraft {
                name: "Email".into(),
                ty: ColumnType::Text,
                required: false,
                unique: true,
                default: None,
            },
        )
        .expect("unique column");
    workspace
        .insert_row(
            "Clients",
            &row(&[
                ("Name", Value::from("A")),
                ("Email", Value::from("a@example.com")),
            ]),
        )
        .expect("row");
    workspace.export_sql(&dump).expect("export");
    workspace.close().expect("close");

    let restored_path = directory.path().join("unique-restored.slab");
    let script = fs::read_to_string(&dump).expect("dump");
    let connection = Connection::open(&restored_path).expect("restore open");
    connection.execute_batch(&script).expect("restore script");
    drop(connection);

    let connection = Connection::open(&restored_path).expect("raw open");
    let duplicate = connection.execute(
        "INSERT INTO \"Clients\" (\"Name\", \"Email\") VALUES ('B', 'a@example.com')",
        [],
    );
    assert!(
        duplicate.is_err(),
        "restored schema lost the unique constraint on Clients.Email"
    );
}

/// A partial migration repair must not become a tamper laundering path. Only
/// a wholly legacy log (every hash missing) may be backfilled; a single NULLed
/// entry in an otherwise hashed log is evidence, not something to rewrite.
#[test]
fn nulled_hashes_do_not_launder_a_tampered_audit_entry() {
    let directory = tempfile::TempDir::new().expect("tempdir");
    let path = directory.path().join("audit-launder.slab");
    let mut workspace =
        Workspace::create(&path, "Audit", Box::new(TestClock::new())).expect("create");
    workspace
        .create_table(&TableDraft {
            name: "Notes".into(),
            columns: vec![text("Body")],
        })
        .expect("table");
    for index in 0..4 {
        workspace
            .insert_row(
                "Notes",
                &row(&[("Body", Value::from(format!("note {index}")))]),
            )
            .expect("row");
    }
    assert!(workspace.integrity_check().expect("check").audit_chain);
    workspace.close().expect("close");

    {
        let connection = Connection::open(&path).expect("raw open");
        connection
            .execute(
                "UPDATE _slab_audit_log
                 SET summary = 'silently rewritten', prev_hash = NULL, entry_hash = NULL
                 WHERE id = 2",
                [],
            )
            .expect("tamper");
    }

    let workspace = Workspace::open(&path, Box::new(TestClock::new())).expect("reopen");
    let report = workspace.integrity_check().expect("check");
    assert!(
        !report.audit_chain,
        "a NULLed hash laundered the tampered entry into a fresh chain"
    );
    assert!(report.audit_tamper.is_some());

    // NULLing every hash must not fall back into the legacy migration path
    // while the chain head still proves this is a modern log.
    let all_path = directory.path().join("audit-launder-all.slab");
    let mut all =
        Workspace::create(&all_path, "Audit", Box::new(TestClock::new())).expect("create all");
    all.create_table(&TableDraft {
        name: "Notes".into(),
        columns: vec![text("Body")],
    })
    .expect("table");
    for index in 0..3 {
        all.insert_row(
            "Notes",
            &row(&[("Body", Value::from(format!("note {index}")))]),
        )
        .expect("row");
    }
    all.close().expect("close all");
    {
        let connection = Connection::open(&all_path).expect("raw open all");
        connection
            .execute(
                "UPDATE _slab_audit_log
                 SET summary = 'silently rewritten', prev_hash = NULL, entry_hash = NULL",
                [],
            )
            .expect("tamper all");
    }
    let all = Workspace::open(&all_path, Box::new(TestClock::new())).expect("reopen all");
    let report = all.integrity_check().expect("check all");
    assert!(
        !report.audit_chain,
        "NULLing every hash laundered the modern log into a legacy migration"
    );
    assert!(report.audit_tamper.is_some());
}

/// A non-UTF-8 parent directory is a valid Unix path, but `VACUUM INTO` needs a
/// Rust string. The failure must remove its scratch file instead of leaving
/// residue behind. APFS rejects such a directory name, so this runs on Linux.
#[cfg(target_os = "linux")]
#[test]
fn non_utf8_snapshot_parent_leaves_no_scratch_file() {
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStringExt;

    let directory = tempfile::TempDir::new().expect("tempdir");
    let path = directory.path().join("live.slab");
    let mut workspace =
        Workspace::create(&path, "Snapshot", Box::new(TestClock::new())).expect("create");
    workspace
        .create_table(&TableDraft {
            name: "Notes".into(),
            columns: vec![text("Body")],
        })
        .expect("table");

    let parent = directory
        .path()
        .join(OsString::from_vec(b"non-utf8-\xff".to_vec()));
    fs::create_dir(&parent).expect("non-utf8 dir");
    let error = workspace
        .snapshot_vacuum_to(parent.join("snapshot.slab"))
        .expect_err("a non-UTF-8 target must fail");
    assert_eq!(error.code_str(), "SLAB_E_WORKSPACE");
    assert_eq!(
        fs::read_dir(&parent).expect("read parent").count(),
        0,
        "a failed snapshot left a scratch file behind"
    );
}
