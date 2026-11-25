#![allow(
    clippy::print_stdout,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic
)]

//! Release measurements for the systems-engine upgrades.
//!
//! Run with `cargo run --release -p slab-storage --example systems_bench`.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use slab_core::{
    Aggregate, Clock, ColumnDraft, ColumnType, Hlc, Page, PageQuery, Record, RowData,
    SortDirection, SortSpec, TableDraft, Value,
};
use slab_storage::Workspace;

struct BenchClock {
    last: i64,
    counter: u32,
}

impl BenchClock {
    fn new() -> Self {
        Self {
            last: 0,
            counter: 0,
        }
    }
}

impl Clock for BenchClock {
    fn now(&mut self) -> Hlc {
        let wall = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| i64::try_from(duration.as_millis()).unwrap_or(i64::MAX))
            .unwrap_or(0)
            .max(self.last);
        if wall == self.last {
            self.counter = self.counter.saturating_add(1);
        } else {
            self.last = wall;
            self.counter = 0;
        }
        Hlc::new(self.last, self.counter, "bench")
    }
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

fn percentile(sorted: &[Duration], percent: usize) -> Duration {
    let index = (sorted.len() * percent / 100).min(sorted.len().saturating_sub(1));
    sorted[index]
}

fn row(values: &[(&str, Value)]) -> RowData {
    values
        .iter()
        .map(|(name, value)| ((*name).to_owned(), value.clone()))
        .collect()
}

fn main() {
    let directory = tempfile::TempDir::new().expect("tempdir");
    pagination(&directory);
    ivm(&directory);
    ipc();
    concurrency(&directory);
    snapshots(&directory);
}

