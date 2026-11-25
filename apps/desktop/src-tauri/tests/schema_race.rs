#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

//! Concurrent readers must never observe a half-applied schema.
//!
//! A read loads metadata and then prepares data SQL. If a rename or table
//! rebuild commits between those two steps, the prepared SQL can name a column
//! that no longer exists. Schema writes therefore exclude read leases.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Barrier};
use std::time::{Duration, Instant};

use slab_core::{ColumnDraft, ColumnType, PageQuery, RowData, TableDraft, Value};
use slabbase_desktop::worker::DbHandle;

fn text(name: &str) -> ColumnDraft {
    ColumnDraft {
        name: name.into(),
        ty: ColumnType::Text,
        required: false,
        unique: false,
        default: None,
    }
}

fn row(body: &str) -> RowData {
    let mut values = RowData::new();
    values.insert("Body".into(), Value::from(body));
    values
}

#[test]
fn readers_survive_concurrent_schema_churn() {
    let directory = tempfile::TempDir::new().expect("tempdir");
    let path = directory.path().join("schema-race.slab");
    let db = Arc::new(DbHandle::new().expect("db"));
    db.create(path.to_string_lossy().into_owned(), "Race".into())
        .expect("create");
    db.write_schema(|workspace| {
        workspace
            .create_table(&TableDraft {
                name: "Docs".into(),
                columns: vec![text("Body"), text("Extra")],
            })
            .map_err(slabbase_desktop::ApiError::from)?;
        let rows: Vec<RowData> = (0..300)
            .map(|index| {
                let mut values = row(&format!("body {index}"));
                values.insert("Extra".into(), Value::from(format!("extra {index}")));
                values
            })
            .collect();
        workspace
            .insert_rows("Docs", &rows)
            .map_err(slabbase_desktop::ApiError::from)?;
        Ok(())
    })
    .expect("seed");

    let stop = Arc::new(AtomicBool::new(false));
    let reads = Arc::new(AtomicUsize::new(0));
    let barrier = Arc::new(Barrier::new(17));
    let mut readers = Vec::new();
    for _ in 0..16 {
        let db = Arc::clone(&db);
        let stop = Arc::clone(&stop);
        let reads = Arc::clone(&reads);
        let barrier = Arc::clone(&barrier);
        readers.push(std::thread::spawn(move || {
            barrier.wait();
            let mut errors: Vec<String> = Vec::new();
            while !stop.load(Ordering::Relaxed) {
                let outcome = db.read(|workspace| {
                    let page = workspace.page(&PageQuery {
                        table: "Docs".into(),
                        limit: 50,
                        ..PageQuery::default()
                    })?;
                    if let Some(record) = page.rows.first() {
                        let _ = workspace.read_record("Docs", record.id)?;
                    }
                    Ok(())
                });
                if let Err(error) = outcome {
                    errors.push(format!("{}: {}", error.code, error.message));
                    if errors.len() >= 20 {
                        break;
                    }
                }
                reads.fetch_add(1, Ordering::Relaxed);
                std::thread::yield_now();
            }
            errors
        }));
    }
    barrier.wait();

    let deadline = Instant::now() + Duration::from_secs(2);
    let mut renamed = false;
    let mut round = 0_u32;
    while Instant::now() < deadline {
        db.write_schema(|workspace| {
            if renamed {
                workspace
                    .rename_column("Docs", "Extra A", "Extra")
                    .map_err(slabbase_desktop::ApiError::from)
            } else {
                workspace
                    .rename_column("Docs", "Extra", "Extra A")
                    .map_err(slabbase_desktop::ApiError::from)
            }
        })
        .expect("rename");
        renamed = !renamed;
        round = round.saturating_add(1);
        if round.is_multiple_of(5) {
            let scratch = format!("Scratch {round}");
            db.write_schema(|workspace| {
                workspace
                    .add_column("Docs", &text(&scratch))
                    .map_err(slabbase_desktop::ApiError::from)?;
                Ok(())
            })
            .expect("add column");
            db.write_schema(|workspace| {
                workspace
                    .drop_column("Docs", &scratch)
                    .map_err(slabbase_desktop::ApiError::from)?;
                Ok(())
            })
            .expect("drop column");
        }
    }
    stop.store(true, Ordering::Relaxed);

    let mut errors = Vec::new();
    for reader in readers {
        errors.extend(reader.join().expect("reader join"));
    }
    assert!(
        reads.load(Ordering::Relaxed) > 0,
        "the stress test never completed a read"
    );
    assert!(
        errors.is_empty(),
        "schema churn produced {} read errors: {:?}",
        errors.len(),
        &errors[..errors.len().min(10)]
    );
}
