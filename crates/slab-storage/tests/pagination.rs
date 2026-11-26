#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod support;

use std::time::{Duration, Instant};

use std::collections::BTreeSet;

use slab_core::{
    Aggregate, ColumnDraft, ColumnType, DeleteAction, Filter, FilterOp, PageQuery, RelationDraft,
    RowData, SlabDate, SortDirection, SortSpec, TableDraft, Value,
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

fn integer(name: &str) -> ColumnDraft {
    ColumnDraft {
        name: name.into(),
        ty: ColumnType::Integer,
        required: false,
        unique: false,
        default: None,
    }
}

fn date(name: &str) -> ColumnDraft {
    ColumnDraft {
        name: name.into(),
        ty: ColumnType::Date,
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

fn walk(workspace: &Workspace, direction: SortDirection) -> (Vec<i64>, Vec<Value>) {
    let mut query = PageQuery {
        table: "People".into(),
        limit: 17,
        sort: vec![SortSpec {
            column: "Name".into(),
            direction,
        }],
        ..PageQuery::default()
    };
    let mut ids = Vec::new();
    let mut names = Vec::new();
    loop {
        let page = workspace.page(&query).expect("page");
        for record in &page.rows {
            ids.push(record.id);
            names.push(record.get("Name").cloned().unwrap_or(Value::Null));
        }
        assert!(ids.len() <= 1_000, "pagination revisited rows");
        match page.next_cursor {
            Some(cursor) => query.cursor = Some(cursor),
            None => break,
        }
    }
    (ids, names)
}

#[test]
fn keyset_pagination_stays_fast_at_one_hundred_thousand_rows() {
    let directory = tempfile::TempDir::new().expect("tempdir");
    let path = directory.path().join("bench.slab");
    let mut workspace =
        Workspace::create(&path, "Bench", Box::new(TestClock::new())).expect("create");
    workspace
        .create_table(&TableDraft {
            name: "Bench".into(),
            columns: vec![text("Name"), integer("Amount")],
        })
        .expect("table");
    workspace.close().expect("close");

    // Load the table through an ordinary SQLite connection: this both seeds the
    // benchmark and demonstrates that a workspace is a plain SQLite database.
    {
        let mut connection = rusqlite::Connection::open(&path).expect("open raw");
        let transaction = connection.transaction().expect("transaction");
        {
            let mut insert = transaction
                .prepare("INSERT INTO \"Bench\" (\"Name\", \"Amount\") VALUES (?1, ?2)")
                .expect("prepare");
            for index in 0..100_000_i64 {
                insert
                    .execute(rusqlite::params![format!("Row {index}"), index])
                    .expect("insert");
            }
        }
        transaction.commit().expect("commit");
        connection
            .execute_batch("CREATE INDEX \"bench_amount_idx\" ON \"Bench\" (\"Amount\", \"id\");")
            .expect("index");
    }

    let workspace = Workspace::open(&path, Box::new(TestClock::new())).expect("reopen");
    let mut query = PageQuery {
        table: "Bench".into(),
        limit: 100,
        sort: vec![SortSpec {
            column: "Amount".into(),
            direction: SortDirection::Asc,
        }],
        ..PageQuery::default()
    };
    let mut durations: Vec<Duration> = Vec::new();
    let mut rows_seen = 0_usize;
    loop {
        let started = Instant::now();
        let page = workspace.page(&query).expect("page");
        durations.push(started.elapsed());
        rows_seen += page.rows.len();
        match page.next_cursor {
            Some(cursor) => query.cursor = Some(cursor),
            None => break,
        }
        assert!(durations.len() < 1_100, "pagination did not terminate");
    }
    assert_eq!(rows_seen, 100_000);
    assert_eq!(durations.len(), 1_000);
    let mut sorted = durations.clone();
    sorted.sort();
    let p95 = sorted[sorted.len() * 95 / 100];
    let median = sorted[sorted.len() / 2];
    let budget = if cfg!(debug_assertions) {
        Duration::from_millis(250)
    } else {
        Duration::from_millis(15)
    };
    assert!(
        p95 < budget,
        "p95 page latency {p95:?} exceeded {budget:?} (median {median:?}, pages {})",
        durations.len()
    );
}

/// Duplicate sort keys and a large NULL tail must still produce an exact
/// partition of the table: no row twice, no row skipped, in either direction.
#[test]
fn keyset_pagination_never_skips_or_repeats_ties_and_nulls() {
    let directory = tempfile::TempDir::new().expect("tempdir");
    let path = directory.path().join("ties.slab");
    let mut workspace =
        Workspace::create(&path, "Ties", Box::new(TestClock::new())).expect("create");
    workspace
        .create_table(&TableDraft {
            name: "People".into(),
            columns: vec![text("Name")],
        })
        .expect("table");
    workspace.close().expect("close");

    {
        let mut connection = rusqlite::Connection::open(&path).expect("raw open");
        let transaction = connection.transaction().expect("transaction");
        {
            let mut insert = transaction
                .prepare("INSERT INTO \"People\" (\"Name\") VALUES (?1)")
                .expect("prepare");
            for _ in 0..400 {
                insert.execute(rusqlite::params!["Active"]).expect("tie");
            }
            for _ in 0..300 {
                insert
                    .execute(rusqlite::params![Option::<String>::None])
                    .expect("null");
            }
            for index in 0..300 {
                insert
                    .execute(rusqlite::params![format!("Name {index:04}")])
                    .expect("distinct");
            }
        }
        transaction.commit().expect("commit");
    }

    let workspace = Workspace::open(&path, Box::new(TestClock::new())).expect("reopen");
    for direction in [SortDirection::Asc, SortDirection::Desc] {
        let (ids, names) = walk(&workspace, direction);
        assert_eq!(ids.len(), 1_000, "direction {direction:?} lost rows");
        let unique: BTreeSet<i64> = ids.iter().copied().collect();
        assert_eq!(
            unique.len(),
            1_000,
            "direction {direction:?} returned duplicate ids"
        );
        assert_eq!(unique.iter().next(), Some(&1));
        assert_eq!(unique.iter().next_back(), Some(&1_000));
        assert!(
            names[..700].iter().all(|value| !value.is_null()),
            "direction {direction:?} did not order the non-NULL rows first"
        );
        assert!(
            names[700..].iter().all(Value::is_null),
            "direction {direction:?} did not keep NULLS LAST"
        );
    }
}

/// Text filters are literal substring operations, not LIKE patterns. `%`, `_`,
/// and `\` match themselves, and empty text stays distinct from NULL.
#[test]
fn text_filters_treat_wildcards_as_literals_and_keep_null_distinct() {
    let directory = tempfile::TempDir::new().expect("tempdir");
    let path = directory.path().join("filters.slab");
    let mut workspace =
        Workspace::create(&path, "Filters", Box::new(TestClock::new())).expect("create");
    workspace
        .create_table(&TableDraft {
            name: "Samples".into(),
            columns: vec![text("Name")],
        })
        .expect("table");
    for name in [
        "100%",
        "100X",
        "a_b",
        "axb",
        "back\\slash",
        "backXslash",
        "",
    ] {
        workspace
            .insert_row("Samples", &row(&[("Name", Value::from(name))]))
            .expect("sample");
    }
    workspace
        .insert_row("Samples", &RowData::new())
        .expect("null sample");

    let names_for = |op: FilterOp, value: Option<Value>| -> Vec<Option<String>> {
        let page = workspace
            .page(&PageQuery {
                table: "Samples".into(),
                limit: 100,
                filters: vec![Filter {
                    column: "Name".into(),
                    op,
                    value,
                }],
                ..PageQuery::default()
            })
            .expect("filtered page");
        page.rows
            .iter()
            .map(|record| {
                record
                    .get("Name")
                    .and_then(Value::as_text)
                    .map(str::to_owned)
            })
            .collect()
    };

    assert_eq!(
        names_for(FilterOp::Contains, Some(Value::from("100%"))),
        vec![Some("100%".to_owned())]
    );
    assert_eq!(
        names_for(FilterOp::Contains, Some(Value::from("a_b"))),
        vec![Some("a_b".to_owned())]
    );
    assert_eq!(
        names_for(FilterOp::EndsWith, Some(Value::from("\\slash"))),
        vec![Some("back\\slash".to_owned())]
    );
    assert_eq!(
        names_for(FilterOp::StartsWith, Some(Value::from("a_"))),
        vec![Some("a_b".to_owned())]
    );
    assert_eq!(
        names_for(FilterOp::Eq, Some(Value::from(""))),
        vec![Some(String::new())],
        "an empty-string equality filter also matched NULL"
    );
    assert_eq!(
        names_for(FilterOp::Eq, Some(Value::Null)),
        vec![None],
        "a NULL equality filter also matched empty text"
    );
}

/// Date comparisons and sorting work at the documented endpoints.
#[test]
fn date_filters_cover_the_supported_range_endpoints() {
    let directory = tempfile::TempDir::new().expect("tempdir");
    let path = directory.path().join("dates.slab");
    let mut workspace =
        Workspace::create(&path, "Dates", Box::new(TestClock::new())).expect("create");
    workspace
        .create_table(&TableDraft {
            name: "Events".into(),
            columns: vec![date("On")],
        })
        .expect("table");
    for iso in ["0001-01-01", "2024-02-29", "9999-12-31"] {
        workspace
            .insert_row(
                "Events",
                &row(&[("On", Value::Date(SlabDate::parse(iso).expect("date")))]),
            )
            .expect("event");
    }

    let count = |op: FilterOp, iso: &str| -> usize {
        workspace
            .page(&PageQuery {
                table: "Events".into(),
                limit: 100,
                filters: vec![Filter {
                    column: "On".into(),
                    op,
                    value: Some(Value::Date(SlabDate::parse(iso).expect("date"))),
                }],
                ..PageQuery::default()
            })
            .expect("date page")
            .rows
            .len()
    };

    assert_eq!(count(FilterOp::Gte, "0001-01-01"), 3);
    assert_eq!(count(FilterOp::Lte, "9999-12-31"), 3);
    assert_eq!(count(FilterOp::Eq, "0001-01-01"), 1);
    assert_eq!(count(FilterOp::Eq, "9999-12-31"), 1);

    let page = workspace
        .page(&PageQuery {
            table: "Events".into(),
            limit: 10,
            sort: vec![SortSpec {
                column: "On".into(),
                direction: SortDirection::Asc,
            }],
            ..PageQuery::default()
        })
        .expect("sorted dates");
    assert_eq!(
        page.rows.first().and_then(|row| row.get("On")),
        Some(&Value::Date(SlabDate::parse("0001-01-01").expect("date")))
    );
    assert_eq!(
        page.rows.last().and_then(|row| row.get("On")),
        Some(&Value::Date(SlabDate::parse("9999-12-31").expect("date")))
    );
}

/// Virtual columns have no physical storage. Sorting or filtering them must be
/// refused up front, so the advisor can never suggest an index that cannot
/// exist.
#[test]
fn virtual_columns_never_reach_sort_filter_or_the_index_advisor() {
    let directory = tempfile::TempDir::new().expect("tempdir");
    let path = directory.path().join("virtual.slab");
    let mut workspace =
        Workspace::create(&path, "Virtual", Box::new(TestClock::new())).expect("create");
    workspace
        .create_table(&TableDraft {
            name: "Invoices".into(),
            columns: vec![text("Number"), integer("Amount")],
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
            &ColumnDraft {
                name: "Doubled".into(),
                ty: ColumnType::Formula {
                    expression: "Amount * 2".into(),
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
            &ColumnDraft {
                name: "Total".into(),
                ty: ColumnType::Lookup {
                    relation: "Items".into(),
                    column: "Amount".into(),
                    aggregate: Aggregate::Sum,
                },
                required: false,
                unique: false,
                default: None,
            },
        )
        .expect("lookup");
    let invoice = workspace
        .insert_row(
            "Invoices",
            &row(&[
                ("Number", Value::from("INV-1")),
                ("Amount", Value::Integer(5)),
            ]),
        )
        .expect("invoice")
        .id;
    workspace
        .insert_row(
            "Items",
            &row(&[
                ("Invoice", Value::Integer(invoice)),
                ("Amount", Value::Integer(7)),
            ]),
        )
        .expect("item");

    for column in ["Doubled", "Total"] {
        let sort_error = workspace
            .page(&PageQuery {
                table: "Invoices".into(),
                limit: 10,
                sort: vec![SortSpec {
                    column: column.into(),
                    direction: SortDirection::Asc,
                }],
                ..PageQuery::default()
            })
            .expect_err("virtual sort must be rejected");
        assert_eq!(sort_error.code_str(), "SLAB_E_QUERY");

        let filter_error = workspace
            .page(&PageQuery {
                table: "Invoices".into(),
                limit: 10,
                filters: vec![Filter {
                    column: column.into(),
                    op: FilterOp::Eq,
                    value: Some(Value::Integer(1)),
                }],
                ..PageQuery::default()
            })
            .expect_err("virtual filter must be rejected");
        assert_eq!(filter_error.code_str(), "SLAB_E_QUERY");

        let index_error = workspace
            .create_index("Invoices", column)
            .expect_err("virtual index must be rejected");
        assert_eq!(index_error.code_str(), "SLAB_E_COMMAND");
    }
}

#[test]
fn decimal_filters_rescale_and_reject_sub_scale_bounds() {
    let mut workspace =
        Workspace::open_in_memory("Filters", Box::new(TestClock::new())).expect("workspace");
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
    for amount in ["12.30", "12.30", "12.30", "1.23"] {
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
    let filter = Filter {
        column: "Amount".into(),
        op: FilterOp::Eq,
        value: Some(Value::Text("12.3".into())),
    };
    let first = workspace
        .page(&PageQuery {
            table: "Invoices".into(),
            limit: 2,
            filters: vec![filter.clone()],
            ..PageQuery::default()
        })
        .expect("first filtered page");
    assert_eq!(first.rows.len(), 2);
    let cursor = first.next_cursor.clone().expect("next cursor");
    let second = workspace
        .page(&PageQuery {
            table: "Invoices".into(),
            limit: 2,
            cursor: Some(cursor),
            filters: vec![filter],
            ..PageQuery::default()
        })
        .expect("second filtered page");
    assert_eq!(second.rows.len(), 1);
    assert!(second.next_cursor.is_none());
    assert!(
        workspace
            .page(&PageQuery {
                table: "Invoices".into(),
                limit: 10,
                filters: vec![Filter {
                    column: "Amount".into(),
                    op: FilterOp::Lt,
                    value: Some(Value::Text("12.304".into())),
                }],
                ..PageQuery::default()
            })
            .is_err(),
        "a sub-scale bound must not be rounded into a different result set"
    );
}
