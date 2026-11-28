#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

//! Streaming import adapters.
//!
//! The crate knows nothing about SQLite. An adapter reads rows, infers column
//! types, and hands converted rows to an `ImportSink`. The desktop shell
//! adapts `slab-storage` to that trait.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::io::Read;

use slab_core::error::codes;
use slab_core::{
    ColumnType, CoreError, Decimal, ErrorCode, Money, RowData, RowId, SlabDate, SlabError,
    SlabTimestamp, Table, Value,
};

mod adapters;

pub use adapters::{CsvAdapter, JsonAdapter};

/// Characters a spreadsheet treats as the start of a formula.
pub const SPREADSHEET_PREFIXES: [char; 7] = ['=', '+', '-', '@', '\t', '\r', '\n'];

/// Whether `csv_cell` must prefix `text` with an apostrophe.
///
/// A literal leading apostrophe is doubled so the guard stays reversible even
/// when the original text already begins with one.
pub fn spreadsheet_guard_needed(text: &str) -> bool {
    text.trim_start_matches('\'')
        .starts_with(&SPREADSHEET_PREFIXES[..])
}

/// Removes the single apostrophe `csv_cell` adds to a spreadsheet-formula
/// prefix. Text that was not guarded is returned unchanged.
pub fn unguard_spreadsheet_text(text: &str) -> &str {
    let Some(rest) = text.strip_prefix('\'') else {
        return text;
    };
    if rest
        .trim_start_matches('\'')
        .starts_with(&SPREADSHEET_PREFIXES[..])
    {
        rest
    } else {
        text
    }
}

/// Supported import formats.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImportFormat {
    /// Comma- or delimiter-separated values.
    Csv,
    /// A JSON array of objects or newline-delimited objects.
    Json,
}

impl ImportFormat {
    /// The lowercase format name.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Csv => "csv",
            Self::Json => "json",
        }
    }
}

/// Options for reading a source.
#[derive(Clone, Debug)]
pub struct ImportOptions {
    /// Treat the first CSV record as headers.
    pub has_headers: bool,
    /// CSV field delimiter.
    pub delimiter: u8,
    /// Maximum sample rows returned by a preview.
    pub max_preview_rows: u32,
    /// Maximum issues returned or collected.
    pub max_issues: u32,
    /// Skip rows that fail validation instead of stopping.
    pub skip_invalid: bool,
    /// Maximum rows used to infer column types. Ingestion still streams all rows.
    pub inference_limit: u32,
    /// Number of converted rows sent to the sink in one batch.
    pub batch_size: u32,
}

impl Default for ImportOptions {
    fn default() -> Self {
        Self {
            has_headers: true,
            delimiter: b',',
            max_preview_rows: 20,
            max_issues: 100,
            skip_invalid: false,
            inference_limit: 5_000,
            batch_size: 500,
        }
    }
}

/// One inferred column.
#[derive(Clone, Debug, PartialEq)]
pub struct InferredColumn {
    /// Source column name.
    pub name: String,
    /// The narrowest type that fits every sampled value.
    pub ty: ColumnType,
    /// True when an empty or missing value was seen.
    pub nullable: bool,
    /// Up to three sample values.
    pub samples: Vec<String>,
}

/// One preview row.
#[derive(Clone, Debug, PartialEq)]
pub struct PreviewRow {
    /// One-based row number, excluding the header.
    pub row_number: u64,
    /// Raw values in column order.
    pub values: Vec<Option<String>>,
}

/// A row-level issue.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RowIssue {
    /// One-based row number.
    pub row_number: u64,
    /// Source or target column, when the issue is column-specific.
    pub column: Option<String>,
    /// Human-readable reason.
    pub message: String,
}

/// A preview of a source file.
#[derive(Clone, Debug, PartialEq)]
pub struct ImportPreview {
    /// Source format.
    pub format: ImportFormat,
    /// Columns in first-seen order.
    pub columns: Vec<InferredColumn>,
    /// Sample rows.
    pub sample: Vec<PreviewRow>,
    /// Total rows scanned.
    pub rows_scanned: u64,
    /// Rows actually used for type inference.
    pub inference_rows: u64,
    /// True when scanning stopped feeding type inference at the configured cap.
    pub inference_limited: bool,
    /// Structural issues found while scanning.
    pub issues: Vec<RowIssue>,
}

