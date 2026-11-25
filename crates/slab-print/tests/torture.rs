#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::BTreeMap;

use slab_core::{
    Column, ColumnType, Layout, LayoutItem, LayoutItemKind, Record, RecordBundle, Schema, Table,
    Value,
};
use slab_print::{PrintOptions, TableReport, render_html, render_pdf};

fn column(table: &str, name: &str, ty: ColumnType, position: u32) -> Column {
    Column {
        id: format!("c_{name}"),
        table: table.into(),
        name: name.into(),
        ty,
        required: false,
        unique: false,
        default: None,
        position,
    }
}

fn schema_for(columns: Vec<Column>) -> Schema {
    Schema {
        tables: vec![Table {
            id: "t".into(),
            name: "Report".into(),
            columns,
            created_at: 0,
        }],
        relations: Vec::new(),
        layouts: Vec::new(),
    }
}

fn assert_pdf(bytes: &[u8]) {
    let text = std::str::from_utf8(bytes).expect("PDF is valid UTF-8 for this writer");
    assert!(text.starts_with("%PDF-1.7\n"), "missing PDF header");
    assert!(text.trim_end().ends_with("%%EOF"), "missing EOF marker");
    let startxref = text.rfind("startxref\n").expect("startxref");
    let number: usize = text[startxref + "startxref\n".len()..]
        .lines()
        .next()
        .expect("startxref value")
        .trim()
        .parse()
        .expect("startxref number");
    let xref = text.find("\nxref\n").expect("xref") + 1;
    assert_eq!(number, xref, "startxref must point at the xref table");
    assert!(text[number..].starts_with("xref\n"));
    let header = text[xref + "xref\n".len()..]
        .lines()
        .next()
        .expect("xref header");
    let mut parts = header.split_whitespace();
    assert_eq!(parts.next(), Some("0"));
    let count: usize = parts.next().expect("count").parse().expect("count");
    assert!(count > 1);
    let entries_start = xref + "xref\n".len() + header.len() + 1;
    for index in 1..count {
        let start = entries_start + index * 20;
        let entry = &text[start..start + 20];
        let offset: usize = entry[..10].parse().expect("xref offset");
        let expected = format!("{index} 0 obj");
        assert!(
            text[offset..].starts_with(&expected),
            "xref object {index} offset {offset} does not point at {expected}"
        );
    }
}

fn report_table() -> (Schema, Vec<Record>) {
    let schema = schema_for(vec![
        column("Report", "Name", ColumnType::Text, 0),
        column("Report", "Amount", ColumnType::Integer, 1),
        column("Report", "Notes", ColumnType::Text, 2),
    ]);
    let rows = (0..5_000)
        .map(|index| Record {
            id: i64::from(index) + 1,
            values: BTreeMap::from([
                ("Name".to_owned(), Value::Text(format!("Row {index}"))),
                ("Amount".to_owned(), Value::Integer(i64::from(index))),
                (
                    "Notes".to_owned(),
                    Value::Text(format!(
                        "https://example.test/{index:040}/{}",
                        "x".repeat(200)
                    )),
                ),
            ]),
        })
        .collect();
    (schema, rows)
}

#[test]
fn five_thousand_row_filtered_table_pdf_is_structurally_valid() {
    let (schema, rows) = report_table();
    let mut report = TableReport::new(&schema, "Report", &PrintOptions::default()).expect("report");
    for chunk in rows.chunks(200) {
        report.push(chunk);
    }
    let result = report.finish().expect("pdf");
    assert_eq!(result.rows_printed, 5_000);
    assert!(!result.truncated);
    assert!(result.pages > 50, "pages {}", result.pages);
    assert_pdf(&result.pdf);
    let text = String::from_utf8_lossy(&result.pdf);
    assert!(text.contains("Row 4999"));
    assert!(text.contains(&format!("Page {} of {}", result.pages, result.pages)));
}

