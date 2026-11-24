//! Reading rows, keyset pages, search, and computed columns.

use std::time::Instant;

use rusqlite::params;
use rusqlite::types::Value as SqlValue;
use slab_core::{
    Aggregate, ColumnType, Cursor, Decimal, FilterOp, MAX_PAGE_SIZE, Money, Page, PageQuery,
    Record, RecordBundle, RowData, RowId, Schema, SearchHit, SortDirection, Table, Value,
};
use slab_formula::{EvalContext, FormulaDag, FormulaError, evaluate_expression, parse};

use crate::error::StorageError;
use crate::sql::{decode_value, encode_value, quote_ident, stored_columns};
use crate::workspace::{Workspace, now_ms, read_stored_record};

impl Workspace {
    /// Reads one row with computed columns applied and policy redaction.
    pub fn read_record(&self, table_name: &str, id: RowId) -> Result<Record, StorageError> {
        let schema = self.schema()?;
        let table = schema
            .table(table_name)
            .ok_or_else(|| StorageError::NotFound(table_name.to_owned()))?;
        self.check_policy(table_name)?;
        let mut record = read_stored_record(&self.conn, table, id)?
            .ok_or_else(|| StorageError::NotFound(format!("{table_name} #{id}")))?;
        self.compute_record(&schema, table, &mut record)?;
        self.redact_record(table, &mut record);
        Ok(record)
    }

    /// Reads a record and its portal rows for print and form rendering.
    pub fn bundle(
        &self,
        table_name: &str,
        id: RowId,
        portal_limit: u32,
    ) -> Result<RecordBundle, StorageError> {
        let schema = self.schema()?;
        let table = schema
            .table(table_name)
            .ok_or_else(|| StorageError::NotFound(table_name.to_owned()))?;
        let record = self.read_record(table_name, id)?;
        let mut bundle = RecordBundle::new(record);
        let limit = portal_limit.clamp(1, MAX_PAGE_SIZE);
        for relation in schema.child_relations(&table.name) {
            let query = PageQuery {
                table: relation.child_table.clone(),
                limit,
                cursor: None,
                sort: Vec::new(),
                filters: vec![slab_core::Filter {
                    column: relation.child_column.clone(),
                    op: FilterOp::Eq,
                    value: Some(Value::Integer(id)),
                }],
                search: None,
            };
            let page = self.page(&query)?;
            bundle.portals.insert(relation.name.clone(), page.rows);
        }
        Ok(bundle)
    }

