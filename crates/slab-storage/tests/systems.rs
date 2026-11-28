#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod support;

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use rusqlite::Connection;
use slab_core::{
    Aggregate, ColumnDraft, ColumnType, DeleteAction, Filter, FilterOp, IndexHint, PageQuery,
    Record, RelationDraft, RowData, SortDirection, SortSpec, TableDraft, Value,
};
use slab_storage::{QueryCancel, SnapshotKind, StorageError, Workspace};
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

fn count_rows(path: &std::path::Path, table: &str) -> i64 {
    let connection = Connection::open(path).expect("open raw");
    connection
        .query_row(&format!("SELECT COUNT(*) FROM \"{table}\""), [], |row| {
            row.get(0)
        })
        .expect("count")
}

fn sqlite3(path: &std::path::Path, sql: &str) -> String {
    let output = std::process::Command::new("sqlite3")
        .arg(path)
        .arg(sql)
        .output()
        .expect("run sqlite3");
    assert!(
        output.status.success(),
        "sqlite3 failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

fn has_index_direct(conn: &Connection, table: &str, column: &str) -> bool {
    let mut stmt = conn
        .prepare(&format!("PRAGMA index_list(\"{table}\")"))
        .expect("index list");
    let names: Vec<String> = stmt
        .query_map([], |row| row.get::<_, String>(1))
        .expect("names")
        .map(|row| row.expect("name"))
        .collect();
    drop(stmt);
    for name in names {
        let mut info = conn
            .prepare(&format!("PRAGMA index_info(\"{name}\")"))
            .expect("index info");
        let columns: Vec<String> = info
            .query_map([], |row| row.get::<_, String>(2))
            .expect("columns")
            .map(|row| row.expect("column"))
            .collect();
        if columns.first().map(String::as_str) == Some(column) {
            return true;
        }
    }
    false
}

#[test]
fn committed_fixture_migrates_and_verifies() {
    let source =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/design-studio.slab");
    if !source.exists() {
        return;
    }
    let directory = tempfile::TempDir::new().expect("tempdir");
    let copy = directory.path().join("design-studio.slab");
    std::fs::copy(&source, &copy).expect("copy fixture");
    let workspace = Workspace::open(&copy, Box::new(TestClock::new())).expect("open fixture");
    let report = workspace.integrity_check().expect("check");
    assert_eq!(report.integrity, "ok");
    assert!(report.audit_chain, "fixture audit chain did not migrate");
    assert!(report.audit_entries > 0);
    assert_eq!(
        workspace.info().expect("info").format_version,
        slab_core::FORMAT_VERSION
    );
    workspace.close().expect("close");
}

#[test]
fn close_leaves_one_self_contained_file() {
    let directory = tempfile::TempDir::new().expect("tempdir");
    let path = directory.path().join("closed.slab");
    let mut workspace =
        Workspace::create(&path, "Closed", Box::new(TestClock::new())).expect("create");
    workspace
        .create_table(&TableDraft {
            name: "Notes".into(),
            columns: vec![text("Body")],
        })
        .expect("table");
    workspace
        .insert_rows(
            "Notes",
            &[
                row(&[("Body", Value::from("one"))]),
                row(&[("Body", Value::from("two"))]),
            ],
        )
        .expect("rows");
    assert!(path.with_extension("slab-wal").exists() || path.exists());
    workspace.close().expect("close");

    assert!(!path.with_extension("slab-wal").exists(), "orphan WAL left");
    assert!(!path.with_extension("slab-shm").exists(), "orphan SHM left");
    assert_eq!(count_rows(&path, "Notes"), 2);
    assert_eq!(sqlite3(&path, "PRAGMA integrity_check;"), "ok");
}

#[test]
fn concurrent_readers_survive_a_twenty_thousand_row_write() {
    let directory = tempfile::TempDir::new().expect("tempdir");
    let path = directory.path().join("concurrent.slab");
    let mut workspace =
        Workspace::create(&path, "Concurrent", Box::new(TestClock::new())).expect("create");
    workspace
        .create_table(&TableDraft {
            name: "Docs".into(),
            columns: vec![text("Body")],
        })
        .expect("table");
    let seed: Vec<RowData> = (0..500)
        .map(|index| row(&[("Body", Value::from(format!("needle {index}")))]))
        .collect();
    workspace.insert_rows("Docs", &seed).expect("seed");

    let done = std::sync::Arc::new(AtomicBool::new(false));
    let writer_done = std::sync::Arc::clone(&done);
    let writer_path = path.clone();
    let writer = std::thread::spawn(move || {
        let connection = Connection::open(&writer_path).expect("raw writer");
        connection
            .busy_timeout(Duration::from_millis(5_000))
            .expect("busy timeout");
        connection
            .execute_batch("PRAGMA journal_mode = WAL; PRAGMA synchronous = NORMAL;")
            .expect("wal");
        connection.execute_batch("BEGIN IMMEDIATE").expect("begin");
        {
            let mut insert = connection
                .prepare("INSERT INTO \"Docs\" (\"Body\") VALUES (?1)")
                .expect("prepare");
            for index in 0..20_000 {
                insert
                    .execute(rusqlite::params![format!("bulk {index}")])
                    .expect("insert");
                if index % 100 == 0 {
                    std::thread::yield_now();
                }
            }
        }
        connection.execute_batch("COMMIT").expect("commit");
        writer_done.store(true, Ordering::SeqCst);
    });

    let mut reads = 0_u32;
    let mut query = PageQuery {
        table: "Docs".into(),
        limit: 100,
        sort: vec![SortSpec {
            column: "Body".into(),
            direction: SortDirection::Asc,
        }],
        ..PageQuery::default()
    };
    while !done.load(Ordering::Relaxed) {
        let page = workspace.page(&query).expect("reader page");
        assert!(!page.rows.is_empty());
        workspace.search("needle", 20).expect("reader search");
        query.cursor = page.next_cursor;
        if query.cursor.is_none() {
            query.cursor = None;
        }
        reads = reads.saturating_add(1);
    }
    writer.join().expect("writer join");
    assert!(reads > 0, "no read completed while the write was active");
    assert_eq!(
        workspace.table_summaries().expect("summaries")[0].row_count,
        20_500
    );
}

fn rollup_workspace(directory: &tempfile::TempDir) -> (std::path::PathBuf, Workspace) {
    let path = directory.path().join("rollups.slab");
    let mut workspace =
        Workspace::create(&path, "Rollups", Box::new(TestClock::new())).expect("create");
    workspace
        .create_table(&TableDraft {
            name: "Invoices".into(),
            columns: vec![text("Number")],
        })
        .expect("parent");
    workspace
        .create_table(&TableDraft {
            name: "Items".into(),
            columns: vec![
                integer("Invoice"),
                text("Label"),
                integer("Amount"),
                integer("Qty"),
            ],
        })
        .expect("child");
    workspace
        .create_relation(&RelationDraft {
            name: "Items".into(),
            parent_table: "Invoices".into(),
            child_table: "Items".into(),
            child_column: "Invoice".into(),
            on_delete: DeleteAction::Cascade,
        })
        .expect("relation");
    for draft in [
        lookup("Total", "Items", "Amount", Aggregate::Sum),
        lookup("Lines", "Items", "Label", Aggregate::Count),
        lookup("Average", "Items", "Amount", Aggregate::Avg),
        lookup("Smallest", "Items", "Amount", Aggregate::Min),
        lookup("Largest", "Items", "Amount", Aggregate::Max),
    ] {
        workspace.add_column("Invoices", &draft).expect("lookup");
    }
    (path, workspace)
}

fn rollup_values(workspace: &Workspace, invoice_id: i64) -> Vec<Value> {
    let record = workspace
        .read_record("Invoices", invoice_id)
        .expect("parent read");
    ["Total", "Lines", "Average", "Smallest", "Largest"]
        .iter()
        .map(|column| record.values.get(*column).cloned().unwrap_or(Value::Null))
        .collect()
}

fn clear_cache(path: &std::path::Path) {
    let connection = Connection::open(path).expect("raw open");
    connection
        .execute("DELETE FROM _slab_rollups", [])
        .expect("clear cache");
}

#[test]
fn incremental_rollups_match_full_recomputation_over_one_thousand_operations() {
    let directory = tempfile::TempDir::new().expect("tempdir");
    let (path, mut workspace) = rollup_workspace(&directory);
    let invoices: Vec<i64> = (0..3)
        .map(|index| {
            workspace
                .insert_row(
                    "Invoices",
                    &row(&[("Number", Value::from(format!("INV-{index}")))]),
                )
                .expect("invoice")
                .id
        })
        .collect();

    let mut children: Vec<(i64, i64)> = Vec::new();
    let mut last_removed: Option<i64> = None;
    let mut seed = 0x5EED_1234_u64;
    let mut next = || {
        seed = seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1);
        seed >> 33
    };

    for step in 0..1_000 {
        match next() % 10 {
            0..=2 => {
                let invoice = invoices[usize::try_from(next() % 3).unwrap()];
                let amount = i64::try_from(next() % 1_000).unwrap();
                let quote = workspace
                    .insert_row(
                        "Items",
                        &row(&[
                            ("Invoice", Value::Integer(invoice)),
                            ("Label", Value::from(format!("line {step}"))),
                            ("Amount", Value::Integer(amount)),
                            ("Qty", Value::Integer(i64::try_from(next() % 20).unwrap())),
                        ]),
                    )
                    .expect("insert child");
                children.push((quote.id, invoice));
            }
            3 | 4 => {
                if let Some((id, _)) =
                    children.get(usize::try_from(next()).unwrap_or(0) % children.len().max(1))
                {
                    let amount = i64::try_from(next() % 1_000).unwrap();
                    let _ = workspace.update_row(
                        "Items",
                        *id,
                        &row(&[("Amount", Value::Integer(amount))]),
                    );
                }
            }
            5 => {
                if let Some((id, _)) =
                    children.get(usize::try_from(next()).unwrap_or(0) % children.len().max(1))
                {
                    let qty = i64::try_from(next() % 50).unwrap();
                    let _ =
                        workspace.update_row("Items", *id, &row(&[("Qty", Value::Integer(qty))]));
                }
            }
            6 => {
                if !children.is_empty() {
                    let index = usize::try_from(next()).unwrap_or(0) % children.len();
                    let (id, _) = children.remove(index);
                    let _ = workspace.delete_row("Items", id);
                    last_removed = Some(id);
                }
            }
            7 => {
                let _ = workspace.undo();
            }
            8 => {
                let _ = workspace.redo();
            }
            _ => {
                if let Some((id, _)) =
                    children.get(usize::try_from(next()).unwrap_or(0) % children.len().max(1))
                {
                    let invoice = invoices[usize::try_from(next() % 3).unwrap()];
                    let _ = workspace.update_row(
                        "Items",
                        *id,
                        &row(&[("Invoice", Value::Integer(invoice))]),
                    );
                }
            }
        }
        if let Some(id) = last_removed
            && step % 17 == 0
        {
            let _ = workspace.redo();
            last_removed = None;
            let _ = id;
        }
        for invoice in &invoices {
            let maintained = rollup_values(&workspace, *invoice);
            clear_cache(&path);
            let fresh = rollup_values(&workspace, *invoice);
            assert_eq!(
                maintained, fresh,
                "rollup mismatch after step {step} on invoice {invoice}"
            );
        }
        // Re-warm every parent so the next iteration exercises the incremental
        // delta path instead of a full recomputation.
        for invoice in &invoices {
            let _ = rollup_values(&workspace, *invoice);
        }
    }
    workspace.close().expect("close");
}

#[test]
fn rollup_delta_latency_stays_under_two_milliseconds() {
    let directory = tempfile::TempDir::new().expect("tempdir");
    let (_path, mut workspace) = rollup_workspace(&directory);
    let invoice = workspace
        .insert_row("Invoices", &row(&[("Number", Value::from("BENCH"))]))
        .expect("invoice")
        .id;
    workspace.begin_import().expect("begin");
    for batch in 0..20 {
        let rows: Vec<RowData> = (0..500)
            .map(|index| {
                row(&[
                    ("Invoice", Value::Integer(invoice)),
                    (
                        "Label",
                        Value::from(format!("line {}", batch * 500 + index)),
                    ),
                    ("Amount", Value::Integer(index % 500)),
                    ("Qty", Value::Integer(1)),
                ])
            })
            .collect();
        workspace
            .insert_import_rows("Items", &rows)
            .expect("import");
    }
    workspace.commit_import().expect("commit");
    let child_id = workspace
        .page(&PageQuery {
            table: "Items".into(),
            limit: 1,
            ..PageQuery::default()
        })
        .expect("page")
        .rows[0]
        .id;
    let _ = rollup_values(&workspace, invoice);

    let mut durations = Vec::new();
    for step in 0..40_i64 {
        let started = Instant::now();
        workspace
            .update_row("Items", child_id, &row(&[("Amount", Value::Integer(step))]))
            .expect("update");
        let _ = rollup_values(&workspace, invoice);
        durations.push(started.elapsed());
    }
    durations.sort();
    let median = durations[durations.len() / 2];
    if !cfg!(debug_assertions) {
        let budget = if std::env::var_os("CI").is_some() {
            Duration::from_millis(5)
        } else {
            Duration::from_millis(2)
        };
        assert!(
            median < budget,
            "rollup edit median {median:?} exceeded {budget:?}"
        );
    }
}

#[test]
fn audit_chain_detects_external_tamper_and_deletion() {
    let directory = tempfile::TempDir::new().expect("tempdir");
    let cases = [
        "UPDATE _slab_audit_log SET summary = summary || '!' WHERE id = (SELECT MIN(id) FROM _slab_audit_log);",
        "DELETE FROM _slab_audit_log WHERE id = (SELECT MIN(id) FROM _slab_audit_log);",
        "DELETE FROM _slab_audit_log WHERE id = (SELECT MAX(id) FROM _slab_audit_log);",
    ];
    for (index, statement) in cases.iter().enumerate() {
        let path = directory.path().join(format!("tamper-{index}.slab"));
        let mut workspace =
            Workspace::create(&path, "Audit", Box::new(TestClock::new())).expect("create");
        workspace
            .create_table(&TableDraft {
                name: "Notes".into(),
                columns: vec![text("Body")],
            })
            .expect("table");
        for row_index in 0..4 {
            workspace
                .insert_row(
                    "Notes",
                    &row(&[("Body", Value::from(format!("note {row_index}")))]),
                )
                .expect("row");
        }
        assert!(workspace.integrity_check().expect("check").audit_chain);
        workspace.close().expect("close");

        sqlite3(&path, statement);
        let workspace = Workspace::open(&path, Box::new(TestClock::new())).expect("reopen");
        let report = workspace.integrity_check().expect("check");
        assert!(!report.audit_chain, "case {index} tamper was not detected");
        assert!(report.audit_tamper.is_some());
    }
}

#[test]
fn restore_revision_updates_rollups_and_is_undoable() {
    let directory = tempfile::TempDir::new().expect("tempdir");
    let (_path, mut workspace) = rollup_workspace(&directory);
    let invoice = workspace
        .insert_row("Invoices", &row(&[("Number", Value::from("INV-R"))]))
        .expect("invoice")
        .id;
    let first = workspace
        .insert_row(
            "Items",
            &row(&[
                ("Invoice", Value::Integer(invoice)),
                ("Label", Value::from("first")),
                ("Amount", Value::Integer(100)),
            ]),
        )
        .expect("first");
    workspace
        .insert_row(
            "Items",
            &row(&[
                ("Invoice", Value::Integer(invoice)),
                ("Label", Value::from("second")),
                ("Amount", Value::Integer(200)),
            ]),
        )
        .expect("second");
    assert_eq!(rollup_values(&workspace, invoice)[0], Value::Integer(300));

    workspace
        .update_row("Items", first.id, &row(&[("Amount", Value::Integer(500))]))
        .expect("update");
    assert_eq!(rollup_values(&workspace, invoice)[0], Value::Integer(700));

    let history = workspace
        .record_history("Items", first.id)
        .expect("history");
    let insert_revision = history.first().expect("insert revision");
    assert_eq!(insert_revision.operation, "insert");
    let audit_before = workspace
        .audit_entries(1, 0)
        .expect("audit")
        .first()
        .expect("entry")
        .id;

    let restored = workspace
        .restore_record_revision("Items", first.id, insert_revision.audit_id)
        .expect("restore");
    assert_eq!(restored.get("Amount"), Some(&Value::Integer(100)));
    assert_eq!(rollup_values(&workspace, invoice)[0], Value::Integer(300));
    let report = workspace.integrity_check().expect("check");
    assert!(report.audit_chain);
    assert!(report.audit_entries > 0);
    let latest = workspace
        .audit_entries(1, 0)
        .expect("audit")
        .first()
        .cloned()
        .expect("entry");
    assert_eq!(latest.operation, "restore");
    assert!(latest.id > audit_before);

    workspace.undo().expect("undo restore");
    assert_eq!(rollup_values(&workspace, invoice)[0], Value::Integer(700));
    assert!(workspace.integrity_check().expect("check").audit_chain);
}

#[test]
fn index_advisor_suggests_then_clears() {
    let directory = tempfile::TempDir::new().expect("tempdir");
    let path = directory.path().join("advisor.slab");
    let mut workspace =
        Workspace::create(&path, "Advisor", Box::new(TestClock::new())).expect("create");
    workspace
        .create_table(&TableDraft {
            name: "People".into(),
            columns: vec![text("Name"), text("City"), integer("Score")],
        })
        .expect("table");
    {
        let connection = Connection::open(&path).expect("raw");
        connection
            .busy_timeout(Duration::from_millis(5_000))
            .expect("timeout");
        connection.execute_batch("BEGIN IMMEDIATE").expect("begin");
        {
            let mut insert = connection
                .prepare(
                    "INSERT INTO \"People\" (\"Name\", \"City\", \"Score\") VALUES (?1, ?2, ?3)",
                )
                .expect("prepare");
            for index in 0..25_000_i64 {
                insert
                    .execute(rusqlite::params![
                        format!("Person {index}"),
                        format!("City {}", index % 97),
                        index
                    ])
                    .expect("insert");
            }
        }
        connection.execute_batch("COMMIT").expect("commit");
    }

    let query = PageQuery {
        table: "People".into(),
        limit: 100,
        sort: vec![SortSpec {
            column: "City".into(),
            direction: SortDirection::Asc,
        }],
        ..PageQuery::default()
    };
    let unindexed = workspace.page(&query).expect("unindexed page");
    let early = Connection::open_with_flags(&path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
        .expect("early reader");
    let before_count: i64 = early
        .query_row(
            "SELECT COUNT(*) FROM pragma_index_list('People')",
            [],
            |row| row.get(0),
        )
        .expect("before count");
    assert_eq!(before_count, 0);
    let hint: &IndexHint = unindexed.index_hint.as_ref().expect("index hint");
    assert_eq!(hint.table, "People");
    assert_eq!(hint.column, "City");
    assert_eq!(hint.suggested_index, "slab_idx_People_City");
    assert_eq!(hint.scanned_rows, 25_000);
    assert!(hint.detail.contains("SCAN"));

    workspace.create_index("People", "City").expect("index");
    let after_count: i64 = early
        .query_row(
            "SELECT COUNT(*) FROM pragma_index_list('People')",
            [],
            |row| row.get(0),
        )
        .expect("after count");
    assert_eq!(after_count, 1, "early reader missed the new index");
    let listing = sqlite3(&path, "PRAGMA index_list(\"People\");");
    assert!(
        listing.contains("slab_idx_People_City"),
        "index missing from SQLite: {listing}"
    );
    let info = sqlite3(&path, "PRAGMA index_info(\"slab_idx_People_City\");");
    assert!(info.contains("City"), "index info missing City: {info}");
    {
        use rusqlite::OpenFlags;
        let reader = Connection::open_with_flags(&path, OpenFlags::SQLITE_OPEN_READ_ONLY)
            .expect("read-only");
        let seen: i64 = reader
            .query_row(
                "SELECT COUNT(*) FROM pragma_index_list('People')
                 WHERE name = 'slab_idx_People_City'",
                [],
                |row| row.get(0),
            )
            .expect("index list");
        assert_eq!(seen, 1, "fresh read-only connection cannot see the index");
        assert!(has_index_direct(&reader, "People", "City"));
    }
    let indexed = workspace.page(&query).expect("indexed page");
    assert!(
        indexed.index_hint.is_none(),
        "hint survived index creation: {:?}",
        indexed.index_hint
    );
    if !cfg!(debug_assertions) {
        assert!(
            indexed.elapsed_ms * 2.0 < unindexed.elapsed_ms,
            "indexed latency {}ms did not drop cleanly below unindexed {}ms",
            indexed.elapsed_ms,
            unindexed.elapsed_ms
        );
    }
}

#[test]
fn snapshots_pass_external_integrity_and_hash_checks() {
    let directory = tempfile::TempDir::new().expect("tempdir");
    let (_path, mut workspace) = rollup_workspace(&directory);
    let invoice = workspace
        .insert_row("Invoices", &row(&[("Number", Value::from("SNAP"))]))
        .expect("invoice")
        .id;
    workspace
        .insert_row(
            "Items",
            &row(&[
                ("Invoice", Value::Integer(invoice)),
                ("Label", Value::from("snap")),
                ("Amount", Value::Integer(42)),
            ]),
        )
        .expect("child");

    let clone_path = directory.path().join("clone.slab");
    let clone = workspace.snapshot_to(&clone_path).expect("clone snapshot");
    assert!(matches!(
        clone.kind,
        SnapshotKind::ApfsClone | SnapshotKind::VacuumInto
    ));
    assert!(!clone_path.with_extension("slab-wal").exists());
    assert!(!clone_path.with_extension("slab-shm").exists());

    let vacuum_path = directory.path().join("vacuum.slab");
    let vacuum = workspace.snapshot_vacuum_to(&vacuum_path).expect("vacuum");
    assert_eq!(vacuum.kind, SnapshotKind::VacuumInto);

    for snapshot in [&clone_path, &vacuum_path] {
        assert_eq!(sqlite3(snapshot, "PRAGMA integrity_check;"), "ok");
        assert_eq!(sqlite3(snapshot, "PRAGMA foreign_key_check;"), "");
        assert_eq!(count_rows(snapshot, "Items"), 1);
        let reopened = Workspace::open(snapshot, Box::new(TestClock::new())).expect("reopen");
        let report = reopened.integrity_check().expect("check");
        assert!(report.audit_chain);
        let restored = rollup_values(&reopened, invoice);
        assert_eq!(restored[0], Value::Integer(42));
        reopened.close().expect("close");
    }
}

#[test]
fn interrupt_maps_to_benign_cancellation() {
    let connection = Connection::open_in_memory().expect("open");
    let cancel = QueryCancel::new();
    cancel.register(connection.get_interrupt_handle());
    let worker = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(25));
        cancel.interrupt();
    });
    let started = Instant::now();
    let result: Result<i64, rusqlite::Error> = connection.query_row(
        "WITH RECURSIVE cnt(x) AS (
            SELECT 1 UNION ALL SELECT x + 1 FROM cnt WHERE x < 100000000
         ) SELECT SUM(x) FROM cnt",
        [],
        |row| row.get(0),
    );
    assert!(started.elapsed() < Duration::from_secs(5));
    worker.join().expect("join");
    let error = StorageError::from(result.expect_err("query should be interrupted"));
    assert!(matches!(error, StorageError::Cancelled));
    assert_eq!(error.code_str(), "SLAB_E_CANCELLED");
}

