//! Regression tests for defects found during the failure-mode audit.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod support;

use rusqlite::Connection;
use slab_core::{
    Aggregate, ColumnDraft, ColumnType, DeleteAction, Filter, FilterOp, GridSpec, Layout,
    LayoutItem, LayoutItemKind, PageQuery, RelationDraft, RowData, TableDraft, Value,
};
use slab_storage::{StorageError, Workspace};
use support::TestClock;

fn workspace() -> Workspace {
    Workspace::open_in_memory("Audit", Box::new(TestClock::new())).expect("workspace")
}

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

fn select(name: &str, options: &[&str]) -> ColumnDraft {
    ColumnDraft {
        name: name.into(),
        ty: ColumnType::Select {
            options: options.iter().map(|option| (*option).to_owned()).collect(),
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

fn deal_table() -> TableDraft {
    TableDraft {
        name: "Deals".into(),
        columns: vec![text("Name"), select("Stage", &["Open", "Won"])],
    }
}

#[test]
fn reading_a_select_column_preserves_its_value_variant() {
    let mut workspace = workspace();
    workspace.create_table(&deal_table()).expect("table");
    let deal = workspace
        .insert_row(
            "Deals",
            &row(&[
                ("Name", Value::Text("Northlight".into())),
                ("Stage", Value::Select("Open".into())),
            ]),
        )
        .expect("insert");
    assert_eq!(
        deal.values.get("Stage"),
        Some(&Value::Select("Open".into()))
    );
}

#[test]
fn updating_another_column_keeps_a_select_value_writable() {
    let mut workspace = workspace();
    workspace.create_table(&deal_table()).expect("table");
    let deal = workspace
        .insert_row(
            "Deals",
            &row(&[
                ("Name", Value::Text("Northlight".into())),
                ("Stage", Value::Select("Open".into())),
            ]),
        )
        .expect("insert");
    let updated = workspace
        .update_row(
            "Deals",
            deal.id,
            &row(&[("Name", Value::Text("Southlight".into()))]),
        )
        .expect("partial update must keep the stored select value");
    assert_eq!(
        updated.values.get("Name"),
        Some(&Value::Text("Southlight".into()))
    );
    assert_eq!(
        updated.values.get("Stage"),
        Some(&Value::Select("Open".into()))
    );
}

#[test]
fn updating_another_column_keeps_an_attachment_value_writable() {
    let mut workspace = workspace();
    workspace
        .create_table(&TableDraft {
            name: "Files".into(),
            columns: vec![
                text("Name"),
                ColumnDraft {
                    name: "Path".into(),
                    ty: ColumnType::AttachmentRef,
                    required: false,
                    unique: false,
                    default: None,
                },
            ],
        })
        .expect("table");
    let file = workspace
        .insert_row(
            "Files",
            &row(&[
                ("Name", Value::Text("Contract".into())),
                ("Path", Value::AttachmentRef("/tmp/contract.pdf".into())),
            ]),
        )
        .expect("insert");
    assert_eq!(
        file.values.get("Path"),
        Some(&Value::AttachmentRef("/tmp/contract.pdf".into()))
    );
    let updated = workspace
        .update_row(
            "Files",
            file.id,
            &row(&[("Name", Value::Text("Draft".into()))]),
        )
        .expect("partial update must keep the stored attachment value");
    assert_eq!(
        updated.values.get("Path"),
        Some(&Value::AttachmentRef("/tmp/contract.pdf".into()))
    );
}

#[test]
fn a_failed_formula_read_does_not_leave_a_committed_row() {
    let mut workspace = workspace();
    workspace
        .create_table(&TableDraft {
            name: "Quotients".into(),
            columns: vec![
                integer("A"),
                integer("B"),
                ColumnDraft {
                    name: "Q".into(),
                    ty: ColumnType::Formula {
                        expression: "[A] / [B]".into(),
                    },
                    required: false,
                    unique: false,
                    default: None,
                },
            ],
        })
        .expect("table");
    let outcome = workspace.insert_row(
        "Quotients",
        &row(&[("A", Value::Integer(1)), ("B", Value::Integer(0))]),
    );
    assert!(outcome.is_err(), "expected a division-by-zero error");
    let audit = workspace.audit_entries(10, 0).expect("audit");
    assert!(
        !audit.iter().any(|entry| entry.operation == "insert"),
        "insert returned an error but committed an audit entry: {audit:?}"
    );
    assert_eq!(
        workspace
            .page(&PageQuery::first("Quotients", 10))
            .expect("page")
            .rows
            .len(),
        0,
        "insert returned an error but left the row committed"
    );
}

#[test]
fn an_imported_row_that_breaks_a_formula_rolls_back() {
    let mut workspace = workspace();
    workspace
        .create_table(&TableDraft {
            name: "Rates".into(),
            columns: vec![
                ColumnDraft {
                    name: "Rate".into(),
                    ty: ColumnType::Decimal { scale: 6 },
                    required: false,
                    unique: false,
                    default: None,
                },
                ColumnDraft {
                    name: "Inverse".into(),
                    ty: ColumnType::Formula {
                        expression: "1 / [Rate]".into(),
                    },
                    required: false,
                    unique: false,
                    default: None,
                },
            ],
        })
        .expect("table");
    workspace.begin_import().expect("begin");
    workspace
        .insert_import_rows(
            "Rates",
            &[row(&[(
                "Rate",
                Value::Decimal(slab_core::Decimal::new(0, 6).expect("zero")),
            )])],
        )
        .expect("types-only import batch");
    assert!(
        workspace.commit_import().is_err(),
        "an import committed a row whose formula cannot be read"
    );
    assert_eq!(
        workspace
            .page(&PageQuery::first("Rates", 10))
            .expect("page")
            .rows
            .len(),
        0
    );
}

#[test]
fn undo_restores_a_child_reached_by_two_cascade_relations() {
    let mut workspace = workspace();
    workspace
        .create_table(&TableDraft {
            name: "People".into(),
            columns: vec![text("Name")],
        })
        .expect("people");
    workspace
        .create_table(&TableDraft {
            name: "Projects".into(),
            columns: vec![text("Name"), integer("Lead"), integer("Sponsor")],
        })
        .expect("projects");
    for (name, column) in [("Lead Projects", "Lead"), ("Sponsor Projects", "Sponsor")] {
        workspace
            .create_relation(&RelationDraft {
                name: name.into(),
                parent_table: "People".into(),
                child_table: "Projects".into(),
                child_column: column.into(),
                on_delete: DeleteAction::Cascade,
            })
            .expect("relation");
    }
    let person = workspace
        .insert_row("People", &row(&[("Name", Value::Text("Ada".into()))]))
        .expect("person");
    let project = workspace
        .insert_row(
            "Projects",
            &row(&[
                ("Name", Value::Text("Atlas".into())),
                ("Lead", Value::Integer(person.id)),
                ("Sponsor", Value::Integer(person.id)),
            ]),
        )
        .expect("project");

    workspace.delete_row("People", person.id).expect("delete");
    assert_eq!(
        workspace
            .page(&PageQuery::first("Projects", 10))
            .expect("page")
            .rows
            .len(),
        0
    );
    workspace
        .undo()
        .expect("undo must restore the parent and the cascaded child");
    assert_eq!(
        workspace
            .read_record("People", person.id)
            .expect("person")
            .get("Name"),
        Some(&Value::Text("Ada".into()))
    );
    assert_eq!(
        workspace
            .read_record("Projects", project.id)
            .expect("child restored")
            .get("Name"),
        Some(&Value::Text("Atlas".into()))
    );
}

#[test]
fn undo_restores_a_diamond_cascade_with_two_required_parents() {
    let mut workspace = workspace();
    for (table, columns) in [
        ("Roots", vec![text("Name")]),
        ("Lefts", vec![integer("Root")]),
        ("Rights", vec![integer("Root")]),
        (
            "Leaves",
            vec![text("Name"), integer("Left"), integer("Right")],
        ),
    ] {
        workspace
            .create_table(&TableDraft {
                name: table.into(),
                columns,
            })
            .expect("table");
    }
    for (name, parent, child, column) in [
        ("A Lefts", "Roots", "Lefts", "Root"),
        ("B Rights", "Roots", "Rights", "Root"),
        ("Left Leaves", "Lefts", "Leaves", "Left"),
        ("Right Leaves", "Rights", "Leaves", "Right"),
    ] {
        workspace
            .create_relation(&RelationDraft {
                name: name.into(),
                parent_table: parent.into(),
                child_table: child.into(),
                child_column: column.into(),
                on_delete: DeleteAction::Cascade,
            })
            .expect("relation");
    }
    let root = workspace
        .insert_row("Roots", &row(&[("Name", Value::from("root"))]))
        .expect("root");
    let left = workspace
        .insert_row("Lefts", &row(&[("Root", root.id.into())]))
        .expect("left");
    let right = workspace
        .insert_row("Rights", &row(&[("Root", root.id.into())]))
        .expect("right");
    let leaf = workspace
        .insert_row(
            "Leaves",
            &row(&[
                ("Name", Value::from("leaf")),
                ("Left", left.id.into()),
                ("Right", right.id.into()),
            ]),
        )
        .expect("leaf");

    workspace.delete_row("Roots", root.id).expect("delete");
    assert!(workspace.read_record("Leaves", leaf.id).is_err());
    workspace.undo().expect("undo diamond cascade");
    for (table, expected) in [
        ("Roots", &root),
        ("Lefts", &left),
        ("Rights", &right),
        ("Leaves", &leaf),
    ] {
        assert_eq!(
            &workspace
                .read_record(table, expected.id)
                .expect("restored row"),
            expected
        );
    }
    assert_eq!(
        workspace.search("leaf", 10).expect("search")[0].row_id,
        leaf.id
    );
    workspace.redo().expect("redo diamond cascade");
    assert!(workspace.search("leaf", 10).expect("search").is_empty());
    workspace.undo().expect("undo again");
    let report = workspace.integrity_check().expect("integrity");
    assert!(report.audit_chain);
    assert_eq!(report.foreign_key_violations, 0);
}

#[test]
fn a_diamond_cascade_decrements_a_lookup_only_once() {
    let mut workspace = workspace();
    workspace
        .create_table(&TableDraft {
            name: "Companies".into(),
            columns: vec![text("Name")],
        })
        .expect("companies");
    workspace
        .create_table(&TableDraft {
            name: "People".into(),
            columns: vec![text("Name")],
        })
        .expect("people");
    workspace
        .create_table(&TableDraft {
            name: "Projects".into(),
            columns: vec![
                text("Name"),
                integer("Company"),
                integer("Lead"),
                integer("Sponsor"),
            ],
        })
        .expect("projects");
    workspace
        .create_relation(&RelationDraft {
            name: "Company Projects".into(),
            parent_table: "Companies".into(),
            child_table: "Projects".into(),
            child_column: "Company".into(),
            on_delete: DeleteAction::Restrict,
        })
        .expect("company relation");
    for (name, column) in [("Lead Projects", "Lead"), ("Sponsor Projects", "Sponsor")] {
        workspace
            .create_relation(&RelationDraft {
                name: name.into(),
                parent_table: "People".into(),
                child_table: "Projects".into(),
                child_column: column.into(),
                on_delete: DeleteAction::Cascade,
            })
            .expect("person relation");
    }
    workspace
        .add_column(
            "Companies",
            &ColumnDraft {
                name: "Projects".into(),
                ty: ColumnType::Lookup {
                    relation: "Company Projects".into(),
                    column: "Company".into(),
                    aggregate: Aggregate::Count,
                },
                required: false,
                unique: false,
                default: None,
            },
        )
        .expect("lookup");

    let company = workspace
        .insert_row(
            "Companies",
            &row(&[("Name", Value::Text("Northlight".into()))]),
        )
        .expect("company");
    let person = workspace
        .insert_row("People", &row(&[("Name", Value::Text("Ada".into()))]))
        .expect("person");
    workspace
        .insert_row(
            "Projects",
            &row(&[
                ("Name", Value::Text("Atlas".into())),
                ("Company", Value::Integer(company.id)),
                ("Lead", Value::Integer(person.id)),
                ("Sponsor", Value::Integer(person.id)),
            ]),
        )
        .expect("diamond project");
    for name in ["Borealis", "Cinder"] {
        workspace
            .insert_row(
                "Projects",
                &row(&[
                    ("Name", Value::Text(name.into())),
                    ("Company", Value::Integer(company.id)),
                ]),
            )
            .expect("other project");
    }
    assert_eq!(
        workspace
            .read_record("Companies", company.id)
            .expect("warm read")
            .get("Projects"),
        Some(&Value::Integer(3))
    );

    workspace.delete_row("People", person.id).expect("delete");
    assert_eq!(
        workspace
            .read_record("Companies", company.id)
            .expect("read after delete")
            .get("Projects"),
        Some(&Value::Integer(2)),
        "the duplicated cascade captured the same child twice and double-decremented the lookup"
    );
    workspace
        .undo()
        .expect("undo must restore the cascaded project");
    assert_eq!(
        workspace
            .read_record("Companies", company.id)
            .expect("read after undo")
            .get("Projects"),
        Some(&Value::Integer(3))
    );
}

#[test]
fn insert_audit_summary_names_the_assigned_row() {
    let mut workspace = workspace();
    workspace
        .create_table(&TableDraft {
            name: "Notes".into(),
            columns: vec![text("Body")],
        })
        .expect("table");
    let note = workspace
        .insert_row("Notes", &row(&[("Body", Value::Text("hello".into()))]))
        .expect("insert");
    let audit = workspace.audit_entries(10, 0).expect("audit");
    let insert = audit
        .iter()
        .find(|entry| entry.operation == "insert")
        .expect("insert entry");
    assert_eq!(insert.row_id, Some(note.id));
    assert_eq!(
        insert.summary,
        format!("Insert Notes #{}", note.id),
        "audit summary names the wrong row id"
    );
}

#[test]
fn undo_of_a_column_rename_restores_the_form_timestamp() {
    let mut workspace = workspace();
    workspace
        .create_table(&TableDraft {
            name: "Clients".into(),
            columns: vec![text("Name")],
        })
        .expect("table");
    let saved = workspace
        .save_layout(Layout {
            id: String::new(),
            name: "Client".into(),
            table: "Clients".into(),
            grid: GridSpec::default(),
            items: vec![LayoutItem {
                id: "field".into(),
                kind: LayoutItemKind::Field {
                    column: "Name".into(),
                },
                x: 0,
                y: 0,
                w: 6,
                h: 2,
            }],
            updated_at: 0,
        })
        .expect("layout");
    workspace
        .rename_column("Clients", "Name", "Full Name")
        .expect("rename");
    let renamed = workspace
        .schema()
        .expect("schema")
        .layouts
        .into_iter()
        .find(|layout| layout.id == saved.id)
        .expect("layout survives");
    assert_eq!(
        renamed.updated_at, saved.updated_at,
        "rename changed the form's updated_at timestamp"
    );
    workspace.undo().expect("undo");
    let after = workspace
        .schema()
        .expect("schema")
        .layouts
        .into_iter()
        .find(|layout| layout.id == saved.id)
        .expect("layout survives");
    assert_eq!(
        after.updated_at, saved.updated_at,
        "undo changed the form's updated_at timestamp"
    );
}

#[test]
fn a_foreign_database_with_a_meta_table_is_not_mutated() {
    let directory = tempfile::TempDir::new().expect("tempdir");
    let path = directory.path().join("foreign.slab");
    let connection = Connection::open(&path).expect("raw create");
    connection
        .execute_batch(
            "CREATE TABLE _slab_meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
             CREATE TABLE Clients (id INTEGER PRIMARY KEY AUTOINCREMENT, Name TEXT);
             INSERT INTO Clients (Name) VALUES ('Northlight');
             INSERT INTO _slab_meta (key, value) VALUES ('name', 'Someone Else');",
        )
        .expect("seed foreign database");
    drop(connection);
    let before = std::fs::read(&path).expect("read before");

    let outcome = Workspace::open(&path, Box::new(TestClock::new()));
    assert!(
        outcome.is_err(),
        "a database without SlabBase metadata must be rejected"
    );
    let after = std::fs::read(&path).expect("read after");
    assert_eq!(
        before, after,
        "a rejected open rewrote a foreign database byte-for-byte"
    );
}

fn invoice_table() -> TableDraft {
    TableDraft {
        name: "Invoices".into(),
        columns: vec![
            ColumnDraft {
                name: "Amount".into(),
                ty: ColumnType::Decimal { scale: 2 },
                required: false,
                unique: false,
                default: None,
            },
            ColumnDraft {
                name: "Paid".into(),
                ty: ColumnType::Boolean,
                required: false,
                unique: false,
                default: None,
            },
            ColumnDraft {
                name: "Whole".into(),
                ty: ColumnType::Decimal { scale: 0 },
                required: false,
                unique: false,
                default: None,
            },
            ColumnDraft {
                name: "Price".into(),
                ty: ColumnType::Currency {
                    currency: slab_core::CurrencyCode::parse("USD").expect("usd"),
                },
                required: false,
                unique: false,
                default: None,
            },
            ColumnDraft {
                name: "When".into(),
                ty: ColumnType::Timestamp,
                required: false,
                unique: false,
                default: None,
            },
        ],
    }
}

fn invoice_workspace() -> Workspace {
    let mut workspace = workspace();
    workspace.create_table(&invoice_table()).expect("table");
    let rows = [
        (
            "12.34",
            Some(true),
            "42",
            12_345,
            "2026-11-25T09:12:44.017Z",
        ),
        ("-12.34", None, "0", 0, "2026-01-01T00:00:00.000Z"),
        (
            "-92233720368547758.08",
            Some(false),
            "-7",
            -5,
            "2026-12-31T23:59:59.999Z",
        ),
    ];
    for (amount, paid, whole, price, when) in rows {
        let mut values = row(&[
            (
                "Amount",
                Value::Decimal(slab_core::Decimal::parse(amount).expect("amount")),
            ),
            (
                "Whole",
                Value::Decimal(slab_core::Decimal::parse(whole).expect("whole")),
            ),
            (
                "Price",
                Value::Currency(
                    slab_core::Money::from_minor(
                        slab_core::CurrencyCode::parse("USD").expect("usd"),
                        price,
                    )
                    .expect("money"),
                ),
            ),
            (
                "When",
                Value::Timestamp(slab_core::SlabTimestamp::parse(when).expect("timestamp")),
            ),
        ]);
        values.insert("Paid".into(), paid.map_or(Value::Null, Value::Boolean));
        workspace.insert_row("Invoices", &values).expect("insert");
    }
    workspace
}

fn count(workspace: &Workspace, column: &str, op: FilterOp, text: &str) -> usize {
    workspace
        .page(&PageQuery {
            table: "Invoices".into(),
            limit: 50,
            filters: vec![Filter {
                column: column.into(),
                op,
                value: Some(Value::Text(text.into())),
            }],
            ..PageQuery::default()
        })
        .expect("page")
        .rows
        .len()
}

#[test]
fn filtering_matches_the_canonical_text_for_typed_columns() {
    let workspace = invoice_workspace();
    assert_eq!(count(&workspace, "Amount", FilterOp::Contains, "12.34"), 2);
    assert_eq!(count(&workspace, "Amount", FilterOp::Contains, "-12.34"), 1);
    assert_eq!(
        count(
            &workspace,
            "Amount",
            FilterOp::Contains,
            "-92233720368547758.08"
        ),
        1,
        "the minimum decimal must not make SQLite's abs() overflow"
    );
    assert_eq!(count(&workspace, "Whole", FilterOp::Contains, "42"), 1);
    assert_eq!(count(&workspace, "Whole", FilterOp::Contains, "-7"), 1);
    assert_eq!(count(&workspace, "Price", FilterOp::Contains, "123.45"), 1);
    assert_eq!(
        count(&workspace, "Price", FilterOp::Contains, "USD"),
        0,
        "the grid shows the amount, not the currency code"
    );
    assert_eq!(count(&workspace, "When", FilterOp::Contains, "2026-"), 3);
    assert_eq!(count(&workspace, "When", FilterOp::Contains, "2026-11"), 1);
}

#[test]
fn filtering_booleans_does_not_match_null_as_yes() {
    let workspace = invoice_workspace();
    assert_eq!(count(&workspace, "Paid", FilterOp::Contains, "Yes"), 1);
    assert_eq!(count(&workspace, "Paid", FilterOp::Contains, "No"), 1);
    assert_eq!(count(&workspace, "Paid", FilterOp::StartsWith, "Y"), 1);
}

#[test]
fn typed_boolean_text_filters_match_the_grid_labels() {
    let workspace = invoice_workspace();
    for op in [FilterOp::Contains, FilterOp::StartsWith, FilterOp::EndsWith] {
        for paid in [true, false] {
            let page = workspace
                .page(&PageQuery {
                    table: "Invoices".into(),
                    limit: 10,
                    filters: vec![Filter {
                        column: "Paid".into(),
                        op,
                        value: Some(Value::Boolean(paid)),
                    }],
                    ..PageQuery::default()
                })
                .expect("page");
            assert_eq!(page.rows.len(), 1, "{op:?} {paid}");
            assert_eq!(page.rows[0].get("Paid"), Some(&Value::Boolean(paid)));
        }
    }
    assert_eq!(count(&workspace, "Paid", FilterOp::Contains, "true"), 0);
}

#[test]
fn text_filters_fold_ascii_case_and_preserve_non_ascii_case() {
    let mut workspace = workspace();
    workspace
        .create_table(&TableDraft {
            name: "Names".into(),
            columns: vec![text("Name")],
        })
        .expect("table");
    let record = workspace
        .insert_row("Names", &row(&[("Name", Value::from("Élan"))]))
        .expect("insert");

    for (op, matching, non_matching) in [
        (FilterOp::Contains, "ÉL", "él"),
        (FilterOp::StartsWith, "ÉL", "él"),
        (FilterOp::EndsWith, "ÉLAN", "élan"),
    ] {
        let results = |needle: &str| {
            workspace
                .page(&PageQuery {
                    filters: vec![Filter {
                        column: "Name".into(),
                        op,
                        value: Some(Value::from(needle)),
                    }],
                    ..PageQuery::first("Names", 10)
                })
                .expect("page")
                .rows
        };
        assert_eq!(results(matching), vec![record.clone()], "{op:?}");
        assert!(results(non_matching).is_empty(), "{op:?}");
    }
}

#[test]
fn equality_filters_parse_displayed_text_or_reject_it() {
    let workspace = invoice_workspace();
    for (column, text) in [
        ("Amount", "12.34"),
        ("Whole", "42"),
        ("Paid", "Yes"),
        ("Price", "USD 123.45"),
        ("When", "2026-11-25T09:12:44.017Z"),
    ] {
        assert_eq!(
            count(&workspace, column, FilterOp::Eq, text),
            1,
            "eq filter for {column} {text:?}"
        );
    }
    assert_eq!(count(&workspace, "Paid", FilterOp::NotEq, "Yes"), 2);
    let error = workspace
        .page(&PageQuery {
            table: "Invoices".into(),
            limit: 10,
            filters: vec![Filter {
                column: "Amount".into(),
                op: FilterOp::Eq,
                value: Some(Value::Text("not-a-number".into())),
            }],
            ..PageQuery::default()
        })
        .expect_err("a malformed numeric filter must not silently return no rows");
    assert!(error.to_string().contains("does not fit"));
}

struct DenySecret;

impl slab_core::PolicyHook for DenySecret {
    fn can_read_table(&self, table: &str) -> bool {
        table != "Secret"
    }
}

#[test]
fn insert_recovers_the_writer_then_checks_policy_before_validation() {
    let mut workspace = workspace();
    workspace
        .create_table(&TableDraft {
            name: "Secret".into(),
            columns: vec![text("Name")],
        })
        .expect("table");
    let invalid = row(&[("Unknown column", Value::from("invalid"))]);
    workspace.begin_import().expect("begin import");
    workspace
        .insert_import_rows("Secret", &[row(&[("Name", Value::from("pending"))])])
        .expect("import row");
    workspace.set_policy(std::sync::Arc::new(DenySecret));
    let error = workspace
        .insert_row("Secret", &invalid)
        .expect_err("the writer was busy");
    assert!(
        matches!(error, StorageError::Command(ref message) if message.contains("rolled back")),
        "unexpected error: {error}"
    );
    let error = workspace
        .insert_row("Secret", &invalid)
        .expect_err("table is denied");
    assert!(
        matches!(error, StorageError::Query(ref message) if message.contains("policy")),
        "an invalid insert exposed the schema of a denied table: {error}"
    );
    workspace.set_policy(std::sync::Arc::new(slab_core::NoopPolicy));
    assert!(
        workspace
            .page(&PageQuery::first("Secret", 10))
            .expect("page")
            .rows
            .is_empty()
    );
    workspace.begin_import().expect("writer is available");
    workspace.rollback_import().expect("rollback");
}

fn create_cascade_policy_tables(workspace: &mut Workspace) {
    for (table, columns) in [
        ("Public", vec![text("Name")]),
        ("Middle", vec![integer("Public")]),
        ("Secret", vec![text("Name"), integer("Middle")]),
    ] {
        workspace
            .create_table(&TableDraft {
                name: table.into(),
                columns,
            })
            .expect("table");
    }
    for (parent, child) in [("Public", "Middle"), ("Middle", "Secret")] {
        workspace
            .create_relation(&RelationDraft {
                name: format!("{parent} {child}"),
                parent_table: parent.into(),
                child_table: child.into(),
                child_column: parent.into(),
                on_delete: DeleteAction::Cascade,
            })
            .expect("relation");
    }
}

#[test]
fn cascade_delete_authorizes_actual_descendants_before_mutating() {
    let mut workspace = workspace();
    create_cascade_policy_tables(&mut workspace);
    let empty = workspace
        .insert_row("Public", &row(&[("Name", Value::from("empty"))]))
        .expect("empty parent");
    let parent = workspace
        .insert_row("Public", &row(&[("Name", Value::from("parent"))]))
        .expect("parent");
    let middle = workspace
        .insert_row("Middle", &row(&[("Public", parent.id.into())]))
        .expect("middle");
    let secret = workspace
        .insert_row(
            "Secret",
            &row(&[
                ("Name", Value::from("needle")),
                ("Middle", middle.id.into()),
            ]),
        )
        .expect("secret");
    let audit_before = workspace.audit_entries(100, 0).expect("audit").len();
    let undo_before = workspace.undo_state().depth;

    workspace.set_policy(std::sync::Arc::new(DenySecret));
    let error = workspace
        .delete_row("Public", parent.id)
        .expect_err("the actual cascade reaches a denied table");
    assert!(matches!(error, StorageError::Query(ref message) if message.contains("policy")));
    assert_eq!(workspace.undo_state().depth, undo_before);
    workspace.set_policy(std::sync::Arc::new(slab_core::NoopPolicy));
    assert_eq!(
        workspace.audit_entries(100, 0).expect("audit").len(),
        audit_before
    );
    assert_eq!(
        workspace.read_record("Public", parent.id).expect("parent"),
        parent
    );
    assert_eq!(
        workspace.read_record("Middle", middle.id).expect("middle"),
        middle
    );
    assert_eq!(
        workspace.read_record("Secret", secret.id).expect("secret"),
        secret
    );
    assert_eq!(workspace.search("needle", 10).expect("search").len(), 1);
    assert!(workspace.integrity_check().expect("integrity").audit_chain);

    workspace.set_policy(std::sync::Arc::new(DenySecret));
    workspace
        .delete_row("Public", empty.id)
        .expect("an empty cascade reaches no denied rows");
}

#[test]
fn redo_authorizes_cascade_descendants_added_since_the_original_delete() {
    let directory = tempfile::TempDir::new().expect("tempdir");
    let path = directory.path().join("policy-cascade.slab");
    let mut workspace =
        Workspace::create(&path, "Cascade", Box::new(TestClock::new())).expect("workspace");
    create_cascade_policy_tables(&mut workspace);
    let parent = workspace
        .insert_row("Public", &row(&[("Name", Value::from("parent"))]))
        .expect("parent");
    let middle = workspace
        .insert_row("Middle", &row(&[("Public", parent.id.into())]))
        .expect("middle");
    workspace.delete_row("Public", parent.id).expect("delete");
    workspace.undo().expect("undo");
    let conn = Connection::open(&path).expect("open external writer");
    conn.execute(
        "INSERT INTO Secret (Name, Middle) VALUES ('external', ?1)",
        [middle.id],
    )
    .expect("add a descendant");
    let child_id = conn.last_insert_rowid();
    drop(conn);

    workspace.set_policy(std::sync::Arc::new(DenySecret));
    let error = workspace.redo().expect_err("redo reaches a denied table");
    assert!(matches!(error, StorageError::Query(ref message) if message.contains("policy")));
    assert!(workspace.undo_state().can_redo);

    workspace.set_policy(std::sync::Arc::new(slab_core::NoopPolicy));
    assert!(workspace.read_record("Secret", child_id).is_ok());
    workspace.redo().expect("authorized redo");
    workspace.undo().expect("restore the actual redo cascade");
    assert_eq!(
        workspace
            .read_record("Secret", child_id)
            .expect("child")
            .get("Name"),
        Some(&Value::from("external"))
    );
    assert!(workspace.integrity_check().expect("integrity").audit_chain);
}

#[test]
fn a_denied_table_cannot_be_written_through_the_readback_path() {
    let mut workspace = workspace();
    workspace
        .create_table(&TableDraft {
            name: "Secret".into(),
            columns: vec![text("Name")],
        })
        .expect("table");
    let original = workspace
        .insert_row(
            "Secret",
            &row(&[("Name", Value::Text("needle secret".into()))]),
        )
        .expect("seed row");
    let audit_before = workspace.audit_entries(50, 0).expect("audit").len();
    let insert_audit = workspace
        .audit_entries(50, 0)
        .expect("audit")
        .into_iter()
        .find(|entry| entry.operation == "insert" && entry.table.as_deref() == Some("Secret"))
        .expect("insert audit")
        .id;

    workspace.set_policy(std::sync::Arc::new(DenySecret));
    assert!(
        workspace
            .insert_row("Secret", &row(&[("Name", Value::Text("after".into()))]))
            .is_err()
    );
    assert!(
        workspace
            .update_row(
                "Secret",
                original.id,
                &row(&[("Name", Value::Text("changed".into()))])
            )
            .is_err()
    );
    assert!(workspace.delete_row("Secret", original.id).is_err());
    assert!(workspace.record_history("Secret", original.id).is_err());
    assert!(
        workspace
            .restore_record_revision("Secret", original.id, insert_audit)
            .is_err()
    );
    let denied = workspace
        .update_row(
            "Secret",
            999_999,
            &row(&[("Name", Value::Text("missing".into()))]),
        )
        .expect_err("a denied update must fail before the row lookup");
    assert!(
        matches!(denied, StorageError::Query(ref message) if message.contains("policy")),
        "a denied update leaked whether the row exists: {denied}"
    );
    assert!(
        workspace
            .search("needle", 10)
            .expect("search")
            .iter()
            .all(|hit| hit.table != "Secret"),
        "search leaked a denied table"
    );
    assert!(
        workspace
            .audit_entries(50, 0)
            .expect("audit")
            .iter()
            .all(|entry| entry.table.as_deref() != Some("Secret")),
        "audit list leaked a denied table"
    );
    let directory = tempfile::TempDir::new().expect("tempdir");
    let export = directory.path().join("denied.sql");
    assert!(workspace.export_sql(&export).is_err());
    assert!(!export.exists(), "a denied export created a file");

    workspace.set_policy(std::sync::Arc::new(slab_core::NoopPolicy));
    assert_eq!(
        workspace.audit_entries(50, 0).expect("audit").len(),
        audit_before,
        "a policy-denied write committed an audit entry"
    );
    assert_eq!(
        workspace
            .read_record("Secret", original.id)
            .expect("read")
            .get("Name"),
        Some(&Value::Text("needle secret".into()))
    );
}

struct RedactSecret;

impl slab_core::PolicyHook for RedactSecret {
    fn redact(&self, table: &str, column: &str, _value: &Value) -> Option<Value> {
        (table == "Secret" && column == "Name").then(|| Value::Text("[redacted]".into()))
    }
}

#[test]
fn history_and_sql_export_apply_field_redaction() {
    let mut workspace = workspace();
    workspace
        .create_table(&TableDraft {
            name: "Secret".into(),
            columns: vec![text("Name")],
        })
        .expect("table");
    let secret = workspace
        .insert_row(
            "Secret",
            &row(&[("Name", Value::Text("needle secret".into()))]),
        )
        .expect("insert");
    workspace.set_policy(std::sync::Arc::new(RedactSecret));

    let revisions = workspace
        .record_history("Secret", secret.id)
        .expect("history");
    let values: Vec<&Value> = revisions
        .iter()
        .flat_map(|revision| &revision.diffs)
        .filter_map(|diff| diff.after.as_ref())
        .collect();
    assert!(
        values
            .iter()
            .all(|value| **value == Value::Text("[redacted]".into())),
        "history leaked a redacted value: {values:?}"
    );

    let mut dump = Vec::new();
    workspace.export_sql_to(&mut dump).expect("sql export");
    let dump = String::from_utf8(dump).expect("utf8 dump");
    assert!(dump.contains("[redacted]"));
    assert!(
        dump.contains("INSERT INTO \"Secret\" (\"id\", \"Name\")"),
        "the SQL export must keep the physical row id"
    );
    assert!(
        !dump.contains("needle secret"),
        "SQL export leaked a redacted value"
    );
}

struct WrongRedact;

impl slab_core::PolicyHook for WrongRedact {
    fn redact(&self, _table: &str, _column: &str, _value: &Value) -> Option<Value> {
        Some(Value::Text("[redacted]".into()))
    }
}

#[test]
fn sql_export_rejects_an_incompatible_redaction() {
    let mut workspace = workspace();
    workspace
        .create_table(&TableDraft {
            name: "Invoices".into(),
            columns: vec![ColumnDraft {
                name: "Amount".into(),
                ty: ColumnType::Decimal { scale: 2 },
                required: false,
                unique: false,
                default: None,
            }],
        })
        .expect("table");
    workspace
        .insert_row(
            "Invoices",
            &row(&[(
                "Amount",
                Value::Decimal(slab_core::Decimal::parse("12.30").expect("amount")),
            )]),
        )
        .expect("insert");
    workspace.set_policy(std::sync::Arc::new(WrongRedact));
    let error = workspace
        .export_sql_to(Vec::new())
        .expect_err("an unreadable redaction must not publish a dump");
    assert!(
        error.to_string().contains("does not fit column"),
        "unexpected error: {error}"
    );
}

#[test]
fn undo_does_not_mutate_a_table_denied_by_policy() {
    let mut workspace = workspace();
    workspace
        .create_table(&TableDraft {
            name: "Secret".into(),
            columns: vec![text("Name")],
        })
        .expect("table");
    workspace
        .insert_row("Secret", &row(&[("Name", Value::Text("secret".into()))]))
        .expect("insert");
    workspace.set_policy(std::sync::Arc::new(DenySecret));

    let denied = workspace.undo().expect_err("undo must respect the policy");
    assert!(
        matches!(denied, StorageError::Query(ref message) if message.contains("policy")),
        "undo bypassed the policy: {denied}"
    );
    workspace.set_policy(std::sync::Arc::new(slab_core::NoopPolicy));
    workspace.undo().expect("undo after the policy is lifted");
    assert!(
        workspace
            .page(&PageQuery {
                table: "Secret".into(),
                limit: 10,
                ..PageQuery::default()
            })
            .expect("page")
            .rows
            .is_empty(),
        "undo did not remove the inserted row"
    );
}

#[test]
fn undo_recovers_an_abandoned_import_before_checking_policy() {
    let mut workspace = workspace();
    workspace
        .create_table(&TableDraft {
            name: "Secret".into(),
            columns: vec![text("Name")],
        })
        .expect("table");
    workspace
        .insert_row("Secret", &row(&[("Name", Value::Text("secret".into()))]))
        .expect("insert");
    workspace.begin_import().expect("begin import");
    workspace.set_policy(std::sync::Arc::new(DenySecret));

    let error = workspace
        .undo()
        .expect_err("undo must refuse the open import");
    assert!(
        error
            .to_string()
            .contains("abandoned transaction was rolled back"),
        "undo checked policy before recovering the writer: {error}"
    );
    workspace.set_policy(std::sync::Arc::new(slab_core::NoopPolicy));
    workspace
        .begin_import()
        .expect("the writer is usable again");
    workspace.rollback_import().expect("rollback");
}

#[test]
fn search_does_not_lose_authorized_hits_behind_denied_ones() {
    let mut workspace = workspace();
    for table in ["Secret", "Public"] {
        workspace
            .create_table(&TableDraft {
                name: table.into(),
                columns: vec![text("Name")],
            })
            .expect("table");
    }
    for index in 0..250 {
        workspace
            .insert_row(
                "Secret",
                &row(&[("Name", Value::Text(format!("needle {index}")))]),
            )
            .expect("secret insert");
    }
    workspace
        .insert_row(
            "Public",
            &row(&[(
                "Name",
                Value::Text(format!("needle {}", "filler ".repeat(2_000))),
            )]),
        )
        .expect("public insert");
    workspace.set_policy(std::sync::Arc::new(DenySecret));

    let hits = workspace.search("needle", 200).expect("search");
    assert!(
        hits.iter().any(|hit| hit.table == "Public"),
        "an authorized hit beyond the denied prefix was dropped"
    );
}

#[test]
fn extreme_timestamps_match_the_clamped_display() {
    let mut workspace = workspace();
    workspace
        .create_table(&TableDraft {
            name: "Events".into(),
            columns: vec![ColumnDraft {
                name: "When".into(),
                ty: ColumnType::Timestamp,
                required: false,
                unique: false,
                default: None,
            }],
        })
        .expect("table");
    workspace
        .insert_row(
            "Events",
            &row(&[(
                "When",
                Value::Timestamp(slab_core::SlabTimestamp::from_millis(i64::MAX)),
            )]),
        )
        .expect("insert");
    let page = workspace
        .page(&PageQuery {
            table: "Events".into(),
            limit: 10,
            filters: vec![Filter {
                column: "When".into(),
                op: FilterOp::Contains,
                value: Some(Value::Text("9999-12-31".into())),
            }],
            ..PageQuery::default()
        })
        .expect("page");
    assert_eq!(page.rows.len(), 1);
}

#[test]
fn a_decimal_filter_rescales_to_the_column_scale() {
    let mut workspace = workspace();
    workspace
        .create_table(&TableDraft {
            name: "Invoices".into(),
            columns: vec![ColumnDraft {
                name: "Amount".into(),
                ty: ColumnType::Decimal { scale: 2 },
                required: false,
                unique: false,
                default: None,
            }],
        })
        .expect("table");
    for amount in ["12.30", "1.23", "12.31"] {
        workspace
            .insert_row(
                "Invoices",
                &row(&[(
                    "Amount",
                    Value::Decimal(slab_core::Decimal::parse(amount).expect("amount")),
                )]),
            )
            .expect("insert");
    }
    for (op, text, expected) in [
        (FilterOp::Eq, "12.3", 1),
        (FilterOp::Eq, "12", 0),
        (FilterOp::Gt, "12.3", 1),
        (FilterOp::Lt, "12.3", 1),
    ] {
        assert_eq!(
            count(&workspace, "Amount", op, text),
            expected,
            "{op:?} {text:?}"
        );
    }
    // A sub-scale bound must be rejected, not rounded into a different set.
    for op in [FilterOp::Eq, FilterOp::Lt, FilterOp::Gt] {
        let outcome = workspace.page(&PageQuery {
            table: "Invoices".into(),
            limit: 10,
            filters: vec![Filter {
                column: "Amount".into(),
                op,
                value: Some(Value::Text("12.304".into())),
            }],
            ..PageQuery::default()
        });
        assert!(outcome.is_err(), "{op:?} accepted a sub-scale bound");
    }
}

#[test]
fn an_attachment_formula_compares_against_text() {
    let mut workspace = workspace();
    workspace
        .create_table(&TableDraft {
            name: "Files".into(),
            columns: vec![
                ColumnDraft {
                    name: "Path".into(),
                    ty: ColumnType::AttachmentRef,
                    required: false,
                    unique: false,
                    default: None,
                },
                ColumnDraft {
                    name: "Matches".into(),
                    ty: ColumnType::Formula {
                        expression: "[Path] = \"/tmp/x\"".into(),
                    },
                    required: false,
                    unique: false,
                    default: None,
                },
            ],
        })
        .expect("table");
    let file = workspace
        .insert_row(
            "Files",
            &row(&[("Path", Value::AttachmentRef("/tmp/x".into()))]),
        )
        .expect("insert");
    assert_eq!(
        file.get("Matches"),
        Some(&Value::Boolean(true)),
        "attachment comparison regressed to false"
    );
}

#[test]
fn typed_decimal_filters_normalize_without_rounding() {
    let mut workspace = workspace();
    workspace
        .create_table(&TableDraft {
            name: "Invoices".into(),
            columns: vec![ColumnDraft {
                name: "Amount".into(),
                ty: ColumnType::Decimal { scale: 2 },
                required: false,
                unique: false,
                default: None,
            }],
        })
        .expect("table");
    workspace
        .insert_row(
            "Invoices",
            &row(&[(
                "Amount",
                Value::Decimal(slab_core::Decimal::parse("12.30").expect("amount")),
            )]),
        )
        .expect("insert");
    let filter = |value: Value| {
        workspace.page(&PageQuery {
            table: "Invoices".into(),
            limit: 10,
            filters: vec![Filter {
                column: "Amount".into(),
                op: FilterOp::Eq,
                value: Some(value),
            }],
            ..PageQuery::default()
        })
    };
    let exact = filter(Value::Decimal(
        slab_core::Decimal::parse("12.3").expect("exact"),
    ))
    .expect("typed filter");
    assert_eq!(exact.rows.len(), 1);
    assert!(
        filter(Value::Decimal(
            slab_core::Decimal::parse("12.304").expect("sub-scale")
        ))
        .is_err(),
        "a typed sub-scale filter must not round"
    );
}

#[test]
fn typed_currency_filters_normalize_without_rounding() {
    use slab_core::{CurrencyCode, Money};

    let workspace = invoice_workspace();
    let price = |value: Value| {
        workspace.page(&PageQuery {
            table: "Invoices".into(),
            limit: 50,
            filters: vec![Filter {
                column: "Price".into(),
                op: FilterOp::Eq,
                value: Some(value),
            }],
            ..PageQuery::default()
        })
    };
    let usd = CurrencyCode::parse("USD").expect("usd");
    let exact = price(Value::Currency(
        Money::from_minor_scaled(usd, 12345, 2).expect("exact"),
    ))
    .expect("typed currency filter");
    assert_eq!(exact.rows.len(), 1);
    assert!(
        price(Value::Currency(
            Money::from_minor_scaled(usd, 12344, 3).expect("sub-scale")
        ))
        .is_err(),
        "a typed sub-scale currency filter must not round"
    );
}
