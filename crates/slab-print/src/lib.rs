#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

//! Report rendering.
//!
//! Layouts and records come from `slab-core`; this crate performs no database
//! access. HTML output is a self-contained document. PDF output is generated
//! directly with base-14 Helvetica fonts, so no font files are redistributed.

use std::fmt::Write as _;

use slab_core::error::codes;
use slab_core::{
    Column, ErrorCode, Layout, LayoutItemKind, Record, RecordBundle, Schema, SlabError, Table,
    Value,
};

mod pdf;

pub use pdf::{TABLE_ROW_CEILING, TablePrintResult, TableReport, render_pdf, render_table_pdf};

/// Paper size.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PageSize {
    /// US Letter, 612 by 792 points.
    Letter,
    /// ISO A4, 595 by 842 points.
    A4,
}

impl PageSize {
    /// Width and height in points.
    pub const fn dimensions(self) -> (f64, f64) {
        match self {
            Self::Letter => (612.0, 792.0),
            Self::A4 => (595.0, 842.0),
        }
    }

    /// The CSS page size name.
    pub const fn css(self) -> &'static str {
        match self {
            Self::Letter => "letter",
            Self::A4 => "A4",
        }
    }
}

/// Print presentation options.
#[derive(Clone, Debug)]
pub struct PrintOptions {
    /// Paper size.
    pub page_size: PageSize,
    /// PDF margin in points, between 0 and 144 inclusive.
    pub margin_pt: f64,
    /// Optional document title override.
    pub title: Option<String>,
    /// Optional subtitle shown under the title.
    pub subtitle: Option<String>,
    /// Include portal tables.
    pub include_portals: bool,
    /// Include fields whose value is null.
    pub include_empty_fields: bool,
}

impl Default for PrintOptions {
    fn default() -> Self {
        Self {
            page_size: PageSize::Letter,
            margin_pt: 48.0,
            title: None,
            subtitle: None,
            include_portals: true,
            include_empty_fields: true,
        }
    }
}

/// Errors produced while rendering.
#[derive(Debug, thiserror::Error)]
pub enum PrintError {
    /// The layout or table reference is invalid.
    #[error("print layout error: {0}")]
    Layout(String),
    /// A table, column, or relation is missing.
    #[error("print data error: {0}")]
    Data(String),
}

impl SlabError for PrintError {
    fn code(&self) -> ErrorCode {
        codes::PRINT
    }
}

