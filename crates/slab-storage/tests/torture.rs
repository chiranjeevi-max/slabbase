#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod support;

use std::fs;
use std::path::PathBuf;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use slab_core::{
    ColumnDraft, ColumnType, CurrencyCode, DeleteAction, Filter, FilterOp, Layout, LayoutItem,
    LayoutItemKind, PageQuery, RelationDraft, RowData, SortDirection, SortSpec, TableDraft, Value,
};
use slab_storage::{StorageError, Workspace};
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

fn formula(name: &str, expression: &str) -> ColumnDraft {
    ColumnDraft {
        name: name.into(),
        ty: ColumnType::Formula {
            expression: expression.into(),
        },
        required: false,
        unique: false,
        default: None,
    }
}

fn row(entries: &[(&str, Value)]) -> RowData {
    entries
        .iter()
        .map(|(name, value)| ((*name).to_owned(), value.clone()))
        .collect()
}

fn workspace() -> Workspace {
    Workspace::open_in_memory("Torture", Box::new(TestClock::new())).expect("workspace")
}

fn table_count(workspace: &Workspace) -> usize {
    workspace
        .schema()
        .expect("schema")
        .tables
        .iter()
        .filter(|table| !table.name.starts_with("_slab_"))
        .count()
}

#[test]
fn reserved_keyword_identifiers_survive_the_whole_lifecycle() {
    let mut workspace = workspace();
    workspace
        .create_table(&TableDraft {
            name: "order".into(),
            columns: vec![
                ColumnDraft {
                    required: true,
                    ..text("select")
                },
                text("group"),
                text("values"),
                text("index"),
                text("table"),
            ],
        })
        .expect("reserved keyword table");
    let inserted = workspace
        .insert_row(
            "order",
            &row(&[
                ("select", Value::Text("Northlight C++".into())),
                ("group", Value::Text("studio".into())),
                ("values", Value::Text("1; DROP TABLE order; --".into())),
                ("index", Value::Text("quoted \"value\"".into())),
                ("table", Value::Text("O'Brien".into())),
            ]),
        )
        .expect("insert");
    assert_eq!(
        inserted.get("select"),
        Some(&Value::Text("Northlight C++".into()))
    );

    let page = workspace
        .page(&PageQuery {
            table: "order".into(),
            limit: 10,
            sort: vec![SortSpec {
                column: "select".into(),
                direction: SortDirection::Asc,
            }],
            filters: vec![Filter {
                column: "select".into(),
                op: FilterOp::Contains,
                value: Some(Value::Text("northlight".into())),
            }],
            ..PageQuery::default()
        })
        .expect("sort and filter");
    assert_eq!(page.rows.len(), 1);

    assert_eq!(workspace.search("northlight", 10).expect("search").len(), 1);

    workspace
        .rename_column("order", "select", "from")
        .expect("reserved column rename");
    workspace
        .rename_table("order", "group")
        .expect("reserved table rename");
    workspace.undo().expect("undo table rename");
    workspace.undo().expect("undo column rename");
    assert_eq!(
        workspace
            .schema()
            .expect("schema")
            .table("order")
            .expect("order")
            .column("select")
            .expect("select")
            .name,
        "select"
    );
    workspace.redo().expect("redo column rename");
    workspace.redo().expect("redo table rename");
    assert!(workspace.schema().expect("schema").table("group").is_some());

    let path = tempfile::NamedTempFile::new()
        .expect("temp")
        .into_temp_path();
    workspace.export_sql(&path).expect("export");
    let sql = fs::read_to_string(&path).expect("read export");
    let exported = rusqlite::Connection::open_in_memory().expect("export db");
    exported.execute_batch(&sql).expect("export is valid SQL");
    let count: i64 = exported
        .query_row("SELECT COUNT(*) FROM \"group\"", [], |row| row.get(0))
        .expect("exported rows");
    assert_eq!(count, 1);
    let value: String = exported
        .query_row("SELECT \"from\" FROM \"group\"", [], |row| row.get(0))
        .expect("exported value");
    assert_eq!(value, "Northlight C++");

    workspace.drop_table("group").expect("drop");
    assert_eq!(table_count(&workspace), 0);
}

