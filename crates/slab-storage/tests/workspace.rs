#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod support;

use slab_core::{
    ColumnDraft, ColumnType, DeleteAction, Filter, FilterOp, Layout, LayoutItem, LayoutItemKind,
    PageQuery, RelationDraft, RowData, SortDirection, SortSpec, TableDraft, Value,
};
use slab_storage::Workspace;
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

fn required_text(name: &str) -> ColumnDraft {
    ColumnDraft {
        required: true,
        ..text(name)
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

fn required_integer(name: &str) -> ColumnDraft {
    ColumnDraft {
        required: true,
        ..integer(name)
    }
}

fn workspace() -> Workspace {
    Workspace::open_in_memory("Test", Box::new(TestClock::new())).expect("workspace")
}

fn row(entries: &[(&str, Value)]) -> RowData {
    entries
        .iter()
        .map(|(name, value)| ((*name).to_owned(), value.clone()))
        .collect()
}

#[test]
fn writes_are_audited_and_undo_redo_round_trip() {
    let mut workspace = workspace();
    workspace
        .create_table(&TableDraft {
            name: "Clients".into(),
            columns: vec![required_text("Name"), text("City")],
        })
        .expect("table");
    let client = workspace
        .insert_row(
            "Clients",
            &row(&[("Name", Value::Text("Northlight".into()))]),
        )
        .expect("insert");
    let audit = workspace.audit_entries(10, 0).expect("audit");
    assert_eq!(audit.len(), 2);
    assert_eq!(audit[0].operation, "insert");
    assert_eq!(audit[0].table.as_deref(), Some("Clients"));

    workspace
        .update_row(
            "Clients",
            client.id,
            &row(&[("City", Value::Text("San Francisco".into()))]),
        )
        .expect("update");
    let state = workspace.undo_state();
    assert!(state.can_undo);
    assert_eq!(state.undo_label.as_deref(), Some("Update Clients #1"));

    workspace.undo().expect("undo");
    let restored = workspace.read_record("Clients", client.id).expect("read");
    assert_eq!(restored.get("City"), Some(&Value::Null));
    workspace.redo().expect("redo");
    let restored = workspace.read_record("Clients", client.id).expect("read");
    assert_eq!(
        restored.get("City"),
        Some(&Value::Text("San Francisco".into()))
    );
}

fn portal_workspace() -> Workspace {
    let mut workspace = workspace();
    workspace
        .create_table(&TableDraft {
            name: "Invoices".into(),
            columns: vec![required_text("Number")],
        })
        .expect("invoices");
    workspace
        .create_table(&TableDraft {
            name: "Line Items".into(),
            columns: vec![
                required_text("Description"),
                required_integer("Invoice"),
                integer("Qty"),
            ],
        })
        .expect("lines");
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
            &ColumnDraft {
                name: "Total".into(),
                ty: ColumnType::Formula {
                    expression: "SUM([Invoice Lines].[Qty])".into(),
                },
                required: false,
                unique: false,
                default: None,
            },
        )
        .expect("formula");
    workspace.clear_history();
    workspace
}

#[test]
fn undo_redo_parity_across_portal_edits() {
    let mut workspace = portal_workspace();
    let invoice = workspace
        .insert_row("Invoices", &row(&[("Number", Value::Text("INV-1".into()))]))
        .expect("invoice");
    let line_a = workspace
        .insert_row(
            "Line Items",
            &row(&[
                ("Description", Value::Text("Design".into())),
                ("Invoice", Value::Integer(invoice.id)),
                ("Qty", Value::Integer(2)),
            ]),
        )
        .expect("line a");
    workspace
        .insert_row(
            "Line Items",
            &row(&[
                ("Description", Value::Text("Proofing".into())),
                ("Invoice", Value::Integer(invoice.id)),
                ("Qty", Value::Integer(3)),
            ]),
        )
        .expect("line b");
    let before = workspace
        .bundle("Invoices", invoice.id, 100)
        .expect("bundle");
    assert_eq!(before.record.get("Total"), Some(&Value::Integer(5)));

    workspace
        .update_row(
            "Line Items",
            line_a.id,
            &row(&[("Qty", Value::Integer(10))]),
        )
        .expect("update");
    let after = workspace
        .bundle("Invoices", invoice.id, 100)
        .expect("bundle");
    assert_eq!(after.record.get("Total"), Some(&Value::Integer(13)));

    workspace.undo().expect("undo update");
    let undone = workspace
        .bundle("Invoices", invoice.id, 100)
        .expect("bundle");
    assert_eq!(undone, before);
    workspace.redo().expect("redo update");
    let redone = workspace
        .bundle("Invoices", invoice.id, 100)
        .expect("bundle");
    assert_eq!(redone, after);

    workspace.clear_history();
    workspace
        .delete_row("Invoices", invoice.id)
        .expect("delete invoice");
    let empty = workspace.bundle("Invoices", invoice.id, 100);
    assert!(empty.is_err());
    workspace.undo().expect("undo delete");
    let restored = workspace
        .bundle("Invoices", invoice.id, 100)
        .expect("bundle");
    assert_eq!(restored, redone);
    assert_eq!(restored.portal("Invoice Lines").len(), 2);
}

#[test]
fn keyset_pagination_orders_nulls_last_and_never_duplicates() {
    let mut workspace = workspace();
    workspace
        .create_table(&TableDraft {
            name: "Items".into(),
            columns: vec![required_text("Name"), integer("Rank")],
        })
        .expect("table");
    let ranks = [Some(5), None, Some(1), Some(3), None, Some(2), Some(4)];
    for (index, rank) in ranks.iter().enumerate() {
        workspace
            .insert_row(
                "Items",
                &row(&[
                    ("Name", Value::Text(format!("Item {index}"))),
                    (
                        "Rank",
                        rank.map_or(Value::Null, |value| Value::Integer(i64::from(value))),
                    ),
                ]),
            )
            .expect("insert");
    }
    let asc = "Rank";
    let mut query = PageQuery {
        table: "Items".into(),
        limit: 2,
        sort: vec![SortSpec {
            column: asc.into(),
            direction: SortDirection::Asc,
        }],
        ..PageQuery::default()
    };
    let mut seen = Vec::new();
    let mut ranks_seen = Vec::new();
    loop {
        let page = workspace.page(&query).expect("page");
        for record in &page.rows {
            seen.push(record.id);
            ranks_seen.push(record.get("Rank").cloned().unwrap_or(Value::Null));
        }
        match page.next_cursor {
            Some(cursor) => query.cursor = Some(cursor),
            None => break,
        }
    }
    assert_eq!(seen.len(), 7);
    let unique: std::collections::BTreeSet<_> = seen.iter().collect();
    assert_eq!(unique.len(), 7);
    assert_eq!(
        ranks_seen,
        vec![
            Value::Integer(1),
            Value::Integer(2),
            Value::Integer(3),
            Value::Integer(4),
            Value::Integer(5),
            Value::Null,
            Value::Null
        ]
    );

    let filtered = PageQuery {
        table: "Items".into(),
        limit: 50,
        filters: vec![
            Filter {
                column: "Rank".into(),
                op: FilterOp::Gte,
                value: Some(Value::Integer(3)),
            },
            Filter {
                column: "Name".into(),
                op: FilterOp::Contains,
                value: Some(Value::Text("item".into())),
            },
        ],
        ..PageQuery::default()
    };
    let page = workspace.page(&filtered).expect("page");
    assert_eq!(page.rows.len(), 3);
}

#[test]
fn search_tracks_inserts_updates_and_deletes() {
    let mut workspace = workspace();
    workspace
        .create_table(&TableDraft {
            name: "Clients".into(),
            columns: vec![required_text("Name"), text("Notes")],
        })
        .expect("table");
    let client = workspace
        .insert_row(
            "Clients",
            &row(&[
                ("Name", Value::Text("Northlight Studio".into())),
                ("Notes", Value::Text("packaging and identity".into())),
            ]),
        )
        .expect("insert");
    let hits = workspace.search("northlight", 10).expect("search");
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].row_id, client.id);
    assert!(hits[0].snippet.contains("<mark>"));

    workspace
        .update_row(
            "Clients",
            client.id,
            &row(&[("Name", Value::Text("Hiraya".into()))]),
        )
        .expect("update");
    assert!(
        workspace
            .search("northlight", 10)
            .expect("search")
            .is_empty()
    );
    assert_eq!(workspace.search("hiraya", 10).expect("search").len(), 1);

    workspace.delete_row("Clients", client.id).expect("delete");
    assert!(workspace.search("hiraya", 10).expect("search").is_empty());
}