/// Renders a record and its portals as a self-contained HTML document.
pub fn render_html(
    schema: &Schema,
    layout: &Layout,
    bundle: &RecordBundle,
    options: &PrintOptions,
) -> Result<String, PrintError> {
    layout
        .validate(schema)
        .map_err(|error| PrintError::Layout(error.to_string()))?;
    let table = schema
        .table(&layout.table)
        .ok_or_else(|| PrintError::Data(format!("missing table {}", layout.table)))?;
    let title = options.title.clone().unwrap_or_else(|| layout.name.clone());
    let mut html = String::new();
    html.push_str("<!doctype html>\n<html lang=\"en\"><head><meta charset=\"utf-8\">\n");
    let _ = writeln!(html, "<title>{}</title>", escape_html(&title));
    html.push_str(
        "<style>\n\
         :root { color-scheme: light; }\n\
         * { box-sizing: border-box; }\n\
         body { margin: 0; color: #1b1b1a; background: #ffffff; font: 11pt/1.45 -apple-system, BlinkMacSystemFont, 'Helvetica Neue', sans-serif; }\n\
         .page { padding: 0; max-width: 720px; margin: 0 auto; }\n\
         header { display: flex; justify-content: space-between; align-items: end; border-bottom: 1px solid #1b1b1a; padding-bottom: 10px; margin-bottom: 22px; }\n\
         h1 { font-size: 20pt; margin: 0; letter-spacing: -0.01em; }\n\
         .meta { text-align: right; color: #5f5e58; font-size: 8.5pt; }\n\
         h2 { font-size: 8.5pt; text-transform: uppercase; letter-spacing: 0.08em; color: #5f5e58; margin: 20px 0 6px; font-weight: 600; }\n\
         .grid { display: grid; grid-template-columns: repeat(12, 1fr); gap: 10px 18px; }\n\
         .field { min-width: 0; }\n\
         .label { font-size: 7.5pt; color: #77756c; text-transform: uppercase; letter-spacing: 0.06em; }\n\
         .value { font-size: 10.5pt; border-bottom: 1px solid #ddd9cc; padding: 3px 0 5px; min-height: 20px; font-variant-numeric: tabular-nums; }\n\
         .portal { margin-top: 10px; }\n\
         table { width: 100%; border-collapse: collapse; font-variant-numeric: tabular-nums; }\n\
         th { text-align: left; font-size: 7.5pt; text-transform: uppercase; letter-spacing: 0.05em; color: #5f5e58; border-bottom: 1px solid #1b1b1a; padding: 6px 6px; font-weight: 600; }\n\
         td { font-size: 9.5pt; padding: 6px 6px; border-bottom: 1px solid #e6e2d6; }\n\
         td.num, th.num { text-align: right; }\n\
         tr { break-inside: avoid; }\n\
         .empty { color: #8a877c; font-style: italic; }\n\
         footer { margin-top: 28px; border-top: 1px solid #ddd9cc; padding-top: 8px; font-size: 7.5pt; color: #8a877c; }\n\
         @page { size: ",
    );
    html.push_str(options.page_size.css());
    html.push_str("; margin: 18mm; }\n@media print { .page { max-width: none; } }\n</style></head><body><div class=\"page\">\n");
    html.push_str("<header><div>");
    let _ = writeln!(html, "<h1>{}</h1>", escape_html(&title));
    if let Some(subtitle) = &options.subtitle {
        let _ = writeln!(html, "<div class=\"meta\">{}</div>", escape_html(subtitle));
    }
    html.push_str("</div><div class=\"meta\">");
    let record_label = record_title(schema, table, &bundle.record);
    let _ = writeln!(html, "{}", escape_html(&record_label));
    html.push_str("</div></header>\n");
    html.push_str("<div class=\"grid\">\n");
    for item in layout.ordered_items() {
        match &item.kind {
            LayoutItemKind::Section { title } => {
                let _ = writeln!(
                    html,
                    "<div style=\"grid-column: span 12\"><h2>{}</h2></div>",
                    escape_html(title)
                );
            }
            LayoutItemKind::Label { text } => {
                let _ = writeln!(
                    html,
                    "<div class=\"field\" style=\"grid-column: span {};\"><div class=\"value\">{}</div></div>",
                    item.w.max(1),
                    escape_html(text)
                );
            }
            LayoutItemKind::Field { column } => {
                let value = bundle.record.get(column).cloned().unwrap_or(Value::Null);
                if value.is_null() && !options.include_empty_fields {
                    continue;
                }
                let label = column;
                let display = value.display_string();
                let _ = writeln!(
                    html,
                    "<div class=\"field\" style=\"grid-column: span {};\"><div class=\"label\">{}</div><div class=\"value\">{}</div></div>",
                    item.w.max(1),
                    escape_html(label),
                    if display.is_empty() {
                        "&nbsp;".to_owned()
                    } else {
                        escape_html(&display)
                    }
                );
            }
            LayoutItemKind::Portal {
                relation,
                columns,
                rows,
            } => {
                if !options.include_portals {
                    continue;
                }
                let relation_definition = schema
                    .relation(relation)
                    .ok_or_else(|| PrintError::Data(format!("missing relation {relation}")))?;
                let child = schema
                    .table(&relation_definition.child_table)
                    .ok_or_else(|| {
                        PrintError::Data(format!(
                            "missing table {}",
                            relation_definition.child_table
                        ))
                    })?;
                let visible: Vec<&Column> = if columns.is_empty() {
                    child.stored_columns().collect()
                } else {
                    columns
                        .iter()
                        .filter_map(|name| child.column(name))
                        .collect()
                };
                let _ = writeln!(
                    html,
                    "<div style=\"grid-column: span 12\"><h2>{}</h2><div class=\"portal\"><table><thead><tr>",
                    escape_html(relation)
                );
                for column in &visible {
                    let class = if is_numeric(&column.ty) {
                        " class=\"num\""
                    } else {
                        ""
                    };
                    let _ = writeln!(html, "<th{class}>{}</th>", escape_html(&column.name));
                }
                html.push_str("</tr></thead><tbody>\n");
                let child_rows = bundle.portal(relation);
                let limit = usize::from(*rows).max(1);
                if child_rows.is_empty() {
                    let _ = writeln!(
                        html,
                        "<tr><td colspan=\"{}\" class=\"empty\">No rows</td></tr>",
                        visible.len().max(1)
                    );
                } else {
                    for row in child_rows.iter().take(limit) {
                        html.push_str("<tr>");
                        for column in &visible {
                            let value = row.get(&column.name).cloned().unwrap_or(Value::Null);
                            let class = if is_numeric(&column.ty) {
                                " class=\"num\""
                            } else {
                                ""
                            };
                            let _ = writeln!(
                                html,
                                "<td{class}>{}</td>",
                                escape_html(&value.display_string())
                            );
                        }
                        html.push_str("</tr>\n");
                    }
                }
                html.push_str("</tbody></table></div></div>\n");
            }
        }
    }
    html.push_str("</div>\n<footer>");
    let _ = writeln!(
        html,
        "Generated by SlabBase · {} · Record #{}",
        escape_html(&layout.name),
        bundle.record.id
    );
    html.push_str("</footer></div></body></html>\n");
    Ok(html)
}

