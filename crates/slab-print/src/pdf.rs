use std::fmt::Write as _;

use slab_core::{Column, Layout, LayoutItem, LayoutItemKind, Record, RecordBundle, Schema, Value};

use super::{PrintError, PrintOptions, record_title};

fn check_pdf_margin(margin: f64) -> Result<f64, PrintError> {
    if (0.0..=144.0).contains(&margin) {
        Ok(margin)
    } else {
        Err(PrintError::Layout(
            "PDF margin must be between 0 and 144 points".into(),
        ))
    }
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
    let margin = check_pdf_margin(options.margin_pt)?;
    let content_width = page_width - margin * 2.0;
    let mut pdf = PdfBuilder::new(page_width, page_height, margin, title.clone());
    pdf.heading(
        &record_title(schema, table, &bundle.record),
        10.0,
        PdfFont::Regular,
    );
    if let Some(subtitle) = &options.subtitle {
        pdf.text(margin, pdf.y, 8.0, subtitle, PdfFont::Regular);
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
                pdf.heading(title, 9.0, PdfFont::Bold);
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
        let margin = check_pdf_margin(options.margin_pt)?;
        let content_width = page_width - margin * 2.0;
        let mut pdf = PdfBuilder::new(page_width, page_height, margin, title);
        let columns: Vec<Column> = table.columns.clone();
        if columns.is_empty() {
            pdf.text(margin, pdf.y, 10.0, "No columns", PdfFont::Regular);
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
                let font = PdfFont::Regular;
                self.pdf.text(
                    self.pdf.margin + self.column_width * count_to_f64(index) + 2.0,
                    self.pdf.y,
                    7.5,
                    &font.fit_text(&value.display_string(), self.column_width - 6.0, 7.5),
                    font,
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
                PdfFont::Bold,
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
    pdf.heading(relation, 8.5, PdfFont::Bold);
    pdf.y -= 4.0;
    let column_width = content_width / count_to_f64(visible.len());
    pdf.table_header(&visible, column_width);
    if table_rows.is_empty() {
        pdf.text(pdf.margin, pdf.y, 8.5, "No rows", PdfFont::Regular);
        pdf.y -= 14.0;
    } else {
        for row in table_rows.iter().take(row_count) {
            if pdf.y - 16.0 < pdf.margin + 20.0 {
                pdf.begin_page();
                pdf.table_header(&visible, column_width);
            }
            for (index, column) in visible.iter().enumerate() {
                let value = row.get(&column.name).cloned().unwrap_or(Value::Null);
                let font = PdfFont::Regular;
                pdf.text(
                    pdf.margin + column_width * count_to_f64(index) + 2.0,
                    pdf.y,
                    7.5,
                    &font.fit_text(&value.display_string(), column_width - 6.0, 7.5),
                    font,
                );
            }
            pdf.y -= 13.0;
            pdf.rule_light();
        }
    }
    pdf.y -= 8.0;
    Ok(())
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

    fn text(&mut self, x: f64, y: f64, size: f64, value: &str, font: PdfFont) {
        if value.is_empty() {
            return;
        }
        let font = font.resource();
        let _ = writeln!(
            self.current,
            "0.12 0.12 0.11 rg BT /{font} {size:.1} Tf 1 0 0 1 {x:.2} {y:.2} Tm {} Tj ET",
            pdf_string(value)
        );
    }

    fn heading(&mut self, value: &str, size: f64, font: PdfFont) {
        self.text(self.margin, self.y, size, value, font);
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
        let font = PdfFont::Regular;
        self.text(x, y, 9.5, &font.fit_text(value, width, 9.5), font);
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
            let font = PdfFont::Bold;
            self.text(
                self.margin + width * count_to_f64(index) + 3.0,
                self.y,
                6.8,
                &font.fit_text(&column.name.to_uppercase(), width - 6.0, 6.8),
                font,
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

/// Helvetica advance widths for the WinAnsi Latin-1 block, 0xA0..=0xFF.
const WINANSI_LATIN1_WIDTHS: [u16; 96] = [
    278, 333, 556, 556, 556, 556, 260, 556, 333, 737, 370, 556, 584, 333, 737, 333, 400, 584, 333,
    333, 333, 556, 537, 278, 333, 333, 365, 556, 834, 834, 834, 611, 667, 667, 667, 667, 667, 667,
    1000, 722, 667, 667, 667, 667, 278, 278, 278, 278, 722, 722, 778, 778, 778, 778, 778, 584, 778,
    722, 722, 722, 722, 667, 667, 611, 556, 556, 556, 556, 556, 556, 889, 500, 556, 556, 556, 556,
    278, 278, 278, 278, 556, 556, 556, 556, 556, 556, 556, 584, 611, 556, 556, 556, 556, 500, 556,
    500,
];

/// Helvetica-Bold advances in WinAnsi byte order, from 0x20 through 0xFF.
/// The glyph advances come from Adobe's Core 14 Helvetica-Bold AFM.
const HELVETICA_BOLD_WIDTHS: [u16; 224] = [
    278, 333, 474, 556, 556, 889, 722, 238, 333, 333, 389, 584, 278, 333, 278, 278, 556, 556, 556,
    556, 556, 556, 556, 556, 556, 556, 333, 333, 584, 584, 584, 611, 975, 722, 722, 722, 722, 667,
    611, 778, 722, 278, 556, 722, 611, 833, 722, 778, 667, 778, 722, 667, 611, 722, 667, 944, 667,
    667, 611, 333, 278, 333, 584, 556, 333, 556, 611, 556, 611, 556, 333, 611, 611, 278, 278, 556,
    278, 889, 611, 611, 611, 611, 389, 556, 333, 611, 556, 778, 556, 556, 500, 389, 280, 389, 584,
    350, 556, 350, 278, 556, 500, 1000, 556, 556, 333, 1000, 667, 333, 1000, 350, 611, 350, 350,
    278, 278, 500, 500, 350, 556, 1000, 333, 1000, 556, 333, 944, 350, 500, 667, 278, 333, 556,
    556, 556, 556, 280, 556, 333, 737, 370, 556, 584, 333, 737, 333, 400, 584, 333, 333, 333, 611,
    556, 278, 333, 333, 365, 556, 834, 834, 834, 611, 722, 722, 722, 722, 722, 722, 1000, 722, 667,
    667, 667, 667, 278, 278, 278, 278, 722, 722, 778, 778, 778, 778, 778, 584, 778, 722, 722, 722,
    722, 667, 667, 611, 556, 556, 556, 556, 556, 556, 889, 556, 556, 556, 556, 556, 278, 278, 278,
    278, 611, 611, 611, 611, 611, 611, 611, 584, 611, 611, 611, 611, 611, 556, 611, 556,
];

fn count_to_f64(value: usize) -> f64 {
    f64::from(u32::try_from(value).unwrap_or(u32::MAX))
}

#[derive(Clone, Copy)]
pub(super) enum PdfFont {
    Regular,
    Bold,
}

impl PdfFont {
    const fn resource(self) -> &'static str {
        match self {
            Self::Regular => "F1",
            Self::Bold => "F2",
        }
    }

    fn byte_width(self, byte: u8) -> u64 {
        let width = match (self, byte) {
            (Self::Bold, 0x20..=0xFF) => HELVETICA_BOLD_WIDTHS[usize::from(byte - 0x20)],
            (Self::Regular, 0x20..=0x7E) => HELVETICA_WIDTHS[usize::from(byte - 0x20)],
            (Self::Regular, 0x80..=0x9F) => WINANSI_WIDTHS[usize::from(byte - 0x80)],
            (Self::Regular, 0xA0..=0xFF) => WINANSI_LATIN1_WIDTHS[usize::from(byte - 0xA0)],
            _ => 556,
        };
        u64::from(width)
    }

    /// A fallback such as `₹` → `INR` advances by three glyphs.
    pub(super) fn char_width(self, ch: char) -> u64 {
        if let Some(byte) = winansi_byte(ch) {
            return self.byte_width(byte);
        }
        if let Some(fallback) = ascii_fallback(ch) {
            return fallback.bytes().map(|byte| self.byte_width(byte)).sum();
        }
        self.byte_width(b'?')
    }

    pub(super) fn text_width(self, value: &str, size: f64) -> f64 {
        // A single oversized token can exceed u32 in glyph units.
        let total: u64 = value.chars().map(|ch| self.char_width(ch)).sum();
        total as f64 * size / 1000.0
    }

    pub(super) fn fit_text(self, value: &str, width: f64, size: f64) -> String {
        if width <= 0.0 {
            return String::new();
        }
        if self.text_width(value, size) <= width {
            return value.to_owned();
        }
        // Accumulate advances so a run of zero-width characters never
        // requires a full prefix rescan for each glyph.
        let ellipsis = self.text_width("…", size);
        if ellipsis > width {
            return String::new();
        }
        let mut output = String::new();
        let mut used = 0.0;
        for ch in value.chars() {
            let glyph = self.char_width(ch) as f64 * size / 1000.0;
            if used + glyph + ellipsis > width {
                output.push('…');
                return output;
            }
            output.push(ch);
            used += glyph;
        }
        output
    }
}
