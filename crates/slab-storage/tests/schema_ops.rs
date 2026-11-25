#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod support;

use slab_core::{
    ColumnDraft, ColumnType, DeleteAction, Layout, LayoutItem, LayoutItemKind, PageQuery,
    RelationDraft, RowData, TableDraft, Value,
};
use slab_storage::Workspace;
use support::TestClock;

fn workspace() -> Workspace {
    Workspace::open_in_memory("Schema", Box::new(TestClock::new())).expect("workspace")
}

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

fn row(entries: &[(&str, Value)]) -> RowData {
    entries
        .iter()
        .map(|(name, value)| ((*name).to_owned(), value.clone()))
        .collect()
}

fn field_layout(table: &str, column: &str) -> Layout {
    Layout {
        id: "layout".into(),
        name: "Main form".into(),
        table: table.into(),
        grid: slab_core::GridSpec::default(),
        items: vec![LayoutItem {
            id: "field".into(),
            kind: LayoutItemKind::Field {
                column: column.into(),
            },
            x: 0,
            y: 0,
            w: 6,
            h: 2,
        }],
        updated_at: 0,
    }
}

#[test]
fn rename_table_updates_relations_metadata_and_search() {
    let mut workspace = workspace();
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
    workspace
        .save_layout(field_layout("Clients", "Name"))
        .expect("layout");
    let client = workspace
        .insert_row(
            "Clients",
            &row(&[("Name", Value::Text("Northlight Studio".into()))]),
        )
        .expect("client");
    workspace
        .insert_row(
            "Projects",
            &row(&[
                ("Name", Value::Text("Identity".into())),
                ("Client", Value::Integer(client.id)),
            ]),
        )
        .expect("project");

    workspace
        .rename_table("Clients", "Customers")
        .expect("rename");
    let schema = workspace.schema().expect("schema");
    assert!(schema.table("Customers").is_some());
    assert!(schema.table("Clients").is_none());
    assert_eq!(
        schema
            .relation("Client Projects")
            .expect("relation")
            .parent_table,
        "Customers"
    );
    assert_eq!(
        schema
            .table("Customers")
            .expect("customers")
            .columns
            .first()
            .expect("column")
            .table,
        "Customers"
    );
    assert_eq!(schema.layouts[0].table, "Customers");
    let hits = workspace.search("northlight", 10).expect("search");
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].table, "Customers");
    assert!(
        workspace
            .audit_entries(20, 0)
            .expect("audit")
            .iter()
            .any(|entry| entry.operation == "rename_table")
    );

    workspace.undo().expect("undo rename");
    let schema = workspace.schema().expect("schema");
    assert!(schema.table("Clients").is_some());
    assert_eq!(
        schema
            .relation("Client Projects")
            .expect("relation")
            .parent_table,
        "Clients"
    );
    assert_eq!(
        workspace.search("northlight", 10).expect("search")[0].table,
        "Clients"
    );
}