#[test]
fn cycles_are_rejected_before_any_schema_change() {
    let mut workspace = workspace();
    let error = workspace
        .create_table(&TableDraft {
            name: "Calc".into(),
            columns: vec![
                ColumnDraft {
                    name: "Base".into(),
                    ty: ColumnType::Integer,
                    required: false,
                    unique: false,
                    default: None,
                },
                ColumnDraft {
                    name: "A".into(),
                    ty: ColumnType::Formula {
                        expression: "[B] + 1".into(),
                    },
                    required: false,
                    unique: false,
                    default: None,
                },
                ColumnDraft {
                    name: "B".into(),
                    ty: ColumnType::Formula {
                        expression: "[A] + 1".into(),
                    },
                    required: false,
                    unique: false,
                    default: None,
                },
            ],
        })
        .expect_err("cycle");
    assert_eq!(error.code_str(), "SLAB_E_CYCLE");
    assert!(workspace.schema().expect("schema").tables.is_empty());
}

#[test]
fn layouts_round_trip_and_validate_references() {
    let mut workspace = workspace();
    workspace
        .create_table(&TableDraft {
            name: "Clients".into(),
            columns: vec![required_text("Name")],
        })
        .expect("table");
    let layout = Layout {
        id: String::new(),
        name: "Client".into(),
        table: "Clients".into(),
        grid: slab_core::GridSpec::default(),
        items: vec![LayoutItem {
            id: "name".into(),
            kind: LayoutItemKind::Field {
                column: "Name".into(),
            },
            x: 0,
            y: 0,
            w: 6,
            h: 2,
        }],
        updated_at: 0,
    };
    let saved = workspace.save_layout(layout).expect("save");
    assert!(!saved.id.is_empty());
    let schema = workspace.schema().expect("schema");
    assert_eq!(schema.layouts.len(), 1);
    assert_eq!(schema.layouts[0].items.len(), 1);
}

