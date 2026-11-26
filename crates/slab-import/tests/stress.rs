#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::BTreeMap;
use std::io::Read;

use slab_core::{Column, ColumnType, CurrencyCode, RowData, RowId, Table};
use slab_import::{
    CsvAdapter, ImportError, ImportOptions, ImportSink, JsonAdapter, apply, preview,
    suggest_mapping, validate,
};

fn column(table: &str, name: &str, ty: ColumnType, required: bool, position: u32) -> Column {
    Column {
        id: format!("c_{name}"),
        table: table.into(),
        name: name.into(),
        ty,
        required,
        unique: false,
        default: None,
        position,
    }
}

fn clients() -> Table {
    Table {
        id: "t_clients".into(),
        name: "Clients".into(),
        columns: vec![
            column("Clients", "Name", ColumnType::Text, true, 0),
            column("Clients", "Seats", ColumnType::Integer, false, 1),
            column("Clients", "Notes", ColumnType::Text, false, 2),
        ],
        created_at: 0,
    }
}

fn typed_target() -> Table {
    Table {
        id: "t_types".into(),
        name: "Types".into(),
        columns: vec![
            column("Types", "When", ColumnType::Date, true, 0),
            column(
                "Types",
                "Amount",
                ColumnType::Currency {
                    currency: CurrencyCode::parse("JPY").expect("jpy"),
                },
                false,
                1,
            ),
        ],
        created_at: 0,
    }
}

struct BatchedSink {
    rows: usize,
    batches: Vec<usize>,
    values: Vec<RowData>,
    fail_after: Option<usize>,
}

impl BatchedSink {
    fn new(fail_after: Option<usize>) -> Self {
        Self {
            rows: 0,
            batches: Vec::new(),
            values: Vec::new(),
            fail_after,
        }
    }
}

impl ImportSink for BatchedSink {
    fn insert_row(&mut self, _table: &str, values: &RowData) -> Result<RowId, ImportError> {
        if self.fail_after.is_some_and(|limit| self.rows >= limit) {
            return Err(ImportError::Sink("simulated failure".into()));
        }
        self.rows += 1;
        self.values.push(values.clone());
        Ok(i64::try_from(self.rows).expect("id"))
    }

    fn insert_batch(&mut self, _table: &str, rows: &[RowData]) -> Result<Vec<RowId>, ImportError> {
        if self
            .fail_after
            .is_some_and(|limit| self.rows + rows.len() > limit)
        {
            return Err(ImportError::Sink("simulated batch failure".into()));
        }
        self.batches.push(rows.len());
        let mut ids = Vec::with_capacity(rows.len());
        for values in rows {
            self.rows += 1;
            ids.push(i64::try_from(self.rows).expect("id"));
            self.values.push(values.clone());
        }
        Ok(ids)
    }
}

#[test]
fn csv_bom_is_stripped_before_header_mapping() {
    let source = "\u{FEFF}Name,Seats,Notes\r\nNorthlight,12,\"multi\r\nline\"\r\n";
    let mut adapter = CsvAdapter;
    let preview = preview(
        &mut adapter,
        &mut source.as_bytes(),
        &ImportOptions::default(),
    )
    .expect("preview");
    assert_eq!(preview.columns[0].name, "Name");
    assert_eq!(preview.rows_scanned, 1);
    assert_eq!(
        preview.sample[0].values[2].as_deref(),
        Some("multi\r\nline")
    );
    let target = clients();
    let mapping = suggest_mapping(
        &preview
            .columns
            .iter()
            .map(|column| column.name.clone())
            .collect::<Vec<_>>(),
        &target,
    );
    assert_eq!(mapping.get("Name"), Some(&"Name".to_owned()));
    assert_eq!(mapping.get("Seats"), Some(&"Seats".to_owned()));
}

