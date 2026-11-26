//! SQL generation and value encoding shared by the workspace methods.

use rusqlite::Row;
use rusqlite::types::Value as SqlValue;
use slab_core::{
    Column, ColumnType, Decimal, Money, RoundingMode, SlabDate, SlabTimestamp, Table, Value,
};

use crate::error::StorageError;

/// Quotes an SQL identifier.
pub(crate) fn quote_ident(name: &str) -> String {
    let mut quoted = String::with_capacity(name.len() + 2);
    quoted.push('"');
    for ch in name.chars() {
        if ch == '"' {
            quoted.push('"');
        }
        quoted.push(ch);
    }
    quoted.push('"');
    quoted
}

/// Quotes an SQL string literal.
pub(crate) fn quote_literal(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

/// The physical SQLite type for a column.
pub(crate) const fn storage_type(column_type: &ColumnType) -> &'static str {
    match column_type {
        ColumnType::Text
        | ColumnType::Select { .. }
        | ColumnType::AttachmentRef
        | ColumnType::Date => "TEXT",
        _ => "INTEGER",
    }
}

/// Physical columns of a table.
pub(crate) fn stored_columns(table: &Table) -> Vec<&Column> {
    table.stored_columns().collect()
}

/// Normalizes a value to the exact representation stored for a column.
pub(crate) fn normalize_value(
    value: &Value,
    column_type: &ColumnType,
) -> Result<Value, StorageError> {
    if value.is_null() {
        return Ok(Value::Null);
    }
    match column_type {
        ColumnType::Decimal { scale } => match value {
            Value::Integer(integer) => Ok(Value::Decimal(
                Decimal::from_integer(*integer).rescale(*scale, RoundingMode::HalfEven)?,
            )),
            Value::Decimal(decimal) => Ok(Value::Decimal(
                decimal.rescale(*scale, RoundingMode::HalfEven)?,
            )),
            other => Err(StorageError::Query(format!(
                "expected decimal, found {}",
                other.type_name()
            ))),
        },
        ColumnType::Currency { currency } => match value {
            Value::Currency(money) => {
                if money.currency() != *currency {
                    return Err(StorageError::Query(format!(
                        "expected {}, found {}",
                        currency,
                        money.currency()
                    )));
                }
                let amount = money
                    .amount()
                    .rescale(currency.default_scale(), RoundingMode::HalfEven)?;
                Ok(Value::Currency(Money::from_minor_scaled(
                    *currency,
                    amount.units(),
                    amount.scale(),
                )?))
            }
            other => Err(StorageError::Query(format!(
                "expected currency, found {}",
                other.type_name()
            ))),
        },
        ColumnType::Formula { .. } | ColumnType::Lookup { .. } => Err(StorageError::Command(
            "computed columns cannot be written".into(),
        )),
        _ => Ok(value.clone()),
    }
}

/// Encodes a normalized value as a bound SQLite parameter.
pub(crate) fn encode_value(
    value: &Value,
    column_type: &ColumnType,
) -> Result<SqlValue, StorageError> {
    Ok(match value {
        Value::Null => SqlValue::Null,
        Value::Text(text) | Value::Select(text) | Value::AttachmentRef(text) => {
            SqlValue::Text(text.clone())
        }
        Value::Integer(integer) => SqlValue::Integer(*integer),
        Value::Boolean(flag) => SqlValue::Integer(i64::from(*flag)),
        Value::Decimal(decimal) => {
            if !matches!(column_type, ColumnType::Decimal { .. }) {
                return Err(StorageError::Query(
                    "decimal value in a non-decimal column".into(),
                ));
            }
            SqlValue::Integer(decimal.units())
        }
        Value::Currency(money) => {
            if !matches!(column_type, ColumnType::Currency { .. }) {
                return Err(StorageError::Query(
                    "currency value in a non-currency column".into(),
                ));
            }
            SqlValue::Integer(money.minor_units())
        }
        Value::Date(date) => SqlValue::Text(date.iso()),
        Value::Timestamp(stamp) => SqlValue::Integer(stamp.millis()),
    })
}

