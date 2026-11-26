#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

//! Report rendering.
//!
//! Layouts and records come from `slab-core`; this crate performs no database
//! access. HTML output is a self-contained document. PDF output is generated
//! directly with base-14 Helvetica fonts, so no font files are redistributed.

use std::fmt::Write as _;

use slab_core::error::codes;
use slab_core::{
    Column, ErrorCode, Layout, LayoutItem, LayoutItemKind, Record, RecordBundle, Schema, SlabError,
    Table, Value,
};

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
    /// Margin in PDF points.
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

/// Renders a record and its portals as a PDF byte stream.
pub fn render_pdf(
    schema: &Schema,
    layout: &Layout,
    bundle: &RecordBundle,
    options: &PrintOptions,
) -> Result<Vec<u8>, PrintError> {
    layout
        .validate(schema)
        .map_err(|error| PrintError::Layout(error.to_string()))?;
    let table = schema
        .table(&layout.table)
        .ok_or_else(|| PrintError::Data(format!("missing table {}", layout.table)))?;
    let title = options.title.clone().unwrap_or_else(|| layout.name.clone());
    let (page_width, page_height) = options.page_size.dimensions();
    let margin = options.margin_pt;
    let content_width = page_width - margin * 2.0;
    let mut pdf = PdfBuilder::new(page_width, page_height, margin, title.clone());
    pdf.heading(&record_title(schema, table, &bundle.record), 10.0, false);
    if let Some(subtitle) = &options.subtitle {
        pdf.text(margin, pdf.y, 8.0, subtitle, false);
        pdf.y -= 14.0;
    }
    pdf.y -= 6.0;
    let grid_columns = f64::from(layout.grid.columns.max(1));
    let mut previous_row: Option<u16> = None;
    let ordered = layout.ordered_items();
    let mut index = 0;
    while index < ordered.len() {
        let item = ordered[index];
        if matches!(item.kind, LayoutItemKind::Portal { .. }) {
            previous_row = None;
            draw_pdf_portal(&mut pdf, schema, bundle, item, options, content_width)?;
            index += 1;
            continue;
        }
        if matches!(item.kind, LayoutItemKind::Section { .. }) {
            previous_row = None;
            if let LayoutItemKind::Section { title } = &item.kind {
                pdf.ensure(30.0);
                pdf.y -= 8.0;
                pdf.heading(title, 9.0, true);
                pdf.rule();
                pdf.y -= 8.0;
            }
            index += 1;
            continue;
        }
        // Group fields and labels that share a row so they render side by side.
        // Stop at any other item on the same row (a portal or section): the
        // outer loop must still draw it, not swallow it into this group.
        let row_y = item.y;
        let mut group: Vec<&LayoutItem> = Vec::new();
        while index < ordered.len()
            && ordered[index].y == row_y
            && matches!(
                ordered[index].kind,
                LayoutItemKind::Field { .. } | LayoutItemKind::Label { .. }
            )
        {
            group.push(ordered[index]);
            index += 1;
        }
        if group.is_empty() {
            continue;
        }
        let row_height = 34.0;
        pdf.ensure(row_height);
        if previous_row.is_some_and(|previous| row_y > previous.saturating_add(1)) {
            pdf.y -= 6.0;
        }
        previous_row = Some(row_y);
        for item in group {
            let x = margin + (f64::from(item.x) / grid_columns) * content_width;
            let width = (f64::from(item.w.max(1)) / grid_columns) * content_width - 10.0;
            match &item.kind {
                LayoutItemKind::Field { column } => {
                    let value = bundle.record.get(column).cloned().unwrap_or(Value::Null);
                    if value.is_null() && !options.include_empty_fields {
                        continue;
                    }
                    pdf.label(x, column);
                    pdf.field_value(x, pdf.y - 13.0, &value.display_string(), width);
                }
                LayoutItemKind::Label { text } => {
                    pdf.field_value(x, pdf.y - 13.0, text, width);
                }
                _ => {}
            }
        }
        pdf.y -= row_height;
    }
    pdf.finish()
}