    /// Returns one keyset-paginated page.
    pub fn page(&self, query: &PageQuery) -> Result<Page, StorageError> {
        let started = Instant::now();
        let schema = self.schema()?;
        let table = schema
            .table(&query.table)
            .ok_or_else(|| StorageError::NotFound(query.table.clone()))?;
        self.check_policy(&table.name)?;
        let limit = if query.limit == 0 {
            50
        } else {
            query.limit.min(MAX_PAGE_SIZE)
        };
        if query.sort.len() > 1 {
            return Err(StorageError::Query(
                "v1.0 supports one sort column per page".into(),
            ));
        }
        let (sort_column, direction) = match query.sort.first() {
            Some(sort) => (sort.column.as_str(), sort.direction),
            None => ("id", SortDirection::Asc),
        };
        if sort_column != "id" {
            let column = table
                .column(sort_column)
                .ok_or_else(|| StorageError::Query(format!("unknown sort column {sort_column}")))?;
            if !column.ty.is_stored() {
                return Err(StorageError::Query(
                    "computed columns cannot be sorted in a paginated grid".into(),
                ));
            }
        }
        let dag = FormulaDag::build(&schema)?;
        let columns = stored_columns(table);
        let mut projections = vec![quote_ident("id")];
        projections.extend(columns.iter().map(|column| quote_ident(&column.name)));
        let mut predicates: Vec<String> = Vec::new();
        let mut bound: Vec<SqlValue> = Vec::new();

        if let Some(cursor) = &query.cursor {
            if cursor.column != sort_column {
                return Err(StorageError::Query(format!(
                    "cursor column {} does not match sort column {sort_column}",
                    cursor.column
                )));
            }
            append_cursor_predicate(
                table,
                sort_column,
                direction,
                cursor,
                &mut predicates,
                &mut bound,
            )?;
        }
        for filter in &query.filters {
            append_filter(table, filter, &mut predicates, &mut bound)?;
        }
        if let Some(term) = &query.search {
            let term = term.trim();
            if !term.is_empty() {
                predicates.push(format!(
                    "{} IN (SELECT CAST(row_id AS INTEGER) FROM _slab_fts WHERE table_name = ? AND _slab_fts MATCH ?)",
                    quote_ident("id")
                ));
                bound.push(SqlValue::Text(table.name.clone()));
                bound.push(SqlValue::Text(fts_query(term)));
            }
        }
        let where_clause = if predicates.is_empty() {
            String::new()
        } else {
            format!("WHERE {}", predicates.join(" AND "))
        };
        let order_clause = if sort_column == "id" {
            format!(
                "ORDER BY {} {}",
                quote_ident("id"),
                match direction {
                    SortDirection::Asc => "ASC",
                    SortDirection::Desc => "DESC",
                }
            )
        } else {
            format!(
                "ORDER BY ({} IS NULL) ASC, {} {}, {} ASC",
                quote_ident(sort_column),
                quote_ident(sort_column),
                match direction {
                    SortDirection::Asc => "ASC",
                    SortDirection::Desc => "DESC",
                },
                quote_ident("id")
            )
        };
        let limit_index = bound.len() + 1;
        let sql = format!(
            "SELECT {} FROM {} {where_clause} {order_clause} LIMIT ?{limit_index}",
            projections.join(", "),
            quote_ident(&table.name),
        );
        bound.push(SqlValue::Integer(i64::from(limit) + 1));
        let mut stmt = self.conn.prepare(&sql)?;
        let mut rows = stmt.query(rusqlite::params_from_iter(bound))?;
        let mut records: Vec<Record> = Vec::new();
        let mut sort_values: Vec<Value> = Vec::new();
        let mut extra = false;
        while let Some(row) = rows.next()? {
            if records.len() >= usize::try_from(limit).unwrap_or(usize::MAX) {
                extra = true;
                break;
            }
            let id: RowId = row.get(0)?;
            let mut values = RowData::new();
            for (index, column) in columns.iter().enumerate() {
                values.insert(
                    column.name.clone(),
                    decode_value(row, index + 1, &column.ty)?,
                );
            }
            let sort_value = if sort_column == "id" {
                Value::Integer(id)
            } else {
                values.get(sort_column).cloned().unwrap_or(Value::Null)
            };
            let mut record = Record { id, values };
            self.compute_record_with_dag(&schema, table, &mut record, &dag)?;
            self.redact_record(table, &mut record);
            records.push(record);
            sort_values.push(sort_value);
        }
        let next_cursor = if extra {
            records
                .last()
                .zip(sort_values.last())
                .map(|(record, value)| Cursor {
                    column: sort_column.to_owned(),
                    value: value.clone(),
                    row_id: record.id,
                })
        } else {
            None
        };
        let elapsed_ms = started.elapsed().as_secs_f64() * 1_000.0;
        Ok(Page {
            rows: records,
            next_cursor,
            elapsed_ms,
            total_rows: None,
        })
    }

    /// Full-text search across every indexed table.
    pub fn search(&self, term: &str, limit: u32) -> Result<Vec<SearchHit>, StorageError> {
        let term = term.trim();
        if term.is_empty() {
            return Ok(Vec::new());
        }
        let limit = i64::from(limit.clamp(1, 200));
        let sql = "SELECT table_name, CAST(row_id AS INTEGER),
                          snippet(_slab_fts, 2, '<mark>', '</mark>', '…', 12),
                          bm25(_slab_fts)
                   FROM _slab_fts
                   WHERE _slab_fts MATCH ?1
                   ORDER BY bm25(_slab_fts)
                   LIMIT ?2";
        let mut stmt = self.conn.prepare(sql)?;
        let rows = stmt.query_map(params![fts_query(term), limit], |row| {
            Ok(SearchHit {
                table: row.get(0)?,
                row_id: row.get(1)?,
                snippet: row.get(2)?,
                rank: row.get(3)?,
            })
        })?;
        let mut hits = Vec::new();
        for row in rows {
            hits.push(row?);
        }
        Ok(hits)
    }

    fn check_policy(&self, table: &str) -> Result<(), StorageError> {
        if self.policy.can_read_table(table) {
            Ok(())
        } else {
            Err(StorageError::Query(format!(
                "read policy denies table {table}"
            )))
        }
    }

    fn redact_record(&self, table: &Table, record: &mut Record) {
        for (name, value) in &mut record.values {
            if let Some(replacement) = self.policy.redact(&table.name, name, value) {
                *value = replacement;
            }
        }
    }