#[test]
fn restore_after_delete_recreates_the_row_and_rollup() {
    let directory = tempfile::TempDir::new().expect("tempdir");
    let (_path, mut workspace) = rollup_workspace(&directory);
    let invoice = workspace
        .insert_row("Invoices", &row(&[("Number", Value::from("GONE"))]))
        .expect("invoice")
        .id;
    let child = workspace
        .insert_row(
            "Items",
            &row(&[
                ("Invoice", Value::Integer(invoice)),
                ("Label", Value::from("removed")),
                ("Amount", Value::Integer(75)),
            ]),
        )
        .expect("child");
    workspace.delete_row("Items", child.id).expect("delete");
    assert_eq!(rollup_values(&workspace, invoice)[0], Value::Integer(0));
    let history = workspace
        .record_history("Items", child.id)
        .expect("history");
    let insert = history
        .iter()
        .find(|revision| revision.operation == "insert")
        .expect("insert revision");
    let restored = workspace
        .restore_record_revision("Items", child.id, insert.audit_id)
        .expect("restore");
    assert_eq!(restored.id, child.id);
    assert_eq!(restored.get("Amount"), Some(&Value::Integer(75)));
    assert_eq!(rollup_values(&workspace, invoice)[0], Value::Integer(75));
    assert!(workspace.integrity_check().expect("check").audit_chain);
    workspace.undo().expect("undo");
    assert_eq!(rollup_values(&workspace, invoice)[0], Value::Integer(0));
}