/// Renders a table of rows as HTML.
pub fn render_table_html(
    schema: &Schema,
    table_name: &str,
    rows: &[Record],
    options: &PrintOptions,
) -> Result<String, PrintError> {
    render_table_html_with_status(schema, table_name, rows, options, false)
}

/// Renders a table of rows as HTML, with an explicit warning when the caller
/// stopped at a safety ceiling.
pub fn render_table_html_with_status(
    schema: &Schema,
    table_name: &str,
    rows: &[Record],
    options: &PrintOptions,
    truncated: bool,
) -> Result<String, PrintError> {
    let table = schema
        .table(table_name)
        .ok_or_else(|| PrintError::Data(format!("missing table {table_name}")))?;
    let columns: Vec<&Column> = table.columns.iter().collect();
    let title = options.title.clone().unwrap_or_else(|| table.name.clone());
    let mut html = String::new();
    html.push_str("<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\">");
    let _ = writeln!(html, "<title>{}</title>", escape_html(&title));
    html.push_str(
        "<style>body{font:10pt/1.4 -apple-system,BlinkMacSystemFont,'Helvetica Neue',sans-serif;color:#1b1b1a;margin:0;padding:0}h1{font-size:18pt;margin:0 0 14px;border-bottom:1px solid #1b1b1a;padding-bottom:8px}table{width:100%;border-collapse:collapse;font-variant-numeric:tabular-nums}th{text-align:left;font-size:7.5pt;text-transform:uppercase;letter-spacing:.05em;color:#5f5e58;border-bottom:1px solid #1b1b1a;padding:6px}td{font-size:8.5pt;padding:5px 6px;border-bottom:1px solid #e6e2d6}tr{break-inside:avoid}td.num,th.num{text-align:right}@page{size:",
    );
    html.push_str(options.page_size.css());
    html.push_str(";margin:14mm}</style></head><body>");
    let _ = writeln!(html, "<h1>{}</h1>", escape_html(&title));
    html.push_str("<table><thead><tr>");
    for column in &columns {
        let class = if is_numeric(&column.ty) {
            " class=\"num\""
        } else {
            ""
        };
        let _ = writeln!(html, "<th{class}>{}</th>", escape_html(&column.name));
    }
    html.push_str("</tr></thead><tbody>");
    for row in rows {
        html.push_str("<tr>");
        for column in &columns {
            let value = row.get(&column.name).cloned().unwrap_or(Value::Null);
            let class = if is_numeric(&column.ty) {
                " class=\"num\""
            } else {
                ""
            };
            let _ = writeln!(
                html,
                "<td{class}>{}</td>",
                escape_html(&value.display_string())
            );
        }
        html.push_str("</tr>");
    }
    html.push_str("</tbody></table>");
    if truncated {
        html.push_str(
            "<p style=\"margin:14px 0 0;padding:8px 10px;border:1px solid #a23b2a;color:#a23b2a;font-size:9pt\">Export stopped at 25,000 rows. Filter the table to continue.</p>",
        );
    }
    html.push_str("</body></html>");
    Ok(html)
}