/// One cell as it appeared in the source, before type conversion.
///
/// `Text` is a scalar; `Json` keeps an object, array, number, or boolean from
/// a JSON source so a tagged `Value` can be recognized by the target type
/// instead of being flattened to a string.
#[derive(Clone, Debug, PartialEq)]
pub enum SourceValue {
    /// The key was absent, or the JSON value was `null`.
    Null,
    /// A scalar string.
    Text(String),
    /// A non-string JSON value.
    Json(serde_json::Value),
}

impl SourceValue {
    /// The text used for inference and preview, matching the old flattening
    /// (a tagged `Value` renders through `display_string`; other objects keep
    /// their JSON text).
    fn to_text(&self) -> Option<String> {
        match self {
            Self::Null => None,
            Self::Text(text) => Some(text.clone()),
            Self::Json(value) => match serde_json::from_value::<Value>(value.clone()) {
                Ok(typed) => Some(typed.display_string()),
                Err(_) => Some(value.to_string()),
            },
        }
    }
}

/// A row read from a source before type conversion.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SourceRow {
    /// One-based row number.
    pub number: u64,
    /// Values keyed by column name.
    pub values: BTreeMap<String, SourceValue>,
    /// True for CSV, where an empty field means SQL NULL. JSON preserves an
    /// explicit empty string so an export round-trips.
    pub empty_text_is_null: bool,
    /// Structural issues in this row.
    pub issues: Vec<String>,
}

/// A reader that streams source rows to callbacks.
pub trait ImportAdapter {
    /// The format this adapter reads.
    fn format(&self) -> ImportFormat;

    /// Streams columns and rows. `on_columns` is called before the first row
    /// and again when a JSON source reveals a new key.
    fn read(
        &mut self,
        reader: &mut dyn Read,
        options: &ImportOptions,
        on_columns: &mut dyn FnMut(&[String]),
        on_row: &mut dyn FnMut(SourceRow),
    ) -> Result<(), ImportError>;
}

/// Infers a schema and samples a source.
pub fn preview(
    adapter: &mut dyn ImportAdapter,
    reader: &mut dyn Read,
    options: &ImportOptions,
) -> Result<ImportPreview, ImportError> {
    let format = adapter.format();
    let columns: RefCell<Vec<InferredColumn>> = RefCell::new(Vec::new());
    let mut issues: Vec<RowIssue> = Vec::new();
    let mut sample: Vec<PreviewRow> = Vec::new();
    let mut rows_scanned = 0_u64;
    let mut inference_rows = 0_u64;
    let inference_limit = u64::from(options.inference_limit.max(1));
    let max_issues = usize::try_from(options.max_issues).unwrap_or(usize::MAX);
    adapter.read(
        reader,
        options,
        &mut |names| {
            let mut columns = columns.borrow_mut();
            for name in names {
                if !columns.iter().any(|column| &column.name == name) {
                    columns.push(InferredColumn {
                        name: name.clone(),
                        ty: ColumnType::Text,
                        nullable: false,
                        samples: Vec::new(),
                    });
                }
            }
        },
        &mut |row| {
            rows_scanned += 1;
            let infer_this_row = inference_rows < inference_limit;
            if infer_this_row {
                inference_rows += 1;
            }
            let mut columns = columns.borrow_mut();
            if infer_this_row {
                for column in &mut *columns {
                    let value = row.values.get(&column.name).and_then(SourceValue::to_text);
                    match value.filter(|value| !value.trim().is_empty()) {
                        Some(value) => {
                            let candidate = classify(&value);
                            if column.samples.is_empty() {
                                column.ty = candidate.clone();
                            } else {
                                column.ty = merge_types(&column.ty, &candidate);
                            }
                            if column.samples.len() < 3
                                && !column.samples.iter().any(|sample| sample == &value)
                            {
                                column.samples.push(value);
                            }
                        }
                        None => column.nullable = true,
                    }
                }
            }
            for issue in &row.issues {
                if issues.len() < max_issues {
                    issues.push(RowIssue {
                        row_number: row.number,
                        column: None,
                        message: issue.clone(),
                    });
                }
            }
            if sample.len() < usize::try_from(options.max_preview_rows).unwrap_or(0) {
                sample.push(PreviewRow {
                    row_number: row.number,
                    values: columns
                        .iter()
                        .map(|column| row.values.get(&column.name).and_then(SourceValue::to_text))
                        .collect(),
                });
            }
        },
    )?;
    let mut columns = columns.into_inner();
    for column in &mut columns {
        if column.samples.is_empty() {
            column.ty = ColumnType::Text;
        }
    }
    Ok(ImportPreview {
        format,
        columns,
        sample,
        rows_scanned,
        inference_rows,
        inference_limited: rows_scanned > inference_limit,
        issues,
    })
}