#[test]
fn hostile_identifiers_are_rejected_without_touching_the_workspace() {
    let mut workspace = workspace();
    workspace
        .create_table(&TableDraft {
            name: "Clients".into(),
            columns: vec![text("Name")],
        })
        .expect("clients");
    let before = workspace.schema().expect("schema");
    let audit_before = workspace.audit_entries(100, 0).expect("audit").len();

    let hostile = [
        "\"quoted\"",
        "o'reilly",
        "a; DROP TABLE Clients; --",
        " leading",
        "trailing ",
        "emoji_😀",
        "line\nbreak",
        "a.b",
        "a]b",
        "_slab_meta",
        "id",
        "",
        &"x".repeat(65),
    ];
    for name in hostile {
        let create = workspace.create_table(&TableDraft {
            name: name.into(),
            columns: vec![text("Name")],
        });
        assert!(create.is_err(), "create accepted hostile name {name:?}");
        let rename = workspace.rename_table("Clients", name);
        assert!(rename.is_err(), "rename accepted hostile name {name:?}");
        let column = workspace.rename_column("Clients", "Name", name);
        assert!(
            column.is_err(),
            "column rename accepted hostile name {name:?}"
        );
    }

    let after = workspace.schema().expect("schema");
    assert_eq!(after, before);
    assert_eq!(
        workspace.audit_entries(100, 0).expect("audit").len(),
        audit_before
    );
    assert_eq!(
        workspace
            .page(&PageQuery::first("Clients", 10))
            .expect("page")
            .rows
            .len(),
        0
    );
}

#[test]
fn fts_search_never_returns_a_raw_sqlite_syntax_error() {
    let mut workspace = workspace();
    workspace
        .create_table(&TableDraft {
            name: "Docs".into(),
            columns: vec![text("Body")],
        })
        .expect("docs");
    workspace
        .insert_row(
            "Docs",
            &row(&[(
                "Body",
                Value::Text("northlight C++ foo-bar tax_rate Renée € —".into()),
            )]),
        )
        .expect("row");
    workspace
        .insert_row(
            "Docs",
            &row(&[(
                "Body",
                Value::Text("unclosed quote OR AND NOT col:value ^start ()[]{}".into()),
            )]),
        )
        .expect("row");

    let hostile = [
        "C++",
        "foo-bar",
        "northlight*",
        "\"unclosed quote",
        "OR AND NOT",
        "col:value",
        "^start",
        "()[]{}",
        "",
        "   ",
        "\"",
        "a\"b",
        "*",
        "-",
        "NEAR(a b)",
        "Renée",
        "€",
        "\0",
        "north\0light",
        &"a".repeat(1_000),
    ];
    for term in hostile {
        let result = workspace.search(term, 20);
        assert!(result.is_ok(), "search {term:?} failed: {result:?}");
    }
    assert_eq!(workspace.search("northlight", 20).expect("search").len(), 1);
    assert_eq!(workspace.search("C++", 20).expect("search").len(), 1);
    assert!(workspace.search("\0", 20).expect("nul search").is_empty());
}

#[test]
fn column_rename_rewrites_exact_tokens_only() {
    let mut workspace = workspace();
    workspace
        .create_table(&TableDraft {
            name: "Calc".into(),
            columns: vec![
                integer("rate"),
                integer("tax_rate"),
                text("created_at"),
                formula(
                    "Result",
                    "[rate] * 2 + [tax_rate] + LEN(\"rate\") + LEN(\"tax_rate\")",
                ),
            ],
        })
        .expect("calc");
    let before = workspace.schema().expect("schema");

    workspace
        .rename_column("Calc", "rate", "unit_rate")
        .expect("rename");
    let schema = workspace.schema().expect("schema");
    let ColumnType::Formula { expression } = &schema
        .table("Calc")
        .expect("calc")
        .column("Result")
        .expect("result")
        .ty
    else {
        panic!("formula expected");
    };
    let parsed = slab_formula::parse(expression).expect("parse");
    assert_eq!(
        parsed.bare_columns(),
        vec!["unit_rate".to_owned(), "tax_rate".to_owned()]
    );
    assert_eq!(expression.matches("\"rate\"").count(), 1);
    assert_eq!(expression.matches("\"tax_rate\"").count(), 1);
    assert!(!expression.contains("[rate]"));
    assert!(expression.contains("[tax_rate]"));

    workspace
        .rename_column("Calc", "tax_rate", "tax")
        .expect("rename tax");
    let schema = workspace.schema().expect("schema");
    let ColumnType::Formula { expression } = &schema
        .table("Calc")
        .expect("calc")
        .column("Result")
        .expect("result")
        .ty
    else {
        panic!("formula expected");
    };
    let parsed = slab_formula::parse(expression).expect("parse");
    assert_eq!(
        parsed.bare_columns(),
        vec!["unit_rate".to_owned(), "tax".to_owned()]
    );
    assert_eq!(expression.matches("\"tax_rate\"").count(), 1);

    workspace.undo().expect("undo tax");
    workspace.undo().expect("undo rate");
    assert_eq!(workspace.schema().expect("schema"), before);
}