    fn compute_record(
        &self,
        schema: &Schema,
        table: &Table,
        record: &mut Record,
    ) -> Result<(), StorageError> {
        let dag = FormulaDag::build(schema)?;
        self.compute_record_with_dag(schema, table, record, &dag)
    }

    fn compute_record_with_dag(
        &self,
        schema: &Schema,
        table: &Table,
        record: &mut Record,
        dag: &FormulaDag,
    ) -> Result<(), StorageError> {
        // Lookups are computed before formulas so a formula can reference them.
        for column in &table.columns {
            if let ColumnType::Lookup {
                relation,
                column: child_column,
                aggregate,
            } = &column.ty
            {
                let value = self.aggregate_in(
                    schema,
                    &table.name,
                    record.id,
                    relation,
                    child_column,
                    *aggregate,
                )?;
                record.values.insert(column.name.clone(), value);
            }
        }
        let order = dag.evaluation_order(&table.name)?;
        for column_name in order {
            let Some(column) = table.column(&column_name) else {
                continue;
            };
            let ColumnType::Formula { expression } = &column.ty else {
                continue;
            };
            let parsed = parse(expression)?;
            let value = {
                let context = RowContext {
                    workspace: self,
                    schema,
                    table: &table.name,
                    record_id: record.id,
                    values: &record.values,
                };
                evaluate_expression(&parsed, &context)?
            };
            record.values.insert(column_name, value);
        }
        Ok(())
    }

    /// Computes one aggregate over the children of a parent row.
    pub fn aggregate_for_parent(
        &self,
        parent_table: &str,
        parent_id: RowId,
        relation_name: &str,
        child_column: &str,
        aggregate: Aggregate,
    ) -> Result<Value, StorageError> {
        let schema = self.schema()?;
        self.aggregate_in(
            &schema,
            parent_table,
            parent_id,
            relation_name,
            child_column,
            aggregate,
        )
    }