/// Maximum rows written to one table PDF. Reaching it is reported explicitly.
pub const TABLE_ROW_CEILING: usize = 25_000;

/// The result of a streamed table report.
#[derive(Clone, Debug, PartialEq)]
pub struct TablePrintResult {
    /// PDF bytes.
    pub pdf: Vec<u8>,
    /// Rows written before the ceiling or end of input.
    pub rows_printed: usize,
    /// True when the safety ceiling stopped the export.
    pub truncated: bool,
    /// Number of PDF pages.
    pub pages: usize,
}

/// A stateful table report that accepts keyset pages incrementally.
pub struct TableReport {
    pdf: PdfBuilder,
    columns: Vec<Column>,
    column_width: f64,
    rows_printed: usize,
    truncated: bool,
}

impl TableReport {
    /// Creates a report and writes the header for the first page.
    pub fn new(
        schema: &Schema,
        table_name: &str,
        options: &PrintOptions,
    ) -> Result<Self, PrintError> {
        let table = schema
            .table(table_name)
            .ok_or_else(|| PrintError::Data(format!("missing table {table_name}")))?;
        let title = options.title.clone().unwrap_or_else(|| table.name.clone());
        let (page_width, page_height) = options.page_size.dimensions();
        let margin = options.margin_pt;
        let content_width = page_width - margin * 2.0;
        let mut pdf = PdfBuilder::new(page_width, page_height, margin, title);
        let columns: Vec<Column> = table.columns.clone();
        if columns.is_empty() {
            pdf.text(margin, pdf.y, 10.0, "No columns", false);
        } else {
            let column_width = content_width / count_to_f64(columns.len());
            pdf.table_header_refs(&columns, column_width);
            return Ok(Self {
                pdf,
                columns,
                column_width,
                rows_printed: 0,
                truncated: false,
            });
        }
        Ok(Self {
            pdf,
            columns,
            column_width: content_width,
            rows_printed: 0,
            truncated: false,
        })
    }

    /// True when the safety ceiling has been reached.
    pub fn is_full(&self) -> bool {
        self.rows_printed >= TABLE_ROW_CEILING
    }

    /// Marks the report as truncated when the caller knows more rows exist.
    pub fn mark_truncated(&mut self) {
        self.truncated = true;
    }

    /// Appends rows and returns how many were written.
    pub fn push(&mut self, rows: &[Record]) -> usize {
        let mut accepted = 0;
        for row in rows {
            if self.rows_printed >= TABLE_ROW_CEILING {
                self.truncated = true;
                break;
            }
            if self.columns.is_empty() {
                break;
            }
            if self.pdf.y - 16.0 < self.pdf.margin + 20.0 {
                self.pdf.begin_page();
                self.pdf.table_header_refs(&self.columns, self.column_width);
            }
            for (index, column) in self.columns.iter().enumerate() {
                let value = row.get(&column.name).cloned().unwrap_or(Value::Null);
                self.pdf.text(
                    self.pdf.margin + self.column_width * count_to_f64(index) + 2.0,
                    self.pdf.y,
                    7.5,
                    &fit_text(&value.display_string(), self.column_width - 6.0, 7.5),
                    false,
                );
            }
            self.pdf.y -= 14.0;
            self.pdf.rule_light();
            self.rows_printed += 1;
            accepted += 1;
        }
        accepted
    }

    /// Finishes the report, drawing an explicit warning when the ceiling was hit.
    pub fn finish(mut self) -> Result<TablePrintResult, PrintError> {
        if self.truncated {
            self.pdf.ensure(30.0);
            self.pdf.y -= 8.0;
            self.pdf.text(
                self.pdf.margin,
                self.pdf.y,
                8.0,
                &format!(
                    "Export stopped at {TABLE_ROW_CEILING} rows; filter the table to continue."
                ),
                true,
            );
            self.pdf.y -= 12.0;
        }
        let pages = self.pdf.page_count();
        let pdf = self.pdf.finish()?;
        Ok(TablePrintResult {
            pdf,
            rows_printed: self.rows_printed,
            truncated: self.truncated,
            pages,
        })
    }
}

