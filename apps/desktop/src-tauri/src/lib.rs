#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

//! SlabBase desktop shell.

pub mod commands;
pub mod dto;
pub mod error;
pub mod worker;

use std::sync::Arc;

use tauri::Manager;

pub use dto::*;
pub use error::ApiError;

/// Shared application state. The database itself lives on the worker thread.
pub struct AppState {
    /// Handle used by commands to submit work.
    pub db: Arc<worker::DbHandle>,
}

/// Builds and runs the desktop window.
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let builder = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let db = worker::DbHandle::new()?;
            if let Some(path) = requested_workspace() {
                let _ = db.open(path);
            }
            app.manage(AppState { db: Arc::new(db) });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::open_workspace,
            commands::create_workspace,
            commands::close_workspace,
            commands::workspace_info,
            commands::load_sample,
            commands::get_schema,
            commands::create_table,
            commands::add_column,
            commands::create_relation,
            commands::save_layout,
            commands::delete_layout,
            commands::page_rows,
            commands::read_record,
            commands::read_bundle,
            commands::insert_row,
            commands::update_row,
            commands::delete_row,
            commands::undo,
            commands::redo,
            commands::undo_state,
            commands::search,
            commands::audit_entries,
            commands::backup_workspace,
            commands::integrity_check,
            commands::preview_import,
            commands::validate_import,
            commands::apply_import,
            commands::export_csv,
            commands::export_json,
            commands::export_sql,
            commands::render_form_html,
            commands::render_table_html,
            commands::export_form_pdf,
            commands::export_table_pdf,
        ]);

    if let Err(error) = builder.run(tauri::generate_context!()) {
        report_startup_error(&error);
    }
}

fn requested_workspace() -> Option<String> {
    let mut args = std::env::args().skip(1);
    while let Some(argument) = args.next() {
        if argument == "--workspace" {
            return args.next();
        }
        if let Some(path) = argument.strip_prefix("--workspace=") {
            return Some(path.to_owned());
        }
    }
    std::env::var("SLABBASE_WORKSPACE").ok()
}

#[allow(clippy::print_stderr)]
fn report_startup_error(error: &tauri::Error) {
    // The window may not exist yet, so stderr is the only reliable channel.
    eprintln!("SlabBase failed to start: {error}");
}