#[test]
fn formula_columns_see_updated_rollups() {
    let directory = tempfile::TempDir::new().expect("tempdir");
    let (_path, mut workspace) = rollup_workspace(&directory);
    workspace
        .add_column(
            "Invoices",
            &ColumnDraft {
                name: "Doubled".into(),
                ty: ColumnType::Formula {
                    expression: "Total * 2".into(),
                },
                required: false,
                unique: false,
                default: None,
            },
        )
        .expect("formula");
    let invoice = workspace
        .insert_row("Invoices", &row(&[("Number", Value::from("FORMULA"))]))
        .expect("invoice")
        .id;
    let child = workspace
        .insert_row(
            "Items",
            &row(&[
                ("Invoice", Value::Integer(invoice)),
                ("Label", Value::from("line")),
                ("Amount", Value::Integer(21)),
            ]),
        )
        .expect("child");
    let first = workspace.read_record("Invoices", invoice).expect("read");
    assert_eq!(first.get("Total"), Some(&Value::Integer(21)));
    assert_eq!(first.get("Doubled"), Some(&Value::Integer(42)));
    workspace
        .update_row("Items", child.id, &row(&[("Amount", Value::Integer(50))]))
        .expect("update");
    let second = workspace.read_record("Invoices", invoice).expect("read");
    assert_eq!(second.get("Total"), Some(&Value::Integer(50)));
    assert_eq!(second.get("Doubled"), Some(&Value::Integer(100)));
}