#[test]
fn fifty_thousand_malformed_csv_rows_stream_in_bounded_batches() {
    let mut source = String::with_capacity(8_000_000);
    source.push('\u{FEFF}');
    source.push_str("Name,Seats,Notes\r\n");
    for index in 0..50_000 {
        if index % 997 == 0 {
            source.push_str(&format!(
                "\"Renée — € {index}\",{index},\"line one\r\nline \"\"two\"\" — done\"\r\n"
            ));
        } else {
            source.push_str(&format!("Client {index},{index},note {index}\r\n"));
        }
    }
    let target = clients();
    let mapping = BTreeMap::from([
        ("Name".to_owned(), "Name".to_owned()),
        ("Seats".to_owned(), "Seats".to_owned()),
        ("Notes".to_owned(), "Notes".to_owned()),
    ]);
    let options = ImportOptions {
        inference_limit: 500,
        batch_size: 500,
        ..ImportOptions::default()
    };

    let started = std::time::Instant::now();
    let mut adapter = CsvAdapter;
    let preview = preview(&mut adapter, &mut source.as_bytes(), &options).expect("preview");
    let preview_elapsed = started.elapsed();
    assert_eq!(preview.rows_scanned, 50_000);
    assert_eq!(preview.inference_rows, 500);
    assert!(preview.inference_limited);
    assert!(preview.sample.len() <= 20);
    assert!(
        preview_elapsed < std::time::Duration::from_secs(10),
        "preview took {preview_elapsed:?}"
    );

    let mut sink = BatchedSink::new(None);
    let summary = apply(
        &mut adapter,
        &mut source.as_bytes(),
        &options,
        &target,
        &mapping,
        &mut sink,
    )
    .expect("apply");
    assert_eq!(summary.inserted, 50_000);
    assert_eq!(sink.rows, 50_000);
    assert_eq!(sink.batches.iter().sum::<usize>(), 50_000);
    assert_eq!(sink.batches.len(), 100);
    assert!(sink.batches.iter().all(|size| *size <= 500));
    assert_eq!(
        sink.values[0].get("Name"),
        Some(&slab_core::Value::Text("Renée — € 0".into()))
    );
    assert_eq!(
        sink.values[0].get("Notes"),
        Some(&slab_core::Value::Text(
            "line one\r\nline \"two\" — done".into()
        ))
    );
}

#[test]
fn ragged_csv_fails_at_the_first_bad_row_without_reading_further() {
    let mut source = String::from("Name,Seats,Notes\n");
    for index in 0..800 {
        source.push_str(&format!("Client {index},{index},note\n"));
    }
    source.push_str("short row,12\n");
    source.push_str("after,12,ok\n");
    let target = clients();
    let mapping = BTreeMap::from([
        ("Name".to_owned(), "Name".to_owned()),
        ("Seats".to_owned(), "Seats".to_owned()),
        ("Notes".to_owned(), "Notes".to_owned()),
    ]);
    let options = ImportOptions {
        batch_size: 500,
        ..ImportOptions::default()
    };
    let mut sink = BatchedSink::new(None);
    let mut adapter = CsvAdapter;
    let error = apply(
        &mut adapter,
        &mut source.as_bytes(),
        &options,
        &target,
        &mapping,
        &mut sink,
    )
    .expect_err("ragged row must fail");
    assert!(matches!(error, ImportError::Validation { .. }));
    assert_eq!(sink.rows, 500, "only complete batches may reach the sink");
}

#[test]
fn invalid_dates_and_currencies_are_reported_before_apply() {
    let source = "When,Amount\n2026-11-30,1500\n2026-02-30,not-money\n2026-12-01,1250\n";
    let target = typed_target();
    let mapping = BTreeMap::from([
        ("When".to_owned(), "When".to_owned()),
        ("Amount".to_owned(), "Amount".to_owned()),
    ]);
    let mut adapter = CsvAdapter;
    let report = validate(
        &mut adapter,
        &mut source.as_bytes(),
        &ImportOptions::default(),
        &target,
        &mapping,
    )
    .expect("validate");
    assert_eq!(report.rows, 3);
    assert_eq!(report.valid_rows, 2);
    assert_eq!(report.issues.len(), 2);
    assert!(report.issues.iter().all(|issue| issue.row_number == 2));
    assert!(
        report
            .issues
            .iter()
            .any(|issue| issue.message.contains("date"))
    );
    assert!(
        report
            .issues
            .iter()
            .any(|issue| issue.message.contains("invalid"))
    );
}