fn classify(value: &str) -> ColumnType {
    let trimmed = value.trim();
    if matches!(
        trimmed.to_ascii_lowercase().as_str(),
        "true" | "false" | "yes" | "no"
    ) {
        return ColumnType::Boolean;
    }
    if trimmed.parse::<i64>().is_ok() {
        return ColumnType::Integer;
    }
    if Decimal::parse(trimmed).is_ok() {
        return ColumnType::Decimal { scale: 3 };
    }
    if SlabDate::parse(trimmed).is_ok() {
        return ColumnType::Date;
    }
    if SlabTimestamp::parse(trimmed).is_ok() {
        return ColumnType::Timestamp;
    }
    ColumnType::Text
}

fn merge_types(current: &ColumnType, candidate: &ColumnType) -> ColumnType {
    if current == candidate {
        return current.clone();
    }
    match (current, candidate) {
        (ColumnType::Text, _) | (_, ColumnType::Text) => ColumnType::Text,
        (ColumnType::Integer, ColumnType::Decimal { scale })
        | (ColumnType::Decimal { scale }, ColumnType::Integer) => {
            ColumnType::Decimal { scale: *scale }
        }
        (ColumnType::Date, ColumnType::Timestamp) | (ColumnType::Timestamp, ColumnType::Date) => {
            ColumnType::Timestamp
        }
        _ => ColumnType::Text,
    }
}

/// A row-level validation report.
#[derive(Clone, Debug, PartialEq)]
pub struct ValidationReport {
    /// Total data rows read.
    pub rows: u64,
    /// Rows that converted cleanly.
    pub valid_rows: u64,
    /// Issues found.
    pub issues: Vec<RowIssue>,
}

/// Validates a source against a target table and column mapping.
pub fn validate(
    adapter: &mut dyn ImportAdapter,
    reader: &mut dyn Read,
    options: &ImportOptions,
    target: &Table,
    mapping: &BTreeMap<String, String>,
) -> Result<ValidationReport, ImportError> {
    let mut rows = 0_u64;
    let mut valid_rows = 0_u64;
    let mut issues = Vec::new();
    let max_issues = usize::try_from(options.max_issues).unwrap_or(usize::MAX);
    adapter.read(reader, options, &mut |_| {}, &mut |row| {
        rows += 1;
        let mut row_valid = true;
        for issue in &row.issues {
            row_valid = false;
            if issues.len() < max_issues {
                issues.push(RowIssue {
                    row_number: row.number,
                    column: None,
                    message: issue.clone(),
                });
            }
        }
        match convert_row(target, mapping, &row) {
            Ok(_) => {}
            Err(row_issues) => {
                row_valid = false;
                for issue in row_issues {
                    if issues.len() < max_issues {
                        issues.push(issue);
                    }
                }
            }
        }
        if row_valid {
            valid_rows += 1;
        }
    })?;
    Ok(ValidationReport {
        rows,
        valid_rows,
        issues,
    })
}

/// A sink supplied by the caller. The shell adapts storage to this trait.
pub trait ImportSink {
    /// Inserts one validated row and returns its id.
    fn insert_row(&mut self, table: &str, values: &RowData) -> Result<RowId, ImportError>;

    /// Inserts a bounded batch. The default preserves correctness for sinks
    /// that cannot commit in bulk; storage-backed sinks override it.
    fn insert_batch(&mut self, table: &str, rows: &[RowData]) -> Result<Vec<RowId>, ImportError> {
        let mut ids = Vec::with_capacity(rows.len());
        for row in rows {
            ids.push(self.insert_row(table, row)?);
        }
        Ok(ids)
    }
}

/// The result of applying an import.
#[derive(Clone, Debug, PartialEq)]
pub struct ImportSummary {
    /// Rows inserted.
    pub inserted: u64,
    /// Rows skipped because of validation issues.
    pub skipped: u64,
    /// Issues encountered while applying.
    pub issues: Vec<RowIssue>,
}

