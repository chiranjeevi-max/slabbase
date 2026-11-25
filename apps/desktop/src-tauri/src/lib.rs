#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

//! SlabBase desktop shell.

pub mod commands;
pub mod dto;
pub mod error;
pub mod worker;

use std::sync::Arc;

#[cfg(not(desktop))]
use tauri::Manager;
#[cfg(desktop)]
use tauri::menu::{
    AboutMetadata, MenuBuilder, MenuItem, MenuItemBuilder, PredefinedMenuItem, SubmenuBuilder,
};
#[cfg(desktop)]
use tauri::{AppHandle, Emitter, Manager};

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
        .enable_macos_default_menu(false)
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            #[cfg(desktop)]
            install_menu(app)?;
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
            commands::set_window_surface,
            commands::load_sample,
            commands::get_schema,
            commands::create_table,
            commands::add_column,
            commands::create_relation,
            commands::drop_relation,
            commands::rename_table,
            commands::rename_column,
            commands::drop_table,
            commands::drop_column,
            commands::list_indexes,
            commands::toggle_index,
            commands::create_index,
            commands::cancel_search,
            commands::save_layout,
            commands::delete_layout,
            commands::page_rows,
            commands::page_portal,
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
            commands::record_history,
            commands::restore_record_revision,
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

#[cfg(desktop)]
fn menu_item(
    handle: &AppHandle,
    id: &str,
    text: &str,
    accelerator: Option<&str>,
) -> tauri::Result<MenuItem<tauri::Wry>> {
    let mut builder = MenuItemBuilder::with_id(id, text);
    if let Some(accelerator) = accelerator {
        builder = builder.accelerator(accelerator);
    }
    builder.build(handle)
}

#[cfg(desktop)]
fn install_menu(app: &mut tauri::App) -> tauri::Result<()> {
    let handle = app.handle().clone();

    let appearance = menu_item(&handle, "appearance", "Appearance…", Some("Cmd+,"))?;
    let new_workspace = menu_item(
        &handle,
        "new-workspace",
        "New Workspace…",
        Some("Cmd+Shift+N"),
    )?;
    let open_workspace = menu_item(&handle, "open-workspace", "Open Workspace…", Some("Cmd+O"))?;
    let load_sample = menu_item(&handle, "load-sample", "Load Sample Workspace", None)?;
    let new_table = menu_item(&handle, "new-table", "New Table…", Some("Cmd+Alt+T"))?;
    let new_record = menu_item(&handle, "new-record", "New Record", Some("Cmd+N"))?;
    let import = menu_item(&handle, "import", "Import Data…", Some("Cmd+Shift+I"))?;
    let export = menu_item(&handle, "export", "Export…", Some("Cmd+Shift+E"))?;
    let backup = menu_item(&handle, "backup", "Backup Snapshot…", Some("Cmd+Shift+B"))?;
    let print = menu_item(&handle, "print", "Print / Export PDF…", Some("Cmd+P"))?;
    let close_workspace = menu_item(&handle, "close-workspace", "Close Workspace", None)?;
    let undo = menu_item(&handle, "edit-undo", "Undo", Some("Cmd+Z"))?;
    let redo = menu_item(&handle, "edit-redo", "Redo", Some("Cmd+Shift+Z"))?;
    let search = menu_item(
        &handle,
        "search",
        "Search / Command Palette…",
        Some("Cmd+K"),
    )?;
    let table_view = menu_item(&handle, "view-table", "Table View", Some("Cmd+1"))?;
    let form_view = menu_item(&handle, "view-form", "Form Studio", Some("Cmd+2"))?;
    let schema_view = menu_item(&handle, "view-schema", "Relations && Schema", Some("Cmd+3"))?;
    let audit_view = menu_item(&handle, "view-audit", "Audit Log", Some("Cmd+4"))?;
    let toggle_sidebar = menu_item(
        &handle,
        "toggle-sidebar",
        "Toggle Sidebar",
        Some("Cmd+Alt+S"),
    )?;
    let toggle_inspector = menu_item(
        &handle,
        "toggle-inspector",
        "Toggle Inspector",
        Some("Cmd+Alt+I"),
    )?;
    let integrity = menu_item(&handle, "check-integrity", "Check Database Integrity", None)?;

    let about = AboutMetadata {
        name: Some("SlabBase".into()),
        version: Some(env!("CARGO_PKG_VERSION").into()),
        copyright: Some("Apache-2.0".into()),
        ..AboutMetadata::default()
    };

    let app_menu = SubmenuBuilder::new(&handle, "SlabBase")
        .about(Some(about))
        .separator()
        .item(&appearance)
        .separator()
        .services()
        .separator()
        .hide()
        .hide_others()
        .show_all()
        .separator()
        .quit()
        .build()?;

    let file_menu = SubmenuBuilder::new(&handle, "File")
        .item(&new_workspace)
        .item(&open_workspace)
        .item(&load_sample)
        .separator()
        .item(&new_table)
        .item(&new_record)
        .separator()
        .item(&import)
        .item(&export)
        .item(&backup)
        .separator()
        .item(&print)
        .separator()
        .item(&close_workspace)
        .close_window()
        .build()?;

    let edit_menu = SubmenuBuilder::new(&handle, "Edit")
        .item(&undo)
        .item(&redo)
        .separator()
        .cut()
        .copy()
        .paste()
        .select_all()
        .separator()
        .item(&search)
        .build()?;

    let fullscreen = PredefinedMenuItem::fullscreen(&handle, None)?;
    let view_menu = SubmenuBuilder::new(&handle, "View")
        .item(&table_view)
        .item(&form_view)
        .item(&schema_view)
        .item(&audit_view)
        .separator()
        .item(&toggle_sidebar)
        .item(&toggle_inspector)
        .separator()
        .item(&fullscreen)
        .build()?;

    let minimize = PredefinedMenuItem::minimize(&handle, None)?;
    let zoom = PredefinedMenuItem::maximize(&handle, Some("Zoom"))?;
    let window_close = PredefinedMenuItem::close_window(&handle, None)?;
    let window_menu = SubmenuBuilder::new(&handle, "Window")
        .item(&minimize)
        .item(&zoom)
        .separator()
        .item(&window_close)
        .build()?;

    let help_menu = SubmenuBuilder::new(&handle, "Help")
        .item(&integrity)
        .build()?;

    let menu = MenuBuilder::new(&handle)
        .item(&app_menu)
        .item(&file_menu)
        .item(&edit_menu)
        .item(&view_menu)
        .item(&window_menu)
        .item(&help_menu)
        .build()?;

    app.set_menu(menu)?;
    handle.on_menu_event(|app, event| {
        let id = event.id().as_ref().to_string();
        let _ = app.emit("menu-action", id);
    });

    Ok(())
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
