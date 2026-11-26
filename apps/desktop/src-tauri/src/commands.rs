//! Tauri command handlers.
//!
//! Read commands lease a read-only workspace and run concurrently through
//! `spawn_blocking`. Write commands serialize on the writer mutex. The webview
//! thread never touches SQLite or the filesystem for workspace operations.

use std::collections::BTreeMap;
use std::fs::File;
use std::io::{BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use slab_core::{
    AuditEntry, Column, ColumnDraft, GridSpec, IndexSpec, Layout, LayoutItem, LayoutItemKind,
    PageFrame, PageQuery, PortalFrame, PortalQuery, Record, RecordBundle, RecordRevision, Relation,
    RelationDraft, RowData, RowId, Schema, SearchHit, Table, TableDraft, UndoState, Value,
};
use slab_import::{CsvAdapter, ImportAdapter, ImportError, ImportSink, JsonAdapter};
use slab_storage::Workspace;
use tauri::State;

use crate::AppState;
use crate::dto::{
    ImportOptionsDto, ImportPreviewDto, ImportSummaryDto, ImportValidationDto, IntegrityReportDto,
    PrintOptionsDto, TableHtmlResultDto, TablePdfResultDto, WorkspaceInfoDto,
};
use crate::error::ApiError;

async fn blocking<T, F>(work: F) -> Result<T, ApiError>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, ApiError> + Send + 'static,
{
    tauri::async_runtime::spawn_blocking(work)
        .await
        .map_err(|error| ApiError::internal(format!("database task failed: {error}")))?
}

async fn run_read<T, F>(state: &State<'_, AppState>, work: F) -> Result<T, ApiError>
where
    T: Send + 'static,
    F: FnOnce(&Workspace) -> Result<T, ApiError> + Send + 'static,
{
    let db = Arc::clone(&state.db);
    blocking(move || db.read(work)).await
}

async fn run_write<T, F>(state: &State<'_, AppState>, work: F) -> Result<T, ApiError>
where
    T: Send + 'static,
    F: FnOnce(&mut Workspace) -> Result<T, ApiError> + Send + 'static,
{
    let db = Arc::clone(&state.db);
    blocking(move || db.write(work)).await
}

async fn run_schema_write<T, F>(state: &State<'_, AppState>, work: F) -> Result<T, ApiError>
where
    T: Send + 'static,
    F: FnOnce(&mut Workspace) -> Result<T, ApiError> + Send + 'static,
{
    let db = Arc::clone(&state.db);
    blocking(move || db.write_schema(work)).await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn open_workspace(
    state: State<'_, AppState>,
    path: String,
) -> Result<WorkspaceInfoDto, ApiError> {
    let db = Arc::clone(&state.db);
    blocking(move || db.open(path)).await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn create_workspace(
    state: State<'_, AppState>,
    path: String,
    name: String,
) -> Result<WorkspaceInfoDto, ApiError> {
    let db = Arc::clone(&state.db);
    blocking(move || db.create(path, name)).await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn close_workspace(state: State<'_, AppState>) -> Result<(), ApiError> {
    let db = Arc::clone(&state.db);
    blocking(move || db.close()).await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn workspace_info(
    state: State<'_, AppState>,
) -> Result<Option<WorkspaceInfoDto>, ApiError> {
    let db = Arc::clone(&state.db);
    blocking(move || db.info()).await
}

/// Applies the native macOS material behind the webview. Glass uses the
/// under-window background so the desktop wallpaper reads through the chrome
/// tints; Solid removes the effect view so the opaque CSS surfaces stay crisp.
#[tauri::command(rename_all = "snake_case")]
pub fn set_window_surface(window: tauri::WebviewWindow, surface: String) -> Result<(), ApiError> {
    #[cfg(target_os = "macos")]
    {
        use tauri::utils::config::WindowEffectsConfig;
        use tauri::window::{Effect, EffectState, EffectsBuilder};

        match surface.as_str() {
            "glass" => {
                let effects = EffectsBuilder::new()
                    .effect(Effect::UnderWindowBackground)
                    .state(EffectState::FollowsWindowActiveState)
                    .build();
                window.set_effects(effects)?;
            }
            _ => window.set_effects(None::<WindowEffectsConfig>)?,
        }
    }

    #[cfg(not(target_os = "macos"))]
    {
        let _ = (window, surface);
    }

    Ok(())
}

#[tauri::command(rename_all = "snake_case")]
pub async fn load_sample(state: State<'_, AppState>) -> Result<WorkspaceInfoDto, ApiError> {
    let db = Arc::clone(&state.db);
    blocking(move || db.load_sample()).await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn get_schema(state: State<'_, AppState>) -> Result<Schema, ApiError> {
    run_read(&state, |workspace| Ok(workspace.schema()?)).await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn create_table(
    state: State<'_, AppState>,
    draft: TableDraft,
) -> Result<Table, ApiError> {
    run_schema_write(&state, move |workspace| Ok(workspace.create_table(&draft)?)).await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn add_column(
    state: State<'_, AppState>,
    table: String,
    draft: ColumnDraft,
) -> Result<Column, ApiError> {
    run_schema_write(&state, move |workspace| {
        Ok(workspace.add_column(&table, &draft)?)
    })
    .await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn create_relation(
    state: State<'_, AppState>,
    draft: RelationDraft,
) -> Result<Relation, ApiError> {
    run_schema_write(&state, move |workspace| {
        Ok(workspace.create_relation(&draft)?)
    })
    .await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn save_layout(state: State<'_, AppState>, layout: Layout) -> Result<Layout, ApiError> {
    run_schema_write(&state, move |workspace| Ok(workspace.save_layout(layout)?)).await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn delete_layout(state: State<'_, AppState>, id: String) -> Result<(), ApiError> {
    run_schema_write(&state, move |workspace| {
        workspace.delete_layout(&id)?;
        Ok(())
    })
    .await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn rename_table(
    state: State<'_, AppState>,
    old_name: String,
    new_name: String,
) -> Result<(), ApiError> {
    run_schema_write(&state, move |workspace| {
        workspace.rename_table(&old_name, &new_name)?;
        Ok(())
    })
    .await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn rename_column(
    state: State<'_, AppState>,
    table: String,
    old_name: String,
    new_name: String,
) -> Result<(), ApiError> {
    run_schema_write(&state, move |workspace| {
        workspace.rename_column(&table, &old_name, &new_name)?;
        Ok(())
    })
    .await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn drop_table(state: State<'_, AppState>, name: String) -> Result<(), ApiError> {
    run_schema_write(&state, move |workspace| {
        workspace.drop_table(&name)?;
        Ok(())
    })
    .await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn drop_column(
    state: State<'_, AppState>,
    table: String,
    column: String,
) -> Result<(), ApiError> {
    run_schema_write(&state, move |workspace| {
        workspace.drop_column(&table, &column)?;
        Ok(())
    })
    .await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn drop_relation(state: State<'_, AppState>, name: String) -> Result<(), ApiError> {
    run_schema_write(&state, move |workspace| {
        workspace.drop_relation(&name)?;
        Ok(())
    })
    .await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn list_indexes(
    state: State<'_, AppState>,
    table: String,
) -> Result<Vec<IndexSpec>, ApiError> {
    run_read(&state, move |workspace| Ok(workspace.list_indexes(&table)?)).await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn toggle_index(
    state: State<'_, AppState>,
    table: String,
    column: String,
    enabled: bool,
) -> Result<Vec<IndexSpec>, ApiError> {
    run_schema_write(&state, move |workspace| {
        if enabled {
            workspace.create_index(&table, &column)?;
        } else {
            let indexes = workspace.list_indexes(&table)?;
            if let Some(index) = indexes
                .into_iter()
                .find(|index| index.columns.len() == 1 && index.columns.first() == Some(&column))
            {
                workspace.drop_index(&index.name)?;
            }
        }
        Ok(workspace.list_indexes(&table)?)
    })
    .await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn create_index(
    state: State<'_, AppState>,
    table: String,
    column: String,
) -> Result<IndexSpec, ApiError> {
    run_schema_write(&state, move |workspace| {
        Ok(workspace.create_index(&table, &column)?)
    })
    .await
}

#[tauri::command(rename_all = "snake_case")]
pub fn cancel_search(state: State<'_, AppState>) -> bool {
    state.db.cancel_search_query()
}

#[tauri::command(rename_all = "snake_case")]
pub async fn page_rows(
    state: State<'_, AppState>,
    query: PageQuery,
) -> Result<PageFrame, ApiError> {
    run_read(&state, move |workspace| {
        let schema = workspace.schema()?;
        let table = schema
            .table(&query.table)
            .ok_or_else(|| ApiError::new("SLAB_E_NOT_FOUND", "table is missing"))?;
        let columns = table
            .columns
            .iter()
            .map(|column| column.name.clone())
            .collect();
        Ok(workspace.page(&query)?.into_frame(columns))
    })
    .await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn page_portal(
    state: State<'_, AppState>,
    query: PortalQuery,
) -> Result<PortalFrame, ApiError> {
    run_read(&state, move |workspace| {
        let schema = workspace.schema()?;
        let relation = schema
            .relation(&query.relation)
            .ok_or_else(|| ApiError::new("SLAB_E_NOT_FOUND", "relation is missing"))?;
        let table = schema
            .table(&relation.child_table)
            .ok_or_else(|| ApiError::new("SLAB_E_NOT_FOUND", "child table is missing"))?;
        let columns = table
            .columns
            .iter()
            .map(|column| column.name.clone())
            .collect();
        Ok(workspace.page_portal(&query)?.into_frame(columns))
    })
    .await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn read_record(
    state: State<'_, AppState>,
    table: String,
    id: RowId,
) -> Result<Record, ApiError> {
    run_read(&state, move |workspace| {
        Ok(workspace.read_record(&table, id)?)
    })
    .await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn read_bundle(
    state: State<'_, AppState>,
    table: String,
    id: RowId,
    portal_limit: u32,
) -> Result<RecordBundle, ApiError> {
    run_read(&state, move |workspace| {
        Ok(workspace.bundle(&table, id, portal_limit)?)
    })
    .await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn insert_row(
    state: State<'_, AppState>,
    table: String,
    values: RowData,
) -> Result<Record, ApiError> {
    run_write(&state, move |workspace| {
        Ok(workspace.insert_row(&table, &values)?)
    })
    .await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn update_row(
    state: State<'_, AppState>,
    table: String,
    id: RowId,
    values: RowData,
) -> Result<Record, ApiError> {
    run_write(&state, move |workspace| {
        Ok(workspace.update_row(&table, id, &values)?)
    })
    .await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn delete_row(
    state: State<'_, AppState>,
    table: String,
    id: RowId,
) -> Result<(), ApiError> {
    run_write(&state, move |workspace| {
        workspace.delete_row(&table, id)?;
        Ok(())
    })
    .await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn undo(state: State<'_, AppState>) -> Result<UndoState, ApiError> {
    run_schema_write(&state, |workspace| Ok(workspace.undo()?)).await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn redo(state: State<'_, AppState>) -> Result<UndoState, ApiError> {
    run_schema_write(&state, |workspace| Ok(workspace.redo()?)).await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn undo_state(state: State<'_, AppState>) -> Result<UndoState, ApiError> {
    run_write(&state, |workspace| Ok(workspace.undo_state())).await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn search(
    state: State<'_, AppState>,
    term: String,
    limit: u32,
) -> Result<Vec<SearchHit>, ApiError> {
    run_read(&state, move |workspace| Ok(workspace.search(&term, limit)?)).await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn audit_entries(
    state: State<'_, AppState>,
    limit: u32,
    offset: u32,
) -> Result<Vec<AuditEntry>, ApiError> {
    run_read(&state, move |workspace| {
        Ok(workspace.audit_entries(limit, offset)?)
    })
    .await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn record_history(
    state: State<'_, AppState>,
    table: String,
    row_id: RowId,
) -> Result<Vec<RecordRevision>, ApiError> {
    run_read(&state, move |workspace| {
        Ok(workspace.record_history(&table, row_id)?)
    })
    .await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn restore_record_revision(
    state: State<'_, AppState>,
    table: String,
    row_id: RowId,
    audit_id: i64,
) -> Result<Record, ApiError> {
    run_write(&state, move |workspace| {
        Ok(workspace.restore_record_revision(&table, row_id, audit_id)?)
    })
    .await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn backup_workspace(state: State<'_, AppState>, path: String) -> Result<(), ApiError> {
    run_write(&state, move |workspace| {
        workspace.snapshot_to(&path)?;
        Ok(())
    })
    .await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn integrity_check(state: State<'_, AppState>) -> Result<IntegrityReportDto, ApiError> {
    run_read(&state, |workspace| {
        Ok(IntegrityReportDto::from(workspace.integrity_check()?))
    })
    .await
}

fn adapter_for(format: &str) -> Result<Box<dyn ImportAdapter>, ApiError> {
    match format.to_ascii_lowercase().as_str() {
        "csv" => Ok(Box::new(CsvAdapter)),
        "json" => Ok(Box::new(JsonAdapter)),
        other => Err(ApiError::new(
            "SLAB_E_IMPORT",
            format!("unsupported import format {other}"),
        )),
    }
}

fn open_file(path: &str) -> Result<BufReader<File>, ApiError> {
    File::open(path).map(BufReader::new).map_err(ApiError::from)
}

#[tauri::command(rename_all = "snake_case")]
pub async fn preview_import(
    state: State<'_, AppState>,
    path: String,
    format: String,
    options: ImportOptionsDto,
) -> Result<ImportPreviewDto, ApiError> {
    run_read(&state, move |_workspace| {
        let options = options.to_options()?;
        let mut reader = open_file(&path)?;
        let mut adapter = adapter_for(&format)?;
        let preview = slab_import::preview(adapter.as_mut(), &mut reader, &options)?;
        Ok(ImportPreviewDto::from(preview))
    })
    .await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn validate_import(
    state: State<'_, AppState>,
    path: String,
    format: String,
    target_table: String,
    mapping: BTreeMap<String, String>,
    options: ImportOptionsDto,
) -> Result<ImportValidationDto, ApiError> {
    run_read(&state, move |workspace| {
        let options = options.to_options()?;
        let schema = workspace.schema()?;
        let table = schema
            .table(&target_table)
            .ok_or_else(|| ApiError::new("SLAB_E_NOT_FOUND", "target table is missing"))?
            .clone();
        let mut reader = open_file(&path)?;
        let mut adapter = adapter_for(&format)?;
        let report =
            slab_import::validate(adapter.as_mut(), &mut reader, &options, &table, &mapping)?;
        Ok(ImportValidationDto::from(report))
    })
    .await
}

struct WorkspaceSink<'a> {
    workspace: &'a mut Workspace,
}

impl ImportSink for WorkspaceSink<'_> {
    fn insert_row(&mut self, table: &str, values: &RowData) -> Result<RowId, ImportError> {
        self.workspace
            .insert_import_rows(table, std::slice::from_ref(values))
            .map_err(|error| ImportError::Sink(format!("{}: {error}", error.code_str())))?
            .into_iter()
            .next()
            .ok_or_else(|| ImportError::Sink("insert returned no row id".into()))
    }

    fn insert_batch(&mut self, table: &str, rows: &[RowData]) -> Result<Vec<RowId>, ImportError> {
        self.workspace
            .insert_import_rows(table, rows)
            .map_err(|error| ImportError::Sink(format!("{}: {error}", error.code_str())))
    }
}

#[tauri::command(rename_all = "snake_case")]
pub async fn apply_import(
    state: State<'_, AppState>,
    path: String,
    format: String,
    target_table: String,
    mapping: BTreeMap<String, String>,
    options: ImportOptionsDto,
) -> Result<ImportSummaryDto, ApiError> {
    run_write(&state, move |workspace| {
        let options = options.to_options()?;
        let schema = workspace.schema()?;
        let table = schema
            .table(&target_table)
            .ok_or_else(|| ApiError::new("SLAB_E_NOT_FOUND", "target table is missing"))?
            .clone();
        if !options.skip_invalid {
            let mut reader = open_file(&path)?;
            let mut adapter = adapter_for(&format)?;
            let report =
                slab_import::validate(adapter.as_mut(), &mut reader, &options, &table, &mapping)?;
            if !report.issues.is_empty() || report.valid_rows != report.rows {
                let first = report
                    .issues
                    .first()
                    .map(|issue| issue.message.clone())
                    .unwrap_or_else(|| "validation failed".into());
                return Err(ApiError::new(
                    "SLAB_E_IMPORT",
                    format!(
                        "{} of {} rows are valid; first issue: {first}",
                        report.valid_rows, report.rows
                    ),
                ));
            }
        }
        let mut reader = open_file(&path)?;
        let mut adapter = adapter_for(&format)?;
        workspace.begin_import()?;
        let outcome = {
            let mut sink = WorkspaceSink { workspace };
            slab_import::apply(
                adapter.as_mut(),
                &mut reader,
                &options,
                &table,
                &mapping,
                &mut sink,
            )
            .map_err(ApiError::from)
        };
        match outcome {
            Ok(summary) => {
                workspace.commit_import()?;
                Ok(ImportSummaryDto::from(summary))
            }
            Err(error) => {
                let _ = workspace.rollback_import();
                Err(error)
            }
        }
    })
    .await
}

fn collect_rows_bounded(
    workspace: &Workspace,
    query: &PageQuery,
    maximum: usize,
) -> Result<(Vec<Record>, bool), ApiError> {
    let mut query = query.clone();
    query.limit = 500;
    let mut rows = Vec::new();
    let mut truncated = false;
    loop {
        let page = workspace.page(&query).map_err(ApiError::from)?;
        for row in page.rows {
            if rows.len() >= maximum {
                truncated = true;
                break;
            }
            rows.push(row);
        }
        if truncated {
            break;
        }
        match page.next_cursor {
            Some(cursor) => query.cursor = Some(cursor),
            None => break,
        }
    }
    Ok((rows, truncated))
}

/// A temporary sibling that removes itself unless it was published.
///
/// Exports write here first and rename on success, so a validation error, a
/// cancelled page, or a full disk cannot truncate an existing export.
struct ExportTemp {
    path: PathBuf,
    published: bool,
    /// Device/inode captured when the scratch file was created, so a path
    /// swap before publish is detected on Unix.
    identity: Option<(u64, u64)>,
}

impl ExportTemp {
    fn create(target: &str) -> Result<(Self, File), ApiError> {
        let target = Path::new(target);
        let parent = target
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        let mut options = std::fs::OpenOptions::new();
        options.read(true).write(true).create_new(true);
        // Rows are written before `publish`, so the scratch file must be born
        // with the destination's mode; tightening it at publish would leave a
        // private export world-readable while it is written. The umask can
        // only narrow this further.
        #[cfg(unix)]
        if let Ok(metadata) = std::fs::metadata(target) {
            use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
            options.mode(metadata.permissions().mode() & 0o777);
        }
        for serial in 0..64 {
            let path = parent.join(format!(
                ".slabbase-export-{}-{serial}.tmp",
                std::process::id()
            ));
            match options.open(&path) {
                Ok(file) => {
                    #[cfg(unix)]
                    let identity = {
                        use std::os::unix::fs::MetadataExt;
                        file.metadata()
                            .ok()
                            .map(|metadata| (metadata.dev(), metadata.ino()))
                    };
                    #[cfg(not(unix))]
                    let identity = None;
                    return Ok((
                        Self {
                            path,
                            published: false,
                            identity,
                        },
                        file,
                    ));
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error.into()),
            }
        }
        Err(ApiError::internal(
            "could not allocate an export scratch file",
        ))
    }

    fn publish(mut self, target: &str) -> Result<(), ApiError> {
        if let Some(expected) = self.identity {
            use std::os::unix::fs::MetadataExt;
            let metadata = std::fs::metadata(&self.path)?;
            if (metadata.dev(), metadata.ino()) != expected {
                return Err(ApiError::internal(
                    "the export scratch file was replaced before publishing",
                ));
            }
        }
        // `rename` replaces the inode, which would otherwise replace a private
        // destination's permissions with the scratch file's default mode.
        if let Ok(metadata) = std::fs::metadata(target) {
            std::fs::set_permissions(&self.path, metadata.permissions())?;
        }
        std::fs::rename(&self.path, target)?;
        self.published = true;
        Ok(())
    }
}

impl Drop for ExportTemp {
    fn drop(&mut self) {
        if !self.published {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

/// Renders one exported value.
///
/// A spreadsheet evaluates a leading `=`, `+`, `-`, `@`, tab, carriage return,
/// or line feed even when the field is CSV-quoted. Imported data is untrusted,
/// so text that would be treated as a formula is prefixed with a single quote.
/// Numeric values are never prefixed: a negative amount is data.
fn csv_cell(value: &Value) -> String {
    let text = value.display_string();
    let text_like = matches!(
        value,
        Value::Text(_) | Value::Select(_) | Value::AttachmentRef(_)
    );
    let formula_like = slab_import::spreadsheet_guard_needed(&text);
    if text_like && formula_like {
        format!("'{text}")
    } else {
        text
    }
}

fn csv_record(record: &Record, columns: &[String]) -> Vec<String> {
    let mut row = vec![record.id.to_string()];
    for column in columns {
        let value = record.get(column).cloned().unwrap_or(Value::Null);
        row.push(csv_cell(&value));
    }
    row
}

#[tauri::command(rename_all = "snake_case")]
pub async fn export_csv(
    state: State<'_, AppState>,
    path: String,
    query: PageQuery,
) -> Result<(), ApiError> {
    run_read(&state, move |workspace| {
        let schema = workspace.schema()?;
        let table = schema
            .table(&query.table)
            .ok_or_else(|| ApiError::new("SLAB_E_NOT_FOUND", "table is missing"))?;
        let columns: Vec<String> = table
            .columns
            .iter()
            .map(|column| column.name.clone())
            .collect();
        let (temporary, file) = ExportTemp::create(&path)?;
        let mut writer = csv::WriterBuilder::new()
            .has_headers(false)
            .from_writer(BufWriter::new(file));
        let mut header = vec!["id".to_owned()];
        header.extend(columns.clone());
        writer.write_record(&header)?;
        let mut query = query;
        query.limit = 500;
        loop {
            let page = workspace.page(&query)?;
            for record in &page.rows {
                writer.write_record(csv_record(record, &columns))?;
            }
            match page.next_cursor {
                Some(cursor) => query.cursor = Some(cursor),
                None => break,
            }
        }
        writer.flush()?;
        drop(writer);
        temporary.publish(&path)
    })
    .await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn export_json(
    state: State<'_, AppState>,
    path: String,
    query: PageQuery,
) -> Result<(), ApiError> {
    run_read(&state, move |workspace| {
        export_json_to(workspace, &path, query)
    })
    .await
}

/// Resolves the table before touching the destination, so a typo cannot erase
/// an existing export.
fn export_json_to(workspace: &Workspace, path: &str, mut query: PageQuery) -> Result<(), ApiError> {
    let schema = workspace.schema()?;
    if schema.table(&query.table).is_none() {
        return Err(ApiError::new("SLAB_E_NOT_FOUND", "table is missing"));
    }
    let (temporary, file) = ExportTemp::create(path)?;
    let mut out = BufWriter::new(file);
    out.write_all(b"[\n")?;
    let mut first = true;
    query.limit = 500;
    loop {
        let page = workspace.page(&query)?;
        for record in &page.rows {
            if !first {
                out.write_all(b",\n")?;
            }
            first = false;
            let mut object = serde_json::Map::new();
            object.insert("id".into(), serde_json::Value::from(record.id));
            for (name, value) in &record.values {
                let encoded = if value.is_null() {
                    serde_json::Value::Null
                } else {
                    serde_json::to_value(value)?
                };
                object.insert(name.clone(), encoded);
            }
            serde_json::to_writer(&mut out, &serde_json::Value::Object(object))?;
        }
        match page.next_cursor {
            Some(cursor) => query.cursor = Some(cursor),
            None => break,
        }
    }
    out.write_all(b"\n]\n")?;
    out.flush()?;
    drop(out);
    temporary.publish(path)
}

/// Dumps the workspace through the same scratch-and-publish contract as the
/// CSV and JSON exports, so a failed dump cannot truncate an existing file.
fn export_sql_to(workspace: &Workspace, path: &str) -> Result<(), ApiError> {
    let (temporary, file) = ExportTemp::create(path)?;
    workspace.export_sql_to(file)?;
    temporary.publish(path)
}

#[tauri::command(rename_all = "snake_case")]
pub async fn export_sql(state: State<'_, AppState>, path: String) -> Result<(), ApiError> {
    run_read(&state, move |workspace| export_sql_to(workspace, &path)).await
}

fn layout_for(schema: &Schema, table: &Table, layout_id: Option<&str>) -> Result<Layout, ApiError> {
    if let Some(layout_id) = layout_id {
        return schema
            .layouts
            .iter()
            .find(|layout| layout.id == layout_id)
            .cloned()
            .ok_or_else(|| ApiError::new("SLAB_E_LAYOUT", "form layout is missing"));
    }
    if let Some(layout) = schema
        .layouts
        .iter()
        .find(|layout| layout.table == table.name)
    {
        return Ok(layout.clone());
    }
    Ok(default_layout(table))
}

fn default_layout(table: &Table) -> Layout {
    let mut items = vec![LayoutItem {
        id: "default_section".into(),
        kind: LayoutItemKind::Section {
            title: table.name.clone(),
        },
        x: 0,
        y: 0,
        w: 12,
        h: 1,
    }];
    let mut y = 1_u16;
    for (index, column) in table.columns.iter().enumerate() {
        let x = if index % 2 == 0 { 0 } else { 6 };
        items.push(LayoutItem {
            id: format!("default_{}", column.id),
            kind: LayoutItemKind::Field {
                column: column.name.clone(),
            },
            x,
            y,
            w: 6,
            h: 2,
        });
        if index % 2 == 1 {
            y = y.saturating_add(3);
        }
    }
    Layout {
        id: "default".into(),
        name: format!("{} report", table.name),
        table: table.name.clone(),
        grid: GridSpec::default(),
        items,
        updated_at: 0,
    }
}

#[tauri::command(rename_all = "snake_case")]
pub async fn render_form_html(
    state: State<'_, AppState>,
    table: String,
    id: RowId,
    layout_id: Option<String>,
    options: PrintOptionsDto,
) -> Result<String, ApiError> {
    run_read(&state, move |workspace| {
        let schema = workspace.schema()?;
        let definition = schema
            .table(&table)
            .ok_or_else(|| ApiError::new("SLAB_E_NOT_FOUND", "table is missing"))?;
        let layout = layout_for(&schema, definition, layout_id.as_deref())?;
        let bundle = workspace.bundle(&table, id, 25_000)?;
        Ok(slab_print::render_html(
            &schema,
            &layout,
            &bundle,
            &options.to_options(),
        )?)
    })
    .await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn render_table_html(
    state: State<'_, AppState>,
    table: String,
    query: PageQuery,
    options: PrintOptionsDto,
) -> Result<TableHtmlResultDto, ApiError> {
    run_read(&state, move |workspace| {
        let schema = workspace.schema()?;
        let (rows, truncated) = collect_rows_bounded(workspace, &query, 25_000)?;
        let html = slab_print::render_table_html_with_status(
            &schema,
            &table,
            &rows,
            &options.to_options(),
            truncated,
        )?;
        Ok(TableHtmlResultDto {
            html,
            rows: u64::try_from(rows.len()).unwrap_or(0),
            truncated,
        })
    })
    .await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn export_form_pdf(
    state: State<'_, AppState>,
    path: String,
    table: String,
    id: RowId,
    layout_id: Option<String>,
    options: PrintOptionsDto,
) -> Result<(), ApiError> {
    run_read(&state, move |workspace| {
        let schema = workspace.schema()?;
        let definition = schema
            .table(&table)
            .ok_or_else(|| ApiError::new("SLAB_E_NOT_FOUND", "table is missing"))?;
        let layout = layout_for(&schema, definition, layout_id.as_deref())?;
        let bundle = workspace.bundle(&table, id, 25_000)?;
        let bytes = slab_print::render_pdf(&schema, &layout, &bundle, &options.to_options())?;
        std::fs::write(&path, bytes)?;
        Ok(())
    })
    .await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn export_table_pdf(
    state: State<'_, AppState>,
    path: String,
    table: String,
    query: PageQuery,
    options: PrintOptionsDto,
) -> Result<TablePdfResultDto, ApiError> {
    run_read(&state, move |workspace| {
        let schema = workspace.schema()?;
        let mut report = slab_print::TableReport::new(&schema, &table, &options.to_options())?;
        let mut query = query;
        query.limit = 500;
        loop {
            let page = workspace.page(&query)?;
            report.push(&page.rows);
            if report.is_full() {
                if page.next_cursor.is_some() {
                    report.mark_truncated();
                }
                break;
            }
            match page.next_cursor {
                Some(cursor) => query.cursor = Some(cursor),
                None => break,
            }
        }
        let result = report.finish()?;
        std::fs::write(&path, result.pdf)?;
        Ok(TablePdfResultDto {
            rows: u64::try_from(result.rows_printed).unwrap_or(0),
            pages: u64::try_from(result.pages).unwrap_or(0),
            truncated: result.truncated,
        })
    })
    .await
}

#[cfg(test)]
mod tests {
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

        // A valid table with an invalid sort or filter reaches page()
        // validation after the destination used to be truncated.
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
}

#[cfg(all(test, unix))]
mod permission_tests {
    use super::*;
    use slab_core::{ColumnDraft, ColumnType, TableDraft};
    use slab_sync::HybridLogicalClock;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn export_publish_preserves_private_destination_permissions() {
        let mut workspace =
            Workspace::open_in_memory("Export", Box::new(HybridLogicalClock::new("test")))
                .expect("workspace");
        workspace
            .create_table(&TableDraft {
                name: "Notes".into(),
                columns: vec![ColumnDraft {
                    name: "Body".into(),
                    ty: ColumnType::Text,
                    required: false,
                    unique: false,
                    default: None,
                }],
            })
            .expect("table");

        let directory = tempfile::TempDir::new().expect("tempdir");
        let path = directory.path().join("private.json");
        fs::write(&path, "old").expect("seed");
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).expect("private mode");

        export_json_to(
            &workspace,
            path.to_str().expect("path"),
            PageQuery {
                table: "Notes".into(),
                ..PageQuery::default()
            },
        )
        .expect("export");

        let mode = fs::metadata(&path).expect("metadata").permissions().mode() & 0o777;
        assert_eq!(
            mode, 0o600,
            "publishing widened the destination permissions"
        );
    }

    #[test]
    fn export_scratch_is_private_before_rows_are_written() {
        let directory = tempfile::TempDir::new().expect("tempdir");
        let path = directory.path().join("private.csv");
        fs::write(&path, "old").expect("seed");
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).expect("private mode");

        let (temporary, _file) = ExportTemp::create(path.to_str().expect("path")).expect("scratch");

        let mode = fs::metadata(&temporary.path)
            .expect("scratch metadata")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(
            mode & 0o077,
            0,
            "the scratch file for a private export is readable by others: {mode:o}"
        );
    }

    #[test]
    fn sql_export_does_not_truncate_when_the_scratch_cannot_be_created() {
        let workspace =
            Workspace::open_in_memory("Export", Box::new(HybridLogicalClock::new("test")))
                .expect("workspace");
        let directory = tempfile::TempDir::new().expect("tempdir");
        let parent = directory.path().join("locked");
        fs::create_dir(&parent).expect("locked dir");
        let target = parent.join("export.sql");
        fs::write(&target, "keep me").expect("seed export");
        fs::set_permissions(&parent, fs::Permissions::from_mode(0o500)).expect("lock dir");

        let outcome = export_sql_to(&workspace, target.to_str().expect("path"));
        // Make the directory removable again before any assertion can panic.
        fs::set_permissions(&parent, fs::Permissions::from_mode(0o700)).expect("unlock dir");

        assert!(
            outcome.is_err(),
            "a scratch file in a read-only directory must fail"
        );
        assert_eq!(
            fs::read_to_string(&target).expect("read export"),
            "keep me",
            "a failed SQL export truncated the previous file"
        );
    }
}

#[cfg(all(test, unix))]
mod scratch_identity_tests {
    use super::*;
    use std::os::unix::fs::symlink;

    #[test]
    fn publishing_refuses_a_replaced_scratch_path() {
        let directory = tempfile::TempDir::new().expect("tempdir");
        let target = directory.path().join("export.csv");
        let (temporary, file) =
            ExportTemp::create(target.to_str().expect("path")).expect("scratch");
        let scratch = temporary.path.clone();
        drop(file);
        std::fs::remove_file(&scratch).expect("unlink scratch");
        let other = directory.path().join("other");
        std::fs::write(&other, "attacker").expect("other file");
        symlink(&other, &scratch).expect("symlink");
        assert!(
            temporary.publish(target.to_str().expect("path")).is_err(),
            "a replaced scratch path was published"
        );
        assert!(!target.exists());
    }
}