fn record_title(schema: &Schema, table: &Table, record: &Record) -> String {
    let preferred = ["Number", "Name", "Title", "Description"];
    for name in preferred {
        if table.column(name).is_some() {
            let value = record.get(name).cloned().unwrap_or(Value::Null);
            if !value.is_null() {
                return value.display_string();
            }
        }
    }
    let _ = schema;
    format!("{} #{}", table.name, record.id)
}

fn is_numeric(column_type: &slab_core::ColumnType) -> bool {
    matches!(
        column_type,
        slab_core::ColumnType::Integer
            | slab_core::ColumnType::Decimal { .. }
            | slab_core::ColumnType::Currency { .. }
    )
}

fn escape_html(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for ch in value.chars() {
        match ch {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            '\'' => escaped.push_str("&#39;"),
            other => escaped.push(other),
        }
    }
    escaped
}

#[cfg(test)]
mod tests {
    use super::pdf::PdfFont;
    use super::*;
    use slab_core::{CurrencyCode, Decimal, GridSpec, LayoutItem, Money, Table};
    use std::collections::BTreeMap;

    fn schema_and_bundle() -> (Schema, Layout, RecordBundle) {
        let mut schema = Schema::default();
        schema.tables.push(Table {
            id: "t_invoices".into(),
            name: "Invoices".into(),
            columns: vec![Column {
                id: "c_number".into(),
                table: "Invoices".into(),
                name: "Number".into(),
                ty: slab_core::ColumnType::Text,
                required: true,
                unique: true,
                default: None,
                position: 0,
            }],
            created_at: 0,
        });
        let layout = Layout {
            id: "l".into(),
            name: "Invoice".into(),
            table: "Invoices".into(),
            grid: GridSpec::default(),
            items: vec![
                LayoutItem {
                    id: "f".into(),
                    kind: LayoutItemKind::Field {
                        column: "Number".into(),
                    },
                    x: 0,
                    y: 0,
                    w: 6,
                    h: 2,
                },
                LayoutItem {
                    id: "s".into(),
                    kind: LayoutItemKind::Section {
                        title: "Totals".into(),
                    },
                    x: 0,
                    y: 3,
                    w: 12,
                    h: 1,
                },
            ],
            updated_at: 0,
        };
        let mut values = BTreeMap::new();
        values.insert("Number".to_owned(), Value::Text("INV-2026-001".into()));
        let record = Record { id: 1, values };
        (schema, layout, RecordBundle::new(record))
    }

    #[test]
    fn html_contains_record_and_escapes_markup() {
        let (schema, layout, mut bundle) = schema_and_bundle();
        bundle
            .record
            .values
            .insert("Number".into(), Value::Text("<b>INV</b>".into()));
        let html = render_html(&schema, &layout, &bundle, &PrintOptions::default()).expect("html");
        assert!(html.contains("&lt;b&gt;INV&lt;/b&gt;"));
        assert!(html.contains("@page"));
    }