#[test]
fn extremum_survives_non_extreme_child_deletes() {
    let directory = tempfile::TempDir::new().expect("tempdir");
    let (_path, mut workspace) = rollup_workspace(&directory);
    let invoice = workspace
        .insert_row("Invoices", &row(&[("Number", Value::from("EXT"))]))
        .expect("invoice")
        .id;
    let mut ids = Vec::new();
    for amount in [10, 20, 30] {
        ids.push(
            workspace
                .insert_row(
                    "Items",
                    &row(&[
                        ("Invoice", Value::Integer(invoice)),
                        ("Label", Value::from(format!("line {amount}"))),
                        ("Amount", Value::Integer(amount)),
                    ]),
                )
                .expect("child")
                .id,
        );
    }
    let values = rollup_values(&workspace, invoice);
    assert_eq!(values[3], Value::Integer(10));
    assert_eq!(values[4], Value::Integer(30));

    // The cache is now warm. Removing a middle value must not clear MIN/MAX.
    workspace
        .delete_row("Items", ids[1])
        .expect("delete middle");
    let values = rollup_values(&workspace, invoice);
    assert_eq!(values[3], Value::Integer(10));
    assert_eq!(values[4], Value::Integer(30));

    workspace.delete_row("Items", ids[0]).expect("delete min");
    let values = rollup_values(&workspace, invoice);
    assert_eq!(values[3], Value::Integer(30));
    assert_eq!(values[4], Value::Integer(30));

    workspace.delete_row("Items", ids[2]).expect("delete max");
    let values = rollup_values(&workspace, invoice);
    assert_eq!(values[3], Value::Null);
    assert_eq!(values[4], Value::Null);
}