#[test]
fn rename_column_rewrites_formulas_layouts_and_relations() {
    let mut workspace = workspace();
    workspace
        .create_table(&TableDraft {
            name: "Calc".into(),
            columns: vec![
                integer("Base"),
                ColumnDraft {
                    name: "Double".into(),
                    ty: ColumnType::Formula {
                        expression: "[Base] * 2".into(),
                    },
                    required: false,
                    unique: false,
                    default: None,
                },
            ],
        })
        .expect("calc");
    workspace
        .create_table(&TableDraft {
            name: "Refs".into(),
            columns: vec![integer("Calc Ref")],
        })
        .expect("refs");
    workspace
        .create_relation(&RelationDraft {
            name: "Calc Refs".into(),
            parent_table: "Calc".into(),
            child_table: "Refs".into(),
            child_column: "Calc Ref".into(),
            on_delete: DeleteAction::Cascade,
        })
        .expect("relation");
    workspace
        .save_layout(field_layout("Calc", "Base"))
        .expect("layout");
    let calc = workspace
        .insert_row("Calc", &row(&[("Base", Value::Integer(7))]))
        .expect("row");
    assert_eq!(
        workspace
            .read_record("Calc", calc.id)
            .expect("read")
            .get("Double"),
        Some(&Value::Integer(14))
    );

    workspace
        .rename_column("Calc", "Base", "Value")
        .expect("rename base");
    let schema = workspace.schema().expect("schema");
    let calc_table = schema.table("Calc").expect("calc");
    let formula = calc_table.column("Double").expect("double");
    let ColumnType::Formula { expression } = &formula.ty else {
        panic!("Double should be a formula");
    };
    assert!(expression.contains("[Value]"), "formula was {expression}");
    let layout = schema.layouts.first().expect("layout");
    let LayoutItemKind::Field { column } = &layout.items[0].kind else {
        panic!("field expected");
    };
    assert_eq!(column, "Value");
    assert_eq!(
        workspace
            .read_record("Calc", calc.id)
            .expect("read")
            .get("Double"),
        Some(&Value::Integer(14))
    );

    workspace
        .rename_column("Refs", "Calc Ref", "Link")
        .expect("rename fk");
    let schema = workspace.schema().expect("schema");
    assert_eq!(
        schema.relation("Calc Refs").expect("relation").child_column,
        "Link"
    );
    assert!(
        workspace
            .schema()
            .expect("schema")
            .table("Refs")
            .expect("refs")
            .column("Link")
            .is_some()
    );
    assert!(
        workspace
            .audit_entries(50, 0)
            .expect("audit")
            .iter()
            .any(|entry| entry.operation == "rename_column")
    );
}

#[test]
fn drop_column_blocks_formula_references_and_keeps_schema_intact() {
    let mut workspace = workspace();
    workspace
        .create_table(&TableDraft {
            name: "Calc".into(),
            columns: vec![
                integer("Base"),
                ColumnDraft {
                    name: "Double".into(),
                    ty: ColumnType::Formula {
                        expression: "[Base] * 2".into(),
                    },
                    required: false,
                    unique: false,
                    default: None,
                },
            ],
        })
        .expect("calc");
    let calc = workspace
        .insert_row("Calc", &row(&[("Base", Value::Integer(4))]))
        .expect("row");
    let error = workspace
        .drop_column("Calc", "Base")
        .expect_err("formula reference should block");
    assert_eq!(error.code_str(), "SLAB_E_FORMULA_REF");
    let schema = workspace.schema().expect("schema");
    assert!(schema.table("Calc").expect("calc").column("Base").is_some());
    assert_eq!(
        workspace
            .read_record("Calc", calc.id)
            .expect("read")
            .get("Base"),
        Some(&Value::Integer(4))
    );
    assert!(
        !workspace
            .audit_entries(50, 0)
            .expect("audit")
            .iter()
            .any(|entry| entry.operation == "drop_column")
    );
}

#[test]
fn drop_column_restores_values_and_layout_blocks_are_explicit() {
    let mut workspace = workspace();
    workspace
        .create_table(&TableDraft {
            name: "Notes".into(),
            columns: vec![text("Title"), text("Body")],
        })
        .expect("notes");
    workspace
        .save_layout(field_layout("Notes", "Body"))
        .expect("layout");
    let note = workspace
        .insert_row(
            "Notes",
            &row(&[
                ("Title", Value::Text("A".into())),
                ("Body", Value::Text("Keep me".into())),
            ]),
        )
        .expect("row");
    let error = workspace.drop_column("Notes", "Body").expect_err("layout");
    assert_eq!(error.code_str(), "SLAB_E_FORMULA_REF");
    workspace.delete_layout("layout").expect("delete layout");
    workspace.drop_column("Notes", "Body").expect("drop column");
    assert!(
        workspace
            .schema()
            .expect("schema")
            .table("Notes")
            .expect("notes")
            .column("Body")
            .is_none()
    );
    workspace.undo().expect("undo drop");
    assert_eq!(
        workspace
            .read_record("Notes", note.id)
            .expect("read")
            .get("Body"),
        Some(&Value::Text("Keep me".into()))
    );
}

