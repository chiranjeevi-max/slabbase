#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::BTreeMap;

use slab_core::{Page, Record, Value};

fn page() -> Page {
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
    Page {
        rows,
        next_cursor: None,
        elapsed_ms: 0.8,
        total_rows: Some(100),
        index_hint: None,
    }
}

#[test]
fn positional_frames_reduce_page_payload() {
    let named = serde_json::to_vec(&page()).expect("named page");
    let columns: Vec<String> = (0..10).map(|index| format!("Column {index:02}")).collect();
    let frame = page().into_frame(columns);
    let positional = serde_json::to_vec(&frame).expect("frame");
    let reduction = 100.0 * (1.0 - positional.len() as f64 / named.len() as f64);
    assert!(
        reduction >= 25.0,
        "payload reduction {reduction:.1}% is below 25% (named {} bytes, positional {} bytes)",
        named.len(),
        positional.len()
    );
}

#[test]
fn tuple_rows_rebuild_the_same_named_records() {
    let columns: Vec<String> = (0..10).map(|index| format!("Column {index:02}")).collect();
    let original = page();
    let rebuilt = original.clone().into_frame(columns).records();
    assert_eq!(original.rows, rebuilt);
}