/// Renders a table as a PDF.
pub fn render_table_pdf(
    schema: &Schema,
    table_name: &str,
    rows: &[Record],
    options: &PrintOptions,
) -> Result<Vec<u8>, PrintError> {
    let mut report = TableReport::new(schema, table_name, options)?;
    report.push(rows);
    Ok(report.finish()?.pdf)
}

fn draw_pdf_portal(
    pdf: &mut PdfBuilder,
    schema: &Schema,
    bundle: &RecordBundle,
    item: &LayoutItem,
    options: &PrintOptions,
    content_width: f64,
) -> Result<(), PrintError> {
    let LayoutItemKind::Portal {
        relation,
        columns,
        rows,
    } = &item.kind
    else {
        return Ok(());
    };
    if !options.include_portals {
        return Ok(());
    }
    let relation_definition = schema
        .relation(relation)
        .ok_or_else(|| PrintError::Data(format!("missing relation {relation}")))?;
    let child = schema
        .table(&relation_definition.child_table)
        .ok_or_else(|| {
            PrintError::Data(format!("missing table {}", relation_definition.child_table))
        })?;
    let visible: Vec<&Column> = if columns.is_empty() {
        child.stored_columns().collect()
    } else {
        columns
            .iter()
            .filter_map(|name| child.column(name))
            .collect()
    };
    if visible.is_empty() {
        return Ok(());
    }
    let row_count = usize::from(*rows).max(1);
    let table_rows = bundle.portal(relation);
    pdf.ensure(40.0);
    pdf.heading(relation, 8.5, true);
    pdf.y -= 4.0;
    let column_width = content_width / count_to_f64(visible.len());
    pdf.table_header(&visible, column_width);
    if table_rows.is_empty() {
        pdf.text(pdf.margin, pdf.y, 8.5, "No rows", false);
        pdf.y -= 14.0;
    } else {
        for row in table_rows.iter().take(row_count) {
            if pdf.y - 16.0 < pdf.margin + 20.0 {
                pdf.begin_page();
                pdf.table_header(&visible, column_width);
            }
            for (index, column) in visible.iter().enumerate() {
                let value = row.get(&column.name).cloned().unwrap_or(Value::Null);
                pdf.text(
                    pdf.margin + column_width * count_to_f64(index) + 2.0,
                    pdf.y,
                    7.5,
                    &fit_text(&value.display_string(), column_width - 6.0, 7.5),
                    false,
                );
            }
            pdf.y -= 13.0;
            pdf.rule_light();
        }
    }
    pdf.y -= 8.0;
    Ok(())
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

/// A minimal PDF 1.7 writer with base-14 Helvetica fonts.
struct PdfBuilder {
    page_width: f64,
    page_height: f64,
    margin: f64,
    title: String,
    pages: Vec<String>,
    current: String,
    y: f64,
}

impl PdfBuilder {
    fn new(page_width: f64, page_height: f64, margin: f64, title: String) -> Self {
        let mut builder = Self {
            page_width,
            page_height,
            margin,
            title,
            pages: Vec::new(),
            current: String::new(),
            y: page_height - margin,
        };
        builder.begin_page();
        builder
    }

    fn begin_page(&mut self) {
        if !self.current.is_empty() {
            self.pages.push(std::mem::take(&mut self.current));
        }
        self.y = self.page_height - self.margin;
        let mut current = String::new();
        if self.pages.is_empty() {
            let _ = writeln!(
                current,
                "0.10 0.10 0.10 rg BT /F2 18 Tf 1 0 0 1 {:.2} {:.2} Tm {} Tj ET",
                self.margin,
                self.y,
                pdf_string(&self.title)
            );
            self.y -= 18.0;
            let _ = writeln!(
                current,
                "0.78 0.76 0.70 RG 0.6 w {:.2} {:.2} m {:.2} {:.2} l S",
                self.margin,
                self.y,
                self.page_width - self.margin,
                self.y
            );
            self.y -= 20.0;
        } else {
            let _ = writeln!(
                current,
                "0.55 0.54 0.50 rg BT /F1 7 Tf 1 0 0 1 {:.2} {:.2} Tm {} Tj ET",
                self.margin,
                self.page_height - self.margin + 10.0,
                pdf_string(&self.title)
            );
            let _ = writeln!(
                current,
                "0.84 0.82 0.76 RG 0.4 w {:.2} {:.2} m {:.2} {:.2} l S",
                self.margin,
                self.page_height - self.margin + 6.0,
                self.page_width - self.margin,
                self.page_height - self.margin + 6.0
            );
        }
        self.current = current;
    }

    fn ensure(&mut self, height: f64) {
        if self.y - height < self.margin + 18.0 {
            self.begin_page();
        }
    }

    fn text(&mut self, x: f64, y: f64, size: f64, value: &str, bold: bool) {
        if value.is_empty() {
            return;
        }
        let font = if bold { "F2" } else { "F1" };
        let _ = writeln!(
            self.current,
            "0.12 0.12 0.11 rg BT /{font} {size:.1} Tf 1 0 0 1 {x:.2} {y:.2} Tm {} Tj ET",
            pdf_string(value)
        );
    }

    fn heading(&mut self, value: &str, size: f64, bold: bool) {
        self.text(self.margin, self.y, size, value, bold);
        self.y -= size + 4.0;
    }

    fn label(&mut self, x: f64, value: &str) {
        let _ = writeln!(
            self.current,
            "0.47 0.46 0.42 rg BT /F1 6.3 Tf 1 0 0 1 {x:.2} {y:.2} Tm {} Tj ET",
            pdf_string(&value.to_uppercase()),
            y = self.y
        );
    }

    fn field_value(&mut self, x: f64, y: f64, value: &str, width: f64) {
        self.text(x, y, 9.5, &fit_text(value, width, 9.5), false);
        let _ = writeln!(
            self.current,
            "0.85 0.83 0.77 RG 0.4 w {x:.2} {:.2} m {:.2} {:.2} l S",
            y - 4.0,
            x + width,
            y - 4.0
        );
    }

    fn rule(&mut self) {
        let _ = writeln!(
            self.current,
            "0.72 0.70 0.64 RG 0.5 w {:.2} {:.2} m {:.2} {:.2} l S",
            self.margin,
            self.y,
            self.page_width - self.margin,
            self.y
        );
    }

    fn rule_light(&mut self) {
        let _ = writeln!(
            self.current,
            "0.87 0.85 0.80 RG 0.35 w {:.2} {:.2} m {:.2} {:.2} l S",
            self.margin,
            self.y,
            self.page_width - self.margin,
            self.y
        );
    }

    fn table_header_refs(&mut self, columns: &[Column], width: f64) {
        let refs: Vec<&Column> = columns.iter().collect();
        self.table_header(&refs, width);
    }

    fn table_header(&mut self, columns: &[&Column], width: f64) {
        let _ = writeln!(
            self.current,
            "0.94 0.93 0.88 g {:.2} {:.2} {:.2} 16 re f 0.10 0.10 0.10 g",
            self.margin,
            self.y - 4.0,
            width * count_to_f64(columns.len())
        );
        for (index, column) in columns.iter().enumerate() {
            self.text(
                self.margin + width * count_to_f64(index) + 3.0,
                self.y,
                6.8,
                &column.name.to_uppercase(),
                true,
            );
        }
        self.y -= 15.0;
    }

    fn page_count(&self) -> usize {
        self.pages.len() + usize::from(!self.current.is_empty())
    }

    fn finish(mut self) -> Result<Vec<u8>, PrintError> {
        self.pages.push(std::mem::take(&mut self.current));
        let total = self.pages.len();
        for (index, page) in self.pages.iter_mut().enumerate() {
            let footer = format!("Page {} of {}", index + 1, total);
            let _ = writeln!(
                page,
                "0.55 0.54 0.50 rg BT /F1 7 Tf 1 0 0 1 {:.2} {:.2} Tm {} Tj ET",
                self.margin,
                self.margin * 0.5,
                pdf_string(&footer)
            );
        }
        write_pdf(self.page_width, self.page_height, &self.pages)
    }
}

fn write_pdf(page_width: f64, page_height: f64, pages: &[String]) -> Result<Vec<u8>, PrintError> {
    let mut objects: Vec<String> = Vec::new();
    // 1 catalog, 2 pages, 3 regular font, 4 bold font.
    objects.push("<< /Type /Catalog /Pages 2 0 R >>".to_owned());
    let mut kids = String::new();
    for index in 0..pages.len() {
        let page_id = 5 + index * 2;
        if index > 0 {
            kids.push(' ');
        }
        let _ = write!(kids, "{page_id} 0 R");
    }
    objects.push(format!(
        "<< /Type /Pages /Kids [{kids}] /Count {} >>",
        pages.len()
    ));
    objects.push(
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>"
            .to_owned(),
    );
    objects.push(
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica-Bold /Encoding /WinAnsiEncoding >>"
            .to_owned(),
    );
    for (index, page) in pages.iter().enumerate() {
        let content_id = 6 + index * 2;
        objects.push(format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {page_width:.2} {page_height:.2}] /Resources << /Font << /F1 3 0 R /F2 4 0 R >> >> /Contents {content_id} 0 R >>"
        ));
        // `String::len` is the byte length, and every content byte is ASCII
        // (non-WinAnsi text is octal-escaped), so this is the PDF stream size.
        objects.push(format!(
            "<< /Length {} >>\nstream\n{page}endstream",
            page.len()
        ));
    }
    let mut pdf = String::from("%PDF-1.7\n%\u{00e2}\u{00e3}\u{00cf}\u{00d3}\n");
    let mut offsets = Vec::with_capacity(objects.len());
    for (index, object) in objects.iter().enumerate() {
        offsets.push(pdf.len());
        let _ = write!(pdf, "{} 0 obj\n{object}\nendobj\n", index + 1);
    }
    let xref_offset = pdf.len();
    let _ = write!(pdf, "xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1);
    for offset in offsets {
        let _ = writeln!(pdf, "{offset:010} 00000 n ");
    }
    let _ = write!(
        pdf,
        "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref_offset}\n%%EOF\n",
        objects.len() + 1
    );
    Ok(pdf.into_bytes())
}