    fn aggregate_in(
        &self,
        schema: &Schema,
        parent_table: &str,
        parent_id: RowId,
        relation_name: &str,
        child_column: &str,
        aggregate: Aggregate,
    ) -> Result<Value, StorageError> {
        let relation = schema
            .relation(relation_name)
            .ok_or_else(|| StorageError::NotFound(format!("relation {relation_name}")))?;
        if relation.parent_table != parent_table {
            return Err(StorageError::Query(format!(
                "relation {relation_name} does not belong to {parent_table}"
            )));
        }
        let child = schema.table(&relation.child_table).ok_or_else(|| {
            StorageError::NotFound(format!("child table {}", relation.child_table))
        })?;
        let column = child.column(child_column).ok_or_else(|| {
            StorageError::Query(format!(
                "unknown child column {}.{child_column}",
                child.name
            ))
        })?;
        let where_clause = format!("{} = ?1", quote_ident(&relation.child_column));
        if aggregate == Aggregate::Count {
            let sql = format!(
                "SELECT COUNT({}) FROM {} WHERE {} AND {} IS NOT NULL",
                quote_ident(&column.name),
                quote_ident(&child.name),
                where_clause,
                quote_ident(&column.name)
            );
            let count: i64 = self
                .conn
                .query_row(&sql, params![parent_id], |row| row.get(0))?;
            return Ok(Value::Integer(count));
        }
        if column.ty.is_stored() && matches!(aggregate, Aggregate::Min | Aggregate::Max) {
            let direction = if aggregate == Aggregate::Min {
                "ASC"
            } else {
                "DESC"
            };
            let sql = format!(
                "SELECT {} FROM {} WHERE {} AND {} IS NOT NULL ORDER BY {} {direction} LIMIT 1",
                quote_ident(&column.name),
                quote_ident(&child.name),
                where_clause,
                quote_ident(&column.name),
                quote_ident(&column.name)
            );
            let mut stmt = self.conn.prepare(&sql)?;
            let mut rows = stmt.query(params![parent_id])?;
            return match rows.next()? {
                Some(row) => Ok(decode_value(row, 0, &column.ty)?),
                None => Ok(Value::Null),
            };
        }
        let mut count: i64 = 0;
        let mut sum: Option<Value> = None;
        let mut best: Option<Value> = None;
        if column.ty.is_stored() {
            let sql = format!(
                "SELECT {} FROM {} WHERE {}",
                quote_ident(&column.name),
                quote_ident(&child.name),
                where_clause
            );
            let mut stmt = self.conn.prepare(&sql)?;
            let mut rows = stmt.query(params![parent_id])?;
            while let Some(row) = rows.next()? {
                let value = decode_value(row, 0, &column.ty)?;
                accumulate(&mut count, &mut sum, &mut best, aggregate, value)?;
            }
        } else {
            let sql = format!(
                "SELECT {} FROM {} WHERE {} ORDER BY {}",
                quote_ident("id"),
                quote_ident(&child.name),
                where_clause,
                quote_ident("id")
            );
            let mut stmt = self.conn.prepare(&sql)?;
            let ids = stmt.query_map(params![parent_id], |row| row.get::<_, RowId>(0))?;
            let mut child_ids = Vec::new();
            for id in ids {
                child_ids.push(id?);
            }
            drop(stmt);
            let child_dag = FormulaDag::build(schema)?;
            for id in child_ids {
                let Some(mut record) = read_stored_record(&self.conn, child, id)? else {
                    continue;
                };
                self.compute_record_with_dag(schema, child, &mut record, &child_dag)?;
                let value = record
                    .values
                    .get(&column.name)
                    .cloned()
                    .unwrap_or(Value::Null);
                accumulate(&mut count, &mut sum, &mut best, aggregate, value)?;
            }
        }
        match aggregate {
            Aggregate::Sum => {
                if let Some(sum) = sum {
                    return Ok(sum);
                }
                Ok(empty_aggregate_value(column))
            }
            Aggregate::Avg => {
                let Some(sum) = sum else {
                    return Ok(empty_aggregate_value(column));
                };
                if count == 0 {
                    return Ok(empty_aggregate_value(column));
                }
                match sum {
                    Value::Integer(integer) => Ok(Value::Decimal(
                        Decimal::from_integer(integer).checked_div(Decimal::from_integer(count))?,
                    )),
                    Value::Decimal(decimal) => Ok(Value::Decimal(
                        decimal.checked_div(Decimal::from_integer(count))?,
                    )),
                    Value::Currency(money) => {
                        Ok(Value::Currency(money.checked_mul_ratio(1, count)?))
                    }
                    other => Ok(other),
                }
            }
            Aggregate::Min | Aggregate::Max => Ok(best.unwrap_or(Value::Null)),
            Aggregate::Count => Err(StorageError::Internal(
                "count handled before accumulation".into(),
            )),
        }
    }
}

struct RowContext<'a> {
    workspace: &'a Workspace,
    schema: &'a Schema,
    table: &'a str,
    record_id: RowId,
    values: &'a RowData,
}

impl EvalContext for RowContext<'_> {
    fn column(&self, name: &str) -> Option<Value> {
        self.values.get(name).cloned()
    }

    fn aggregate(
        &self,
        relation: &str,
        column: &str,
        aggregate: Aggregate,
    ) -> Result<Value, FormulaError> {
        self.workspace
            .aggregate_in(
                self.schema,
                self.table,
                self.record_id,
                relation,
                column,
                aggregate,
            )
            .map_err(|error| FormulaError::Eval(error.to_string()))
    }

    fn today(&self) -> slab_core::SlabDate {
        slab_core::SlabTimestamp::from_millis(now_ms()).date()
    }

    fn now(&self) -> slab_core::SlabTimestamp {
        slab_core::SlabTimestamp::from_millis(now_ms())
    }
}

