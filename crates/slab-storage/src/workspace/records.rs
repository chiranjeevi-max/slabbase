use std::collections::BTreeSet;

use rusqlite::{Connection, Transaction, params};
use slab_core::{DeleteAction, PolicyHook, RowData, RowId, Schema, Table, Value};
use slab_formula::FormulaDag;

use crate::error::StorageError;
use crate::read::check_table_policy;
use crate::sql::{decode_value, encode_value, normalize_value, quote_ident, stored_columns};

use super::Workspace;

pub(super) fn prepare_row(
    table: &Table,
    values: &RowData,
    existing: Option<&RowData>,
    inserting: bool,
) -> Result<RowData, StorageError> {
    for key in values.keys() {
        if key == "id" {
            continue;
        }
        let column = table
            .column(key)
            .ok_or_else(|| StorageError::Query(format!("unknown column {}.{key}", table.name)))?;
        if column.ty.is_computed() {
            continue;
        }
    }
    let mut prepared = RowData::new();
    for column in table.stored_columns() {
        let value = if let Some(value) = values.get(&column.name) {
            normalize_value(value, &column.ty)?
        } else if let Some(existing) = existing {
            existing.get(&column.name).cloned().unwrap_or(Value::Null)
        } else if let Some(default) = &column.default {
            normalize_value(default, &column.ty)?
        } else {
            Value::Null
        };
        if column.required && value.is_null() {
            return Err(StorageError::Constraint(format!(
                "{}.{} is required",
                table.name, column.name
            )));
        }
        if !column.ty.accepts(&value) {
            return Err(StorageError::Query(format!(
                "{}.{} does not accept {}",
                table.name,
                column.name,
                value.type_name()
            )));
        }
        prepared.insert(column.name.clone(), value);
    }
    if inserting {
        for key in values.keys() {
            if key != "id" && table.column(key).is_none() {
                return Err(StorageError::Query(format!(
                    "unknown column {}.{key}",
                    table.name
                )));
            }
        }
    }
    Ok(prepared)
}

pub(crate) fn read_stored_record(
    conn: &Connection,
    table: &Table,
    id: RowId,
) -> Result<Option<slab_core::Record>, StorageError> {
    let columns = stored_columns(table);
    let mut projections = vec![quote_ident("id")];
    projections.extend(columns.iter().map(|column| quote_ident(&column.name)));
    let sql = format!(
        "SELECT {} FROM {} WHERE {} = ?1",
        projections.join(", "),
        quote_ident(&table.name),
        quote_ident("id")
    );
    let mut stmt = conn.prepare_cached(&sql)?;
    let mut rows = stmt.query(params![id])?;
    let Some(row) = rows.next()? else {
        return Ok(None);
    };
    let mut values = RowData::new();
    for (index, column) in columns.iter().enumerate() {
        values.insert(
            column.name.clone(),
            decode_value(row, index + 1, &column.ty)?,
        );
    }
    Ok(Some(slab_core::Record { id, values }))
}

/// Reads one stored row and evaluates its computed columns on `conn`.
///
/// `conn` must be the connection that owns the read snapshot (a writer
/// transaction during a write, or a reader replica during a read).
pub(super) fn read_computed_record(
    conn: &Connection,
    schema: &Schema,
    dag: &FormulaDag,
    table: &Table,
    id: RowId,
) -> Result<slab_core::Record, StorageError> {
    let mut record = read_stored_record(conn, table, id)?
        .ok_or_else(|| StorageError::NotFound(format!("{} #{}", table.name, id)))?;
    Workspace::compute_record_with_dag(conn, None, schema, table, &mut record, dag)?;
    Ok(record)
}

pub(super) fn insert_record_tx(
    tx: &Connection,
    table: &Table,
    record: &slab_core::Record,
    explicit_id: bool,
) -> Result<RowId, StorageError> {
    let columns = stored_columns(table);
    let mut names: Vec<String> = Vec::new();
    let mut placeholders: Vec<String> = Vec::new();
    let mut bound: Vec<rusqlite::types::Value> = Vec::new();
    if explicit_id {
        names.push(quote_ident("id"));
        placeholders.push("?1".into());
        bound.push(rusqlite::types::Value::Integer(record.id));
    }
    for column in &columns {
        let value = record
            .values
            .get(&column.name)
            .cloned()
            .unwrap_or(Value::Null);
        if column.required && value.is_null() {
            return Err(StorageError::Constraint(format!(
                "{}.{} is required",
                table.name, column.name
            )));
        }
        names.push(quote_ident(&column.name));
        placeholders.push(format!("?{}", bound.len() + 1));
        bound.push(encode_value(&value, &column.ty)?);
    }
    let sql = if names.is_empty() {
        format!("INSERT INTO {} DEFAULT VALUES", quote_ident(&table.name))
    } else {
        format!(
            "INSERT INTO {} ({}) VALUES ({})",
            quote_ident(&table.name),
            names.join(", "),
            placeholders.join(", ")
        )
    };
    tx.execute(&sql, rusqlite::params_from_iter(bound))?;
    if explicit_id {
        Ok(record.id)
    } else {
        Ok(tx.last_insert_rowid())
    }
}

