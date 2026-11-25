//! Incremental maintenance of parent rollup columns.
//!
//! Every lookup column (`COUNT`, `SUM`, `AVG`, `MIN`, `MAX`) keeps a compact
//! `(count, sum, extreme)` state per `(parent, relation, child column,
//! aggregate)` key. A child mutation adjusts that state in O(1); `MIN` and
//! `MAX` fall back to one indexed `ORDER BY ... LIMIT 1` seek only when the
//! changed child held the previous extremum. Formula columns stay virtual and
//! are evaluated in dependency order on read, so only the affected parent's
//! cached inputs are touched.

use std::collections::BTreeSet;

use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use slab_core::{
    Aggregate, Column, ColumnType, Decimal, Money, Record, Relation, RowId, Schema, Value,
};

use crate::error::StorageError;
use crate::sql::{decode_value, quote_ident};

/// The compact aggregate state stored for one parent.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub(crate) struct RollupState {
    count: i64,
    sum: Option<Value>,
    extreme: Option<Value>,
}

/// One cache key.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct RollupKey {
    parent_table: String,
    parent_id: RowId,
    relation: String,
    child_column: String,
    aggregate: Aggregate,
}

/// A lookup column that references a mutated child table.
#[derive(Clone, Debug)]
struct LookupRef {
    parent_table: String,
    parent_column: String,
    relation: String,
    child_column: String,
    child_table: String,
    child_type: ColumnType,
    aggregate: Aggregate,
}

fn lookup_refs(schema: &Schema, child_table: &str) -> Vec<LookupRef> {
    let mut refs = Vec::new();
    for parent in &schema.tables {
        for column in &parent.columns {
            let ColumnType::Lookup {
                relation,
                column: child_column,
                aggregate,
            } = &column.ty
            else {
                continue;
            };
            let Some(relation) = schema.relation(relation) else {
                continue;
            };
            if relation.child_table != child_table || relation.parent_table != parent.name {
                continue;
            }
            let Some(child) = schema.table(&relation.child_table) else {
                continue;
            };
            let Some(child_definition) = child.column(child_column) else {
                continue;
            };
            refs.push(LookupRef {
                parent_table: parent.name.clone(),
                parent_column: relation.child_column.clone(),
                relation: relation.name.clone(),
                child_column: child_column.clone(),
                child_table: relation.child_table.clone(),
                child_type: child_definition.ty.clone(),
                aggregate: *aggregate,
            });
        }
    }
    refs
}

fn parent_id(record: &Record, child_column: &str) -> Option<RowId> {
    match record.values.get(child_column) {
        Some(Value::Integer(id)) => Some(*id),
        _ => None,
    }
}

fn value_for<'a>(record: &'a Record, column: &str) -> Option<&'a Value> {
    record.values.get(column).filter(|value| !value.is_null())
}

fn key_for(reference: &LookupRef, parent_id: RowId) -> RollupKey {
    RollupKey {
        parent_table: reference.parent_table.clone(),
        parent_id,
        relation: reference.relation.clone(),
        child_column: reference.child_column.clone(),
        aggregate: reference.aggregate,
    }
}

fn load_state(conn: &Connection, key: &RollupKey) -> Result<Option<RollupState>, StorageError> {
    let json: Option<String> = conn
        .query_row(
            "SELECT state_json FROM _slab_rollups
             WHERE parent_table = ?1 AND parent_id = ?2 AND relation_name = ?3
               AND child_column = ?4 AND aggregate = ?5",
            params![
                key.parent_table,
                key.parent_id,
                key.relation,
                key.child_column,
                key.aggregate.as_str(),
            ],
            |row| row.get(0),
        )
        .optional()?;
    match json {
        Some(json) => Ok(Some(serde_json::from_str(&json)?)),
        None => Ok(None),
    }
}

fn store_state(
    conn: &Connection,
    key: &RollupKey,
    state: &RollupState,
) -> Result<(), StorageError> {
    conn.execute(
        "INSERT INTO _slab_rollups
            (parent_table, parent_id, relation_name, child_column, aggregate, state_json)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)
         ON CONFLICT(parent_table, parent_id, relation_name, child_column, aggregate)
         DO UPDATE SET state_json = excluded.state_json",
        params![
            key.parent_table,
            key.parent_id,
            key.relation,
            key.child_column,
            key.aggregate.as_str(),
            serde_json::to_string(state)?,
        ],
    )?;
    Ok(())
}