#[test]
fn table_rename_keeps_relations_portals_rollups_and_undo_intact() {
    let mut workspace = workspace();
    workspace
        .create_table(&TableDraft {
            name: "Invoices".into(),
            columns: vec![ColumnDraft {
                required: true,
                ..text("Number")
            }],
        })
        .expect("invoices");
    workspace
        .create_table(&TableDraft {
            name: "Line Items".into(),
            columns: vec![
                text("Description"),
                integer("Invoice"),
                integer("Qty"),
                integer("Amount"),
            ],
        })
        .expect("line items");
    workspace
        .create_relation(&RelationDraft {
            name: "Invoice Lines".into(),
            parent_table: "Invoices".into(),
            child_table: "Line Items".into(),
            child_column: "Invoice".into(),
            on_delete: DeleteAction::Cascade,
        })
        .expect("relation");
    workspace
        .add_column(
            "Invoices",
            &formula(
                "Total",
                "SUM([Invoice Lines].[Qty]) + SUM([Invoice Lines].[Amount])",
            ),
        )
        .expect("total");
    workspace
        .add_column(
            "Invoices",
            &ColumnDraft {
                name: "Max Amount".into(),
                ty: ColumnType::Lookup {
                    relation: "Invoice Lines".into(),
                    column: "Amount".into(),
                    aggregate: slab_core::Aggregate::Max,
                },
                required: false,
                unique: false,
                default: None,
            },
        )
        .expect("lookup");
    workspace
        .save_layout(Layout {
            id: "layout".into(),
            name: "Invoice".into(),
            table: "Invoices".into(),
            grid: slab_core::GridSpec::default(),
            items: vec![LayoutItem {
                id: "portal".into(),
                kind: LayoutItemKind::Portal {
                    relation: "Invoice Lines".into(),
                    columns: vec!["Description".into(), "Amount".into()],
                    rows: 10,
                },
                x: 0,
                y: 0,
                w: 12,
                h: 4,
            }],
            updated_at: 0,
        })
        .expect("layout");
    let invoice = workspace
        .insert_row("Invoices", &row(&[("Number", Value::Text("INV-1".into()))]))
        .expect("invoice");
    for (qty, amount) in [(2, 100), (3, 250)] {
        workspace
            .insert_row(
                "Line Items",
                &row(&[
                    ("Description", Value::Text("Design".into())),
                    ("Invoice", Value::Integer(invoice.id)),
                    ("Qty", Value::Integer(qty)),
                    ("Amount", Value::Integer(amount)),
                ]),
            )
            .expect("line");
    }
    let before_schema = workspace.schema().expect("schema");
    let before_total = workspace.read_record("Invoices", invoice.id).expect("read");
    assert_eq!(before_total.get("Total"), Some(&Value::Integer(355)));
    assert_eq!(before_total.get("Max Amount"), Some(&Value::Integer(250)));
    workspace.clear_history();

    workspace
        .rename_table("Line Items", "Invoice Entries")
        .expect("rename child");
    workspace
        .rename_table("Invoices", "Bills")
        .expect("rename parent");
    let schema = workspace.schema().expect("schema");
    let relation = schema.relation("Invoice Lines").expect("relation");
    assert_eq!(relation.parent_table, "Bills");
    assert_eq!(relation.child_table, "Invoice Entries");
    let layout = schema.layouts.first().expect("layout");
    assert_eq!(layout.table, "Bills");
    let LayoutItemKind::Portal { relation, .. } = &layout.items[0].kind else {
        panic!("portal expected");
    };
    assert_eq!(relation, "Invoice Lines");
    let report = workspace.integrity_check().expect("integrity");
    assert_eq!(report.integrity, "ok");
    assert_eq!(report.foreign_key_violations, 0);
    let total = workspace.read_record("Bills", invoice.id).expect("read");
    assert_eq!(total.get("Total"), Some(&Value::Integer(355)));
    assert_eq!(total.get("Max Amount"), Some(&Value::Integer(250)));
    assert_eq!(workspace.search("Design", 10).expect("search").len(), 2);

    workspace.undo().expect("undo parent");
    workspace.undo().expect("undo child");
    assert_eq!(workspace.schema().expect("schema"), before_schema);
    let restored = workspace.read_record("Invoices", invoice.id).expect("read");
    assert_eq!(restored.get("Total"), Some(&Value::Integer(355)));
    assert_eq!(workspace.search("Design", 10).expect("search").len(), 2);
}

