use rusqlite::types::Value as SqlValue;
use slab_core::{
    Column, ColumnType, Cursor, Decimal, FilterOp, Money, RoundingMode, SlabDate, SlabTimestamp,
    SortDirection, Table, Value,
};

use crate::error::StorageError;
use crate::sql::{encode_value, normalize_value, quote_ident};

pub(super) fn append_cursor_predicate(
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

pub(super) fn append_filter(
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
