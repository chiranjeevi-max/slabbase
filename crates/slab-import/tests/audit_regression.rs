//! Regression tests for defects found during the failure-mode audit.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use slab_core::{Column, ColumnType, CurrencyCode, Table, Value};
use slab_import::{CsvAdapter, ImportOptions, JsonAdapter, preview};

#[test]
fn headerless_csv_rows_are_numbered_from_one_without_duplicates() {
    let source = "alpha\nbeta\ngamma\n";
    let options = ImportOptions {
        has_headers: false,
        ..ImportOptions::default()
    };
    let preview = preview(&mut CsvAdapter, &mut source.as_bytes(), &options).expect("preview");
    let numbers: Vec<u64> = preview
        .sample
        .iter()
        .map(|sample| sample.row_number)
        .collect();
    assert_eq!(numbers, vec![1, 2, 3]);
}

#[test]
fn csv_validation_accepts_an_attachment_column() {
    let table = Table {
        id: "t_files".into(),
        name: "Files".into(),
        columns: vec![Column {
            id: "c_path".into(),
            table: "Files".into(),
            name: "Path".into(),
            ty: ColumnType::AttachmentRef,
            required: false,
            unique: false,
            default: None,
            position: 0,
        }],
        created_at: 0,
    };
    let mapping = std::collections::BTreeMap::from([("Path".to_owned(), "Path".to_owned())]);
    let report = slab_import::validate(
        &mut CsvAdapter,
        &mut "Path\n~/Brief.pdf\n".as_bytes(),
        &ImportOptions::default(),
        &table,
        &mapping,
    )
    .expect("validate");
    assert_eq!(report.rows, 1);
    assert_eq!(report.valid_rows, 1, "issues: {:?}", report.issues);
}

#[test]
fn a_csv_currency_export_can_be_imported_back() {
    let table = Table {
        id: "t_invoices".into(),
        name: "Invoices".into(),
        columns: vec![Column {
            id: "c_amount".into(),
            table: "Invoices".into(),
            name: "Amount".into(),
            ty: ColumnType::Currency {
                currency: CurrencyCode::parse("USD").expect("usd"),
            },
            required: false,
            unique: false,
            default: None,
            position: 0,
        }],
        created_at: 0,
    };
    let mapping = std::collections::BTreeMap::from([("Amount".to_owned(), "Amount".to_owned())]);
    let rows = apply_rows(&mut CsvAdapter, "Amount\nUSD 123.45\n", &table, &mapping);
    assert_eq!(rows.len(), 1);
    assert_eq!(
        rows[0].get("Amount"),
        Some(&Value::Currency(
            slab_core::Money::from_minor(CurrencyCode::parse("USD").expect("usd"), 12_345,)
                .expect("money"),
        ))
    );
}

#[test]
fn a_tagged_json_export_can_be_imported_back() {
    let table = Table {
        id: "t_clients".into(),
        name: "Clients".into(),
        columns: vec![
            Column {
                id: "c_name".into(),
                table: "Clients".into(),
                name: "Name".into(),
                ty: ColumnType::Text,
                required: false,
                unique: false,
                default: None,
                position: 0,
            },
            Column {
                id: "c_seats".into(),
                table: "Clients".into(),
                name: "Seats".into(),
                ty: ColumnType::Integer,
                required: false,
                unique: false,
                default: None,
                position: 1,
            },
        ],
        created_at: 0,
    };
    let exported = serde_json::json!([
        {
            "id": 1,
            "Name": serde_json::to_value(Value::Text("Northlight".into())).expect("name"),
            "Seats": serde_json::to_value(Value::Integer(12)).expect("seats")
        }
    ])
    .to_string();
    let mapping = std::collections::BTreeMap::from([
        ("Name".to_owned(), "Name".to_owned()),
        ("Seats".to_owned(), "Seats".to_owned()),
    ]);
    let report = slab_import::validate(
        &mut JsonAdapter,
        &mut exported.as_bytes(),
        &ImportOptions::default(),
        &table,
        &mapping,
    )
    .expect("validate");
    assert_eq!(report.rows, 1);
    assert_eq!(report.valid_rows, 1, "issues: {:?}", report.issues);
}

struct MemorySink(Vec<slab_core::RowData>);