#[test]
fn rollups_handle_empty_sets_overflow_and_null_propagation() {
    let mut workspace = workspace();
    workspace
        .create_table(&TableDraft {
            name: "Parents".into(),
            columns: vec![text("Name")],
        })
        .expect("parents");
    workspace
        .create_table(&TableDraft {
            name: "Children".into(),
            columns: vec![integer("Parent"), integer("Amount")],
        })
        .expect("children");
    workspace
        .create_relation(&RelationDraft {
            name: "Child Lines".into(),
            parent_table: "Parents".into(),
            child_table: "Children".into(),
            child_column: "Parent".into(),
            on_delete: DeleteAction::Cascade,
        })
        .expect("relation");
    for (name, expression) in [
        ("Sum", "SUM([Child Lines].[Amount])"),
        ("Avg", "AVG([Child Lines].[Amount])"),
        ("Min", "MIN([Child Lines].[Amount])"),
        ("Max", "MAX([Child Lines].[Amount])"),
        ("Count", "COUNT([Child Lines].[Amount])"),
    ] {
        workspace
            .add_column("Parents", &formula(name, expression))
            .expect("rollup");
    }
    let parent = workspace
        .insert_row("Parents", &row(&[("Name", Value::Text("Empty".into()))]))
        .expect("parent");
    let empty = workspace
        .read_record("Parents", parent.id)
        .expect("read empty");
    assert_eq!(empty.get("Sum"), Some(&Value::Integer(0)));
    assert_eq!(empty.get("Avg"), Some(&Value::Integer(0)));
    assert_eq!(empty.get("Min"), Some(&Value::Null));
    assert_eq!(empty.get("Max"), Some(&Value::Null));
    assert_eq!(empty.get("Count"), Some(&Value::Integer(0)));

    let first = workspace
        .insert_row(
            "Children",
            &row(&[
                ("Parent", Value::Integer(parent.id)),
                ("Amount", Value::Integer(i64::MAX)),
            ]),
        )
        .expect("max");
    workspace
        .insert_row(
            "Children",
            &row(&[
                ("Parent", Value::Integer(parent.id)),
                ("Amount", Value::Integer(1)),
            ]),
        )
        .expect("one");
    let error = workspace
        .read_record("Parents", parent.id)
        .expect_err("overflow must be structured");
    assert!(
        error.to_string().contains("overflow") || error.to_string().contains("sum"),
        "unexpected overflow error: {error}"
    );

    workspace
        .update_row(
            "Children",
            first.id,
            &row(&[("Amount", Value::Integer(10))]),
        )
        .expect("update");
    let values = workspace.read_record("Parents", parent.id).expect("read");
    assert_eq!(
        values.get("Avg"),
        Some(&Value::Decimal(
            slab_core::Decimal::parse("5.5").expect("decimal")
        ))
    );
    assert_eq!(values.get("Min"), Some(&Value::Integer(1)));
    assert_eq!(values.get("Max"), Some(&Value::Integer(10)));
    assert_eq!(values.get("Count"), Some(&Value::Integer(2)));
}

#[test]
fn twenty_five_level_formula_chain_evaluates_in_order() {
    let mut workspace = workspace();
    let mut columns = vec![integer("Base")];
    columns.push(formula("Level 01", "[Base] + 1"));
    for level in 2..=25 {
        columns.push(formula(
            &format!("Level {level:02}"),
            &format!("[Level {:02}] + 1", level - 1),
        ));
    }
    workspace
        .create_table(&TableDraft {
            name: "Chain".into(),
            columns,
        })
        .expect("chain");
    let record = workspace
        .insert_row("Chain", &row(&[("Base", Value::Integer(1))]))
        .expect("insert");
    let read = workspace.read_record("Chain", record.id).expect("read");
    assert_eq!(read.get("Level 25"), Some(&Value::Integer(26)));
}