/// Maps a character to its WinAnsi byte. The base-14 Helvetica fonts use
/// WinAnsiEncoding, which covers Latin-1 plus a small CP1252 punctuation block.
fn winansi_byte(ch: char) -> Option<u8> {
    match ch {
        '\u{20AC}' => Some(0x80),
        '\u{201A}' => Some(0x82),
        '\u{0192}' => Some(0x83),
        '\u{201E}' => Some(0x84),
        '\u{2026}' => Some(0x85),
        '\u{2020}' => Some(0x86),
        '\u{2021}' => Some(0x87),
        '\u{02C6}' => Some(0x88),
        '\u{2030}' => Some(0x89),
        '\u{0160}' => Some(0x8A),
        '\u{2039}' => Some(0x8B),
        '\u{0152}' => Some(0x8C),
        '\u{017D}' => Some(0x8E),
        '\u{2018}' => Some(0x91),
        '\u{2019}' => Some(0x92),
        '\u{201C}' => Some(0x93),
        '\u{201D}' => Some(0x94),
        '\u{2022}' => Some(0x95),
        '\u{2013}' => Some(0x96),
        '\u{2014}' => Some(0x97),
        '\u{02DC}' => Some(0x98),
        '\u{2122}' => Some(0x99),
        '\u{0161}' => Some(0x9A),
        '\u{203A}' => Some(0x9B),
        '\u{0153}' => Some(0x9C),
        '\u{017E}' => Some(0x9E),
        '\u{0178}' => Some(0x9F),
        '\u{00A0}' => Some(b' '),
        ch if ch.is_ascii() => Some(ch as u8),
        ch if (0xA0..=0xFF).contains(&(ch as u32)) => Some(ch as u8),
        _ => None,
    }
}