impl slab_import::ImportSink for MemorySink {
    fn insert_row(
        &mut self,
        _table: &str,
        values: &slab_core::RowData,
    ) -> Result<slab_core::RowId, slab_import::ImportError> {
        self.0.push(values.clone());
        Ok(slab_core::RowId::from(
            i64::try_from(self.0.len()).expect("row count fits"),
        ))
    }
}

fn apply_rows(
    adapter: &mut dyn slab_import::ImportAdapter,
    source: &str,
    table: &Table,
    mapping: &std::collections::BTreeMap<String, String>,
) -> Vec<slab_core::RowData> {
    let mut sink = MemorySink(Vec::new());
    slab_import::apply(
        adapter,
        &mut source.as_bytes(),
        &ImportOptions::default(),
        table,
        mapping,
        &mut sink,
    )
    .expect("apply");
    sink.0
}

fn single_text_table(required: bool) -> Table {
    Table {
        id: "t_notes".into(),
        name: "Notes".into(),
        columns: vec![Column {
            id: "c_body".into(),
            table: "Notes".into(),
            name: "Body".into(),
            ty: ColumnType::Text,
            required,
            unique: false,
            default: None,
            position: 0,
        }],
        created_at: 0,
    }
}

#[test]
fn a_json_export_preserves_empty_and_whitespace_text() {
    let table = single_text_table(true);
    let source = r#"[{"Body":{"Text":""}},{"Body":{"Text":"   "}}]"#;
    let rows = apply_rows(
        &mut JsonAdapter,
        source,
        &table,
        &std::collections::BTreeMap::from([("Body".to_owned(), "Body".to_owned())]),
    );
    assert_eq!(rows[0].get("Body"), Some(&Value::Text(String::new())));
    assert_eq!(rows[1].get("Body"), Some(&Value::Text("   ".into())));
}