    #[test]
    fn pdf_has_valid_header_and_trailer() {
        let (schema, layout, bundle) = schema_and_bundle();
        let pdf = render_pdf(&schema, &layout, &bundle, &PrintOptions::default()).expect("pdf");
        let text = String::from_utf8_lossy(&pdf);
        assert!(text.starts_with("%PDF-1.7"));
        assert!(text.contains("xref"));
        assert!(text.contains("%%EOF"));
        assert!(text.contains("INV-2026-001"));
    }

    #[test]
    fn pdf_renderers_reject_margins_that_leave_no_usable_page() {
        let (schema, layout, bundle) = schema_and_bundle();
        for margin_pt in [f64::NAN, f64::INFINITY, -1.0, 300.0] {
            let options = PrintOptions {
                margin_pt,
                ..PrintOptions::default()
            };
            assert!(render_pdf(&schema, &layout, &bundle, &options).is_err());
            assert!(TableReport::new(&schema, "Invoices", &options).is_err());
        }
    }

    #[test]
    fn wide_pdf_columns_omit_text_that_cannot_fit() {
        let (mut schema, _, mut bundle) = schema_and_bundle();
        let table = &mut schema.tables[0];
        let original = table.columns[0].clone();
        table.columns.clear();
        for index in 0..100 {
            let name = format!("Wide Column {index}");
            bundle
                .record
                .values
                .insert(name.clone(), Value::Text("Oversized value".into()));
            table.columns.push(Column {
                id: format!("c_{index}"),
                name,
                ..original.clone()
            });
        }
        let mut report =
            TableReport::new(&schema, "Invoices", &PrintOptions::default()).expect("report");
        report.push(&[bundle.record]);
        let pdf = report.finish().expect("PDF");
        let text = String::from_utf8_lossy(&pdf.pdf);
        assert!(!text.contains("WIDE COLUMN"));
        assert!(!text.contains("Oversized value"));
        assert_eq!(pdf.rows_printed, 1);
        assert!(
            PdfFont::Regular
                .fit_text("Oversized value", 2.0, 7.5)
                .is_empty()
        );
    }

    #[test]
    fn pdf_table_headers_fit_the_rendered_bold_font() {
        let (mut schema, _, _) = schema_and_bundle();
        let table = &mut schema.tables[0];
        let second = Column {
            id: "c_status".into(),
            name: "Status".into(),
            position: 1,
            ..table.columns[0].clone()
        };
        table.columns[0].name = "A".repeat(60);
        table.columns.push(second);

        let pdf =
            render_table_pdf(&schema, "Invoices", &[], &PrintOptions::default()).expect("PDF");
        let stream = String::from_utf8_lossy(&pdf);
        // Two 258-point cells leave 252 points each after padding. In bold,
        // 50 A's and an ellipsis would advance 252.28 points at 6.8pt.
        let header = stream
            .lines()
            .find(|line| line.contains("/F2 6.8 Tf 1 0 0 1 51.00"))
            .expect("header");
        assert!(header.ends_with(&format!("({}\\205) Tj ET", "A".repeat(49))));
    }

    #[test]
    fn table_report_renders_all_rows_across_pages() {
        let mut schema = Schema::default();
        schema.tables.push(Table {
            id: "t_items".into(),
            name: "Items".into(),
            columns: vec![Column {
                id: "c_name".into(),
                table: "Items".into(),
                name: "Name".into(),
                ty: slab_core::ColumnType::Text,
                required: true,
                unique: false,
                default: None,
                position: 0,
            }],
            created_at: 0,
        });
        let rows: Vec<Record> = (0..1_500)
            .map(|index| {
                let mut values = std::collections::BTreeMap::new();
                values.insert("Name".to_owned(), Value::Text(format!("Row {index}")));
                Record {
                    id: i64::from(index),
                    values,
                }
            })
            .collect();
        let mut report =
            TableReport::new(&schema, "Items", &PrintOptions::default()).expect("report");
        for chunk in rows.chunks(500) {
            assert_eq!(report.push(chunk), chunk.len());
        }
        let result = report.finish().expect("pdf");
        assert_eq!(result.rows_printed, 1_500);
        assert!(!result.truncated);
        assert!(result.pages > 10, "pages was {}", result.pages);
        let text = String::from_utf8_lossy(&result.pdf);
        assert!(text.contains("Row 0"));
        assert!(text.contains("Row 1499"));
    }

