#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

//! Streaming import adapters.
//!
//! The crate knows nothing about SQLite. An adapter reads rows, infers column
//! types, and hands converted rows to an `ImportSink`. The desktop shell
//! adapts `slab-storage` to that trait.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::io::Read;

use slab_core::error::codes;
use slab_core::{
    ColumnType, CoreError, Decimal, ErrorCode, Money, RowData, RowId, SlabDate, SlabError,
    SlabTimestamp, Table, Value,
};

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
}

impl Default for ImportOptions {
    fn default() -> Self {
        Self {
            has_headers: true,
            delimiter: b',',
            max_preview_rows: 20,
            max_issues: 100,
            skip_invalid: false,
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
    /// Structural issues found while scanning.
    pub issues: Vec<RowIssue>,
}

/// A row read from a source before type conversion.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SourceRow {
    /// One-based row number.
    pub number: u64,
    /// Values keyed by column name.
    pub values: BTreeMap<String, Option<String>>,
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

/// CSV adapter.
#[derive(Clone, Copy, Debug, Default)]
pub struct CsvAdapter;

impl ImportAdapter for CsvAdapter {
    fn format(&self) -> ImportFormat {
        ImportFormat::Csv
    }

    fn read(
        &mut self,
        reader: &mut dyn Read,
        options: &ImportOptions,
        on_columns: &mut dyn FnMut(&[String]),
        on_row: &mut dyn FnMut(SourceRow),
    ) -> Result<(), ImportError> {
        let mut csv_reader = csv::ReaderBuilder::new()
            .delimiter(options.delimiter)
            .has_headers(false)
            .flexible(true)
            .from_reader(reader);
        let mut records = csv_reader.records();
        let first = match records.next() {
            Some(record) => Some(record?),
            None => return Ok(()),
        };
        let headers: Vec<String> = if options.has_headers {
            first
                .as_ref()
                .map(|record| {
                    record
                        .iter()
                        .enumerate()
                        .map(|(index, value)| {
                            if value.trim().is_empty() {
                                format!("Column {}", index + 1)
                            } else {
                                value.trim().to_owned()
                            }
                        })
                        .collect()
                })
                .unwrap_or_default()
        } else {
            (0..first.as_ref().map(csv::StringRecord::len).unwrap_or(0))
                .map(|index| format!("Column {}", index + 1))
                .collect()
        };
        let mut seen = BTreeSet::new();
        for header in &headers {
            if !seen.insert(header.clone()) {
                return Err(ImportError::Format(format!("duplicate header {header:?}")));
            }
        }
        on_columns(&headers);
        let mut row_number = 0_u64;
        if !options.has_headers
            && let Some(record) = first
        {
            emit_csv_row(&headers, &record, 1, on_row);
        }
        for record in records {
            row_number += 1;
            let record = record?;
            emit_csv_row(&headers, &record, row_number, on_row);
        }
        Ok(())
    }
}

fn emit_csv_row(
    headers: &[String],
    record: &csv::StringRecord,
    row_number: u64,
    on_row: &mut dyn FnMut(SourceRow),
) {
    let mut row = SourceRow {
        number: row_number,
        ..SourceRow::default()
    };
    if record.len() != headers.len() {
        row.issues.push(format!(
            "row has {} fields but the header has {}",
            record.len(),
            headers.len()
        ));
    }
    for (index, header) in headers.iter().enumerate() {
        row.values
            .insert(header.clone(), record.get(index).map(str::to_owned));
    }
    on_row(row);
}

/// JSON adapter for arrays of objects and NDJSON.
#[derive(Clone, Copy, Debug, Default)]
pub struct JsonAdapter;

impl ImportAdapter for JsonAdapter {
    fn format(&self) -> ImportFormat {
        ImportFormat::Json
    }

    fn read(
        &mut self,
        reader: &mut dyn Read,
        _options: &ImportOptions,
        on_columns: &mut dyn FnMut(&[String]),
        on_row: &mut dyn FnMut(SourceRow),
    ) -> Result<(), ImportError> {
        let values: serde_json::Value = serde_json::from_reader(reader)?;
        let objects: Vec<&serde_json::Map<String, serde_json::Value>> = match &values {
            serde_json::Value::Array(items) => {
                items.iter().filter_map(|item| item.as_object()).collect()
            }
            serde_json::Value::Object(object) => vec![object],
            _ => {
                return Err(ImportError::Format(
                    "JSON source must be an object or an array of objects".into(),
                ));
            }
        };
        let mut columns: Vec<String> = Vec::new();
        let mut seen: BTreeSet<String> = BTreeSet::new();
        for object in objects {
            let mut added = false;
            for key in object.keys() {
                if seen.insert(key.clone()) {
                    columns.push(key.clone());
                    added = true;
                }
            }
            if added {
                on_columns(&columns);
            }
        }
        for (index, object) in values
            .as_array()
            .map_or_else(|| vec![&values], |items| items.iter().collect())
            .into_iter()
            .enumerate()
        {
            let row_number = u64::try_from(index).unwrap_or(u64::MAX) + 1;
            let mut row = SourceRow {
                number: row_number,
                ..SourceRow::default()
            };
            let Some(object) = object.as_object() else {
                row.issues.push("row is not an object".into());
                on_row(row);
                continue;
            };
            for column in &columns {
                let value = object.get(column);
                row.values.insert(
                    column.clone(),
                    value.map(|value| match value {
                        serde_json::Value::String(text) => text.clone(),
                        serde_json::Value::Null => String::new(),
                        other => other.to_string(),
                    }),
                );
                if value.is_none() {
                    row.issues.push(format!("missing value for {column}"));
                }
            }
            on_row(row);
        }
        Ok(())
    }
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
            let mut columns = columns.borrow_mut();
            for column in &mut *columns {
                let value = row.values.get(&column.name).and_then(Option::as_deref);
                match value.filter(|value| !value.trim().is_empty()) {
                    Some(value) => {
                        let candidate = classify(value);
                        if column.samples.is_empty() {
                            column.ty = candidate.clone();
                        } else {
                            column.ty = merge_types(&column.ty, &candidate);
                        }
                        if column.samples.len() < 3 && !column.samples.iter().any(|s| s == value) {
                            column.samples.push(value.to_owned());
                        }
                    }
                    None => column.nullable = true,
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
                        .map(|column| row.values.get(&column.name).cloned().flatten())
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
    let mut failure: Option<ImportError> = None;
    adapter.read(reader, options, &mut |_| {}, &mut |row| {
        if failure.is_some() {
            return;
        }
        match convert_row(target, mapping, &row) {
            Ok(values) => match sink.insert_row(&target.name, &values) {
                Ok(_) => summary.inserted += 1,
                Err(error) => {
                    summary.skipped += 1;
                    if !options.skip_invalid {
                        failure = Some(error);
                    }
                }
            },
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
    if let Some(error) = failure {
        return Err(error);
    }
    Ok(summary)
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
        let raw = row.values.get(source).cloned().flatten();
        let value = match raw {
            Some(text) if !text.trim().is_empty() => match convert_value(&text, &column.ty) {
                Ok(value) => value,
                Err(message) => {
                    issues.push(RowIssue {
                        row_number: row.number,
                        column: Some(destinaton.clone()),
                        message,
                    });
                    continue;
                }
            },
            _ => Value::Null,
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

fn convert_value(text: &str, column_type: &ColumnType) -> Result<Value, String> {
    let trimmed = text.trim();
    match column_type {
        ColumnType::Text | ColumnType::AttachmentRef => Ok(Value::Text(text.to_owned())),
        ColumnType::Select { options } => {
            if options.iter().any(|option| option == trimmed) {
                Ok(Value::Select(trimmed.to_owned()))
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
        ColumnType::Currency { currency } => {
            let decimal = Decimal::parse(trimmed).map_err(|error| error.to_string())?;
            Money::from_decimal(*currency, decimal)
                .map(Value::Currency)
                .map_err(|error| error.to_string())
        }
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
            .find(|column| column.name.eq_ignore_ascii_case(source))
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
