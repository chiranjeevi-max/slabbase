//! Reading rows, keyset pages, search, and computed columns.

use std::time::Instant;

use rusqlite::params;
use rusqlite::types::Value as SqlValue;
use slab_core::{
    Aggregate, Column, ColumnType, Cursor, Decimal, FilterOp, IndexHint, MAX_PAGE_SIZE, Money,
    Page, PageQuery, PortalPage, PortalQuery, Record, RecordBundle, RoundingMode, RowData, RowId,
    Schema, SearchHit, SlabDate, SlabTimestamp, SortDirection, Table, Value,
};
use slab_formula::{EvalContext, FormulaDag, FormulaError, evaluate_expression, parse};

use crate::error::StorageError;
use crate::rollup;
use crate::sql::{decode_value, encode_value, normalize_value, quote_ident, stored_columns};
use crate::workspace::{Workspace, now_ms, read_stored_record};

impl Workspace {
    /// Reads one row with computed columns applied and policy redaction.
    pub fn read_record(&self, table_name: &str, id: RowId) -> Result<Record, StorageError> {
        let schema = self.schema()?;
        let table = schema
            .table(table_name)
            .ok_or_else(|| StorageError::NotFound(table_name.to_owned()))?;
        self.check_policy(table_name)?;
        let conn = self.read_conn();
        let cache_conn = self.cache_conn();
        let mut record = read_stored_record(conn, table, id)?
            .ok_or_else(|| StorageError::NotFound(format!("{table_name} #{id}")))?;
        let dag = FormulaDag::build(&schema)?;
        Self::compute_record_with_dag(conn, cache_conn, &schema, table, &mut record, &dag)?;
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
        let maximum = portal_limit.clamp(1, 25_000);
        for relation in schema.child_relations(&table.name) {
            let mut rows = Vec::new();
            let mut cursor = None;
            loop {
                let page = self.page(&PageQuery {
                    table: relation.child_table.clone(),
                    limit: MAX_PAGE_SIZE,
                    cursor,
                    sort: Vec::new(),
                    filters: vec![slab_core::Filter {
                        column: relation.child_column.clone(),
                        op: FilterOp::Eq,
                        value: Some(Value::Integer(id)),
                    }],
                    search: None,
                })?;
                rows.extend(page.rows);
                if rows.len() >= usize::try_from(maximum).unwrap_or(usize::MAX) {
                    rows.truncate(usize::try_from(maximum).unwrap_or(usize::MAX));
                    break;
                }
                match page.next_cursor {
                    Some(next) => cursor = Some(next),
                    None => break,
                }
            }
            bundle.portals.insert(relation.name.clone(), rows);
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
                "ORDER BY {} {} NULLS LAST, {} ASC",
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
        let explain_bound = bound.clone();
        let conn = self.read_conn();
        let cache_conn = self.cache_conn();
        let _active = ActiveQuery::begin(self.cancel.as_ref(), conn.get_interrupt_handle());
        let mut stmt = conn.prepare_cached(&sql)?;
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
            Self::compute_record_with_dag(conn, cache_conn, &schema, table, &mut record, &dag)?;
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
        let active_column = if sort_column != "id" {
            Some(sort_column)
        } else {
            query
                .filters
                .iter()
                .map(|filter| filter.column.as_str())
                .find(|column| *column != "id")
        };
        let index_hint = self.index_hint(table, &sql, &explain_bound, active_column, elapsed_ms)?;
        Ok(Page {
            rows: records,
            next_cursor,
            elapsed_ms,
            total_rows: None,
            index_hint,
        })
    }

    /// Returns one keyset page of child rows for a relation, plus the exact
    /// number of children for the parent.
    pub fn page_portal(&self, request: &PortalQuery) -> Result<PortalPage, StorageError> {
        let schema = self.schema()?;
        let relation = schema
            .relation(&request.relation)
            .ok_or_else(|| StorageError::NotFound(format!("relation {}", request.relation)))?;
        let child = schema.table(&relation.child_table).ok_or_else(|| {
            StorageError::NotFound(format!("child table {}", relation.child_table))
        })?;
        let page = self.page(&PageQuery {
            table: child.name.clone(),
            limit: request.limit,
            cursor: request.cursor.clone(),
            sort: Vec::new(),
            filters: vec![slab_core::Filter {
                column: relation.child_column.clone(),
                op: FilterOp::Eq,
                value: Some(Value::Integer(request.parent_id)),
            }],
            search: None,
        })?;
        let total: i64 = self.read_conn().query_row(
            &format!(
                "SELECT COUNT(*) FROM {} WHERE {} = ?1",
                quote_ident(&child.name),
                quote_ident(&relation.child_column)
            ),
            params![request.parent_id],
            |row| row.get(0),
        )?;
        Ok(PortalPage {
            rows: page.rows,
            next_cursor: page.next_cursor,
            total_rows: u64::try_from(total).unwrap_or(0),
            elapsed_ms: page.elapsed_ms,
            index_hint: page.index_hint,
        })
    }

    /// Full-text search across every indexed table.
    pub fn search(&self, term: &str, limit: u32) -> Result<Vec<SearchHit>, StorageError> {
        let term = term.trim();
        if term.is_empty() {
            return Ok(Vec::new());
        }
        let limit = usize::try_from(limit.clamp(1, 200)).unwrap_or(200);
        let conn = self.read_conn();
        let _active = ActiveQuery::begin(self.search_cancel.as_ref(), conn.get_interrupt_handle());
        // No SQL LIMIT: denied tables must not consume the authorized page.
        // Stop as soon as enough readable hits are collected.
        let sql = "SELECT table_name, CAST(row_id AS INTEGER),
                          snippet(_slab_fts, 2, '<mark>', '</mark>', '…', 12),
                          bm25(_slab_fts)
                   FROM _slab_fts
                   WHERE _slab_fts MATCH ?1
                   ORDER BY bm25(_slab_fts)";
        let mut stmt = conn.prepare_cached(sql)?;
        let mut rows = stmt.query(params![fts_query(term)])?;
        let mut hits = Vec::new();
        while let Some(row) = rows.next()? {
            let hit = SearchHit {
                table: row.get(0)?,
                row_id: row.get(1)?,
                snippet: row.get(2)?,
                rank: row.get(3)?,
            };
            if self.policy.can_read_table(&hit.table) {
                hits.push(hit);
                if hits.len() == limit {
                    break;
                }
            }
        }
        Ok(hits)
    }

    pub(crate) fn check_policy(&self, table: &str) -> Result<(), StorageError> {
        if self.policy.can_read_table(table) {
            Ok(())
        } else {
            Err(StorageError::Query(format!(
                "read policy denies table {table}"
            )))
        }
    }

    pub(crate) fn redact_record(&self, table: &Table, record: &mut Record) {
        for (name, value) in &mut record.values {
            if let Some(replacement) = self.policy.redact(&table.name, name, value) {
                *value = replacement;
            }
        }
    }

    pub(crate) fn compute_record_with_dag(
        read_conn: &rusqlite::Connection,
        cache_conn: Option<&rusqlite::Connection>,
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
                let value = Self::aggregate_in(
                    read_conn,
                    cache_conn,
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
                    read_conn,
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
        let conn = self.read_conn();
        let cache_conn = self.cache_conn();
        Self::aggregate_in(
            conn,
            cache_conn,
            &schema,
            parent_table,
            parent_id,
            relation_name,
            child_column,
            aggregate,
        )
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "the aggregate target fields are inherently positional"
    )]
    fn aggregate_in(
        read_conn: &rusqlite::Connection,
        cache_conn: Option<&rusqlite::Connection>,
        schema: &Schema,
        parent_table: &str,
        parent_id: RowId,
        relation_name: &str,
        child_column: &str,
        aggregate: Aggregate,
    ) -> Result<Value, StorageError> {
        Self::aggregate_in_mode(
            read_conn,
            cache_conn,
            schema,
            AggregateTarget {
                parent_table,
                parent_id,
                relation_name,
                child_column,
                aggregate,
            },
            true,
        )
    }

    /// Formula-level aggregates are evaluated against live child rows. They are
    /// intentionally uncached so an arbitrary formula aggregate can never read
    /// a rollup entry that no declared lookup column maintains.
    fn aggregate_scan_in(
        read_conn: &rusqlite::Connection,
        schema: &Schema,
        parent_table: &str,
        parent_id: RowId,
        relation_name: &str,
        child_column: &str,
        aggregate: Aggregate,
    ) -> Result<Value, StorageError> {
        Self::aggregate_in_mode(
            read_conn,
            None,
            schema,
            AggregateTarget {
                parent_table,
                parent_id,
                relation_name,
                child_column,
                aggregate,
            },
            false,
        )
    }

    fn aggregate_in_mode(
        read_conn: &rusqlite::Connection,
        cache_conn: Option<&rusqlite::Connection>,
        schema: &Schema,
        target: AggregateTarget<'_>,
        use_cache: bool,
    ) -> Result<Value, StorageError> {
        let AggregateTarget {
            parent_table,
            parent_id,
            relation_name,
            child_column,
            aggregate,
        } = target;
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
        if use_cache
            && let Some(state) = rollup::cached_state(
                read_conn,
                parent_table,
                parent_id,
                relation_name,
                child_column,
                aggregate,
            )?
        {
            return rollup::finalize_state(&state, aggregate, column);
        }
        let state = Self::compute_aggregate_state(
            read_conn, cache_conn, schema, relation, child, column, parent_id, aggregate,
        )?;
        let value = rollup::finalize_state(&state, aggregate, column)?;
        if use_cache
            && column.ty.is_stored()
            && let Some(cache_conn) = cache_conn
        {
            rollup::store_computed(
                cache_conn,
                parent_table,
                parent_id,
                relation_name,
                child_column,
                aggregate,
                &state,
            )?;
        }
        Ok(value)
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "the aggregate target fields are inherently positional"
    )]
    fn compute_aggregate_state(
        read_conn: &rusqlite::Connection,
        cache_conn: Option<&rusqlite::Connection>,
        schema: &Schema,
        relation: &slab_core::Relation,
        child: &Table,
        column: &Column,
        parent_id: RowId,
        aggregate: Aggregate,
    ) -> Result<rollup::RollupState, StorageError> {
        let where_clause = format!("{} = ?1", quote_ident(&relation.child_column));
        let conn = read_conn;
        if aggregate == Aggregate::Count && column.ty.is_stored() {
            let sql = format!(
                "SELECT COUNT({}) FROM {} WHERE {} AND {} IS NOT NULL",
                quote_ident(&column.name),
                quote_ident(&child.name),
                where_clause,
                quote_ident(&column.name)
            );
            let count: i64 = conn.query_row(&sql, params![parent_id], |row| row.get(0))?;
            return Ok(rollup::state_from_parts(count, None, None));
        }
        if column.ty.is_stored() && matches!(aggregate, Aggregate::Min | Aggregate::Max) {
            let direction = if aggregate == Aggregate::Min {
                "ASC"
            } else {
                "DESC"
            };
            let count_sql = format!(
                "SELECT COUNT({}) FROM {} WHERE {} AND {} IS NOT NULL",
                quote_ident(&column.name),
                quote_ident(&child.name),
                where_clause,
                quote_ident(&column.name)
            );
            let count: i64 = conn.query_row(&count_sql, params![parent_id], |row| row.get(0))?;
            let sql = format!(
                "SELECT {} FROM {} WHERE {} AND {} IS NOT NULL ORDER BY {} {direction} LIMIT 1",
                quote_ident(&column.name),
                quote_ident(&child.name),
                where_clause,
                quote_ident(&column.name),
                quote_ident(&column.name)
            );
            let mut stmt = conn.prepare_cached(&sql)?;
            let mut rows = stmt.query(params![parent_id])?;
            return match rows.next()? {
                Some(row) => {
                    let value = decode_value(row, 0, &column.ty)?;
                    Ok(rollup::state_from_parts(count, None, Some(value)))
                }
                None => Ok(rollup::state_from_parts(0, None, None)),
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
            let mut stmt = conn.prepare_cached(&sql)?;
            let mut rows = stmt.query(params![parent_id])?;
            while let Some(row) = rows.next()? {
                let value = decode_value(row, 0, &column.ty)?;
                rollup::accumulate(&mut count, &mut sum, &mut best, aggregate, value)?;
            }
        } else {
            let sql = format!(
                "SELECT {} FROM {} WHERE {} ORDER BY {}",
                quote_ident("id"),
                quote_ident(&child.name),
                where_clause,
                quote_ident("id")
            );
            let mut stmt = conn.prepare_cached(&sql)?;
            let ids = stmt.query_map(params![parent_id], |row| row.get::<_, RowId>(0))?;
            let mut child_ids = Vec::new();
            for id in ids {
                child_ids.push(id?);
            }
            drop(stmt);
            let child_dag = FormulaDag::build(schema)?;
            for id in child_ids {
                let Some(mut record) = read_stored_record(conn, child, id)? else {
                    continue;
                };
                Self::compute_record_with_dag(
                    read_conn,
                    cache_conn,
                    schema,
                    child,
                    &mut record,
                    &child_dag,
                )?;
                let value = record
                    .values
                    .get(&column.name)
                    .cloned()
                    .unwrap_or(Value::Null);
                rollup::accumulate(&mut count, &mut sum, &mut best, aggregate, value)?;
            }
        }
        Ok(rollup::state_from_parts(count, sum, best))
    }

    fn index_hint(
        &self,
        table: &Table,
        sql: &str,
        bound: &[SqlValue],
        active_column: Option<&str>,
        elapsed_ms: f64,
    ) -> Result<Option<IndexHint>, StorageError> {
        let Some(active_column) = active_column else {
            return Ok(None);
        };
        if active_column == "id" {
            return Ok(None);
        }
        let conn = self.read_conn();
        if has_index_for_column(conn, &table.name, active_column)? {
            return Ok(None);
        }
        let scanned_rows: i64 = conn.query_row(
            &format!("SELECT COUNT(*) FROM {}", quote_ident(&table.name)),
            [],
            |row| row.get(0),
        )?;
        if elapsed_ms <= 20.0 && scanned_rows < 1_000 {
            return Ok(None);
        }
        let explain = format!("EXPLAIN QUERY PLAN {sql}");
        let mut stmt = conn.prepare_cached(&explain)?;
        let mut rows = stmt.query(rusqlite::params_from_iter(bound.iter().cloned()))?;
        let mut details = Vec::new();
        let mut scan = false;
        while let Some(row) = rows.next()? {
            let detail: String = row.get(3)?;
            if detail.starts_with("SCAN ") {
                scan = true;
            }
            details.push(detail);
        }
        if !scan {
            return Ok(None);
        }
        Ok(Some(IndexHint {
            table: table.name.clone(),
            column: active_column.to_owned(),
            suggested_index: format!("slab_idx_{}_{active_column}", table.name).replace(' ', "_"),
            elapsed_ms,
            scanned_rows: u64::try_from(scanned_rows).unwrap_or(0),
            detail: details.join("; "),
        }))
    }
}

fn has_index_for_column(
    conn: &rusqlite::Connection,
    table: &str,
    column: &str,
) -> Result<bool, StorageError> {
    let list_sql = format!("PRAGMA index_list({})", quote_ident(table));
    let mut stmt = conn.prepare_cached(&list_sql)?;
    let indexes = stmt.query_map([], |row| row.get::<_, String>(1))?;
    let mut names = Vec::new();
    for index in indexes {
        names.push(index?);
    }
    drop(stmt);
    for name in names {
        let info_sql = format!("PRAGMA index_info({})", quote_ident(&name));
        let mut info = conn.prepare_cached(&info_sql)?;
        let columns = info.query_map([], |row| row.get::<_, String>(2))?;
        let mut first = None;
        for item in columns {
            if first.is_none() {
                first = Some(item?);
            }
        }
        if first.as_deref() == Some(column) {
            return Ok(true);
        }
    }
    Ok(false)
}

/// One aggregate request, used to keep the cached and live paths in sync.
#[derive(Clone, Copy)]
struct AggregateTarget<'a> {
    parent_table: &'a str,
    parent_id: RowId,
    relation_name: &'a str,
    child_column: &'a str,
    aggregate: Aggregate,
}