#[test]
fn concurrent_external_writer_and_reader_keep_audit_and_integrity() {
    let directory = tempfile::tempdir().expect("tempdir");
    let path = directory.path().join("concurrent.slab");
    let mut workspace =
        Workspace::create(&path, "Concurrent", Box::new(TestClock::new())).expect("create");
    workspace
        .create_table(&TableDraft {
            name: "Events".into(),
            columns: vec![text("Label")],
        })
        .expect("table");
    workspace.clear_history();

    let (ready_tx, ready_rx) = mpsc::channel();
    let (done_tx, done_rx) = mpsc::channel();
    let writer_path = path.clone();
    let writer = thread::spawn(move || {
        let connection = rusqlite::Connection::open(writer_path).expect("external");
        connection
            .busy_timeout(Duration::from_secs(5))
            .expect("timeout");
        connection
            .execute_batch(
                "BEGIN IMMEDIATE; CREATE TABLE IF NOT EXISTS external_probe (id INTEGER);",
            )
            .expect("begin");
        ready_tx.send(()).expect("ready");
        thread::sleep(Duration::from_millis(120));
        connection
            .execute_batch("COMMIT;")
            .expect("commit external");
        done_tx.send(()).expect("done");
    });
    ready_rx.recv().expect("writer ready");
    for index in 0..40 {
        workspace
            .insert_row(
                "Events",
                &row(&[("Label", Value::Text(format!("event {index}")))]),
            )
            .expect("contended insert");
    }
    done_rx.recv().expect("writer finished");
    writer.join().expect("writer thread");

    let reader = rusqlite::Connection::open(&path).expect("reader");
    let count: i64 = reader
        .query_row("SELECT COUNT(*) FROM \"Events\"", [], |row| row.get(0))
        .expect("external count");
    assert_eq!(count, 40);
    let audit: i64 = reader
        .query_row(
            "SELECT COUNT(*) FROM _slab_audit_log WHERE operation = 'insert'",
            [],
            |row| row.get(0),
        )
        .expect("external audit");
    assert_eq!(audit, 40);
    let report = workspace.integrity_check().expect("integrity");
    assert_eq!(report.integrity, "ok");
    assert_eq!(report.foreign_key_violations, 0);
    assert_eq!(report.journal_mode, "wal");
}

#[test]
fn hostile_workspace_files_fail_cleanly_without_mutation() {
    let directory = tempfile::tempdir().expect("tempdir");

    let empty_path = directory.path().join("empty.slab");
    fs::write(&empty_path, []).expect("empty file");
    let empty_error = Workspace::open(&empty_path, Box::new(TestClock::new()))
        .err()
        .expect("empty file must fail");
    assert!(matches!(empty_error, StorageError::Workspace(_)));
    assert_eq!(fs::metadata(&empty_path).expect("metadata").len(), 0);

    let garbage_path = directory.path().join("garbage.slab");
    let garbage = b"this is definitely not a sqlite database\n\x00\x01\x02";
    fs::write(&garbage_path, garbage).expect("garbage");
    let garbage_error = Workspace::open(&garbage_path, Box::new(TestClock::new()))
        .err()
        .expect("garbage must fail");
    assert!(matches!(garbage_error, StorageError::Workspace(_)));
    assert_eq!(fs::read(&garbage_path).expect("garbage bytes"), garbage);

    let plain_path = directory.path().join("plain.sqlite");
    let plain = rusqlite::Connection::open(&plain_path).expect("plain");
    plain
        .execute_batch("CREATE TABLE unrelated (id INTEGER); INSERT INTO unrelated VALUES (7);")
        .expect("plain schema");
    drop(plain);
    let plain_bytes = fs::read(&plain_path).expect("plain bytes");
    let plain_error = Workspace::open(&plain_path, Box::new(TestClock::new()))
        .err()
        .expect("plain database must fail");
    assert!(matches!(plain_error, StorageError::Workspace(_)));
    assert_eq!(fs::read(&plain_path).expect("plain after"), plain_bytes);

    let valid_path = directory.path().join("valid.slab");
    let mut valid =
        Workspace::create(&valid_path, "Valid", Box::new(TestClock::new())).expect("create");
    valid
        .create_table(&TableDraft {
            name: "Clients".into(),
            columns: vec![text("Name")],
        })
        .expect("table");
    valid
        .insert_row(
            "Clients",
            &row(&[("Name", Value::Text("Northlight".into()))]),
        )
        .expect("row");
    valid.close().expect("close");
    let valid_bytes = fs::read(&valid_path).expect("valid bytes");

    let truncated_path = directory.path().join("truncated.slab");
    fs::write(&truncated_path, &valid_bytes[..100]).expect("truncated");
    let truncated = Workspace::open(&truncated_path, Box::new(TestClock::new()));
    assert!(truncated.is_err(), "truncated workspace must fail cleanly");

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let readonly_path = directory.path().join("readonly.slab");
        fs::copy(&valid_path, &readonly_path).expect("copy");
        fs::set_permissions(&readonly_path, fs::Permissions::from_mode(0o444)).expect("readonly");
        let readonly = Workspace::open(&readonly_path, Box::new(TestClock::new()));
        assert!(readonly.is_err(), "read-only workspace must fail cleanly");
        fs::set_permissions(&readonly_path, fs::Permissions::from_mode(0o644)).expect("restore");
    }

    let reopened = Workspace::open(&valid_path, Box::new(TestClock::new())).expect("reopen");
    assert_eq!(
        reopened
            .page(&PageQuery::first("Clients", 10))
            .expect("page")
            .rows
            .len(),
        1
    );

    let missing = directory.path().join("missing").join("backup.slab");
    assert!(reopened.backup_to(&missing).is_err());
    assert!(!missing.exists());

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let readonly_dir = directory.path().join("readonly-dir");
        fs::create_dir(&readonly_dir).expect("dir");
        fs::set_permissions(&readonly_dir, fs::Permissions::from_mode(0o555)).expect("dir mode");
        let target = readonly_dir.join("backup.slab");
        assert!(reopened.backup_to(&target).is_err());
        fs::set_permissions(&readonly_dir, fs::Permissions::from_mode(0o755)).expect("restore dir");
    }
}