/// Deterministic ASCII fallbacks for symbols that WinAnsi cannot encode.
fn ascii_fallback(ch: char) -> Option<&'static str> {
    match ch {
        '\u{20B9}' => Some("INR"),
        '\u{20BD}' => Some("RUB"),
        '\u{20A9}' => Some("KRW"),
        '\u{20AA}' => Some("ILS"),
        '\u{20AB}' => Some("VND"),
        '\u{20B4}' => Some("UAH"),
        '\u{20A6}' => Some("NGN"),
        '\u{20B1}' => Some("PHP"),
        '\u{0E3F}' => Some("THB"),
        '\u{20BA}' => Some("TRY"),
        '\u{20A1}' => Some("CRC"),
        '\u{20B2}' => Some("PYG"),
        '\u{20B8}' => Some("KZT"),
        '\u{20AE}' => Some("MNT"),
        '\u{20B5}' => Some("GHS"),
        '\u{20AD}' => Some("LAK"),
        '\u{20BE}' => Some("GEL"),
        '\u{20BC}' => Some("AZN"),
        '\u{09F3}' => Some("BDT"),
        '\u{17DB}' => Some("KHR"),
        '\u{2212}' => Some("-"),
        '\u{2264}' => Some("<="),
        '\u{2265}' => Some(">="),
        '\u{2260}' => Some("!="),
        '\u{2190}' => Some("<-"),
        '\u{2192}' => Some("->"),
        '\u{2191}' => Some("^"),
        '\u{2193}' => Some("v"),
        '\u{200B}' | '\u{200C}' | '\u{200D}' | '\u{FEFF}' => Some(""),
        '\u{2028}' | '\u{2029}' => Some("\n"),
        _ => None,
    }
}