    #[test]
    fn table_report_draws_a_visible_ceiling_warning() {
        let mut schema = Schema::default();
        schema.tables.push(Table {
            id: "t_items".into(),
            name: "Items".into(),
            columns: vec![Column {
                id: "c_name".into(),
                table: "Items".into(),
                name: "Name".into(),
                ty: slab_core::ColumnType::Text,
                required: false,
                unique: false,
                default: None,
                position: 0,
            }],
            created_at: 0,
        });
        let mut values = std::collections::BTreeMap::new();
        values.insert("Name".to_owned(), Value::Text("Only".into()));
        let mut report =
            TableReport::new(&schema, "Items", &PrintOptions::default()).expect("report");
        report.push(&[Record { id: 1, values }]);
        report.mark_truncated();
        let result = report.finish().expect("pdf");
        assert!(result.truncated);
        assert!(String::from_utf8_lossy(&result.pdf).contains("Export stopped at 25000 rows"));
    }

    #[test]
    fn currency_formats_with_code() {
        let currency = CurrencyCode::parse("USD").expect("usd");
        let money =
            Money::from_decimal(currency, Decimal::parse("12.50").expect("amount")).expect("money");
        assert_eq!(Value::Currency(money).display_string(), "USD 12.50");
    }

    #[test]
    fn a_portal_sharing_a_row_with_a_field_is_not_dropped() {
        use slab_core::{Column, ColumnType, DeleteAction, Relation};

        let mut schema = Schema::default();
        schema.tables.push(Table {
            id: "t_invoices".into(),
            name: "Invoices".into(),
            columns: vec![Column {
                id: "c_number".into(),
                table: "Invoices".into(),
                name: "Number".into(),
                ty: ColumnType::Text,
                required: false,
                unique: false,
                default: None,
                position: 0,
            }],
            created_at: 0,
        });
        schema.tables.push(Table {
            id: "t_items".into(),
            name: "Items".into(),
            columns: vec![
                Column {
                    id: "c_label".into(),
                    table: "Items".into(),
                    name: "Label".into(),
                    ty: ColumnType::Text,
                    required: false,
                    unique: false,
                    default: None,
                    position: 0,
                },
                Column {
                    id: "c_invoice".into(),
                    table: "Items".into(),
                    name: "Invoice".into(),
                    ty: ColumnType::Integer,
                    required: false,
                    unique: false,
                    default: None,
                    position: 1,
                },
            ],
            created_at: 0,
        });
        schema.relations.push(Relation {
            id: "r_lines".into(),
            name: "Invoice Items".into(),
            parent_table: "Invoices".into(),
            child_table: "Items".into(),
            child_column: "Invoice".into(),
            on_delete: DeleteAction::Cascade,
        });
        let layout = Layout {
            id: "l".into(),
            name: "Invoice".into(),
            table: "Invoices".into(),
            grid: GridSpec::default(),
            items: vec![
                LayoutItem {
                    id: "field".into(),
                    kind: LayoutItemKind::Field {
                        column: "Number".into(),
                    },
                    x: 0,
                    y: 0,
                    w: 6,
                    h: 2,
                },
                LayoutItem {
                    id: "portal".into(),
                    kind: LayoutItemKind::Portal {
                        relation: "Invoice Items".into(),
                        columns: vec!["Label".into()],
                        rows: 3,
                    },
                    x: 6,
                    y: 0,
                    w: 6,
                    h: 4,
                },
            ],
            updated_at: 0,
        };
        let mut bundle = RecordBundle::new(Record {
            id: 1,
            values: [("Number".to_owned(), Value::Text("INV-1".into()))]
                .into_iter()
                .collect(),
        });
        bundle.portals.insert(
            "Invoice Items".into(),
            vec![Record {
                id: 1,
                values: [("Label".to_owned(), Value::Text("Line A".into()))]
                    .into_iter()
                    .collect(),
            }],
        );
        let pdf = render_pdf(&schema, &layout, &bundle, &PrintOptions::default()).expect("pdf");
        let text = String::from_utf8_lossy(&pdf);
        assert!(text.contains("Invoice Items"), "portal heading is missing");
        assert!(text.contains("Line A"), "portal row is missing");
    }

