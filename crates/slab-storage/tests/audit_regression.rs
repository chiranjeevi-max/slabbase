//! Regression tests for defects found during the failure-mode audit.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod support;

use rusqlite::Connection;
use slab_core::{
    Aggregate, ColumnDraft, ColumnType, DeleteAction, Filter, FilterOp, GridSpec, Layout,
    LayoutItem, LayoutItemKind, PageQuery, RelationDraft, RowData, TableDraft, Value,
};
use slab_storage::Workspace;
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
