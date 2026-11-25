#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

//! Cancellation must be scoped to one workspace generation.
//!
//! The shell keeps several reader replicas in flight. A stale interrupt from a
//! previous workspace must never reach the workspace currently on screen, and
//! closing a workspace must leave no live registration behind.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use slabbase_desktop::worker::DbHandle;

const SLOW_QUERY: &str = "WITH RECURSIVE cnt(x) AS (
        SELECT 1 UNION ALL SELECT x + 1 FROM cnt WHERE x < 100000000
     ) SELECT SUM(x) FROM cnt";

#[test]
fn switching_workspaces_isolates_stale_cancel_handles() {
    let directory = tempfile::TempDir::new().expect("tempdir");
    let db = DbHandle::new().expect("db");
    db.create(
        directory
            .path()
            .join("first.slab")
            .to_string_lossy()
            .into_owned(),
        "First".into(),
    )
    .expect("create first");
    let stale = db.cancel_handle();

    db.create(
        directory
            .path()
            .join("second.slab")
            .to_string_lossy()
            .into_owned(),
        "Second".into(),
    )
    .expect("create second");
    let current = db.cancel_handle();
    assert!(
        !Arc::ptr_eq(&stale, &current),
        "a workspace switch reused the previous cancellation handle"
    );

    let stale_connection = rusqlite::Connection::open_in_memory().expect("stale connection");
    stale.register(stale_connection.get_interrupt_handle());
    let current_connection = rusqlite::Connection::open_in_memory().expect("current connection");
    current.register(current_connection.get_interrupt_handle());

    let finished = Arc::new(AtomicBool::new(false));
    let worker_finished = Arc::clone(&finished);
    let worker = std::thread::spawn(move || {
        let outcome: Result<i64, rusqlite::Error> =
            current_connection.query_row(SLOW_QUERY, [], |row| row.get(0));
        worker_finished.store(true, Ordering::SeqCst);
        outcome.is_err()
    });

    std::thread::sleep(Duration::from_millis(40));
    assert!(
        stale.interrupt(),
        "the stale registration should still be interruptible"
    );
    std::thread::sleep(Duration::from_millis(60));
    assert!(
        !finished.load(Ordering::SeqCst),
        "a stale workspace cancel interrupted the current workspace query"
    );

    assert!(
        db.cancel_query(),
        "the current generation must remain cancellable"
    );
    assert!(
        worker.join().expect("worker join"),
        "the current query was not interrupted"
    );

    db.close().expect("close");
    assert!(
        !db.cancel_query(),
        "closing the workspace left a live cancellation registration"
    );
}