#[test]
fn a_user_single_key_object_is_not_flattened_into_text() {
    let table = single_text_table(false);
    let source = r#"[{"Body":{"Integer":7}}]"#;
    let rows = apply_rows(
        &mut JsonAdapter,
        source,
        &table,
        &std::collections::BTreeMap::from([("Body".to_owned(), "Body".to_owned())]),
    );
    assert_eq!(
        rows[0].get("Body"),
        Some(&Value::Text(r#"{"Integer":7}"#.into()))
    );
}

#[test]
fn a_tagged_json_integer_reaches_an_integer_column() {
    let table = Table {
        id: "t_counts".into(),
        name: "Counts".into(),
        columns: vec![Column {
            id: "c_count".into(),
            table: "Counts".into(),
            name: "Count".into(),
            ty: ColumnType::Integer,
            required: false,
            unique: false,
            default: None,
            position: 0,
        }],
        created_at: 0,
    };
    let rows = apply_rows(
        &mut JsonAdapter,
        r#"[{"Count":{"Integer":7}}]"#,
        &table,
        &std::collections::BTreeMap::from([("Count".to_owned(), "Count".to_owned())]),
    );
    assert_eq!(rows[0].get("Count"), Some(&Value::Integer(7)));
}

#[test]
fn computed_columns_are_not_auto_mapped() {
    let table = Table {
        id: "t_totals".into(),
        name: "Totals".into(),
        columns: vec![
            Column {
                id: "c_base".into(),
                table: "Totals".into(),
                name: "Base".into(),
                ty: ColumnType::Integer,
                required: false,
                unique: false,
                default: None,
                position: 0,
            },
            Column {
                id: "c_total".into(),
                table: "Totals".into(),
                name: "Total".into(),
                ty: ColumnType::Formula {
                    expression: "[Base] * 2".into(),
                },
                required: false,
                unique: false,
                default: None,
                position: 1,
            },
        ],
        created_at: 0,
    };
    let mapping = slab_import::suggest_mapping(&["Base".into(), "Total".into()], &table);
    assert_eq!(
        mapping,
        std::collections::BTreeMap::from([("Base".to_owned(), "Base".to_owned())])
    );
    let rows = apply_rows(
        &mut CsvAdapter,
        "Base,Total\n3,6\n",
        &table,
        &std::collections::BTreeMap::from([
            ("Base".to_owned(), "Base".to_owned()),
            ("Total".to_owned(), "Total".to_owned()),
        ]),
    );
    assert_eq!(rows[0].get("Base"), Some(&Value::Integer(3)));
    assert_eq!(rows[0].get("Total"), None);
}

#[test]
fn a_guarded_csv_select_and_attachment_round_trip() {
    let table = Table {
        id: "t_guarded".into(),
        name: "Guarded".into(),
        columns: vec![
            Column {
                id: "c_choice".into(),
                table: "Guarded".into(),
                name: "Choice".into(),
                ty: ColumnType::Select {
                    options: vec!["=A".into(), "B".into()],
                },
                required: false,
                unique: false,
                default: None,
                position: 0,
            },
            Column {
                id: "c_path".into(),
                table: "Guarded".into(),
                name: "Path".into(),
                ty: ColumnType::AttachmentRef,
                required: false,
                unique: false,
                default: None,
                position: 1,
            },
        ],
        created_at: 0,
    };
    let rows = apply_rows(
        &mut CsvAdapter,
        "Choice,Path\n'=A,'=file.pdf\n",
        &table,
        &std::collections::BTreeMap::from([
            ("Choice".to_owned(), "Choice".to_owned()),
            ("Path".to_owned(), "Path".to_owned()),
        ]),
    );
    assert_eq!(rows[0].get("Choice"), Some(&Value::Select("=A".into())));
    assert_eq!(
        rows[0].get("Path"),
        Some(&Value::AttachmentRef("=file.pdf".into()))
    );
}

#[test]
fn csv_text_guards_round_trip_exactly() {
    let table = single_text_table(false);
    let source = "Body\n'=1+1\n''=1+1\n'''=1+1\n'hello\n";
    let rows = apply_rows(
        &mut CsvAdapter,
        source,
        &table,
        &std::collections::BTreeMap::from([("Body".to_owned(), "Body".to_owned())]),
    );
    let values: Vec<&Value> = rows.iter().filter_map(|row| row.get("Body")).collect();
    assert_eq!(
        values,
        vec![
            &Value::Text("=1+1".into()),
            &Value::Text("'=1+1".into()),
            &Value::Text("''=1+1".into()),
            &Value::Text("'hello".into()),
        ]
    );
}

#[test]
fn a_json_bom_is_stripped() {
    let table = single_text_table(false);
    let source = "\u{FEFF}[{\"Body\":{\"Text\":\"x\"}}]";
    let rows = apply_rows(
        &mut JsonAdapter,
        source,
        &table,
        &std::collections::BTreeMap::from([("Body".to_owned(), "Body".to_owned())]),
    );
    assert_eq!(rows[0].get("Body"), Some(&Value::Text("x".into())));
}

#[test]
fn an_explicitly_empty_json_number_is_null() {
    let table = Table {
        id: "t_counts".into(),
        name: "Counts".into(),
        columns: vec![Column {
            id: "c_count".into(),
            table: "Counts".into(),
            name: "Count".into(),
            ty: ColumnType::Integer,
            required: false,
            unique: false,
            default: None,
            position: 0,
        }],
        created_at: 0,
    };
    let rows = apply_rows(
        &mut JsonAdapter,
        r#"[{"Count":""}]"#,
        &table,
        &std::collections::BTreeMap::from([("Count".to_owned(), "Count".to_owned())]),
    );
    assert_eq!(rows[0].get("Count"), Some(&Value::Null));
}

#[test]
fn a_literal_text_null_is_preserved() {
    let table = single_text_table(false);
    let rows = apply_rows(
        &mut JsonAdapter,
        r#"[{"Body":"Null"}]"#,
        &table,
        &std::collections::BTreeMap::from([("Body".to_owned(), "Body".to_owned())]),
    );
    assert_eq!(rows[0].get("Body"), Some(&Value::Text("Null".into())));
}

#[test]
fn json_preview_keeps_columns_in_source_order() {
    for source in [
        r#"[{"Zebra":1,"Alpha":2},{"Zebra":3,"Delta":4,"Beta":5}]"#,
        "{\"Zebra\":1,\"Alpha\":2}\n{\"Zebra\":3,\"Delta\":4,\"Beta\":5}",
    ] {
        let preview = preview(
            &mut JsonAdapter,
            &mut source.as_bytes(),
            &ImportOptions::default(),
        )
        .expect("JSON preview");
        let columns: Vec<_> = preview
            .columns
            .iter()
            .map(|column| column.name.as_str())
            .collect();
        assert_eq!(columns, ["Zebra", "Alpha", "Delta", "Beta"]);
    }
}
