//! DDL generation for user tables.

use slab_core::{Column, Relation, Table};

use crate::error::StorageError;
use crate::sql::{quote_ident, quote_literal, sql_literal, storage_type};

/// One column definition.
pub(crate) fn column_definition(column: &Column) -> Result<String, StorageError> {
    let mut definition = format!("{} {}", quote_ident(&column.name), storage_type(&column.ty));
    if column.required {
        definition.push_str(" NOT NULL");
    }
    if column.unique {
        definition.push_str(" UNIQUE");
    }
    if let Some(default) = &column.default
        && !default.is_null()
    {
        definition.push_str(" DEFAULT ");
        definition.push_str(&sql_literal(default, &column.ty)?);
    }
    match &column.ty {
        slab_core::ColumnType::Boolean => {
            definition.push_str(&format!(" CHECK ({} IN (0, 1))", quote_ident(&column.name)));
        }
        slab_core::ColumnType::Select { options } => {
            let list = options
                .iter()
                .map(|option| quote_literal(option))
                .collect::<Vec<_>>()
                .join(", ");
            definition.push_str(&format!(
                " CHECK ({} IS NULL OR {} IN ({list}))",
                quote_ident(&column.name),
                quote_ident(&column.name)
            ));
        }
        _ => {}
    }
    Ok(definition)
}

/// A full `CREATE TABLE` statement. Physical name may differ from the logical
/// table name during a rebuild.
pub(crate) fn create_table_sql(
    table: &Table,
    physical_name: &str,
    relations: &[Relation],
) -> Result<String, StorageError> {
    let mut definitions: Vec<String> = Vec::new();
    definitions.push(format!(
        "{} INTEGER PRIMARY KEY AUTOINCREMENT",
        quote_ident("id")
    ));
    for column in table.stored_columns() {
        definitions.push(column_definition(column)?);
    }
    for relation in relations {
        if relation.child_table != table.name {
            continue;
        }
        definitions.push(format!(
            "CONSTRAINT {} FOREIGN KEY ({}) REFERENCES {}({}) ON DELETE {} ON UPDATE CASCADE",
            quote_ident(&format!("fk_{}", relation.id)),
            quote_ident(&relation.child_column),
            quote_ident(&relation.parent_table),
            quote_ident("id"),
            relation.on_delete.sql(),
        ));
    }
    Ok(format!(
        "CREATE TABLE {} (\n    {}\n)",
        quote_ident(physical_name),
        definitions.join(",\n    ")
    ))
}

/// A unique index for a column added after table creation.
pub(crate) fn unique_index_sql(table: &str, column: &str) -> String {
    format!(
        "CREATE UNIQUE INDEX {} ON {} ({})",
        quote_ident(&format!("slab_uq_{table}_{column}")),
        quote_ident(table),
        quote_ident(column)
    )
}

/// A non-unique index, used for relation child columns.
pub(crate) fn relation_index_sql(table: &str, column: &str) -> String {
    format!(
        "CREATE INDEX IF NOT EXISTS {} ON {} ({})",
        quote_ident(&format!("slab_ix_{table}_{column}")),
        quote_ident(table),
        quote_ident(column)
    )
}
