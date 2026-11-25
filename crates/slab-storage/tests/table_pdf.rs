#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod support;

use slab_core::{
    ColumnDraft, ColumnType, Filter, FilterOp, PageQuery, RowData, SortSpec, TableDraft, Value,
};
use slab_print::{PrintOptions, TableReport};
use slab_storage::Workspace;
use support::TestClock;

#[test]
fn filtered_table_streams_one_thousand_five_hundred_rows_to_pdf() {
    let mut workspace =
        Workspace::open_in_memory("Report", Box::new(TestClock::new())).expect("workspace");
    workspace
        .create_table(&TableDraft {
            name: "Items".into(),
            columns: vec![
                ColumnDraft {
                    name: "Name".into(),
                    ty: ColumnType::Text,
                    required: true,
                    unique: false,
                    default: None,
                },
                ColumnDraft {
                    name: "Amount".into(),
                    ty: ColumnType::Integer,
                    required: false,
                    unique: false,
                    default: None,
                },
            ],
        })
        .expect("table");
    let rows: Vec<RowData> = (0..1_500)
        .map(|index| {
            RowData::from([
                ("Name".to_owned(), Value::Text(format!("Keep {index}"))),
                ("Amount".to_owned(), Value::Integer(index)),
            ])
        })
        .collect();
    workspace.insert_rows("Items", &rows).expect("bulk insert");
    let schema = workspace.schema().expect("schema");
    let mut report = TableReport::new(&schema, "Items", &PrintOptions::default()).expect("report");
    let mut query = PageQuery {
        table: "Items".into(),
        limit: 200,
        sort: vec![SortSpec {
            column: "Amount".into(),
            direction: slab_core::SortDirection::Asc,
        }],
        filters: vec![Filter {
            column: "Name".into(),
            op: FilterOp::StartsWith,
            value: Some(Value::Text("Keep".into())),
        }],
        ..PageQuery::default()
    };
    loop {
        let page = workspace.page(&query).expect("page");
        report.push(&page.rows);
        match page.next_cursor {
            Some(cursor) => query.cursor = Some(cursor),
            None => break,
        }
    }
    let result = report.finish().expect("pdf");
    assert_eq!(result.rows_printed, 1_500);
    assert!(!result.truncated);
    assert!(result.pages > 5);
    let text = String::from_utf8_lossy(&result.pdf);
    assert!(text.contains("Keep 1499"));
}