#[test]
fn drop_table_restores_rows_layouts_and_indexes() {
    let mut workspace = workspace();
    workspace
        .create_table(&TableDraft {
            name: "Items".into(),
            columns: vec![text("Name"), text("Code")],
        })
        .expect("items");
    workspace
        .save_layout(field_layout("Items", "Name"))
        .expect("layout");
    workspace.create_index("Items", "Name").expect("index");
    let item = workspace
        .insert_row(
            "Items",
            &row(&[
                ("Name", Value::Text("Widget".into())),
                ("Code", Value::Text("W-1".into())),
            ]),
        )
        .expect("row");
    workspace.drop_table("Items").expect("drop table");
    let schema = workspace.schema().expect("schema");
    assert!(schema.table("Items").is_none());
    assert!(schema.layouts.is_empty());
    assert_eq!(
        workspace
            .audit_entries(50, 0)
            .expect("audit")
            .iter()
            .filter(|entry| entry.operation == "drop_table")
            .count(),
        1
    );
    workspace.undo().expect("undo drop");
    let schema = workspace.schema().expect("schema");
    assert!(schema.table("Items").is_some());
    assert_eq!(schema.layouts.len(), 1);
    assert_eq!(workspace.list_indexes("Items").expect("indexes").len(), 1);
    let page = workspace
        .page(&PageQuery::first("Items", 10))
        .expect("page");
    assert_eq!(page.rows[0].id, item.id);
    assert_eq!(
        page.rows[0].get("Name"),
        Some(&Value::Text("Widget".into()))
    );
}

#[test]
fn index_toggles_are_idempotent_and_audited() {
    let mut workspace = workspace();
    workspace
        .create_table(&TableDraft {
            name: "Items".into(),
            columns: vec![text("Name")],
        })
        .expect("table");
    let index = workspace.create_index("Items", "Name").expect("create");
    assert_eq!(workspace.list_indexes("Items").expect("list").len(), 1);
    let duplicate = workspace.create_index("Items", "Name").expect("duplicate");
    assert_eq!(duplicate.name, index.name);
    assert_eq!(workspace.list_indexes("Items").expect("list").len(), 1);
    workspace.drop_index(&index.name).expect("drop");
    assert!(workspace.list_indexes("Items").expect("list").is_empty());
    let operations: Vec<String> = workspace
        .audit_entries(20, 0)
        .expect("audit")
        .into_iter()
        .map(|entry| entry.operation)
        .collect();
    assert!(operations.contains(&"create_index".into()));
    assert!(operations.contains(&"drop_index".into()));
}