fn push_pdf_byte(output: &mut String, byte: u8) {
    match byte {
        b'(' => output.push_str("\\("),
        b')' => output.push_str("\\)"),
        b'\\' => output.push_str("\\\\"),
        b'\n' => output.push_str("\\n"),
        b'\r' => output.push_str("\\r"),
        b'\t' => output.push_str("\\t"),
        0x20..=0x7E => output.push(char::from(byte)),
        other => {
            let _ = write!(output, "\\{other:03o}");
        }
    }
}

fn pdf_string(value: &str) -> String {
    let mut output = String::with_capacity(value.len() + 2);
    output.push('(');
    for ch in value.chars() {
        if let Some(byte) = winansi_byte(ch) {
            push_pdf_byte(&mut output, byte);
        } else if let Some(fallback) = ascii_fallback(ch) {
            for byte in fallback.bytes() {
                push_pdf_byte(&mut output, byte);
            }
        } else {
            // Unsupported scripts and emoji are replaced deterministically.
            output.push('?');
        }
    }
    output.push(')');
    output
}

const HELVETICA_WIDTHS: [u16; 95] = [
    278, 278, 355, 556, 556, 889, 667, 191, 333, 333, 389, 584, 278, 333, 278, 278, 556, 556, 556,
    556, 556, 556, 556, 556, 556, 556, 278, 278, 584, 584, 584, 556, 1015, 667, 667, 722, 722, 667,
    611, 778, 722, 278, 500, 667, 556, 833, 722, 778, 667, 778, 722, 667, 611, 722, 667, 944, 667,
    667, 611, 278, 278, 278, 469, 556, 333, 556, 556, 500, 556, 556, 278, 556, 556, 222, 222, 500,
    222, 833, 556, 556, 556, 556, 333, 500, 278, 556, 500, 722, 500, 500, 500, 334, 260, 334, 584,
];