/// Applies an import to a sink.
///
/// Call `validate` first when the caller needs an all-or-nothing decision;
/// with `skip_invalid = false`, this function stops at the first bad row.
pub fn apply(
    adapter: &mut dyn ImportAdapter,
    reader: &mut dyn Read,
    options: &ImportOptions,
    target: &Table,
    mapping: &BTreeMap<String, String>,
    sink: &mut dyn ImportSink,
) -> Result<ImportSummary, ImportError> {
    let mut summary = ImportSummary {
        inserted: 0,
        skipped: 0,
        issues: Vec::new(),
    };
    let max_issues = usize::try_from(options.max_issues).unwrap_or(usize::MAX);
    let batch_size = usize::try_from(options.batch_size.clamp(1, 10_000)).unwrap_or(1);
    let mut batch: Vec<RowData> = Vec::with_capacity(batch_size);
    let mut failure: Option<ImportError> = None;
    adapter.read(reader, options, &mut |_| {}, &mut |row| {
        if failure.is_some() {
            return;
        }
        if !row.issues.is_empty() {
            summary.skipped += 1;
            for issue in &row.issues {
                if summary.issues.len() < max_issues {
                    summary.issues.push(RowIssue {
                        row_number: row.number,
                        column: None,
                        message: issue.clone(),
                    });
                }
            }
            if !options.skip_invalid {
                failure = Some(ImportError::Validation {
                    issues: summary.issues.clone(),
                });
            }
            return;
        }
        match convert_row(target, mapping, &row) {
            Ok(values) => {
                batch.push(values);
                if batch.len() >= batch_size
                    && let Err(error) = flush_batch(sink, &target.name, &mut batch, &mut summary)
                {
                    if !options.skip_invalid {
                        failure = Some(error);
                    } else {
                        summary.skipped += 1;
                        if summary.issues.len() < max_issues {
                            summary.issues.push(RowIssue {
                                row_number: row.number,
                                column: None,
                                message: error.to_string(),
                            });
                        }
                    }
                }
            }
            Err(issues) => {
                summary.skipped += 1;
                let fatal = !options.skip_invalid;
                for issue in issues {
                    if summary.issues.len() < max_issues {
                        summary.issues.push(issue);
                    }
                }
                if fatal {
                    failure = Some(ImportError::Validation {
                        issues: summary.issues.clone(),
                    });
                }
            }
        }
    })?;
    if failure.is_none()
        && let Err(error) = flush_batch(sink, &target.name, &mut batch, &mut summary)
    {
        failure = Some(error);
    }
    if let Some(error) = failure {
        return Err(error);
    }
    Ok(summary)
}

fn flush_batch(
    sink: &mut dyn ImportSink,
    table: &str,
    batch: &mut Vec<RowData>,
    summary: &mut ImportSummary,
) -> Result<(), ImportError> {
    if batch.is_empty() {
        return Ok(());
    }
    let ids = sink.insert_batch(table, batch)?;
    summary.inserted += u64::try_from(ids.len()).unwrap_or(u64::MAX);
    batch.clear();
    Ok(())
}

fn convert_row(
    target: &Table,
    mapping: &BTreeMap<String, String>,
    row: &SourceRow,
) -> Result<RowData, Vec<RowIssue>> {
    let mut issues = Vec::new();
    let mut output = RowData::new();
    for (source, destinaton) in mapping {
        let Some(column) = target.column(destinaton) else {
            issues.push(RowIssue {
                row_number: row.number,
                column: Some(destinaton.clone()),
                message: format!(
                    "target column {} does not exist on {}",
                    destinaton, target.name
                ),
            });
            continue;
        };
        // Computed columns are virtual. A source column that happens to share
        // their name (an export includes them) is skipped, not a row error.
        if column.ty.is_computed() {
            continue;
        }
        let converted = match row.values.get(source) {
            None | Some(SourceValue::Null) => Ok(Value::Null),
            Some(SourceValue::Text(text))
                if text.trim().is_empty()
                    && (row.empty_text_is_null || !matches!(column.ty, ColumnType::Text)) =>
            {
                Ok(Value::Null)
            }
            Some(SourceValue::Text(text)) => {
                convert_value(text, &column.ty, row.empty_text_is_null)
            }
            Some(SourceValue::Json(json)) => convert_json(json, &column.ty),
        };
        let value = match converted {
            Ok(value) => value,
            Err(message) => {
                issues.push(RowIssue {
                    row_number: row.number,
                    column: Some(destinaton.clone()),
                    message,
                });
                continue;
            }
        };
        if column.required && value.is_null() {
            issues.push(RowIssue {
                row_number: row.number,
                column: Some(destinaton.clone()),
                message: format!("{destinaton} is required"),
            });
            continue;
        }
        if !value.is_null() && !column.ty.accepts(&value) {
            issues.push(RowIssue {
                row_number: row.number,
                column: Some(destinaton.clone()),
                message: format!("value does not fit column type {}", column.ty.name()),
            });
            continue;
        }
        output.insert(destinaton.clone(), value);
    }
    if issues.is_empty() {
        Ok(output)
    } else {
        Err(issues)
    }
}