/// Encodes a default value as an SQL literal for DDL.
pub(crate) fn sql_literal(value: &Value, column_type: &ColumnType) -> Result<String, StorageError> {
    let normalized = normalize_value(value, column_type)?;
    Ok(match normalized {
        Value::Null => "NULL".to_owned(),
        Value::Text(text) | Value::Select(text) | Value::AttachmentRef(text) => {
            quote_literal(&text)
        }
        Value::Integer(integer) => integer.to_string(),
        Value::Boolean(flag) => i64::from(flag).to_string(),
        Value::Decimal(decimal) => decimal.units().to_string(),
        Value::Currency(money) => money.minor_units().to_string(),
        Value::Date(date) => quote_literal(&date.iso()),
        Value::Timestamp(stamp) => stamp.millis().to_string(),
    })
}

/// Decodes one column from a query row.
pub(crate) fn decode_value(
    row: &Row<'_>,
    index: usize,
    column_type: &ColumnType,
) -> Result<Value, StorageError> {
    match column_type {
        ColumnType::Text => {
            let value: Option<String> = row.get(index)?;
            Ok(value.map_or(Value::Null, Value::Text))
        }
        ColumnType::Select { .. } => {
            let value: Option<String> = row.get(index)?;
            Ok(value.map_or(Value::Null, Value::Select))
        }
        ColumnType::AttachmentRef => {
            let value: Option<String> = row.get(index)?;
            Ok(value.map_or(Value::Null, Value::AttachmentRef))
        }
        ColumnType::Integer => {
            let value: Option<i64> = row.get(index)?;
            Ok(value.map_or(Value::Null, Value::Integer))
        }
        ColumnType::Boolean => {
            let value: Option<i64> = row.get(index)?;
            Ok(value.map_or(Value::Null, |flag| Value::Boolean(flag != 0)))
        }
        ColumnType::Decimal { scale } => {
            let value: Option<i64> = row.get(index)?;
            Ok(match value {
                Some(units) => Value::Decimal(Decimal::new(units, *scale)?),
                None => Value::Null,
            })
        }
        ColumnType::Currency { currency } => {
            let value: Option<i64> = row.get(index)?;
            Ok(match value {
                Some(minor) => Value::Currency(Money::from_minor(*currency, minor)?),
                None => Value::Null,
            })
        }
        ColumnType::Date => {
            let value: Option<String> = row.get(index)?;
            Ok(match value {
                Some(text) => Value::Date(SlabDate::parse(&text)?),
                None => Value::Null,
            })
        }
        ColumnType::Timestamp => {
            let value: Option<i64> = row.get(index)?;
            Ok(value.map_or(Value::Null, |millis| {
                Value::Timestamp(SlabTimestamp::from_millis(millis))
            }))
        }
        ColumnType::Formula { .. } | ColumnType::Lookup { .. } => Err(StorageError::Internal(
            "computed columns have no physical storage".into(),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use slab_core::CurrencyCode;

    #[test]
    fn quotes_identifiers_and_literals() {
        assert_eq!(quote_ident("Line Items"), "\"Line Items\"");
        assert_eq!(quote_ident("a\"b"), "\"a\"\"b\"");
        assert_eq!(quote_literal("O'Brien"), "'O''Brien'");
    }

    #[test]
    fn decimal_defaults_use_minor_units() {
        let column_type = ColumnType::Decimal { scale: 2 };
        let literal = sql_literal(
            &Value::Decimal(Decimal::parse("12.50").expect("decimal")),
            &column_type,
        )
        .expect("literal");
        assert_eq!(literal, "1250");
    }

    #[test]
    fn currency_defaults_keep_currency_scale() {
        let currency = CurrencyCode::parse("JPY").expect("currency");
        let column_type = ColumnType::Currency { currency };
        let literal = sql_literal(
            &Value::Currency(Money::from_minor(currency, 1500).expect("money")),
            &column_type,
        )
        .expect("literal");
        assert_eq!(literal, "1500");
    }
}