fn delete_key(conn: &Connection, key: &RollupKey) -> Result<(), StorageError> {
    conn.execute(
        "DELETE FROM _slab_rollups
         WHERE parent_table = ?1 AND parent_id = ?2 AND relation_name = ?3
           AND child_column = ?4 AND aggregate = ?5",
        params![
            key.parent_table,
            key.parent_id,
            key.relation,
            key.child_column,
            key.aggregate.as_str(),
        ],
    )?;
    Ok(())
}

/// Drops every cached rollup. Schema changes call this once; the next read
/// rebuilds only the parents it touches.
pub(crate) fn clear(conn: &Connection) -> Result<(), StorageError> {
    conn.execute("DELETE FROM _slab_rollups", [])?;
    Ok(())
}

/// Removes all cached rollups where the row is the parent.
pub(crate) fn forget_parent(
    conn: &Connection,
    parent_table: &str,
    parent_id: RowId,
) -> Result<(), StorageError> {
    conn.execute(
        "DELETE FROM _slab_rollups WHERE parent_table = ?1 AND parent_id = ?2",
        params![parent_table, parent_id],
    )?;
    Ok(())
}

fn compute_state(
    conn: &Connection,
    reference: &LookupRef,
    parent_id: RowId,
) -> Result<RollupState, StorageError> {
    let relation = Relation {
        id: String::new(),
        name: reference.relation.clone(),
        parent_table: reference.parent_table.clone(),
        child_table: reference.child_table.clone(),
        child_column: reference.parent_column.clone(),
        on_delete: slab_core::DeleteAction::Restrict,
    };
    let column = Column {
        id: String::new(),
        table: reference.child_table.clone(),
        name: reference.child_column.clone(),
        ty: reference.child_type.clone(),
        required: false,
        unique: false,
        default: None,
        position: 0,
    };
    compute_state_sql(conn, &relation, &column, reference.aggregate, parent_id)
}

/// Computes a rollup state directly from a stored child column.
fn compute_state_sql(
    conn: &Connection,
    relation: &Relation,
    column: &Column,
    aggregate: Aggregate,
    parent_id: RowId,
) -> Result<RollupState, StorageError> {
    let child = quote_ident(&relation.child_table);
    let child_column = quote_ident(&column.name);
    let parent = quote_ident(&relation.child_column);
    let mut state = RollupState::default();
    match aggregate {
        Aggregate::Count => {
            let sql = format!(
                "SELECT COUNT({child_column}) FROM {child}
                 WHERE {parent} = ?1 AND {child_column} IS NOT NULL"
            );
            state.count = conn.query_row(&sql, params![parent_id], |row| row.get(0))?;
        }
        Aggregate::Min | Aggregate::Max => {
            let direction = if aggregate == Aggregate::Min {
                "ASC"
            } else {
                "DESC"
            };
            let count_sql = format!(
                "SELECT COUNT({child_column}) FROM {child}
                 WHERE {parent} = ?1 AND {child_column} IS NOT NULL"
            );
            state.count = conn.query_row(&count_sql, params![parent_id], |row| row.get(0))?;
            let sql = format!(
                "SELECT {child_column} FROM {child}
                 WHERE {parent} = ?1 AND {child_column} IS NOT NULL
                 ORDER BY {child_column} {direction} LIMIT 1"
            );
            let mut stmt = conn.prepare(&sql)?;
            let mut rows = stmt.query(params![parent_id])?;
            if let Some(row) = rows.next()? {
                state.extreme = Some(decode_value(row, 0, &column.ty)?);
            }
        }
        Aggregate::Sum | Aggregate::Avg => {
            let sql = format!("SELECT {child_column} FROM {child} WHERE {parent} = ?1");
            let mut stmt = conn.prepare(&sql)?;
            let mut rows = stmt.query(params![parent_id])?;
            while let Some(row) = rows.next()? {
                let value = decode_value(row, 0, &column.ty)?;
                accumulate(
                    &mut state.count,
                    &mut state.sum,
                    &mut state.extreme,
                    aggregate,
                    value,
                )?;
            }
        }
    }
    Ok(state)
}

fn add_value(
    state: &mut RollupState,
    aggregate: Aggregate,
    value: &Value,
) -> Result<(), StorageError> {
    if value.is_null() {
        return Ok(());
    }
    state.count = state
        .count
        .checked_add(1)
        .ok_or_else(|| StorageError::Internal("rollup count overflowed".into()))?;
    match aggregate {
        Aggregate::Count => {}
        Aggregate::Sum | Aggregate::Avg => {
            state.sum = Some(match state.sum.take() {
                None => value.clone(),
                Some(sum) => add_values(sum, value.clone())?,
            });
        }
        Aggregate::Min => {
            if state
                .extreme
                .as_ref()
                .is_none_or(|current| value_less(value, current))
            {
                state.extreme = Some(value.clone());
            }
        }
        Aggregate::Max => {
            if state
                .extreme
                .as_ref()
                .is_none_or(|current| value_less(current, value))
            {
                state.extreme = Some(value.clone());
            }
        }
    }
    Ok(())
}