#[test]
fn backup_is_an_independent_valid_database() {
    let directory = tempfile::TempDir::new().expect("tempdir");
    let source_path = directory.path().join("studio.slab");
    let backup_path = directory.path().join("backup.slab");
    let mut workspace =
        Workspace::create(&source_path, "Studio", Box::new(TestClock::new())).expect("create");
    workspace
        .create_table(&TableDraft {
            name: "Clients".into(),
            columns: vec![required_text("Name")],
        })
        .expect("table");
    workspace
        .insert_row(
            "Clients",
            &row(&[("Name", Value::Text("Northlight".into()))]),
        )
        .expect("insert");
    workspace.backup_to(&backup_path).expect("backup");
    workspace.close().expect("close");

    let reopened = Workspace::open(&backup_path, Box::new(TestClock::new())).expect("open backup");
    let report = reopened.integrity_check().expect("integrity");
    assert_eq!(report.integrity, "ok");
    assert_eq!(report.foreign_key_violations, 0);
    assert_eq!(report.journal_mode.to_ascii_lowercase(), "wal");
    let page = reopened
        .page(&PageQuery::first("Clients", 10))
        .expect("page");
    assert_eq!(page.rows.len(), 1);
    let summaries = reopened.table_summaries().expect("summaries");
    let clients = summaries
        .iter()
        .find(|summary| summary.name == "Clients")
        .expect("clients");
    assert_eq!(clients.row_count, 1);
}