#[test]
fn sparse_json_with_heterogeneous_keys_is_valid() {
    let source = r#"[
        {"Name":"Northlight","Seats":12,"Notes":"a","Meta":{"tier":2}},
        {"Notes":"b","Name":"Hiraya"},
        {"Name":"Renée — €","Seats":null,"Big":18446744073709551615},
        {"Name":"Nested","Tags":["a","b"],"Meta":[1,2,3]}
    ]"#;
    let target = Table {
        id: "t_json".into(),
        name: "Clients".into(),
        columns: vec![
            column("Clients", "Name", ColumnType::Text, true, 0),
            column("Clients", "Seats", ColumnType::Integer, false, 1),
            column("Clients", "Notes", ColumnType::Text, false, 2),
            column("Clients", "Meta", ColumnType::Text, false, 3),
            column("Clients", "Big", ColumnType::Text, false, 4),
            column("Clients", "Tags", ColumnType::Text, false, 5),
        ],
        created_at: 0,
    };
    let mapping = BTreeMap::from([
        ("Name".to_owned(), "Name".to_owned()),
        ("Seats".to_owned(), "Seats".to_owned()),
        ("Notes".to_owned(), "Notes".to_owned()),
        ("Meta".to_owned(), "Meta".to_owned()),
        ("Big".to_owned(), "Big".to_owned()),
        ("Tags".to_owned(), "Tags".to_owned()),
    ]);
    let mut adapter = JsonAdapter;
    let report = validate(
        &mut adapter,
        &mut source.as_bytes(),
        &ImportOptions::default(),
        &target,
        &mapping,
    )
    .expect("validate");
    assert_eq!(report.rows, 4);
    assert_eq!(report.valid_rows, 4, "issues: {:?}", report.issues);
    let mut sink = BatchedSink::new(None);
    let summary = apply(
        &mut adapter,
        &mut source.as_bytes(),
        &ImportOptions::default(),
        &target,
        &mapping,
        &mut sink,
    )
    .expect("apply");
    assert_eq!(summary.inserted, 4);
    assert_eq!(
        sink.values[2].get("Big"),
        Some(&slab_core::Value::Text("18446744073709551615".into()))
    );
    assert_eq!(
        sink.values[0].get("Meta"),
        Some(&slab_core::Value::Text("{\"tier\":2}".into()))
    );
}

#[test]
fn twenty_five_thousand_ndjson_objects_stream_and_truncation_fails_cleanly() {
    let mut source = String::with_capacity(4_000_000);
    for index in 0..25_000 {
        if index % 2 == 0 {
            source.push_str(&format!(
                "{{\"Name\":\"Row {index}\",\"Seats\":{index},\"Notes\":\"note\"}}\n"
            ));
        } else {
            source.push_str(&format!("{{\"Seats\":{index},\"Name\":\"Row {index}\"}}\n"));
        }
    }
    let target = clients();
    let mapping = BTreeMap::from([
        ("Name".to_owned(), "Name".to_owned()),
        ("Seats".to_owned(), "Seats".to_owned()),
        ("Notes".to_owned(), "Notes".to_owned()),
    ]);
    let options = ImportOptions {
        batch_size: 500,
        ..ImportOptions::default()
    };
    let mut adapter = JsonAdapter;
    let mut sink = BatchedSink::new(None);
    let summary = apply(
        &mut adapter,
        &mut source.as_bytes(),
        &options,
        &target,
        &mapping,
        &mut sink,
    )
    .expect("ndjson");
    assert_eq!(summary.inserted, 25_000);
    assert_eq!(sink.batches.len(), 50);

    let truncated = r#"{"Name":"one","Seats":1}
{"Name":"two","Seats":2}
{"Name":"three","Seats":"#;
    let mut sink = BatchedSink::new(None);
    let error = apply(
        &mut adapter,
        &mut truncated.as_bytes(),
        &options,
        &target,
        &mapping,
        &mut sink,
    )
    .expect_err("truncated JSON must fail");
    assert!(matches!(error, ImportError::Json(_)), "got {error:?}");
    assert_eq!(sink.rows, 0, "no batch should flush before the error");
}

struct FailingReader {
    data: Vec<u8>,
    position: usize,
    fail_at: usize,
}

impl Read for FailingReader {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        if self.position >= self.fail_at {
            return Err(std::io::Error::new(
                std::io::ErrorKind::BrokenPipe,
                "simulated device failure",
            ));
        }
        let available = self.data.len().saturating_sub(self.position);
        let limit = (self.fail_at - self.position)
            .min(available)
            .min(buffer.len());
        buffer[..limit].copy_from_slice(&self.data[self.position..self.position + limit]);
        self.position += limit;
        Ok(limit)
    }
}

#[test]
fn mid_stream_io_failure_is_reported_as_an_io_error() {
    let mut data = String::from("Name,Seats,Notes\n");
    for index in 0..2_000 {
        data.push_str(&format!("Client {index},{index},note\n"));
    }
    let mut reader = FailingReader {
        data: data.into_bytes(),
        position: 0,
        fail_at: 4_000,
    };
    let target = clients();
    let mapping = BTreeMap::from([
        ("Name".to_owned(), "Name".to_owned()),
        ("Seats".to_owned(), "Seats".to_owned()),
        ("Notes".to_owned(), "Notes".to_owned()),
    ]);
    let mut adapter = CsvAdapter;
    let mut sink = BatchedSink::new(None);
    let error = apply(
        &mut adapter,
        &mut reader,
        &ImportOptions::default(),
        &target,
        &mapping,
        &mut sink,
    )
    .expect_err("io failure");
    assert!(
        matches!(error, ImportError::Csv(ref csv) if csv.is_io_error())
            || matches!(error, ImportError::Io(_)),
        "got {error:?}"
    );
    assert!(sink.rows < 1_000);
}