fn remove_value(
    state: &mut RollupState,
    aggregate: Aggregate,
    value: &Value,
) -> Result<(), StorageError> {
    if value.is_null() {
        return Ok(());
    }
    state.count = state
        .count
        .checked_sub(1)
        .ok_or_else(|| StorageError::Internal("rollup count underflowed".into()))?;
    if matches!(aggregate, Aggregate::Sum | Aggregate::Avg)
        && let Some(sum) = state.sum.take()
    {
        state.sum = Some(sub_values(sum, value.clone())?);
    }
    Ok(())
}

fn was_extreme(state: &RollupState, aggregate: Aggregate, value: Option<&Value>) -> bool {
    if !matches!(aggregate, Aggregate::Min | Aggregate::Max) {
        return false;
    }
    match (state.extreme.as_ref(), value) {
        (None, _) => true,
        (Some(extreme), Some(value)) => extreme == value,
        (Some(_), None) => false,
    }
}

fn add_child(
    conn: &Connection,
    reference: &LookupRef,
    key: &RollupKey,
    value: Option<&Value>,
) -> Result<(), StorageError> {
    if !reference.child_type.is_stored() {
        return delete_key(conn, key);
    }
    let Some(mut state) = load_state(conn, key)? else {
        return store_state(conn, key, &compute_state(conn, reference, key.parent_id)?);
    };
    if let Some(value) = value {
        add_value(&mut state, reference.aggregate, value)?;
    }
    store_state(conn, key, &state)
}

fn remove_child(
    conn: &Connection,
    reference: &LookupRef,
    key: &RollupKey,
    value: Option<&Value>,
) -> Result<(), StorageError> {
    if !reference.child_type.is_stored() {
        return delete_key(conn, key);
    }
    let Some(mut state) = load_state(conn, key)? else {
        return store_state(conn, key, &compute_state(conn, reference, key.parent_id)?);
    };
    if was_extreme(&state, reference.aggregate, value) {
        return store_state(conn, key, &compute_state(conn, reference, key.parent_id)?);
    }
    if let Some(value) = value {
        remove_value(&mut state, reference.aggregate, value)?;
    }
    if state.count <= 0 {
        state.extreme = None;
    }
    store_state(conn, key, &state)
}

fn update_child(
    conn: &Connection,
    reference: &LookupRef,
    key: &RollupKey,
    old: Option<&Value>,
    new: Option<&Value>,
) -> Result<(), StorageError> {
    if !reference.child_type.is_stored() {
        return delete_key(conn, key);
    }
    let Some(mut state) = load_state(conn, key)? else {
        return store_state(conn, key, &compute_state(conn, reference, key.parent_id)?);
    };
    if matches!(reference.aggregate, Aggregate::Min | Aggregate::Max) {
        let old_is_extreme = was_extreme(&state, reference.aggregate, old);
        let new_beats = match (new, state.extreme.as_ref()) {
            (Some(new), Some(current)) if reference.aggregate == Aggregate::Min => {
                value_less(new, current)
            }
            (Some(new), Some(current)) => value_less(current, new),
            (Some(_), None) => true,
            (None, _) => false,
        };
        if old_is_extreme || new_beats {
            return store_state(conn, key, &compute_state(conn, reference, key.parent_id)?);
        }
        return Ok(());
    }
    if let Some(old) = old {
        remove_value(&mut state, reference.aggregate, old)?;
    }
    if let Some(new) = new {
        add_value(&mut state, reference.aggregate, new)?;
    }
    store_state(conn, key, &state)
}

fn each_reference(
    schema: &Schema,
    table: &str,
    mut apply: impl FnMut(&LookupRef) -> Result<(), StorageError>,
) -> Result<(), StorageError> {
    let mut seen = BTreeSet::new();
    for reference in lookup_refs(schema, table) {
        if !seen.insert((
            reference.parent_table.clone(),
            reference.relation.clone(),
            reference.child_column.clone(),
            reference.aggregate,
        )) {
            continue;
        }
        apply(&reference)?;
    }
    Ok(())
}