#[test]
fn record_history_reports_field_level_diffs() {
    let directory = tempfile::TempDir::new().expect("tempdir");
    let (_path, mut workspace) = rollup_workspace(&directory);
    let invoice = workspace
        .insert_row("Invoices", &row(&[("Number", Value::from("HIST"))]))
        .expect("invoice")
        .id;
    let child = workspace
        .insert_row(
            "Items",
            &row(&[
                ("Invoice", Value::Integer(invoice)),
                ("Label", Value::from("alpha")),
                ("Amount", Value::Integer(10)),
            ]),
        )
        .expect("child");
    workspace
        .update_row("Items", child.id, &row(&[("Amount", Value::Integer(20))]))
        .expect("update");
    let history = workspace
        .record_history("Items", child.id)
        .expect("history");
    assert!(history.len() >= 2);
    let update = history
        .iter()
        .find(|revision| revision.operation == "update")
        .expect("update revision");
    let diff = update
        .diffs
        .iter()
        .find(|diff| diff.column == "Amount")
        .expect("amount diff");
    assert_eq!(diff.before, Some(Value::Integer(10)));
    assert_eq!(diff.after, Some(Value::Integer(20)));
}

#[test]
fn filtered_pages_emit_the_index_hint_column() {
    let directory = tempfile::TempDir::new().expect("tempdir");
    let path = directory.path().join("filtered.slab");
    let mut workspace =
        Workspace::create(&path, "Filtered", Box::new(TestClock::new())).expect("create");
    workspace
        .create_table(&TableDraft {
            name: "People".into(),
            columns: vec![text("City")],
        })
        .expect("table");
    {
        let connection = Connection::open(&path).expect("raw");
        connection.execute_batch("BEGIN IMMEDIATE").expect("begin");
        {
            let mut insert = connection
                .prepare("INSERT INTO \"People\" (\"City\") VALUES (?1)")
                .expect("prepare");
            for index in 0..5_000 {
                insert
                    .execute(rusqlite::params![format!("City {}", index % 40)])
                    .expect("insert");
            }
        }
        connection.execute_batch("COMMIT").expect("commit");
    }
    let page = workspace
        .page(&PageQuery {
            table: "People".into(),
            limit: 100,
            filters: vec![Filter {
                column: "City".into(),
                op: FilterOp::Contains,
                value: Some(Value::from("City 1")),
            }],
            ..PageQuery::default()
        })
        .expect("page");
    let hint = page.index_hint.expect("hint");
    assert_eq!(hint.column, "City");
}

