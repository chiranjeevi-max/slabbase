#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::sync::atomic::{AtomicBool, Ordering};
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
fn reads_do_not_wait_for_an_active_import() {
    let directory = tempfile::TempDir::new().expect("tempdir");
    let path = directory.path().join("concurrent.slab");
    let db = Arc::new(DbHandle::new().expect("db"));
    db.create(path.to_string_lossy().into_owned(), "Concurrent".into())
        .expect("create");
    db.write(|workspace| {
        workspace.create_table(&TableDraft {
            name: "Docs".into(),
            columns: vec![text("Body")],
        })?;
        workspace.insert_rows("Docs", &[row("alpha"), row("beta"), row("gamma")])?;
        Ok(())
    })
    .expect("seed");
    assert_eq!(
        db.info().expect("info").expect("workspace").name,
        "Concurrent"
    );

    let active = Arc::new(AtomicBool::new(false));
    let barrier = Arc::new(Barrier::new(2));
    let writer = {
        let db = Arc::clone(&db);
        let active = Arc::clone(&active);
        let barrier = Arc::clone(&barrier);
        std::thread::spawn(move || {
            db.write(|workspace| {
                workspace
                    .begin_import()
                    .map_err(slabbase_desktop::ApiError::from)?;
                let rows: Vec<RowData> = (0..1_000)
                    .map(|index| row(&format!("bulk {index}")))
                    .collect();
                workspace
                    .insert_import_rows("Docs", &rows)
                    .map_err(slabbase_desktop::ApiError::from)?;
                active.store(true, Ordering::SeqCst);
                barrier.wait();
                std::thread::sleep(Duration::from_millis(800));
                workspace
                    .commit_import()
                    .map_err(slabbase_desktop::ApiError::from)?;
                Ok(())
            })
            .expect("write");
        })
    };

    barrier.wait();
    assert!(active.load(Ordering::SeqCst), "writer did not start");
    let started = Instant::now();
    let page = db
        .read(|workspace| {
            Ok(workspace.page(&PageQuery {
                table: "Docs".into(),
                limit: 50,
                ..PageQuery::default()
            })?)
        })
        .expect("concurrent read");
    let elapsed = started.elapsed();
    assert!(
        active.load(Ordering::SeqCst),
        "read waited for the import to finish"
    );
    assert!(
        elapsed < Duration::from_millis(600),
        "read took {elapsed:?}; it was queued behind the write"
    );
    assert_eq!(
        page.rows.len(),
        3,
        "reader observed uncommitted import rows"
    );

    writer.join().expect("writer join");
    let count = db
        .read(|workspace| {
            let summaries = workspace.table_summaries()?;
            Ok(summaries
                .into_iter()
                .find(|table| table.name == "Docs")
                .map(|table| table.row_count)
                .unwrap_or(0))
        })
        .expect("committed read");
    assert_eq!(count, 1_003);
    db.close().expect("close");
    assert!(
        !path.with_extension("slab-wal").exists(),
        "shell close left a WAL sidecar"
    );
    assert!(
        !path.with_extension("slab-shm").exists(),
        "shell close left an SHM sidecar"
    );
}