#[test]
fn drop_table_is_blocked_while_a_relation_exists() {
    let mut workspace = workspace();
    workspace
        .create_table(&TableDraft {
            name: "Clients".into(),
            columns: vec![text("Name")],
        })
        .expect("clients");
    workspace
        .create_table(&TableDraft {
            name: "Projects".into(),
            columns: vec![integer("Client")],
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
    let error = workspace.drop_table("Clients").expect_err("blocked");
    assert_eq!(error.code_str(), "SLAB_E_COMMAND");
    assert!(
        workspace
            .schema()
            .expect("schema")
            .table("Clients")
            .is_some()
    );
}

#[test]
fn rename_to_existing_name_rolls_back_cleanly() {
    let mut workspace = workspace();
    workspace
        .create_table(&TableDraft {
            name: "A".into(),
            columns: vec![text("Name")],
        })
        .expect("A");
    workspace
        .create_table(&TableDraft {
            name: "B".into(),
            columns: vec![text("Name")],
        })
        .expect("B");
    let error = workspace.rename_table("A", "B").expect_err("collision");
    assert_eq!(error.code_str(), "SLAB_E_UNIQUE");
    let schema = workspace.schema().expect("schema");
    assert!(schema.table("A").is_some());
    assert!(schema.table("B").is_some());
}

#[test]
fn rename_column_rewrites_cross_table_aggregate_formulas() {
    let mut workspace = workspace();
    workspace
        .create_table(&TableDraft {
            name: "Invoices".into(),
            columns: vec![text("Number")],
        })
        .expect("invoices");
    workspace
        .create_table(&TableDraft {
            name: "Lines".into(),
            columns: vec![integer("Amount"), integer("Invoice")],
        })
        .expect("lines");
    workspace
        .create_relation(&RelationDraft {
            name: "Invoice Lines".into(),
            parent_table: "Invoices".into(),
            child_table: "Lines".into(),
            child_column: "Invoice".into(),
            on_delete: DeleteAction::Cascade,
        })
        .expect("relation");
    workspace
        .add_column(
            "Invoices",
            &ColumnDraft {
                name: "Total".into(),
                ty: ColumnType::Formula {
                    expression: "SUM([Invoice Lines].[Amount])".into(),
                },
                required: false,
                unique: false,
                default: None,
            },
        )
        .expect("rollup");
    let invoice = workspace
        .insert_row("Invoices", &row(&[("Number", Value::Text("INV".into()))]))
        .expect("invoice");
    workspace
        .insert_row(
            "Lines",
            &row(&[
                ("Amount", Value::Integer(30)),
                ("Invoice", Value::Integer(invoice.id)),
            ]),
        )
        .expect("line");
    assert_eq!(
        workspace
            .read_record("Invoices", invoice.id)
            .expect("read")
            .get("Total"),
        Some(&Value::Integer(30))
    );
    workspace
        .rename_column("Lines", "Amount", "Line Total")
        .expect("rename");
    let schema = workspace.schema().expect("schema");
    let ColumnType::Formula { expression } = &schema
        .table("Invoices")
        .expect("invoices")
        .column("Total")
        .expect("total")
        .ty
    else {
        panic!("formula expected");
    };
    assert!(
        expression.contains("[Line Total]"),
        "formula was {expression}"
    );
    assert_eq!(
        workspace
            .read_record("Invoices", invoice.id)
            .expect("read")
            .get("Total"),
        Some(&Value::Integer(30))
    );
}

#[test]
fn restore_required_column_rebuilds_constraints_and_values() {
    let mut workspace = workspace();
    workspace
        .create_table(&TableDraft {
            name: "Notes".into(),
            columns: vec![
                ColumnDraft {
                    name: "Title".into(),
                    ty: ColumnType::Text,
                    required: true,
                    unique: false,
                    default: None,
                },
                ColumnDraft {
                    name: "Body".into(),
                    ty: ColumnType::Text,
                    required: true,
                    unique: false,
                    default: None,
                },
            ],
        })
        .expect("notes");
    let note = workspace
        .insert_row(
            "Notes",
            &row(&[
                ("Title", Value::Text("Title".into())),
                ("Body", Value::Text("Body".into())),
            ]),
        )
        .expect("row");
    workspace.drop_column("Notes", "Body").expect("drop");
    workspace.undo().expect("undo");
    let record = workspace.read_record("Notes", note.id).expect("read");
    assert_eq!(record.get("Body"), Some(&Value::Text("Body".into())));
    let schema = workspace.schema().expect("schema");
    let column = schema
        .table("Notes")
        .expect("notes")
        .column("Body")
        .expect("body");
    assert!(column.required);
    let page = workspace
        .page(&PageQuery::first("Notes", 10))
        .expect("page");
    assert_eq!(page.rows.len(), 1);
}

#[test]
fn drop_relation_is_transactional_and_undoable() {
    let mut workspace = workspace();
    workspace
        .create_table(&TableDraft {
            name: "Clients".into(),
            columns: vec![text("Name")],
        })
        .expect("clients");
    workspace
        .create_table(&TableDraft {
            name: "Projects".into(),
            columns: vec![integer("Client")],
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
    workspace.drop_relation("Client Projects").expect("drop");
    assert!(workspace.schema().expect("schema").relations.is_empty());
    workspace.undo().expect("undo");
    assert_eq!(workspace.schema().expect("schema").relations.len(), 1);
}

#[test]
fn rename_column_preserves_user_index_and_updates_index_columns() {
    let mut workspace = workspace();
    workspace
        .create_table(&TableDraft {
            name: "Items".into(),
            columns: vec![text("Name")],
        })
        .expect("table");
    workspace.create_index("Items", "Name").expect("index");
    workspace
        .rename_column("Items", "Name", "Label")
        .expect("rename");
    let indexes = workspace.list_indexes("Items").expect("indexes");
    assert_eq!(indexes.len(), 1);
    assert_eq!(indexes[0].columns, vec!["Label".to_owned()]);
}