#[test]
fn record_frames_round_trip_named_records() {
    let directory = tempfile::TempDir::new().expect("tempdir");
    let path = directory.path().join("frames.slab");
    let mut workspace =
        Workspace::create(&path, "Frames", Box::new(TestClock::new())).expect("create");
    workspace
        .create_table(&TableDraft {
            name: "Rows".into(),
            columns: vec![text("Name"), integer("Value")],
        })
        .expect("table");
    workspace
        .insert_row(
            "Rows",
            &row(&[("Name", Value::from("one")), ("Value", Value::Integer(1))]),
        )
        .expect("row");
    let page = workspace
        .page(&PageQuery {
            table: "Rows".into(),
            limit: 10,
            ..PageQuery::default()
        })
        .expect("page");
    let frame = page.into_frame(vec!["Name".into(), "Value".into()]);
    let records: Vec<Record> = frame.records();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].get("Name"), Some(&Value::from("one")));
    assert_eq!(records[0].get("Value"), Some(&Value::Integer(1)));
}

fn rollup_state_dump(
    path: &std::path::Path,
    parent_id: i64,
) -> Vec<(String, String, String, String)> {
    let connection = Connection::open(path).expect("raw open");
    let mut stmt = connection
        .prepare(
            "SELECT relation_name, child_column, aggregate, state_json FROM _slab_rollups
             WHERE parent_table = 'Invoices' AND parent_id = ?1
             ORDER BY relation_name, child_column, aggregate",
        )
        .expect("prepare dump");
    let rows = stmt
        .query_map(rusqlite::params![parent_id], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
        })
        .expect("dump rows");
    rows.map(|row| row.expect("dump row")).collect()
}