#[test]
fn twenty_five_thousand_row_ceiling_is_explicit_and_clean() {
    let schema = schema_for(vec![column("Report", "Name", ColumnType::Text, 0)]);
    let rows: Vec<Record> = (0..25_100)
        .map(|index| Record {
            id: i64::from(index) + 1,
            values: BTreeMap::from([("Name".to_owned(), Value::Text(format!("Row {index}")))]),
        })
        .collect();
    let mut report = TableReport::new(&schema, "Report", &PrintOptions::default()).expect("report");
    report.push(&rows);
    let result = report.finish().expect("pdf");
    assert_eq!(result.rows_printed, 25_000);
    assert!(result.truncated);
    assert_pdf(&result.pdf);
    let text = String::from_utf8_lossy(&result.pdf);
    assert!(text.contains("Export stopped at 25000 rows"));
    assert!(text.contains("Page 1 of"));
}

fn invoice() -> (Schema, Layout, RecordBundle) {
    let schema = Schema {
        tables: vec![Table {
            id: "t".into(),
            name: "Invoices".into(),
            columns: vec![
                column("Invoices", "Name", ColumnType::Text, 0),
                column("Invoices", "Notes", ColumnType::Text, 1),
                column("Invoices", "Token", ColumnType::Text, 2),
            ],
            created_at: 0,
        }],
        relations: Vec::new(),
        layouts: Vec::new(),
    };
    let layout = Layout {
        id: "l".into(),
        name: "Invoice".into(),
        table: "Invoices".into(),
        grid: slab_core::GridSpec::default(),
        items: vec![
            LayoutItem {
                id: "name".into(),
                kind: LayoutItemKind::Field {
                    column: "Name".into(),
                },
                x: 0,
                y: 0,
                w: 6,
                h: 2,
            },
            LayoutItem {
                id: "notes".into(),
                kind: LayoutItemKind::Field {
                    column: "Notes".into(),
                },
                x: 0,
                y: 2,
                w: 12,
                h: 2,
            },
            LayoutItem {
                id: "token".into(),
                kind: LayoutItemKind::Field {
                    column: "Token".into(),
                },
                x: 0,
                y: 4,
                w: 12,
                h: 2,
            },
        ],
        updated_at: 0,
    };
    let bundle = RecordBundle::new(Record {
        id: 1,
        values: BTreeMap::from([
            (
                "Name".to_owned(),
                Value::Text("Renée O'Brien — € £ ¥ ₹ “smart” 日本語".into()),
            ),
            (
                "Notes".to_owned(),
                Value::Text(
                    "First paragraph with an em-dash — and a euro €.\n\nSecond paragraph with a pound £ and yen ¥.\nThird paragraph with an Indian rupee ₹ and smart quotes “quoted” ‘single’."
                        .into(),
                ),
            ),
            (
                "Token".to_owned(),
                Value::Text("https://example.test/".to_owned() + &"a".repeat(500)),
            ),
        ]),
    });
    (schema, layout, bundle)
}

#[test]
fn form_pdf_uses_deterministic_winansi_fallbacks_for_unicode() {
    let (schema, layout, bundle) = invoice();
    let pdf = render_pdf(&schema, &layout, &bundle, &PrintOptions::default()).expect("pdf");
    assert_pdf(&pdf);
    let text = String::from_utf8_lossy(&pdf);
    // Renée -> Latin-1 e-acute at 0xE9, encoded as an octal escape.
    assert!(text.contains("Ren\\351e"), "accent missing: {text}");
    assert!(text.contains("\\200"), "euro must use WinAnsi 0x80");
    assert!(text.contains("\\227"), "em-dash must use WinAnsi 0x97");
    assert!(
        text.contains("\\223") && text.contains("\\224"),
        "smart quotes"
    );
    assert!(
        text.contains("\\243") && text.contains("\\245"),
        "pound and yen"
    );
    assert!(text.contains("INR"), "rupee must fall back to INR");
    assert!(
        text.contains("??"),
        "unsupported CJK must use deterministic '?'"
    );
    assert!(
        !text.contains("Renée"),
        "raw non-ASCII must not leak into the content stream"
    );
    assert!(!text.contains('—'));
}