#[test]
fn deep_hostile_path_round_trips_workspace_backup_and_rows() {
    let directory = tempfile::tempdir().expect("tempdir");
    let deep: PathBuf = directory
        .path()
        .join("deep dir")
        .join("O'Brien \"quoted\" — 日本語")
        .join("space name")
        .join("workspace.slab");
    fs::create_dir_all(deep.parent().expect("parent")).expect("dirs");
    let mut workspace =
        Workspace::create(&deep, "Deep", Box::new(TestClock::new())).expect("create");
    workspace
        .create_table(&TableDraft {
            name: "Clients".into(),
            columns: vec![text("Name")],
        })
        .expect("table");
    workspace
        .insert_row("Clients", &row(&[("Name", Value::Text("Renée".into()))]))
        .expect("row");
    let backup = deep.parent().expect("parent").join("backup target.slab");
    workspace.backup_to(&backup).expect("backup");
    assert!(backup.exists());
    workspace.close().expect("close");

    let reopened = Workspace::open(&deep, Box::new(TestClock::new())).expect("reopen");
    assert_eq!(
        reopened
            .page(&PageQuery::first("Clients", 10))
            .expect("page")
            .rows[0]
            .get("Name"),
        Some(&Value::Text("Renée".into()))
    );
}

#[test]
fn currency_scales_round_trip_without_hardcoded_minor_units() {
    let mut workspace = workspace();
    workspace
        .create_table(&TableDraft {
            name: "Prices".into(),
            columns: vec![
                ColumnDraft {
                    name: "JPY".into(),
                    ty: ColumnType::Currency {
                        currency: CurrencyCode::parse("JPY").expect("jpy"),
                    },
                    required: false,
                    unique: false,
                    default: None,
                },
                ColumnDraft {
                    name: "USD".into(),
                    ty: ColumnType::Currency {
                        currency: CurrencyCode::parse("USD").expect("usd"),
                    },
                    required: false,
                    unique: false,
                    default: None,
                },
                ColumnDraft {
                    name: "KWD".into(),
                    ty: ColumnType::Currency {
                        currency: CurrencyCode::parse("KWD").expect("kwd"),
                    },
                    required: false,
                    unique: false,
                    default: None,
                },
            ],
        })
        .expect("prices");
    let record = workspace
        .insert_row(
            "Prices",
            &row(&[
                (
                    "JPY",
                    Value::Currency(
                        slab_core::Money::from_minor(
                            CurrencyCode::parse("JPY").expect("jpy"),
                            1500,
                        )
                        .expect("money"),
                    ),
                ),
                (
                    "USD",
                    Value::Currency(
                        slab_core::Money::from_minor_scaled(
                            CurrencyCode::parse("USD").expect("usd"),
                            1999,
                            2,
                        )
                        .expect("money"),
                    ),
                ),
                (
                    "KWD",
                    Value::Currency(
                        slab_core::Money::from_minor_scaled(
                            CurrencyCode::parse("KWD").expect("kwd"),
                            1234,
                            3,
                        )
                        .expect("money"),
                    ),
                ),
            ]),
        )
        .expect("insert");
    let read = workspace.read_record("Prices", record.id).expect("read");
    assert_eq!(
        read.get("JPY"),
        Some(&Value::Currency(
            slab_core::Money::from_minor(CurrencyCode::parse("JPY").expect("jpy"), 1500)
                .expect("money")
        ))
    );
    assert_eq!(
        read.get("KWD"),
        Some(&Value::Currency(
            slab_core::Money::from_minor_scaled(CurrencyCode::parse("KWD").expect("kwd"), 1234, 3)
                .expect("money")
        ))
    );
    assert_eq!(
        read.get("USD")
            .and_then(Value::as_money)
            .expect("usd")
            .amount()
            .to_string(),
        "19.99"
    );
}

