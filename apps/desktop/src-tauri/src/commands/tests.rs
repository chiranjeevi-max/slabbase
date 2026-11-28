use super::*;
use slab_core::{ColumnDraft, ColumnType, SortDirection, SortSpec, TableDraft};
use slab_sync::HybridLogicalClock;
use std::fs;

fn text(name: &str) -> ColumnDraft {
    ColumnDraft {
        name: name.into(),
        ty: ColumnType::Text,
        required: false,
        unique: false,
        default: None,
    }
}

#[test]
fn csv_export_neutralizes_spreadsheet_formulas() {
    let record = Record {
        id: 7,
        values: RowData::from([
            (
                "Note".to_owned(),
                Value::from("=HYPERLINK(\"http://attacker\",\"Report\")"),
            ),
            ("Amount".to_owned(), Value::Integer(-5)),
            ("Plus".to_owned(), Value::from("+1+1")),
            ("At".to_owned(), Value::from("@SUM(A1)")),
            ("Newline".to_owned(), Value::from("\n=SUM(A1)")),
            ("Apostrophe".to_owned(), Value::from("'=1+1")),
        ]),
    };
    let columns = vec![
        "Note".to_owned(),
        "Amount".to_owned(),
        "Plus".to_owned(),
        "At".to_owned(),
        "Newline".to_owned(),
        "Apostrophe".to_owned(),
    ];
    let row = csv_record(&record, &columns);
    assert_eq!(row[0], "7");
    assert_eq!(row[1], "'=HYPERLINK(\"http://attacker\",\"Report\")");
    assert_eq!(row[2], "-5", "a negative numeric value is data");
    assert_eq!(row[3], "'+1+1");
    assert_eq!(row[4], "'@SUM(A1)");
    assert_eq!(row[5], "'\n=SUM(A1)");
    assert_eq!(row[6], "''=1+1", "a literal apostrophe is doubled");
}

#[test]
fn json_export_validates_before_truncating_the_target() {
    let mut workspace =
        Workspace::open_in_memory("Export", Box::new(HybridLogicalClock::new("test")))
            .expect("workspace");
    workspace
        .create_table(&TableDraft {
            name: "Notes".into(),
            columns: vec![text("Body")],
        })
        .expect("table");

    let directory = tempfile::TempDir::new().expect("tempdir");
    let path = directory.path().join("existing.json");
    fs::write(&path, "keep me").expect("seed export");

    let error = export_json_to(
        &workspace,
        path.to_str().expect("path"),
        PageQuery {
            table: "Missing".into(),
            ..PageQuery::default()
        },
    )
    .expect_err("an unknown table must fail");
    assert_eq!(error.code, "SLAB_E_NOT_FOUND");
    assert_eq!(
        fs::read_to_string(&path).expect("read export"),
        "keep me",
        "a failed export truncated the previous file"
    );

    // A bad sort reaches page validation after the scratch file is open.
    let invalid_sort = export_json_to(
        &workspace,
        path.to_str().expect("path"),
        PageQuery {
            table: "Notes".into(),
            sort: vec![SortSpec {
                column: "Missing".into(),
                direction: SortDirection::Asc,
            }],
            ..PageQuery::default()
        },
    )
    .expect_err("an unknown sort column must fail");
    assert_eq!(invalid_sort.code, "SLAB_E_QUERY");
    assert_eq!(
        fs::read_to_string(&path).expect("read export"),
        "keep me",
        "a failed page validation truncated the previous file"
    );
}