/// Extremum ties, NULL transitions, undo/redo, and a parent move must keep both
/// the visible aggregates and the serialized `_slab_rollups` state identical to
/// a fresh scan after every step.
#[test]
fn extremum_ties_nulls_undo_redo_and_moves_match_a_fresh_scan() {
    let directory = tempfile::TempDir::new().expect("tempdir");
    let (path, mut workspace) = rollup_workspace(&directory);
    let first = workspace
        .insert_row("Invoices", &row(&[("Number", Value::from("TIES-A"))]))
        .expect("invoice")
        .id;
    let second = workspace
        .insert_row("Invoices", &row(&[("Number", Value::from("TIES-B"))]))
        .expect("invoice")
        .id;

    let mut ids = Vec::new();
    for (label, amount) in [("a", 500), ("b", 500), ("c", 100), ("d", 10)] {
        ids.push(
            workspace
                .insert_row(
                    "Items",
                    &row(&[
                        ("Invoice", Value::Integer(first)),
                        ("Label", Value::from(label)),
                        ("Amount", Value::Integer(amount)),
                        ("Qty", Value::Integer(1)),
                    ]),
                )
                .expect("child")
                .id,
        );
    }

    let mut step = 0;
    let check = |workspace: &Workspace, step: &mut i32| {
        for invoice in [first, second] {
            let maintained = rollup_values(workspace, invoice);
            let maintained_state = rollup_state_dump(&path, invoice);
            clear_cache(&path);
            let fresh = rollup_values(workspace, invoice);
            let fresh_state = rollup_state_dump(&path, invoice);
            assert_eq!(
                maintained, fresh,
                "aggregate mismatch after step {step} on invoice {invoice}"
            );
            assert_eq!(
                maintained_state, fresh_state,
                "cached state mismatch after step {step} on invoice {invoice}"
            );
        }
        // Re-warm both parents. Without this, every mutation below would find
        // an empty cache and recompute, which would hide a broken delta path.
        for invoice in [first, second] {
            let _ = rollup_values(workspace, invoice);
        }
        *step += 1;
    };
    check(&workspace, &mut step);

    workspace
        .delete_row("Items", ids[0])
        .expect("delete tied max");
    check(&workspace, &mut step);

    workspace
        .update_row("Items", ids[1], &row(&[("Amount", Value::Null)]))
        .expect("null the remaining max");
    check(&workspace, &mut step);

    workspace.undo().expect("undo null");
    check(&workspace, &mut step);

    workspace.undo().expect("undo delete");
    check(&workspace, &mut step);

    workspace.redo().expect("redo delete");
    check(&workspace, &mut step);

    workspace.redo().expect("redo null");
    check(&workspace, &mut step);

    // A non-extreme child that becomes NULL still changes the count. The
    // visible MIN/MAX are unaffected, so only the serialized state can catch a
    // missing count adjustment.
    let middle = workspace
        .insert_row(
            "Items",
            &row(&[
                ("Invoice", Value::Integer(first)),
                ("Label", Value::from("middle")),
                ("Amount", Value::Integer(50)),
                ("Qty", Value::Integer(1)),
            ]),
        )
        .expect("middle child")
        .id;
    check(&workspace, &mut step);
    workspace
        .update_row("Items", middle, &row(&[("Amount", Value::Null)]))
        .expect("null a non-extreme child");
    let max_state = rollup_state_dump(&path, first)
        .into_iter()
        .find(|entry| entry.2 == "max")
        .map(|entry| entry.3)
        .expect("max state");
    assert!(
        max_state.contains("\"count\":2"),
        "stale extremum count after a non-extreme NULL update: {max_state}"
    );
    check(&workspace, &mut step);

    // Relabelling a child keeps COUNT unchanged but still runs the update
    // path for the counted column. A COUNT state must keep its unused sum slot
    // empty so the cache stays byte-identical to a fresh scan.
    workspace
        .update_row(
            "Items",
            ids[2],
            &row(&[("Label", Value::from("relabelled"))]),
        )
        .expect("relabel a counted child");
    check(&workspace, &mut step);

    // `d` holds the MIN. Its MAX sibling (`c`) is not the changed value, so a
    // delta path that forgets to adjust the non-extreme count leaves stale
    // state behind even though the visible MAX is right.
    workspace
        .update_row(
            "Items",
            ids[3],
            &row(&[("Invoice", Value::Integer(second))]),
        )
        .expect("move the MIN-holding child to the other parent");
    check(&workspace, &mut step);

    let moved = rollup_values(&workspace, second);
    assert_eq!(moved[0], Value::Integer(10));
    assert_eq!(moved[3], Value::Integer(10));
    assert_eq!(moved[4], Value::Integer(10));
    let remaining = rollup_values(&workspace, first);
    assert_eq!(remaining[3], Value::Integer(100));
    assert_eq!(remaining[4], Value::Integer(100));
}

/// An import that touches parent rollups and is then rolled back must restore
/// both the visible totals and the cached state, with no audit residue.
#[test]
fn rollback_import_restores_rollups_and_audit() {
    let directory = tempfile::TempDir::new().expect("tempdir");
    let (_path, mut workspace) = rollup_workspace(&directory);
    let invoice = workspace
        .insert_row("Invoices", &row(&[("Number", Value::from("IMPORT"))]))
        .expect("invoice")
        .id;
    workspace
        .insert_row(
            "Items",
            &row(&[
                ("Invoice", Value::Integer(invoice)),
                ("Label", Value::from("seed")),
                ("Amount", Value::Integer(100)),
                ("Qty", Value::Integer(1)),
            ]),
        )
        .expect("seed");
    let before = rollup_values(&workspace, invoice);
    assert_eq!(before[0], Value::Integer(100));
    let audit_before = workspace.audit_entries(1_000, 0).expect("audit").len();

    workspace.begin_import().expect("begin");
    workspace
        .insert_import_rows(
            "Items",
            &[
                row(&[
                    ("Invoice", Value::Integer(invoice)),
                    ("Label", Value::from("import a")),
                    ("Amount", Value::Integer(50)),
                    ("Qty", Value::Integer(1)),
                ]),
                row(&[
                    ("Invoice", Value::Integer(invoice)),
                    ("Label", Value::from("import b")),
                    ("Amount", Value::Integer(75)),
                    ("Qty", Value::Integer(1)),
                ]),
            ],
        )
        .expect("import rows");
    workspace.rollback_import().expect("rollback");

    assert_eq!(rollup_values(&workspace, invoice), before);
    assert_eq!(
        workspace.audit_entries(1_000, 0).expect("audit").len(),
        audit_before,
        "rollback left audit entries behind"
    );
    assert!(workspace.integrity_check().expect("integrity").audit_chain);

    workspace.begin_import().expect("begin again");
    workspace
        .insert_import_rows(
            "Items",
            &[row(&[
                ("Invoice", Value::Integer(invoice)),
                ("Label", Value::from("committed")),
                ("Amount", Value::Integer(25)),
                ("Qty", Value::Integer(1)),
            ])],
        )
        .expect("import rows");
    workspace.commit_import().expect("commit");
    assert_eq!(rollup_values(&workspace, invoice)[0], Value::Integer(125));
}