/// Applies an insert to every affected parent cache.
pub(crate) fn maintain_insert(
    conn: &Connection,
    schema: &Schema,
    table: &str,
    record: &Record,
) -> Result<(), StorageError> {
    each_reference(schema, table, |reference| {
        let Some(id) = parent_id(record, &reference.parent_column) else {
            return Ok(());
        };
        let key = key_for(reference, id);
        add_child(
            conn,
            reference,
            &key,
            value_for(record, &reference.child_column),
        )
    })
}

/// Applies an update to every affected parent cache, handling a changed
/// foreign key as one removal and one insertion on different parents.
pub(crate) fn maintain_update(
    conn: &Connection,
    schema: &Schema,
    table: &str,
    before: &Record,
    after: &Record,
) -> Result<(), StorageError> {
    each_reference(schema, table, |reference| {
        let old_parent = parent_id(before, &reference.parent_column);
        let new_parent = parent_id(after, &reference.parent_column);
        let old_value = value_for(before, &reference.child_column);
        let new_value = value_for(after, &reference.child_column);
        match (old_parent, new_parent) {
            (Some(old), Some(new)) if old == new => {
                if old_value != new_value || !reference.child_type.is_stored() {
                    update_child(
                        conn,
                        reference,
                        &key_for(reference, old),
                        old_value,
                        new_value,
                    )?;
                }
                Ok(())
            }
            (Some(old), new_parent) => {
                remove_child(conn, reference, &key_for(reference, old), old_value)?;
                if let Some(new) = new_parent {
                    add_child(conn, reference, &key_for(reference, new), new_value)?;
                }
                Ok(())
            }
            (None, Some(new)) => add_child(conn, reference, &key_for(reference, new), new_value),
            (None, None) => Ok(()),
        }
    })
}

/// Applies a delete to every affected parent cache.
pub(crate) fn maintain_delete(
    conn: &Connection,
    schema: &Schema,
    table: &str,
    record: &Record,
) -> Result<(), StorageError> {
    each_reference(schema, table, |reference| {
        let Some(id) = parent_id(record, &reference.parent_column) else {
            return Ok(());
        };
        remove_child(
            conn,
            reference,
            &key_for(reference, id),
            value_for(record, &reference.child_column),
        )
    })
}

/// Reads a cached rollup state when present.
pub(crate) fn cached_state(
    conn: &Connection,
    parent_table: &str,
    parent_id: RowId,
    relation: &str,
    child_column: &str,
    aggregate: Aggregate,
) -> Result<Option<RollupState>, StorageError> {
    load_state(
        conn,
        &RollupKey {
            parent_table: parent_table.to_owned(),
            parent_id,
            relation: relation.to_owned(),
            child_column: child_column.to_owned(),
            aggregate,
        },
    )
}

/// Stores a fully computed state for one key.
pub(crate) fn store_computed(
    conn: &Connection,
    parent_table: &str,
    parent_id: RowId,
    relation: &str,
    child_column: &str,
    aggregate: Aggregate,
    state: &RollupState,
) -> Result<(), StorageError> {
    store_state(
        conn,
        &RollupKey {
            parent_table: parent_table.to_owned(),
            parent_id,
            relation: relation.to_owned(),
            child_column: child_column.to_owned(),
            aggregate,
        },
        state,
    )
}

/// Finalizes a state into the value the read path returns.
pub(crate) fn finalize_state(
    state: &RollupState,
    aggregate: Aggregate,
    column: &Column,
) -> Result<Value, StorageError> {
    match aggregate {
        Aggregate::Count => Ok(Value::Integer(state.count)),
        Aggregate::Sum => Ok(state
            .sum
            .clone()
            .unwrap_or_else(|| empty_aggregate_value(column))),
        Aggregate::Avg => {
            let Some(sum) = state.sum.clone() else {
                return Ok(empty_aggregate_value(column));
            };
            if state.count == 0 {
                return Ok(empty_aggregate_value(column));
            }
            match sum {
                Value::Integer(integer) => Ok(Value::Decimal(
                    Decimal::from_integer(integer)
                        .checked_div(Decimal::from_integer(state.count))?,
                )),
                Value::Decimal(decimal) => Ok(Value::Decimal(
                    decimal.checked_div(Decimal::from_integer(state.count))?,
                )),
                Value::Currency(money) => {
                    Ok(Value::Currency(money.checked_mul_ratio(1, state.count)?))
                }
                other => Ok(other),
            }
        }
        Aggregate::Min | Aggregate::Max => Ok(state.extreme.clone().unwrap_or(Value::Null)),
    }
}

/// Builds the state a read path produced from a full scan.
pub(crate) fn state_from_parts(
    count: i64,
    sum: Option<Value>,
    extreme: Option<Value>,
) -> RollupState {
    RollupState {
        count,
        sum,
        extreme,
    }
}

