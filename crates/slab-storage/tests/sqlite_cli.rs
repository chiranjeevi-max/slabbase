#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod support;

use std::process::Command;

use slab_core::{ColumnDraft, ColumnType, DeleteAction, RelationDraft, RowData, TableDraft, Value};
use slab_storage::Workspace;
use support::TestClock;

fn text(name: &str) -> ColumnDraft {
    ColumnDraft {
        name: name.into(),
        ty: ColumnType::Text,
        required: false,
        unique: false,
        default: None,
    }
}

fn integer(name: &str) -> ColumnDraft {
    ColumnDraft {
        name: name.into(),
        ty: ColumnType::Integer,
        required: false,
        unique: false,
        default: None,
    }
}

#[test]
fn external_sqlite3_accepts_the_generated_workspace() {
    let directory = tempfile::TempDir::new().expect("tempdir");
    let path = directory.path().join("studio.slab");
    let mut workspace =
        Workspace::create(&path, "Studio", Box::new(TestClock::new())).expect("create");
    workspace
        .create_table(&TableDraft {
            name: "Clients".into(),
            columns: vec![text("Name")],
        })
        .expect("clients");
    workspace
        .create_table(&TableDraft {
            name: "Projects".into(),
            columns: vec![text("Name"), integer("Client")],
        })
        .expect("projects");
    workspace
        .create_relation(&RelationDraft {
            name: "Client Projects".into(),
            parent_table: "Clients".into(),
            child_table: "Projects".into(),
            child_column: "Client".into(),
            on_delete: DeleteAction::Cascade,
        })
        .expect("relation");
    let client = workspace
        .insert_row(
            "Clients",
            &RowData::from([("Name".to_owned(), Value::Text("Northlight".into()))]),
        )
        .expect("client");
    workspace
        .insert_row(
            "Projects",
            &RowData::from([
                ("Name".to_owned(), Value::Text("Identity".into())),
                ("Client".to_owned(), Value::Integer(client.id)),
            ]),
        )
        .expect("project");
    let dump_path = directory.path().join("studio.sql");
    workspace.export_sql(&dump_path).expect("export sql");
    workspace.close().expect("close");

    let restored_path = directory.path().join("restored.slab");
    let restore = Command::new("sqlite3")
        .arg(&restored_path)
        .arg(format!(".read {}", dump_path.display()))
        .output()
        .expect("sqlite3 is available");
    assert!(restore.status.success(), "restoring the SQL export failed");
    let restored = Command::new("sqlite3")
        .arg(&restored_path)
        .arg("PRAGMA integrity_check; SELECT COUNT(*) FROM Projects; SELECT COUNT(*) FROM _slab_layouts;")
        .output()
        .expect("sqlite3 is available");
    let restored_stdout = String::from_utf8_lossy(&restored.stdout);
    assert_eq!(restored_stdout.trim(), "ok\n1\n0");

    let output = Command::new("sqlite3")
        .arg(&path)
        .arg("PRAGMA integrity_check; PRAGMA foreign_key_check;")
        .output()
        .expect("sqlite3 is available");
    assert!(output.status.success(), "sqlite3 failed");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(stdout.trim(), "ok");

    let output = Command::new("sqlite3")
        .arg(&path)
        .arg("SELECT COUNT(*) FROM Clients; SELECT COUNT(*) FROM Projects; PRAGMA journal_mode;")
        .output()
        .expect("sqlite3 is available");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let lines: Vec<&str> = stdout.lines().collect();
    assert_eq!(lines, vec!["1", "1", "wal"]);

    let output = Command::new("sqlite3")
        .arg(&path)
        .arg("SELECT name FROM sqlite_master WHERE type = 'table' AND name LIKE '_slab_%' ORDER BY name;")
        .output()
        .expect("sqlite3 is available");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("_slab_audit_log"));
    assert!(stdout.contains("_slab_columns"));
    assert!(stdout.contains("_slab_tables"));

    let output = Command::new("sqlite3")
        .arg(&path)
        .arg("SELECT type FROM pragma_table_info('Projects') ORDER BY cid;")
        .output()
        .expect("sqlite3 is available");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.lines().any(|line| line == "INTEGER"));
    assert!(!stdout.lines().any(|line| line == "BLOB"));
}