/// Converts a JSON cell with the target type in mind.
///
/// A tagged `Value` is used directly when it matches the column. A tag for a
/// different type is preserved as JSON text in a text-like column instead of
/// being silently flattened (`{"Integer":7}` must not become the text "7").
fn convert_json(value: &serde_json::Value, column_type: &ColumnType) -> Result<Value, String> {
    let scalar = match value {
        serde_json::Value::String(text) => Some(text.clone()),
        serde_json::Value::Number(number) => Some(number.to_string()),
        serde_json::Value::Bool(flag) => Some(flag.to_string()),
        _ => None,
    };
    if let Some(text) = scalar {
        return convert_value(&text, column_type, false);
    }
    let text_like = matches!(
        column_type,
        ColumnType::Text | ColumnType::Select { .. } | ColumnType::AttachmentRef
    );
    match serde_json::from_value::<Value>(value.clone()) {
        Ok(typed) if typed.is_null() => Ok(Value::Null),
        Ok(typed) if column_type.accepts(&typed) => Ok(typed),
        _ if text_like => convert_value(&value.to_string(), column_type, false),
        Ok(_) => Err(format!(
            "value {value} does not fit column type {}",
            column_type.name()
        )),
        Err(_) => Err(format!(
            "value {value} is not a scalar for column type {}",
            column_type.name()
        )),
    }
}

fn convert_value(text: &str, column_type: &ColumnType, spreadsheet: bool) -> Result<Value, String> {
    let trimmed = text.trim();
    let unguard = |value: &str| {
        if spreadsheet {
            unguard_spreadsheet_text(value).to_owned()
        } else {
            value.to_owned()
        }
    };
    match column_type {
        ColumnType::Text => Ok(Value::Text(unguard(text))),
        ColumnType::AttachmentRef => Ok(Value::AttachmentRef(unguard(text))),
        ColumnType::Select { options } => {
            // A select option that starts with a spreadsheet-formula
            // character was protected with an apostrophe on export. Try the
            // unguarded text first, then the literal text (an option may
            // itself start with an apostrophe).
            let candidate = unguard(trimmed);
            if let Some(option) = options
                .iter()
                .find(|option| **option == candidate || (spreadsheet && *option == trimmed))
            {
                Ok(Value::Select(option.clone()))
            } else {
                Err(format!("{trimmed:?} is not one of {}", options.join(", ")))
            }
        }
        ColumnType::Integer => trimmed
            .parse::<i64>()
            .map(Value::Integer)
            .map_err(|_| format!("{trimmed:?} is not an integer")),
        ColumnType::Decimal { .. } => Decimal::parse(trimmed)
            .map(Value::Decimal)
            .map_err(|error| error.to_string()),
        ColumnType::Currency { currency } => Money::parse(trimmed, *currency)
            .map(Value::Currency)
            .map_err(|error| error.to_string()),
        ColumnType::Boolean => match trimmed.to_ascii_lowercase().as_str() {
            "true" | "yes" | "1" => Ok(Value::Boolean(true)),
            "false" | "no" | "0" => Ok(Value::Boolean(false)),
            _ => Err(format!("{trimmed:?} is not a boolean")),
        },
        ColumnType::Date => SlabDate::parse(trimmed)
            .map(Value::Date)
            .map_err(|error| error.to_string()),
        ColumnType::Timestamp => SlabTimestamp::parse(trimmed)
            .map(Value::Timestamp)
            .map_err(|error| error.to_string()),
        ColumnType::Formula { .. } | ColumnType::Lookup { .. } => {
            Err("computed columns cannot be imported".into())
        }
    }
}

