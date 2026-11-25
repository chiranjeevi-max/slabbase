//! Typed payloads for commands that are not already defined in `slab-core`.

use serde::{Deserialize, Serialize};
use slab_import::{ImportOptions, ImportPreview, ImportSummary, RowIssue, ValidationReport};
use slab_print::{PageSize, PrintOptions};
use slab_storage::{IntegrityReport, WorkspaceInfo};
use ts_rs::TS;

use crate::error::ApiError;

/// Workspace information for the shell.
#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../src/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceInfoDto {
    /// Workspace name.
    pub name: String,
    /// File path.
    pub path: Option<String>,
    /// Format version.
    pub format_version: u32,
    /// SQLite journal mode.
    pub journal_mode: String,
    /// Tables and row counts.
    pub tables: Vec<TableSummaryDto>,
}

/// A table and its row count.
#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../src/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct TableSummaryDto {
    /// Table name.
    pub name: String,
    /// Row count.
    pub row_count: u64,
}

impl From<WorkspaceInfo> for WorkspaceInfoDto {
    fn from(info: WorkspaceInfo) -> Self {
        Self {
            name: info.name,
            path: info.path,
            format_version: info.format_version,
            journal_mode: info.journal_mode,
            tables: info
                .tables
                .into_iter()
                .map(|table| TableSummaryDto {
                    name: table.name,
                    row_count: table.row_count,
                })
                .collect(),
        }
    }
}

/// Integrity results.
#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../src/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct IntegrityReportDto {
    /// `PRAGMA integrity_check` output.
    pub integrity: String,
    /// Foreign key violations.
    pub foreign_key_violations: u64,
    /// Journal mode.
    pub journal_mode: String,
    /// Page count.
    pub page_count: u64,
    /// True when the BLAKE3 audit chain is continuous.
    pub audit_chain: bool,
    /// Number of audit entries verified.
    pub audit_entries: u64,
    /// Explicit diagnostic when the chain was altered outside SlabBase.
    pub audit_tamper: Option<String>,
}

impl From<IntegrityReport> for IntegrityReportDto {
    fn from(report: IntegrityReport) -> Self {
        Self {
            integrity: report.integrity,
            foreign_key_violations: report.foreign_key_violations,
            journal_mode: report.journal_mode,
            page_count: report.page_count,
            audit_chain: report.audit_chain,
            audit_entries: report.audit_entries,
            audit_tamper: report.audit_tamper,
        }
    }
}

/// Import options selected in the shell.
#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../src/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct ImportOptionsDto {
    /// Treat the first CSV record as headers.
    pub has_headers: bool,
    /// CSV delimiter.
    pub delimiter: String,
    /// Preview sample size.
    pub max_preview_rows: u32,
    /// Maximum issues reported.
    pub max_issues: u32,
    /// Skip invalid rows instead of stopping.
    pub skip_invalid: bool,
    /// Rows used for type inference.
    pub inference_limit: u32,
    /// Rows per sink transaction.
    pub batch_size: u32,
}

impl Default for ImportOptionsDto {
    fn default() -> Self {
        Self {
            has_headers: true,
            delimiter: ",".into(),
            max_preview_rows: 20,
            max_issues: 100,
            skip_invalid: false,
            inference_limit: 5_000,
            batch_size: 500,
        }
    }
}

impl ImportOptionsDto {
    /// Converts to the adapter options.
    pub fn to_options(&self) -> Result<ImportOptions, ApiError> {
        let delimiter = self
            .delimiter
            .as_bytes()
            .first()
            .copied()
            .ok_or_else(|| ApiError::new("SLAB_E_IMPORT", "delimiter is empty"))?;
        if self.delimiter.len() != 1 {
            return Err(ApiError::new(
                "SLAB_E_IMPORT",
                "delimiter must be one ASCII character",
            ));
        }
        Ok(ImportOptions {
            has_headers: self.has_headers,
            delimiter,
            max_preview_rows: self.max_preview_rows.clamp(1, 200),
            max_issues: self.max_issues.clamp(1, 1_000),
            skip_invalid: self.skip_invalid,
            inference_limit: self.inference_limit.clamp(1, 100_000),
            batch_size: self.batch_size.clamp(1, 10_000),
        })
    }
}

/// One inferred import column.
#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../src/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct ImportColumnDto {
    /// Source column name.
    pub name: String,
    /// Inferred type label.
    pub ty: String,
    /// Whether a blank value was seen.
    pub nullable: bool,
    /// Sample values.
    pub samples: Vec<String>,
}

/// One preview row.
#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../src/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct ImportSampleDto {
    /// One-based row number.
    pub row_number: u64,
    /// Raw values in column order.
    pub values: Vec<Option<String>>,
}