#[test]
fn failed_import_batch_rolls_back_every_batch_and_refuses_commit() {
    let directory = tempfile::TempDir::new().expect("tempdir");
    let (path, mut workspace) = rollup_workspace(&directory);
    let invoice = workspace
        .insert_row("Invoices", &row(&[("Number", Value::from("IMPORT"))]))
        .expect("invoice")
        .id;
    let item = |parent, label| {
        row(&[
            ("Invoice", Value::Integer(parent)),
            ("Label", Value::from(label)),
            ("Amount", Value::Integer(25)),
            ("Qty", Value::Integer(1)),
        ])
    };
    let rollups_before = rollup_values(&workspace, invoice);
    let audit_before = workspace.audit_entries(100, 0).expect("audit");
    workspace.clear_history();

    workspace.begin_import().expect("begin");
    workspace
        .insert_import_rows("Items", &[item(invoice, "pendingfirst")])
        .expect("earlier batch");
    let error = workspace
        .insert_import_rows(
            "Items",
            &[item(invoice, "pendingsecond"), item(-1, "badreference")],
        )
        .expect_err("second row of batch violates foreign key");
    assert_eq!(error.code_str(), "SLAB_E_FK_VIOLATION");
    assert_eq!(
        workspace
            .commit_import()
            .expect_err("failed import cannot commit")
            .code_str(),
        "SLAB_E_COMMAND"
    );
    assert_eq!(count_rows(&path, "Items"), 0);
    assert!(
        workspace
            .search("pendingfirst", 10)
            .expect("search")
            .is_empty()
    );
    assert!(
        workspace
            .search("pendingsecond", 10)
            .expect("search")
            .is_empty()
    );
    assert_eq!(rollup_values(&workspace, invoice), rollups_before);
    assert_eq!(
        workspace.audit_entries(100, 0).expect("audit"),
        audit_before
    );
    assert!(workspace.integrity_check().expect("integrity").audit_chain);
    assert!(!workspace.undo_state().can_undo);

    workspace.begin_import().expect("restart import");
    workspace
        .insert_import_rows("Items", &[item(invoice, "committed")])
        .expect("retry batch");
    workspace.commit_import().expect("commit retry");
    assert_eq!(rollup_values(&workspace, invoice)[0], Value::Integer(25));
    workspace.undo().expect("undo committed import");
    assert_eq!(count_rows(&path, "Items"), 0);
}

/// A lookup over a formula child column is evaluated from live rows. It must
/// keep working across renames of the source column and the formula column, and
/// it must never create a cache entry because there is no physical column.
#[test]
fn computed_child_extrema_survive_column_renames() {
    let directory = tempfile::TempDir::new().expect("tempdir");
    let path = directory.path().join("computed-extrema.slab");
    let mut workspace =
        Workspace::create(&path, "Computed", Box::new(TestClock::new())).expect("create");
    workspace
        .create_table(&TableDraft {
            name: "Invoices".into(),
            columns: vec![text("Number")],
        })
        .expect("invoices");
    workspace
        .create_table(&TableDraft {
            name: "Items".into(),
            columns: vec![integer("Invoice"), integer("Qty"), integer("Price")],
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
            "Items",
            &ColumnDraft {
                name: "Line Total".into(),
                ty: ColumnType::Formula {
                    expression: "Qty * Price".into(),
                },
                required: false,
                unique: false,
                default: None,
            },
        )
        .expect("formula");
    workspace
        .add_column(
            "Invoices",
            &lookup("Largest Line", "Items", "Line Total", Aggregate::Max),
        )
        .expect("max lookup");
    workspace
        .add_column(
            "Invoices",
            &lookup("Smallest Line", "Items", "Line Total", Aggregate::Min),
        )
        .expect("min lookup");

    let invoice = workspace
        .insert_row("Invoices", &row(&[("Number", Value::from("INV-C"))]))
        .expect("invoice")
        .id;
    let first = workspace
        .insert_row(
            "Items",
            &row(&[
                ("Invoice", Value::Integer(invoice)),
                ("Qty", Value::Integer(2)),
                ("Price", Value::Integer(100)),
            ]),
        )
        .expect("item")
        .id;
    workspace
        .insert_row(
            "Items",
            &row(&[
                ("Invoice", Value::Integer(invoice)),
                ("Qty", Value::Integer(3)),
                ("Price", Value::Integer(50)),
            ]),
        )
        .expect("item");

    let extrema = |workspace: &Workspace| {
        let record = workspace
            .read_record("Invoices", invoice)
            .expect("parent read");
        (
            record.get("Largest Line").cloned().unwrap_or(Value::Null),
            record.get("Smallest Line").cloned().unwrap_or(Value::Null),
        )
    };
    assert_eq!(
        extrema(&workspace),
        (Value::Integer(200), Value::Integer(150))
    );

    workspace
        .rename_column("Items", "Price", "Unit Price")
        .expect("rename source");
    assert_eq!(
        extrema(&workspace),
        (Value::Integer(200), Value::Integer(150))
    );

    workspace
        .update_row("Items", first, &row(&[("Qty", Value::Integer(4))]))
        .expect("update qty");
    assert_eq!(
        extrema(&workspace),
        (Value::Integer(400), Value::Integer(150))
    );

    workspace
        .rename_column("Items", "Line Total", "Extended")
        .expect("rename formula");
    assert_eq!(
        extrema(&workspace),
        (Value::Integer(400), Value::Integer(150))
    );

    let connection = Connection::open(&path).expect("raw open");
    let cached: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM _slab_rollups
             WHERE child_column IN ('Line Total', 'Extended')",
            [],
            |row| row.get(0),
        )
        .expect("cached count");
    assert_eq!(cached, 0, "a virtual child column was cached");
}