#[test]
fn import_transaction_rolls_back_every_row_and_commits_fifty_thousand_in_batches() {
    let mut workspace = workspace();
    workspace
        .create_table(&TableDraft {
            name: "Bulk".into(),
            columns: vec![
                text("Name"),
                ColumnDraft {
                    unique: true,
                    ..text("Code")
                },
            ],
        })
        .expect("table");
    workspace.clear_history();

    assert!(workspace.insert_import_rows("Bulk", &[]).is_err());
    workspace.begin_import().expect("begin");
    let mut batch: Vec<RowData> = (0..500)
        .map(|index| {
            row(&[
                ("Name", Value::Text(format!("Row {index}"))),
                ("Code", Value::Text(format!("C-{index}"))),
            ])
        })
        .collect();
    workspace
        .insert_import_rows("Bulk", &batch)
        .expect("first batch");
    batch.push(row(&[
        ("Name", Value::Text("Duplicate".into())),
        ("Code", Value::Text("C-0".into())),
    ]));
    let error = workspace
        .insert_import_rows("Bulk", &batch)
        .expect_err("unique conflict");
    assert_eq!(error.code_str(), "SLAB_E_UNIQUE");
    workspace.rollback_import().expect("rollback");
    assert_eq!(
        workspace
            .page(&PageQuery::first("Bulk", 10))
            .expect("page")
            .rows
            .len(),
        0
    );
    assert_eq!(
        workspace
            .audit_entries(10, 0)
            .expect("audit")
            .iter()
            .filter(|entry| entry.operation == "insert")
            .count(),
        0
    );
    assert!(!workspace.undo_state().can_undo);

    let started = std::time::Instant::now();
    workspace.begin_import().expect("begin large");
    for chunk in 0..100 {
        let rows: Vec<RowData> = (0..500)
            .map(|offset| {
                let index = chunk * 500 + offset;
                row(&[
                    ("Name", Value::Text(format!("Row {index}"))),
                    ("Code", Value::Text(format!("C-{index}"))),
                ])
            })
            .collect();
        workspace.insert_import_rows("Bulk", &rows).expect("batch");
    }
    workspace.commit_import().expect("commit");
    let elapsed = started.elapsed();
    assert!(
        elapsed < std::time::Duration::from_secs(20),
        "50k import took {elapsed:?}"
    );
    let summaries = workspace.table_summaries().expect("summaries");
    let count = summaries
        .iter()
        .find(|summary| summary.name == "Bulk")
        .map(|summary| summary.row_count)
        .expect("bulk summary");
    assert_eq!(count, 50_000);
    let inserted = workspace
        .audit_entries(1_000, 0)
        .expect("audit")
        .iter()
        .filter(|entry| entry.operation == "insert")
        .count();
    assert_eq!(inserted, 1_000);
    let report = workspace.integrity_check().expect("integrity");
    assert_eq!(report.integrity, "ok");
    assert_eq!(report.foreign_key_violations, 0);
}