/// A suggested mapping from source columns to target columns by name.
pub fn suggest_mapping(source_columns: &[String], target: &Table) -> BTreeMap<String, String> {
    let mut mapping = BTreeMap::new();
    for source in source_columns {
        if let Some(column) = target
            .columns
            .iter()
            .find(|column| !column.ty.is_computed() && column.name.eq_ignore_ascii_case(source))
        {
            mapping.insert(source.clone(), column.name.clone());
        }
    }
    mapping
}

/// Errors produced by import adapters and validation.
#[derive(Debug, thiserror::Error)]
pub enum ImportError {
    /// The source was malformed.
    #[error("import format error: {0}")]
    Format(String),
    /// A row failed validation.
    #[error("import validation failed with {} issue(s)", issues.len())]
    Validation {
        /// The issues found.
        issues: Vec<RowIssue>,
    },
    /// The sink rejected a row.
    #[error("import sink error: {0}")]
    Sink(String),
    /// CSV parsing failed.
    #[error("csv error: {0}")]
    Csv(#[from] csv::Error),
    /// JSON parsing failed.
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
    /// The source could not be read.
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    /// A domain conversion failed.
    #[error(transparent)]
    Core(#[from] CoreError),
}

impl SlabError for ImportError {
    fn code(&self) -> ErrorCode {
        match self {
            Self::Core(error) => error.code(),
            _ => codes::IMPORT,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use slab_core::{Column, Table};

    fn table() -> Table {
        Table {
            id: "t".into(),
            name: "Clients".into(),
            columns: vec![
                Column {
                    id: "c1".into(),
                    table: "Clients".into(),
                    name: "Name".into(),
                    ty: ColumnType::Text,
                    required: true,
                    unique: false,
                    default: None,
                    position: 0,
                },
                Column {
                    id: "c2".into(),
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
        }
    }

    #[test]
    fn spreadsheet_guards_are_reversible() {
        for text in ["=1+1", "'=1+1", "''=1+1", "'hello", "plain"] {
            let guarded = if spreadsheet_guard_needed(text) {
                format!("'{text}")
            } else {
                text.to_owned()
            };
            assert_eq!(unguard_spreadsheet_text(&guarded), text);
        }
        assert!(!spreadsheet_guard_needed("hello"));
        assert!(spreadsheet_guard_needed("=1+1"));
        assert!(spreadsheet_guard_needed("'=1+1"));
    }

    #[test]
    fn csv_preview_infers_types() {
        let source = "Name,Seats,Active\nNorthlight,12,true\nHiraya,,false\n";
        let mut adapter = CsvAdapter;
        let preview = preview(
            &mut adapter,
            &mut source.as_bytes(),
            &ImportOptions::default(),
        )
        .expect("preview");
        assert_eq!(preview.rows_scanned, 2);
        assert_eq!(preview.columns[0].ty, ColumnType::Text);
        assert_eq!(preview.columns[1].ty, ColumnType::Integer);
        assert!(preview.columns[1].nullable);
        assert_eq!(preview.columns[2].ty, ColumnType::Boolean);
    }

    #[test]
    fn json_preview_reads_objects() {
        let source = r#"[{"Name":"Northlight","Seats":12},{"Name":"Hiraya","Seats":4}]"#;
        let mut adapter = JsonAdapter;
        let preview = preview(
            &mut adapter,
            &mut source.as_bytes(),
            &ImportOptions::default(),
        )
        .expect("preview");
        assert_eq!(preview.rows_scanned, 2);
        assert_eq!(preview.columns.len(), 2);
    }

    #[test]
    fn json_streams_ndjson_records() {
        let source = "{\"Name\":\"Northlight\",\"Seats\":12}\n{\"Name\":\"Hiraya\",\"Seats\":4}\n";
        let mut adapter = JsonAdapter;
        let preview = preview(
            &mut adapter,
            &mut source.as_bytes(),
            &ImportOptions::default(),
        )
        .expect("preview");
        assert_eq!(preview.rows_scanned, 2);
        assert_eq!(preview.columns.len(), 2);
    }

    #[test]
    fn csv_inference_stops_at_the_configured_prefix() {
        let source = "Name,Seats\nA,1\nB,2\nC,three\nD,four\n";
        let mut adapter = CsvAdapter;
        let options = ImportOptions {
            inference_limit: 2,
            ..ImportOptions::default()
        };
        let preview = preview(&mut adapter, &mut source.as_bytes(), &options).expect("preview");
        assert_eq!(preview.rows_scanned, 4);
        assert_eq!(preview.inference_rows, 2);
        assert!(preview.inference_limited);
        assert_eq!(preview.columns[1].ty, ColumnType::Integer);
    }

    #[test]
    fn large_json_array_commits_in_bounded_batches() {
        struct BatchedSink {
            batches: Vec<usize>,
            rows: usize,
        }
        impl ImportSink for BatchedSink {
            fn insert_row(
                &mut self,
                _table: &str,
                _values: &RowData,
            ) -> Result<RowId, ImportError> {
                self.rows += 1;
                Ok(i64::try_from(self.rows).unwrap_or(0))
            }

            fn insert_batch(
                &mut self,
                _table: &str,
                rows: &[RowData],
            ) -> Result<Vec<RowId>, ImportError> {
                self.batches.push(rows.len());
                self.rows += rows.len();
                Ok((0..rows.len())
                    .map(|index| i64::try_from(self.rows + index).unwrap_or(0))
                    .collect())
            }
        }
        let mut source = String::from("[");
        for index in 0..25_000 {
            if index > 0 {
                source.push(',');
            }
            source.push_str(&format!("{{\"Name\":\"Row {index}\",\"Seats\":{index}}}"));
        }
        source.push(']');
        let mut adapter = JsonAdapter;
        let target = table();
        let mapping = suggest_mapping(&["Name".to_owned(), "Seats".to_owned()], &target);
        let options = ImportOptions {
            batch_size: 500,
            ..ImportOptions::default()
        };
        let mut sink = BatchedSink {
            batches: Vec::new(),
            rows: 0,
        };
        let summary = apply(
            &mut adapter,
            &mut source.as_bytes(),
            &options,
            &target,
            &mapping,
            &mut sink,
        )
        .expect("apply");
        assert_eq!(summary.inserted, 25_000);
        assert_eq!(sink.rows, 25_000);
        assert_eq!(sink.batches.iter().sum::<usize>(), 25_000);
        assert!(sink.batches.len() >= 50);
        assert!(sink.batches.iter().all(|size| *size <= 500));
    }

    #[test]
    fn mapping_validates_required_values() {
        let source = "Name,Seats\n,12\nHiraya,4\n";
        let mut adapter = CsvAdapter;
        let target = table();
        let mapping = suggest_mapping(&["Name".to_owned(), "Seats".to_owned()], &target);
        let report = validate(
            &mut adapter,
            &mut source.as_bytes(),
            &ImportOptions::default(),
            &target,
            &mapping,
        )
        .expect("validate");
        assert_eq!(report.rows, 2);
        assert_eq!(report.valid_rows, 1);
        assert_eq!(report.issues.len(), 1);
        assert_eq!(report.issues[0].column.as_deref(), Some("Name"));
    }

    #[test]
    fn apply_writes_to_a_sink() {
        struct MemorySink(Vec<RowData>);
        impl ImportSink for MemorySink {
            fn insert_row(&mut self, _table: &str, values: &RowData) -> Result<RowId, ImportError> {
                self.0.push(values.clone());
                Ok(i64::try_from(self.0.len()).unwrap_or(0))
            }
        }
        let source = "Name,Seats\nNorthlight,12\nHiraya,4\n";
        let mut adapter = CsvAdapter;
        let target = table();
        let mapping = suggest_mapping(&["Name".to_owned(), "Seats".to_owned()], &target);
        let mut sink = MemorySink(Vec::new());
        let summary = apply(
            &mut adapter,
            &mut source.as_bytes(),
            &ImportOptions::default(),
            &target,
            &mapping,
            &mut sink,
        )
        .expect("apply");
        assert_eq!(summary.inserted, 2);
        assert_eq!(sink.0.len(), 2);
        assert_eq!(sink.0[0].get("Seats"), Some(&Value::Integer(12)));
    }
}
