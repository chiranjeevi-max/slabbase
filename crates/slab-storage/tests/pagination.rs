#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod support;

use std::time::{Duration, Instant};

use slab_core::{ColumnDraft, ColumnType, PageQuery, SortDirection, SortSpec, TableDraft};
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