/// Exact numeric addition used by rollup deltas.
pub(crate) fn add_values(left: Value, right: Value) -> Result<Value, StorageError> {
    match (left, right) {
        (Value::Integer(left), Value::Integer(right)) => Ok(Value::Integer(
            left.checked_add(right)
                .ok_or_else(|| StorageError::Query("aggregate sum overflowed an integer".into()))?,
        )),
        (Value::Currency(left), Value::Currency(right)) => {
            Ok(Value::Currency(left.checked_add(right)?))
        }
        (left, right) => {
            let left = numeric_decimal(left)?;
            let right = numeric_decimal(right)?;
            Ok(Value::Decimal(left.checked_add(right)?))
        }
    }
}

/// Exact numeric subtraction used by rollup deltas.
pub(crate) fn sub_values(left: Value, right: Value) -> Result<Value, StorageError> {
    match (left, right) {
        (Value::Integer(left), Value::Integer(right)) => Ok(Value::Integer(
            left.checked_sub(right)
                .ok_or_else(|| StorageError::Query("aggregate sum overflowed an integer".into()))?,
        )),
        (Value::Currency(left), Value::Currency(right)) => {
            Ok(Value::Currency(left.checked_sub(right)?))
        }
        (left, right) => {
            let left = numeric_decimal(left)?;
            let right = numeric_decimal(right)?;
            Ok(Value::Decimal(left.checked_sub(right)?))
        }
    }
}

/// Widens a numeric value to an exact decimal.
pub(crate) fn numeric_decimal(value: Value) -> Result<Decimal, StorageError> {
    match value {
        Value::Integer(value) => Ok(Decimal::from_integer(value)),
        Value::Decimal(value) => Ok(value),
        Value::Currency(money) => Ok(money.amount()),
        other => Err(StorageError::Query(format!(
            "cannot sum {}",
            other.type_name()
        ))),
    }
}

/// Ordering used by `MIN` and `MAX`.
pub(crate) fn value_less(left: &Value, right: &Value) -> bool {
    match (left, right) {
        (Value::Integer(left), Value::Integer(right)) => left < right,
        (Value::Decimal(left), Value::Decimal(right)) => left < right,
        (Value::Integer(left), Value::Decimal(right)) => Decimal::from_integer(*left) < *right,
        (Value::Decimal(left), Value::Integer(right)) => *left < Decimal::from_integer(*right),
        (Value::Currency(left), Value::Currency(right)) => left.amount() < right.amount(),
        (Value::Text(left), Value::Text(right))
        | (Value::Select(left), Value::Text(right))
        | (Value::Text(left), Value::Select(right))
        | (Value::Select(left), Value::Select(right)) => left < right,
        (Value::Date(left), Value::Date(right)) => left < right,
        (Value::Timestamp(left), Value::Timestamp(right)) => left < right,
        (Value::Boolean(left), Value::Boolean(right)) => left < right,
        _ => false,
    }
}

/// Zero value returned when an aggregate has no children.
pub(crate) fn empty_aggregate_value(column: &Column) -> Value {
    match &column.ty {
        ColumnType::Integer => Value::Integer(0),
        ColumnType::Decimal { scale } => Decimal::zero(*scale)
            .map(Value::Decimal)
            .unwrap_or(Value::Null),
        ColumnType::Currency { currency } => Money::from_minor(*currency, 0)
            .map(Value::Currency)
            .unwrap_or(Value::Null),
        _ => Value::Null,
    }
}

/// Accumulates one value into `(count, sum, extreme)`.
pub(crate) fn accumulate(
    count: &mut i64,
    sum: &mut Option<Value>,
    best: &mut Option<Value>,
    aggregate: Aggregate,
    value: Value,
) -> Result<(), StorageError> {
    if value.is_null() {
        return Ok(());
    }
    *count = count
        .checked_add(1)
        .ok_or_else(|| StorageError::Internal("rollup count overflowed".into()))?;
    match aggregate {
        Aggregate::Sum | Aggregate::Avg => {
            let current = sum.take();
            *sum = Some(match current {
                None => value,
                Some(accumulated) => add_values(accumulated, value)?,
            });
        }
        Aggregate::Min => {
            if best
                .as_ref()
                .is_none_or(|current| value_less(&value, current))
            {
                *best = Some(value);
            }
        }
        Aggregate::Max => {
            if best
                .as_ref()
                .is_none_or(|current| value_less(current, &value))
            {
                *best = Some(value);
            }
        }
        Aggregate::Count => {}
    }
    Ok(())
}