/// Helvetica advance widths for the WinAnsi 0x80..0x9F block.
const WINANSI_WIDTHS: [u16; 32] = [
    556, 556, 222, 556, 333, 1000, 556, 556, 333, 1000, 667, 333, 1000, 556, 611, 556, 556, 222,
    222, 333, 333, 350, 556, 1000, 333, 1000, 556, 333, 944, 556, 500, 667,
];

fn count_to_f64(value: usize) -> f64 {
    f64::from(u32::try_from(value).unwrap_or(u32::MAX))
}

fn byte_width(byte: u8) -> u64 {
    if (32..=126).contains(&byte) {
        u64::from(HELVETICA_WIDTHS[(byte - 32) as usize])
    } else if (0x80..=0x9F).contains(&byte) {
        u64::from(WINANSI_WIDTHS[(byte - 0x80) as usize])
    } else {
        556
    }
}

/// The rendered width of one source character after WinAnsi encoding or the
/// deterministic ASCII fallback. A fallback such as `₹` → `INR` occupies
/// three glyphs, not one.
fn char_width(ch: char) -> u64 {
    if let Some(byte) = winansi_byte(ch) {
        return byte_width(byte);
    }
    if let Some(fallback) = ascii_fallback(ch) {
        return fallback.bytes().map(byte_width).sum();
    }
    byte_width(b'?')
}

fn text_width(value: &str, size: f64) -> f64 {
    // Sum in u64: a single oversized token can exceed u32 in glyph units and
    // wrap (or panic in debug) before the size is applied.
    let total: u64 = value.chars().map(char_width).sum();
    total as f64 * size / 1000.0
}

fn fit_text(value: &str, width: f64, size: f64) -> String {
    if width <= 0.0 || text_width(value, size) <= width {
        return value.to_owned();
    }
    // Accumulate widths instead of rebuilding and re-measuring the growing
    // candidate for every character; zero-width glyphs would otherwise force
    // a full prefix rescan per character.
    let ellipsis = text_width("…", size);
    let mut output = String::new();
    let mut used = 0.0;
    for ch in value.chars() {
        let glyph = char_width(ch) as f64 * size / 1000.0;
        if used + glyph + ellipsis > width {
            output.push('…');
            return output;
        }
        output.push(ch);
        used += glyph;
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use slab_core::{CurrencyCode, Decimal, GridSpec, Money, Table};
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
        let width = text_width(&"@".repeat(8_000_000), 9.0);
        assert!(width > 1.0e7);
    }

    #[test]
    fn fitting_text_keeps_a_value_that_exactly_fits() {
        let value = "AAAA";
        let width = text_width(value, 9.0);
        assert_eq!(fit_text(value, width, 9.0), value);
    }

    #[test]
    fn winansi_punctuation_uses_its_rendered_width() {
        assert_eq!(char_width('–'), 556);
        assert_eq!(char_width('—'), 1000);
    }

    #[test]
    fn fitting_zero_width_text_stays_linear_and_truncates() {
        let value = format!("{}{}", "\u{200B}".repeat(50_000), "@".repeat(50_000));
        let started = std::time::Instant::now();
        let fitted = fit_text(&value, 200.0, 9.0);
        assert!(started.elapsed() < std::time::Duration::from_secs(2));
        assert!(fitted.ends_with('…'));
        assert!(text_width(&fitted, 9.0) <= 200.0 + 10.0);
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