fn accumulate(
    count: &mut i64,
    sum: &mut Option<Value>,
    best: &mut Option<Value>,
    aggregate: Aggregate,
    value: Value,
) -> Result<(), StorageError> {
    if value.is_null() {
        return Ok(());
    }
    *count += 1;
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

fn value_less(left: &Value, right: &Value) -> bool {
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

fn empty_aggregate_value(column: &slab_core::Column) -> Value {
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

fn add_values(left: Value, right: Value) -> Result<Value, StorageError> {
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

fn numeric_decimal(value: Value) -> Result<Decimal, StorageError> {
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

fn append_cursor_predicate(
    table: &Table,
    sort_column: &str,
    direction: SortDirection,
    cursor: &Cursor,
    predicates: &mut Vec<String>,
    bound: &mut Vec<SqlValue>,
) -> Result<(), StorageError> {
    let column = quote_ident(sort_column);
    let id = quote_ident("id");
    if sort_column == "id" {
        let Value::Integer(value) = cursor.value else {
            return Err(StorageError::Query(
                "id cursors must carry an integer".into(),
            ));
        };
        predicates.push(format!(
            "{id} {} ?{}",
            if direction == SortDirection::Asc {
                ">"
            } else {
                "<"
            },
            bound.len() + 1
        ));
        bound.push(SqlValue::Integer(value));
        return Ok(());
    }
    if cursor.value.is_null() {
        predicates.push(format!(
            "({column} IS NULL AND {id} > ?{})",
            bound.len() + 1
        ));
        bound.push(SqlValue::Integer(cursor.row_id));
        return Ok(());
    }
    let column_type = table
        .column(sort_column)
        .ok_or_else(|| StorageError::Query(format!("unknown sort column {sort_column}")))?;
    let encoded = encode_value(&cursor.value, &column_type.ty)?;
    let first = bound.len() + 1;
    let comparison = if direction == SortDirection::Asc {
        ">"
    } else {
        "<"
    };
    predicates.push(format!(
        "({column} IS NULL OR {column} {comparison} ?{first} OR ({column} = ?{first} AND {id} > ?{}))",
        first + 1
    ));
    bound.push(encoded);
    bound.push(SqlValue::Integer(cursor.row_id));
    Ok(())
}

fn append_filter(
    table: &Table,
    filter: &slab_core::Filter,
    predicates: &mut Vec<String>,
    bound: &mut Vec<SqlValue>,
) -> Result<(), StorageError> {
    let column = table
        .column(&filter.column)
        .ok_or_else(|| StorageError::Query(format!("unknown filter column {}", filter.column)))?;
    if !column.ty.is_stored() {
        return Err(StorageError::Query(
            "computed columns cannot be filtered in a paginated grid".into(),
        ));
    }
    let identifier = quote_ident(&filter.column);
    match filter.op {
        FilterOp::IsNull => {
            predicates.push(format!("{identifier} IS NULL"));
            Ok(())
        }
        FilterOp::IsNotNull => {
            predicates.push(format!("{identifier} IS NOT NULL"));
            Ok(())
        }
        FilterOp::Contains | FilterOp::StartsWith | FilterOp::EndsWith => {
            let Some(value) = &filter.value else {
                return Err(StorageError::Query("text filters require a value".into()));
            };
            let text = value.display_string();
            match filter.op {
                FilterOp::Contains => {
                    predicates.push(format!(
                        "instr(lower(CAST({identifier} AS TEXT)), lower(?{})) > 0",
                        bound.len() + 1
                    ));
                    bound.push(SqlValue::Text(text));
                }
                FilterOp::StartsWith => {
                    let position = bound.len() + 1;
                    predicates.push(format!(
                        "substr(lower(CAST({identifier} AS TEXT)), 1, length(?{position})) = lower(?{position})"
                    ));
                    bound.push(SqlValue::Text(text.clone()));
                }
                _ => {
                    let position = bound.len() + 1;
                    predicates.push(format!(
                        "substr(lower(CAST({identifier} AS TEXT)), -length(?{position})) = lower(?{position})"
                    ));
                    bound.push(SqlValue::Text(text));
                }
            }
            Ok(())
        }
        _ => {
            let Some(value) = &filter.value else {
                return Err(StorageError::Query("filter requires a value".into()));
            };
            if value.is_null() {
                return match filter.op {
                    FilterOp::Eq => {
                        predicates.push(format!("{identifier} IS NULL"));
                        Ok(())
                    }
                    FilterOp::NotEq => {
                        predicates.push(format!("{identifier} IS NOT NULL"));
                        Ok(())
                    }
                    _ => Err(StorageError::Query(
                        "null is only valid with = or !=".into(),
                    )),
                };
            }
            let encoded = encode_value(value, &column.ty)?;
            let placeholder = format!("?{}", bound.len() + 1);
            let comparison = match filter.op {
                FilterOp::Eq => "=",
                FilterOp::NotEq => "IS NOT",
                FilterOp::Gt => ">",
                FilterOp::Gte => ">=",
                FilterOp::Lt => "<",
                FilterOp::Lte => "<=",
                _ => {
                    return Err(StorageError::Query("unsupported filter operator".into()));
                }
            };
            predicates.push(format!("{identifier} {comparison} {placeholder}"));
            bound.push(encoded);
            Ok(())
        }
    }
}

fn fts_query(term: &str) -> String {
    format!("\"{}\"", term.replace('"', "\"\""))
}