#[test]
fn html_render_escapes_hostile_text_and_keeps_long_tokens() {
    let (schema, mut layout, mut bundle) = invoice();
    layout.items.truncate(2);
    bundle.record.values.insert(
        "Name".to_owned(),
        Value::Text("<script>alert('x')</script> & done".into()),
    );
    let html = render_html(&schema, &layout, &bundle, &PrintOptions::default()).expect("html");
    assert!(html.contains("&lt;script&gt;alert(&#39;x&#39;)&lt;/script&gt; &amp; done"));
    assert!(!html.contains("<script>alert"));
    assert!(html.contains("First paragraph"));
}

#[test]
fn multi_page_form_pdf_streams_a_long_portal_and_keeps_structure() {
    let parent = Table {
        id: "t_invoices".into(),
        name: "Invoices".into(),
        columns: vec![
            column("Invoices", "Number", ColumnType::Text, 0),
            column("Invoices", "Notes", ColumnType::Text, 1),
        ],
        created_at: 0,
    };
    let child = Table {
        id: "t_lines".into(),
        name: "Line Items".into(),
        columns: vec![
            column("Line Items", "Invoice", ColumnType::Integer, 0),
            column("Line Items", "Description", ColumnType::Text, 1),
            column("Line Items", "Amount", ColumnType::Integer, 2),
        ],
        created_at: 0,
    };
    let schema = Schema {
        tables: vec![parent, child],
        relations: vec![slab_core::Relation {
            id: "r_lines".into(),
            name: "Invoice Lines".into(),
            parent_table: "Invoices".into(),
            child_table: "Line Items".into(),
            child_column: "Invoice".into(),
            on_delete: slab_core::DeleteAction::Cascade,
        }],
        layouts: Vec::new(),
    };
    let layout = Layout {
        id: "l".into(),
        name: "Invoice".into(),
        table: "Invoices".into(),
        grid: slab_core::GridSpec::default(),
        items: vec![
            LayoutItem {
                id: "number".into(),
                kind: LayoutItemKind::Field {
                    column: "Number".into(),
                },
                x: 0,
                y: 0,
                w: 6,
                h: 2,
            },
            LayoutItem {
                id: "notes".into(),
                kind: LayoutItemKind::Field {
                    column: "Notes".into(),
                },
                x: 0,
                y: 2,
                w: 12,
                h: 2,
            },
            LayoutItem {
                id: "portal".into(),
                kind: LayoutItemKind::Portal {
                    relation: "Invoice Lines".into(),
                    columns: vec!["Description".into(), "Amount".into()],
                    rows: 120,
                },
                x: 0,
                y: 4,
                w: 12,
                h: 6,
            },
        ],
        updated_at: 0,
    };
    let mut portals = BTreeMap::new();
    portals.insert(
        "Invoice Lines".to_owned(),
        (0..120)
            .map(|index| Record {
                id: i64::from(index) + 1,
                values: BTreeMap::from([
                    ("Invoice".to_owned(), Value::Integer(1)),
                    (
                        "Description".to_owned(),
                        Value::Text(format!("Line {index} — long unbroken {}", "y".repeat(120))),
                    ),
                    ("Amount".to_owned(), Value::Integer(i64::from(index) * 25)),
                ]),
            })
            .collect(),
    );
    let bundle = RecordBundle {
        record: Record {
            id: 1,
            values: BTreeMap::from([
                ("Number".to_owned(), Value::Text("INV-100".into())),
                (
                    "Notes".to_owned(),
                    Value::Text("First paragraph — €.\n\nSecond paragraph — ₹.".into()),
                ),
            ]),
        },
        portals,
    };
    let pdf = render_pdf(&schema, &layout, &bundle, &PrintOptions::default()).expect("pdf");
    assert_pdf(&pdf);
    let text = String::from_utf8_lossy(&pdf);
    assert!(text.contains("Line 119"));
    assert!(text.contains("Page 1 of"));
    assert!(
        text.matches("/Type /Page ").count() >= 3,
        "expected a multi-page form, got {} pages",
        text.matches("/Type /Page ").count()
    );
}