pub(super) fn update_record_tx(
    tx: &Transaction<'_>,
    table: &Table,
    record: &slab_core::Record,
) -> Result<(), StorageError> {
    let columns = stored_columns(table);
    let mut assignments: Vec<String> = Vec::new();
    let mut bound: Vec<rusqlite::types::Value> = Vec::new();
    for column in &columns {
        let value = record
            .values
            .get(&column.name)
            .cloned()
            .unwrap_or(Value::Null);
        if column.required && value.is_null() {
            return Err(StorageError::Constraint(format!(
                "{}.{} is required",
                table.name, column.name
            )));
        }
        bound.push(encode_value(&value, &column.ty)?);
        assignments.push(format!("{} = ?{}", quote_ident(&column.name), bound.len()));
    }
    if assignments.is_empty() {
        return Ok(());
    }
    bound.push(rusqlite::types::Value::Integer(record.id));
    let sql = format!(
        "UPDATE {} SET {} WHERE {} = ?{}",
        quote_ident(&table.name),
        assignments.join(", "),
        quote_ident("id"),
        bound.len()
    );
    let changed = tx.execute(&sql, rusqlite::params_from_iter(bound))?;
    if changed == 0 {
        return Err(StorageError::NotFound(format!(
            "{} #{}",
            table.name, record.id
        )));
    }
    Ok(())
}

pub(super) fn delete_row_tx(
    tx: &Transaction<'_>,
    table: &str,
    id: RowId,
) -> Result<(), StorageError> {
    let changed = tx.execute(
        &format!(
            "DELETE FROM {} WHERE {} = ?1",
            quote_ident(table),
            quote_ident("id")
        ),
        params![id],
    )?;
    if changed == 0 {
        return Err(StorageError::NotFound(format!("{table} #{id}")));
    }
    Ok(())
}

pub(super) fn capture_cascades(
    conn: &Connection,
    schema: &Schema,
    policy: &dyn PolicyHook,
    table: &str,
    id: RowId,
    seen: &mut BTreeSet<(String, RowId)>,
    out: &mut Vec<slab_core::RecordRef>,
) -> Result<(), StorageError> {
    for relation in schema.relations.iter().filter(|relation| {
        relation.parent_table == table && relation.on_delete == DeleteAction::Cascade
    }) {
        let Some(child) = schema.table(&relation.child_table) else {
            continue;
        };
        let sql = format!(
            "SELECT {} FROM {} WHERE {} = ?1 ORDER BY {}",
            quote_ident("id"),
            quote_ident(&child.name),
            quote_ident(&relation.child_column),
            quote_ident("id")
        );
        let mut stmt = conn.prepare(&sql)?;
        let mut ids = stmt
            .query_map(params![id], |row| row.get::<_, RowId>(0))?
            .peekable();
        if ids.peek().is_none() {
            continue;
        }
        check_table_policy(policy, &child.name)?;
        let child_ids = ids.collect::<Result<Vec<_>, _>>()?;
        drop(stmt);
        for child_id in child_ids {
            // Two cascade paths can reach one child (for example a project
            // whose Lead and Sponsor are the same person). Capture it once so
            // the inverse insert cannot collide with itself.
            if !seen.insert((child.name.clone(), child_id)) {
                continue;
            }
            let Some(row) = read_stored_record(conn, child, child_id)? else {
                continue;
            };
            out.push(slab_core::RecordRef {
                table: child.name.clone(),
                row,
            });
            capture_cascades(conn, schema, policy, &child.name, child_id, seen, out)?;
        }
    }
    Ok(())
}

pub(super) fn rebuild_fts_for_table(
    tx: &Transaction<'_>,
    table: &Table,
) -> Result<(), StorageError> {
    tx.execute(
        "DELETE FROM _slab_fts WHERE table_name = ?1",
        params![table.name],
    )?;
    let columns = stored_columns(table);
    let mut projections = vec![quote_ident("id")];
    projections.extend(columns.iter().map(|column| quote_ident(&column.name)));
    let sql = format!(
        "SELECT {} FROM {}",
        projections.join(", "),
        quote_ident(&table.name)
    );
    let mut stmt = tx.prepare(&sql)?;
    let mut rows = stmt.query([])?;
    while let Some(row) = rows.next()? {
        let id: RowId = row.get(0)?;
        let mut values = RowData::new();
        for (index, column) in columns.iter().enumerate() {
            values.insert(
                column.name.clone(),
                decode_value(row, index + 1, &column.ty)?,
            );
        }
        index_fts(tx, table, id, &values)?;
    }
    Ok(())
}

pub(super) fn index_fts(
    tx: &Connection,
    table: &Table,
    id: RowId,
    values: &RowData,
) -> Result<(), StorageError> {
    let mut body = String::new();
    for column in table.stored_columns() {
        if let Some(value) = values.get(&column.name) {
            if value.is_null() {
                continue;
            }
            if !body.is_empty() {
                body.push('\n');
            }
            body.push_str(&value.display_string());
        }
    }
    if body.trim().is_empty() {
        return Ok(());
    }
    tx.execute(
        "INSERT INTO _slab_fts (table_name, row_id, body) VALUES (?1, ?2, ?3)",
        params![table.name, id, body],
    )?;
    Ok(())
}

pub(super) fn refresh_fts(
    tx: &Connection,
    table: &Table,
    id: RowId,
    values: &RowData,
) -> Result<(), StorageError> {
    delete_fts(tx, &table.name, id)?;
    index_fts(tx, table, id, values)
}

pub(super) fn delete_fts(tx: &Connection, table: &str, id: RowId) -> Result<(), StorageError> {
    tx.execute(
        "DELETE FROM _slab_fts WHERE table_name = ?1 AND CAST(row_id AS INTEGER) = ?2",
        params![table, id],
    )?;
    Ok(())
}