    #[test]
    fn a_huge_token_does_not_overflow_width_arithmetic() {
        let width = PdfFont::Regular.text_width(&"@".repeat(8_000_000), 9.0);
        assert!(width > 1.0e7);
    }

    #[test]
    fn fitting_text_keeps_a_value_that_exactly_fits() {
        let value = "AAAA";
        let font = PdfFont::Regular;
        let width = font.text_width(value, 9.0);
        assert_eq!(font.fit_text(value, width, 9.0), value);
    }

    #[test]
    fn winansi_punctuation_uses_its_rendered_width() {
        assert_eq!(PdfFont::Regular.char_width('–'), 556);
        assert_eq!(PdfFont::Regular.char_width('—'), 1000);
        assert_eq!(PdfFont::Bold.char_width('‘'), 278);
    }

    #[test]
    fn winansi_latin1_uses_its_rendered_width() {
        assert_eq!(PdfFont::Regular.char_width('\u{A0}'), 278);
        assert_eq!(PdfFont::Regular.char_width('é'), 556);
        assert_eq!(PdfFont::Regular.char_width('æ'), 889);
        assert_eq!(PdfFont::Regular.char_width('ÿ'), 500);
        assert_eq!(PdfFont::Bold.char_width('À'), 722);
    }

    #[test]
    fn fitting_zero_width_text_terminates_and_truncates() {
        let value = format!("{}{}", "\u{200B}".repeat(50_000), "@".repeat(50_000));
        let started = std::time::Instant::now();
        let font = PdfFont::Regular;
        let fitted = font.fit_text(&value, 200.0, 9.0);
        assert!(started.elapsed() < std::time::Duration::from_secs(2));
        assert!(fitted.ends_with('…'));
        assert!(font.text_width(&fitted, 9.0) <= 200.0 + 10.0);
    }

    #[test]
    fn a_subtitle_is_rendered_once() {
        let (schema, layout, bundle) = schema_and_bundle();
        let options = PrintOptions {
            subtitle: Some("Quarterly".into()),
            ..PrintOptions::default()
        };
        let html = render_html(&schema, &layout, &bundle, &options).expect("html");
        assert_eq!(html.matches("Quarterly").count(), 1);
    }

    #[test]
    fn a_truncation_notice_is_outside_the_table_body() {
        let mut schema = Schema::default();
        schema.tables.push(Table {
            id: "t_items".into(),
            name: "Items".into(),
            columns: vec![Column {
                id: "c_name".into(),
                table: "Items".into(),
                name: "Name".into(),
                ty: slab_core::ColumnType::Text,
                required: false,
                unique: false,
                default: None,
                position: 0,
            }],
            created_at: 0,
        });
        let rows = vec![Record {
            id: 1,
            values: [("Name".to_owned(), Value::Text("Only".into()))]
                .into_iter()
                .collect(),
        }];
        let html =
            render_table_html_with_status(&schema, "Items", &rows, &PrintOptions::default(), true)
                .expect("html");
        let notice = html.find("Export stopped").expect("notice");
        let close_body = html.find("</tbody>").expect("tbody");
        let close_table = html.find("</table>").expect("table");
        assert!(notice > close_body && notice > close_table);
    }
}