#[test]
fn foreign_keys_reject_orphans() {
    let mut workspace = workspace();
    workspace
        .create_table(&TableDraft {
            name: "Clients".into(),
            columns: vec![required_text("Name")],
        })
        .expect("clients");
    workspace
        .create_table(&TableDraft {
            name: "Projects".into(),
            columns: vec![required_text("Name"), required_integer("Client")],
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
    let error = workspace
        .insert_row(
            "Projects",
            &row(&[
                ("Name", Value::Text("Orphan".into())),
                ("Client", Value::Integer(999)),
            ]),
        )
        .expect_err("orphan");
    assert_eq!(error.code_str(), "SLAB_E_FK_VIOLATION");
}

#[test]
fn default_values_and_required_checks_are_applied() {
    let mut workspace = workspace();
    workspace
        .create_table(&TableDraft {
            name: "Tasks".into(),
            columns: vec![
                required_text("Title"),
                ColumnDraft {
                    name: "Done".into(),
                    ty: ColumnType::Boolean,
                    required: false,
                    unique: false,
                    default: Some(Value::Boolean(false)),
                },
            ],
        })
        .expect("table");
    let task = workspace
        .insert_row("Tasks", &row(&[("Title", Value::Text("Draft".into()))]))
        .expect("insert");
    assert_eq!(task.get("Done"), Some(&Value::Boolean(false)));
    let error = workspace
        .insert_row("Tasks", &row(&[("Done", Value::Boolean(true))]))
        .expect_err("required");
    assert_eq!(error.code_str(), "SLAB_E_TYPE");
}

#[test]
fn committed_sample_workspace_opens_with_live_rollups() {
    let source =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/design-studio.slab");
    assert!(source.exists(), "fixtures/design-studio.slab is missing");
    let directory = tempfile::TempDir::new().expect("tempdir");
    let copy = directory.path().join("studio.slab");
    std::fs::copy(&source, &copy).expect("copy fixture");
    let workspace = Workspace::open(&copy, Box::new(TestClock::new())).expect("open fixture");
    let report = workspace.integrity_check().expect("integrity");
    assert_eq!(report.integrity, "ok");
    assert_eq!(report.foreign_key_violations, 0);
    assert_eq!(report.journal_mode.to_ascii_lowercase(), "wal");

    let page = workspace
        .page(&PageQuery::first("Invoices", 10))
        .expect("invoices");
    assert_eq!(page.rows.len(), 4);
    let first = page.rows.first().expect("first invoice");
    assert_eq!(
        first.get("Number"),
        Some(&Value::Text("INV-2026-001".into()))
    );
    let total = first.get("Total").cloned().expect("total");
    let Value::Currency(money) = total else {
        panic!("Total should be currency, found {:?}", first.get("Total"));
    };
    assert_eq!(money.minor_units(), 2_186_650);
    let bundle = workspace
        .bundle("Invoices", first.id, 100)
        .expect("invoice bundle");
    assert_eq!(bundle.portal("Invoice Lines").len(), 3);
    let schema = workspace.schema().expect("schema");
    assert_eq!(schema.layouts.len(), 3);
}

#[test]
fn portals_paginate_one_thousand_two_hundred_children_without_truncation() {
    let mut workspace = workspace();
    workspace
        .create_table(&TableDraft {
            name: "Invoices".into(),
            columns: vec![required_text("Number")],
        })
        .expect("invoices");
    workspace
        .create_table(&TableDraft {
            name: "Line Items".into(),
            columns: vec![
                required_text("Description"),
                required_integer("Quantity"),
                ColumnDraft {
                    name: "Unit Price".into(),
                    ty: ColumnType::Currency {
                        currency: slab_core::CurrencyCode::from_static(*b"USD"),
                    },
                    required: false,
                    unique: false,
                    default: None,
                },
                required_integer("Invoice"),
                ColumnDraft {
                    name: "Amount".into(),
                    ty: ColumnType::Formula {
                        expression: "ROUND([Quantity] * [Unit Price], 2)".into(),
                    },
                    required: false,
                    unique: false,
                    default: None,
                },
            ],
        })
        .expect("lines");
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
            &ColumnDraft {
                name: "Subtotal".into(),
                ty: ColumnType::Formula {
                    expression: "SUM([Invoice Lines].[Amount])".into(),
                },
                required: false,
                unique: false,
                default: None,
            },
        )
        .expect("rollup");
    let invoice = workspace
        .insert_row(
            "Invoices",
            &row(&[("Number", Value::Text("INV-1200".into()))]),
        )
        .expect("invoice");
    let mut children = Vec::with_capacity(1_200);
    for index in 0..1_200 {
        children.push(row(&[
            ("Description", Value::Text(format!("Line {index}"))),
            ("Quantity", Value::Integer(1)),
            (
                "Unit Price",
                Value::Currency(
                    slab_core::Money::from_minor(
                        slab_core::CurrencyCode::from_static(*b"USD"),
                        100,
                    )
                    .expect("money"),
                ),
            ),
            ("Invoice", Value::Integer(invoice.id)),
        ]));
    }
    let inserted = workspace
        .insert_rows("Line Items", &children)
        .expect("batch insert");
    assert_eq!(inserted.len(), 1_200);

    let mut cursor = None;
    let mut ids = std::collections::BTreeSet::new();
    let mut pages = 0_u32;
    loop {
        let page = workspace
            .page_portal(&slab_core::PortalQuery {
                relation: "Invoice Lines".into(),
                parent_id: invoice.id,
                limit: 100,
                cursor,
            })
            .expect("portal page");
        assert_eq!(page.total_rows, 1_200);
        assert!(page.rows.len() <= 100);
        for child in &page.rows {
            assert!(ids.insert(child.id));
        }
        pages += 1;
        match page.next_cursor {
            Some(next) => cursor = Some(next),
            None => break,
        }
        assert!(pages < 20, "portal pagination did not terminate");
    }
    assert_eq!(pages, 12);
    assert_eq!(ids.len(), 1_200);
    let parent = workspace
        .read_record("Invoices", invoice.id)
        .expect("parent");
    assert_eq!(
        parent.get("Subtotal"),
        Some(&Value::Currency(
            slab_core::Money::from_minor(slab_core::CurrencyCode::from_static(*b"USD"), 120_000,)
                .expect("subtotal")
        ))
    );
}