fn pagination(directory: &tempfile::TempDir) {
    let path = directory.path().join("pagination.slab");
    let mut workspace =
        Workspace::create(&path, "Bench", Box::new(BenchClock::new())).expect("create");
    workspace
        .create_table(&TableDraft {
            name: "Bench".into(),
            columns: vec![text("Name"), integer("Amount")],
        })
        .expect("table");
    workspace.close().expect("close");
    {
        let mut connection = rusqlite::Connection::open(&path).expect("raw");
        let transaction = connection.transaction().expect("tx");
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
    let mut workspace = Workspace::open(&path, Box::new(BenchClock::new())).expect("open");
    let mut query = PageQuery {
        table: "Bench".into(),
        limit: 100,
        sort: vec![SortSpec {
            column: "Amount".into(),
            direction: SortDirection::Asc,
        }],
        ..PageQuery::default()
    };
    let mut durations = Vec::new();
    loop {
        let started = Instant::now();
        let page = workspace.page(&query).expect("page");
        durations.push(started.elapsed());
        match page.next_cursor {
            Some(cursor) => query.cursor = Some(cursor),
            None => break,
        }
    }
    durations.sort();
    println!(
        "pagination_100k pages={} median_ms={:.3} p95_ms={:.3}",
        durations.len(),
        durations[durations.len() / 2].as_secs_f64() * 1_000.0,
        percentile(&durations, 95).as_secs_f64() * 1_000.0,
    );

    let name_query = PageQuery {
        table: "Bench".into(),
        limit: 100,
        sort: vec![SortSpec {
            column: "Name".into(),
            direction: SortDirection::Asc,
        }],
        ..PageQuery::default()
    };
    let unindexed = workspace.page(&name_query).expect("unindexed");
    let hint = unindexed
        .index_hint
        .as_ref()
        .map(|hint| hint.column.clone())
        .unwrap_or_default();
    workspace
        .create_index("Bench", "Name")
        .expect("create index");
    let indexed = workspace.page(&name_query).expect("indexed");
    println!(
        "index_advisor unindexed_ms={:.3} indexed_ms={:.3} hinted_column={hint} hint_after={}",
        unindexed.elapsed_ms,
        indexed.elapsed_ms,
        indexed.index_hint.is_some()
    );
    workspace.close().expect("close");
}

fn concurrency(directory: &tempfile::TempDir) {
    let path = directory.path().join("concurrency.slab");
    let mut workspace =
        Workspace::create(&path, "Concurrency", Box::new(BenchClock::new())).expect("create");
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

    let done = Arc::new(AtomicBool::new(false));
    let writer_done = Arc::clone(&done);
    let writer_path = path.clone();
    let writer = std::thread::spawn(move || {
        let connection = rusqlite::Connection::open(&writer_path).expect("writer");
        connection
            .busy_timeout(Duration::from_millis(5_000))
            .expect("timeout");
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
    let mut busy_errors = 0_u32;
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
        match workspace.page(&query) {
            Ok(page) => query.cursor = page.next_cursor,
            Err(error) => {
                if format!("{error}").contains("locked") || format!("{error}").contains("busy") {
                    busy_errors = busy_errors.saturating_add(1);
                }
                query.cursor = None;
            }
        }
        if workspace.search("needle", 20).is_err() {
            busy_errors = busy_errors.saturating_add(1);
        }
        reads = reads.saturating_add(1);
    }
    writer.join().expect("writer join");
    println!("concurrency_20k reads={reads} busy_errors={busy_errors}");
    workspace.close().expect("close");
}

fn ivm(directory: &tempfile::TempDir) {
    let path = directory.path().join("ivm.slab");
    let mut workspace =
        Workspace::create(&path, "IVM", Box::new(BenchClock::new())).expect("create");
    workspace
        .create_table(&TableDraft {
            name: "Invoices".into(),
            columns: vec![text("Number")],
        })
        .expect("parent");
    workspace
        .create_table(&TableDraft {
            name: "Items".into(),
            columns: vec![integer("Invoice"), integer("Amount")],
        })
        .expect("child");
    workspace
        .create_relation(&slab_core::RelationDraft {
            name: "Items".into(),
            parent_table: "Invoices".into(),
            child_table: "Items".into(),
            child_column: "Invoice".into(),
            on_delete: slab_core::DeleteAction::Cascade,
        })
        .expect("relation");
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
        .insert_row("Invoices", &row(&[("Number", Value::from("INV"))]))
        .expect("invoice")
        .id;
    workspace.begin_import().expect("begin");
    for batch in 0..20 {
        let rows: Vec<RowData> = (0..500)
            .map(|index| {
                row(&[
                    ("Invoice", Value::Integer(invoice)),
                    ("Amount", Value::Integer(i64::from(batch * 500 + index))),
                ])
            })
            .collect();
        workspace
            .insert_import_rows("Items", &rows)
            .expect("import");
    }
    workspace.commit_import().expect("commit");
    let child = workspace
        .page(&PageQuery {
            table: "Items".into(),
            limit: 1,
            ..PageQuery::default()
        })
        .expect("page")
        .rows[0]
        .id;
    let _ = workspace.read_record("Invoices", invoice).expect("read");

    let mut durations = Vec::new();
    for step in 0..40_i64 {
        let started = Instant::now();
        workspace
            .update_row("Items", child, &row(&[("Amount", Value::Integer(step))]))
            .expect("update");
        let _ = workspace.read_record("Invoices", invoice).expect("read");
        durations.push(started.elapsed());
    }
    durations.sort();
    println!(
        "ivm_10k_children edits={} median_ms={:.3} p95_ms={:.3}",
        durations.len(),
        durations[durations.len() / 2].as_secs_f64() * 1_000.0,
        percentile(&durations, 95).as_secs_f64() * 1_000.0,
    );
    workspace.close().expect("close");
}

fn ipc() {
    let columns: Vec<String> = (0..10).map(|index| format!("Column {index:02}")).collect();
    let rows = (0..100)
        .map(|row| {
            let mut values = BTreeMap::new();
            for (index, column) in columns.iter().enumerate() {
                values.insert(column.clone(), Value::from(format!("cell {row}-{index}")));
            }
            Record {
                id: row + 1,
                values,
            }
        })
        .collect();
    let page = Page {
        rows,
        next_cursor: None,
        elapsed_ms: 0.8,
        total_rows: Some(100),
        index_hint: None,
    };
    let named = serde_json::to_vec(&page).expect("named").len();
    let frame = page.into_frame(columns);
    let tuple = serde_json::to_vec(&frame).expect("frame").len();
    let reduction = 100.0 * (1.0 - tuple as f64 / named as f64);
    println!("ipc_100x10 named_bytes={named} tuple_bytes={tuple} reduction_pct={reduction:.1}");
}

fn snapshots(directory: &tempfile::TempDir) {
    let path = directory.path().join("snapshot-source.slab");
    let mut workspace =
        Workspace::create(&path, "Snapshots", Box::new(BenchClock::new())).expect("create");
    workspace
        .create_table(&TableDraft {
            name: "Docs".into(),
            columns: vec![text("Body")],
        })
        .expect("table");
    workspace.begin_import().expect("begin");
    for batch in 0..20 {
        let rows: Vec<RowData> = (0..500)
            .map(|index| {
                row(&[(
                    "Body",
                    Value::from(format!("document body {} {}", batch, index)),
                )])
            })
            .collect();
        workspace.insert_import_rows("Docs", &rows).expect("import");
    }
    workspace.commit_import().expect("commit");

    let clone_path = directory.path().join("clone.slab");
    let clone = workspace.snapshot_to(&clone_path).expect("clone");
    println!(
        "snapshot_clone kind={:?} ms={:.3} bytes={}",
        clone.kind, clone.elapsed_ms, clone.bytes
    );
    let vacuum_path = directory.path().join("vacuum.slab");
    let vacuum = workspace.snapshot_vacuum_to(&vacuum_path).expect("vacuum");
    println!(
        "snapshot_vacuum kind={:?} ms={:.3} bytes={}",
        vacuum.kind, vacuum.elapsed_ms, vacuum.bytes
    );
    workspace.close().expect("close");
}