/// Registers the active query handle and retires exactly that registration on
/// every exit path.
struct ActiveQuery<'a> {
    cancel: &'a crate::workspace::QueryCancel,
    registration: crate::workspace::CancelRegistration,
}

impl<'a> ActiveQuery<'a> {
    fn begin(cancel: &'a crate::workspace::QueryCancel, handle: rusqlite::InterruptHandle) -> Self {
        let registration = cancel.register(handle);
        Self {
            cancel,
            registration,
        }
    }
}

impl Drop for ActiveQuery<'_> {
    fn drop(&mut self) {
        self.cancel.clear_registration(self.registration);
    }
}

struct RowContext<'a> {
    read_conn: &'a rusqlite::Connection,
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
        Workspace::aggregate_scan_in(
            self.read_conn,
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
            // Match what the grid shows, not the physical representation:
            // decimals are stored as minor units and booleans as 0/1.
            let display = display_expression(column);
            match filter.op {
                FilterOp::Contains => {
                    predicates.push(format!(
                        "instr(lower({display}), lower(?{})) > 0",
                        bound.len() + 1
                    ));
                    bound.push(SqlValue::Text(text));
                }
                FilterOp::StartsWith => {
                    let position = bound.len() + 1;
                    predicates.push(format!(
                        "substr(lower({display}), 1, length(?{position})) = lower(?{position})"
                    ));
                    bound.push(SqlValue::Text(text.clone()));
                }
                _ => {
                    let position = bound.len() + 1;
                    predicates.push(format!(
                        "substr(lower({display}), -length(?{position})) = lower(?{position})"
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
            let value = filter_value(column, value)?;
            let encoded = encode_value(&value, &column.ty)?;
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
    // FTS5 string quoting handles punctuation, operators, and unbalanced
    // quotes, but a NUL byte terminates the parser's view of the string and
    // produces a raw syntax error. Strip NULs before quoting so every user
    // input is a valid phrase query.
    let cleaned: String = term.chars().filter(|ch| *ch != '\0').collect();
    if cleaned.trim().is_empty() {
        return "\"\"".to_owned();
    }
    format!("\"{}\"", cleaned.replace('"', "\"\""))
}

/// Coerces a filter value into the column's stored type.
///
/// The shell may send the text a user typed (`12.34`, `Yes`, `USD 1.20`) for
/// an `=`, `!=`, or ordering filter. Comparing that text directly to minor
/// units or 0/1 silently returned no rows; parse it and reject malformed input
/// with a typed query error instead. Typed values are normalized without
/// rounding, so a filter with more precision than the column is rejected
/// rather than answering a different question.
fn filter_value(column: &Column, value: &Value) -> Result<Value, StorageError> {
    let Value::Text(text) = value else {
        let normalized = normalize_value(value, &column.ty).map_err(|error| {
            StorageError::Query(format!(
                "filter value does not fit column {}: {error}",
                column.name
            ))
        })?;
        let exact = match (value, &normalized) {
            (Value::Decimal(left), Value::Decimal(right)) => left == right,
            (Value::Integer(left), Value::Decimal(right)) => Decimal::from_integer(*left) == *right,
            (Value::Currency(left), Value::Currency(right)) => left == right,
            _ => normalized == *value,
        };
        if !exact {
            return Err(StorageError::Query(format!(
                "filter value has more precision than column {}",
                column.name
            )));
        }
        return Ok(normalized);
    };
    let trimmed = text.trim();
    let parsed: Result<Value, ()> = match &column.ty {
        ColumnType::Text | ColumnType::Select { .. } | ColumnType::AttachmentRef => {
            return Ok(value.clone());
        }
        ColumnType::Integer => trimmed.parse::<i64>().map(Value::Integer).map_err(|_| ()),
        ColumnType::Decimal { scale } => Decimal::parse(trimmed)
            .and_then(|decimal| {
                let normalized = decimal.rescale(*scale, RoundingMode::HalfEven)?;
                if normalized != decimal {
                    return Err(slab_core::CoreError::InvalidDecimal {
                        input: trimmed.to_owned(),
                    });
                }
                Ok(Value::Decimal(normalized))
            })
            .map_err(|_| ()),
        ColumnType::Currency { currency } => Money::parse_exact(trimmed, *currency)
            .map(Value::Currency)
            .map_err(|_| ()),
        ColumnType::Boolean => match trimmed.to_ascii_lowercase().as_str() {
            "true" | "yes" | "1" => Ok(Value::Boolean(true)),
            "false" | "no" | "0" => Ok(Value::Boolean(false)),
            _ => Err(()),
        },
        ColumnType::Date => SlabDate::parse(trimmed).map(Value::Date).map_err(|_| ()),
        ColumnType::Timestamp => SlabTimestamp::parse(trimmed)
            .map(Value::Timestamp)
            .map_err(|_| ()),
        ColumnType::Formula { .. } | ColumnType::Lookup { .. } => {
            return Ok(value.clone());
        }
    };
    let parsed = parsed.map_err(|()| {
        StorageError::Query(format!(
            "filter value {trimmed:?} does not fit column {}",
            column.name
        ))
    })?;
    normalize_value(&parsed, &column.ty).map_err(|error| {
        StorageError::Query(format!(
            "filter value {trimmed:?} does not fit column {}: {error}",
            column.name
        ))
    })
}

/// The canonical text used for `contains`/`startsWith`/`endsWith`.
///
/// This matches the grid cell for text, select, decimal, currency, and boolean
/// columns; booleans are `Yes`/`No`, not `Value::display_string`'s
/// `true`/`false`. Integers match their ungrouped decimal form, while the grid
/// may add locale grouping. Timestamps use ISO-8601 here while the grid renders
/// a locale short date, so a locale-formatted value is not matched.
fn display_expression(column: &Column) -> String {
    let identifier = quote_ident(&column.name);
    match &column.ty {
        ColumnType::Boolean => format!(
            "CASE WHEN {identifier} IS NULL THEN NULL \
             WHEN {identifier} = 0 THEN 'No' ELSE 'Yes' END"
        ),
        ColumnType::Decimal { scale } => decimal_display(&identifier, *scale),
        ColumnType::Currency { currency } => decimal_display(&identifier, currency.default_scale()),
        ColumnType::Timestamp => format!(
            "strftime('%Y-%m-%dT%H:%M:%fZ', \
             MIN(MAX({identifier}, {}), {}) / 1000.0, 'unixepoch')",
            slab_core::date::MIN_SUPPORTED_MILLIS,
            slab_core::date::MAX_SUPPORTED_MILLIS
        ),
        _ => format!("CAST({identifier} AS TEXT)"),
    }
}

fn decimal_display(identifier: &str, scale: u8) -> String {
    if scale == 0 {
        return format!("CAST({identifier} AS TEXT)");
    }
    let factor = 10_i64.pow(u32::from(scale));
    // Integer division truncates toward zero, so derive the sign separately
    // and format the magnitude. Take `abs` of the quotient and remainder, not
    // of the whole value: SQLite's `abs(i64::MIN)` raises integer overflow.
    // `substr(..., -scale)` keeps leading zeros.
    format!(
        "((CASE WHEN {identifier} < 0 THEN '-' ELSE '' END) \
         || CAST(abs({identifier} / {factor}) AS TEXT) || '.' \
         || substr('000000000' || CAST(abs({identifier} % {factor}) AS TEXT), -{scale}))"
    )
}