/// One issue.
#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../src/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct ImportIssueDto {
    /// Row number.
    pub row_number: u64,
    /// Column, when applicable.
    pub column: Option<String>,
    /// Reason.
    pub message: String,
}

impl From<RowIssue> for ImportIssueDto {
    fn from(issue: RowIssue) -> Self {
        Self {
            row_number: issue.row_number,
            column: issue.column,
            message: issue.message,
        }
    }
}

/// A preview payload.
#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../src/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct ImportPreviewDto {
    /// Source format.
    pub format: String,
    /// Inferred columns.
    pub columns: Vec<ImportColumnDto>,
    /// Sample rows.
    pub sample: Vec<ImportSampleDto>,
    /// Rows scanned.
    pub rows_scanned: u64,
    /// Rows used for type inference.
    pub inference_rows: u64,
    /// True when inference stopped at the configured prefix.
    pub inference_limited: bool,
    /// Structural issues.
    pub issues: Vec<ImportIssueDto>,
}

impl From<ImportPreview> for ImportPreviewDto {
    fn from(preview: ImportPreview) -> Self {
        Self {
            format: preview.format.as_str().to_owned(),
            columns: preview
                .columns
                .into_iter()
                .map(|column| ImportColumnDto {
                    name: column.name,
                    ty: column.ty.name().to_owned(),
                    nullable: column.nullable,
                    samples: column.samples,
                })
                .collect(),
            sample: preview
                .sample
                .into_iter()
                .map(|row| ImportSampleDto {
                    row_number: row.row_number,
                    values: row.values,
                })
                .collect(),
            rows_scanned: preview.rows_scanned,
            inference_rows: preview.inference_rows,
            inference_limited: preview.inference_limited,
            issues: preview
                .issues
                .into_iter()
                .map(ImportIssueDto::from)
                .collect(),
        }
    }
}

/// A validation report.
#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../src/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct ImportValidationDto {
    /// Rows read.
    pub rows: u64,
    /// Rows that convert cleanly.
    pub valid_rows: u64,
    /// Issues.
    pub issues: Vec<ImportIssueDto>,
}

impl From<ValidationReport> for ImportValidationDto {
    fn from(report: ValidationReport) -> Self {
        Self {
            rows: report.rows,
            valid_rows: report.valid_rows,
            issues: report
                .issues
                .into_iter()
                .map(ImportIssueDto::from)
                .collect(),
        }
    }
}

/// The result of an import.
#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../src/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct ImportSummaryDto {
    /// Rows inserted.
    pub inserted: u64,
    /// Rows skipped.
    pub skipped: u64,
    /// Issues.
    pub issues: Vec<ImportIssueDto>,
}

impl From<ImportSummary> for ImportSummaryDto {
    fn from(summary: ImportSummary) -> Self {
        Self {
            inserted: summary.inserted,
            skipped: summary.skipped,
            issues: summary
                .issues
                .into_iter()
                .map(ImportIssueDto::from)
                .collect(),
        }
    }
}

/// The result of a streamed table PDF.
#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../src/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct TablePdfResultDto {
    /// Rows written to the PDF.
    pub rows: u64,
    /// Number of PDF pages.
    pub pages: u64,
    /// True when the safety ceiling stopped the export.
    pub truncated: bool,
}

/// A rendered table HTML proof with its scope.
#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../src/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct TableHtmlResultDto {
    /// Self-contained HTML.
    pub html: String,
    /// Rows included.
    pub rows: u64,
    /// True when the safety ceiling stopped the proof.
    pub truncated: bool,
}

/// Print options selected in the shell.
#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../src/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct PrintOptionsDto {
    /// `letter` or `a4`.
    pub page_size: String,
    /// Optional title.
    pub title: Option<String>,
    /// Optional subtitle.
    pub subtitle: Option<String>,
    /// Include portal tables.
    pub include_portals: bool,
    /// Include empty fields.
    pub include_empty_fields: bool,
    /// Margin in points.
    pub margin_pt: Option<f64>,
}

impl Default for PrintOptionsDto {
    fn default() -> Self {
        Self {
            page_size: "letter".into(),
            title: None,
            subtitle: None,
            include_portals: true,
            include_empty_fields: true,
            margin_pt: None,
        }
    }
}

impl PrintOptionsDto {
    /// Converts to renderer options.
    pub fn to_options(&self) -> PrintOptions {
        PrintOptions {
            page_size: if self.page_size.eq_ignore_ascii_case("a4") {
                PageSize::A4
            } else {
                PageSize::Letter
            },
            margin_pt: self.margin_pt.unwrap_or(48.0).clamp(18.0, 144.0),
            title: self.title.clone(),
            subtitle: self.subtitle.clone(),
            include_portals: self.include_portals,
            include_empty_fields: self.include_empty_fields,
        }
    }
}
