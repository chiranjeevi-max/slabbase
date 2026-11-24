//! The value type stored in every user column.

use serde::{Deserialize, Serialize};

use crate::date::{SlabDate, SlabTimestamp};
use crate::decimal::{Decimal, Money};

/// A single cell value.
///
/// `Null` is distinct from every typed value. `Decimal` and `Date` variants
/// serialize as strings so their precision survives JSON; `Currency` keeps its
/// currency code and exact amount.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(
    feature = "ts",
    ts(export, export_to = "../../../apps/desktop/src/bindings/")
)]
pub enum Value {
    /// SQL NULL.
    Null,
    /// UTF-8 text.
    Text(String),
    /// 64-bit signed integer.
    Integer(i64),
    /// Exact fixed-precision decimal.
    Decimal(#[cfg_attr(feature = "ts", ts(type = "string"))] Decimal),
    /// Exact monetary amount.
    Currency(Money),
    /// Boolean stored as 0 or 1.
    Boolean(bool),
    /// Calendar date.
    Date(#[cfg_attr(feature = "ts", ts(type = "string"))] SlabDate),
    /// UTC timestamp.
    Timestamp(#[cfg_attr(feature = "ts", ts(type = "string"))] SlabTimestamp),
    /// One value from a declared select list.
    Select(String),
    /// A path or identifier referencing an external attachment.
    AttachmentRef(String),
}

impl Value {
    /// A stable type name for error messages and schema checks.
    pub const fn type_name(&self) -> &'static str {
        match self {
            Self::Null => "null",
            Self::Text(_) => "text",
            Self::Integer(_) => "integer",
            Self::Decimal(_) => "decimal",
            Self::Currency(_) => "currency",
            Self::Boolean(_) => "boolean",
            Self::Date(_) => "date",
            Self::Timestamp(_) => "timestamp",
            Self::Select(_) => "select",
            Self::AttachmentRef(_) => "attachment",
        }
    }

    /// True for `Null`.
    pub const fn is_null(&self) -> bool {
        matches!(self, Self::Null)
    }

    /// Text content for `Text`, `Select`, and `AttachmentRef`.
    pub fn as_text(&self) -> Option<&str> {
        match self {
            Self::Text(value) | Self::Select(value) | Self::AttachmentRef(value) => Some(value),
            _ => None,
        }
    }

    /// Integer content for `Integer`.
    pub const fn as_integer(&self) -> Option<i64> {
        match self {
            Self::Integer(value) => Some(*value),
            _ => None,
        }
    }

    /// Boolean content for `Boolean`.
    pub const fn as_boolean(&self) -> Option<bool> {
        match self {
            Self::Boolean(value) => Some(*value),
            _ => None,
        }
    }

    /// Decimal content, widening integers. Currency values are not converted.
    pub fn as_decimal(&self) -> Option<Decimal> {
        match self {
            Self::Decimal(value) => Some(*value),
            Self::Integer(value) => Some(Decimal::from_integer(*value)),
            _ => None,
        }
    }

    /// Money content for `Currency`.
    pub const fn as_money(&self) -> Option<Money> {
        match self {
            Self::Currency(value) => Some(*value),
            _ => None,
        }
    }

    /// Date content.
    pub const fn as_date(&self) -> Option<SlabDate> {
        match self {
            Self::Date(value) => Some(*value),
            _ => None,
        }
    }

    /// Timestamp content.
    pub const fn as_timestamp(&self) -> Option<SlabTimestamp> {
        match self {
            Self::Timestamp(value) => Some(*value),
            _ => None,
        }
    }

    /// A display string used by the print renderer, audit summaries, and CSV
    /// export. It intentionally does not add thousands separators.
    pub fn display_string(&self) -> String {
        match self {
            Self::Null => String::new(),
            Self::Text(value) | Self::Select(value) | Self::AttachmentRef(value) => value.clone(),
            Self::Integer(value) => value.to_string(),
            Self::Decimal(value) => value.to_string(),
            Self::Currency(value) => value.to_string(),
            Self::Boolean(value) => {
                if *value {
                    "true".to_owned()
                } else {
                    "false".to_owned()
                }
            }
            Self::Date(value) => value.iso(),
            Self::Timestamp(value) => value.iso(),
        }
    }

    /// Truthiness used by formula conditionals.
    pub fn is_truthy(&self) -> bool {
        match self {
            Self::Null => false,
            Self::Boolean(value) => *value,
            Self::Integer(value) => *value != 0,
            Self::Decimal(value) => !value.is_zero(),
            Self::Currency(value) => !value.amount().is_zero(),
            Self::Text(value) | Self::Select(value) | Self::AttachmentRef(value) => {
                !value.is_empty()
            }
            Self::Date(_) | Self::Timestamp(_) => true,
        }
    }
}

impl From<i64> for Value {
    fn from(value: i64) -> Self {
        Self::Integer(value)
    }
}

impl From<bool> for Value {
    fn from(value: bool) -> Self {
        Self::Boolean(value)
    }
}

impl From<String> for Value {
    fn from(value: String) -> Self {
        Self::Text(value)
    }
}

impl From<&str> for Value {
    fn from(value: &str) -> Self {
        Self::Text(value.to_owned())
    }
}

impl From<Decimal> for Value {
    fn from(value: Decimal) -> Self {
        Self::Decimal(value)
    }
}

impl From<Money> for Value {
    fn from(value: Money) -> Self {
        Self::Currency(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decimal_serializes_as_a_string() {
        let value = Value::Decimal(Decimal::parse("12.50").expect("decimal"));
        let json = serde_json::to_string(&value).expect("serialize");
        assert_eq!(json, "{\"Decimal\":\"12.50\"}");
        let decoded: Value = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(decoded, value);
    }

    #[test]
    fn currency_round_trips_with_exact_amount() {
        let money = Money::from_minor(
            crate::CurrencyCode::parse("EUR").expect("currency"),
            123_456,
        )
        .expect("money");
        let value = Value::Currency(money);
        let json = serde_json::to_string(&value).expect("serialize");
        let decoded: Value = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(decoded, value);
    }
}
