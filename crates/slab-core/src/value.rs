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

    #[test]
    fn cell_values_keep_stable_names_and_display_strings() {
        let decimal = Decimal::parse("12.50").expect("decimal");
        let eur = crate::CurrencyCode::parse("EUR").expect("currency");
        let money = Money::from_minor(eur, -1_250).expect("money");
        let date = SlabDate::parse("2026-11-27").expect("date");
        let timestamp = SlabTimestamp::from_millis(-1);

        for (value, type_name, display) in [
            (Value::Null, "null", ""),
            (Value::from("Jones"), "text", "Jones"),
            (Value::Integer(1_375), "integer", "1375"),
            (Value::Decimal(decimal), "decimal", "12.50"),
            (Value::Currency(money), "currency", "EUR -12.50"),
            (Value::Boolean(true), "boolean", "true"),
            (Value::Boolean(false), "boolean", "false"),
            (Value::Date(date), "date", "2026-11-27"),
            (
                Value::Timestamp(timestamp),
                "timestamp",
                "1969-12-31T23:59:59.999Z",
            ),
            (Value::Select("open".into()), "select", "open"),
            (
                Value::AttachmentRef("invoice.pdf".into()),
                "attachment",
                "invoice.pdf",
            ),
        ] {
            assert_eq!(value.type_name(), type_name);
            assert_eq!(value.display_string(), display);
            assert_eq!(value.is_null(), value == Value::Null);
        }
    }

    #[test]
    fn typed_accessors_preserve_values_and_widen_only_integers() {
        let decimal = Decimal::parse("12.50").expect("decimal");
        let eur = crate::CurrencyCode::parse("EUR").expect("currency");
        let money = Money::from_minor(eur, 1_250).expect("money");
        let date = SlabDate::parse("2026-11-27").expect("date");
        let timestamp = SlabTimestamp::from_millis(1_000);
        let integer = Value::Integer(-42);

        assert_eq!(Value::from("Jones").as_text(), Some("Jones"));
        assert_eq!(Value::Select("open".into()).as_text(), Some("open"));
        assert_eq!(
            Value::AttachmentRef("invoice.pdf".into()).as_text(),
            Some("invoice.pdf")
        );
        assert_eq!(integer.as_integer(), Some(-42));
        assert_eq!(integer.as_decimal(), Some(Decimal::from_integer(-42)));
        assert_eq!(Value::Boolean(true).as_boolean(), Some(true));
        assert_eq!(Value::Boolean(false).as_boolean(), Some(false));
        assert_eq!(Value::Decimal(decimal).as_decimal(), Some(decimal));
        assert_eq!(Value::Currency(money).as_decimal(), None);
        assert_eq!(Value::Currency(money).as_money(), Some(money));
        assert_eq!(Value::Date(date).as_date(), Some(date));
        assert_eq!(Value::Timestamp(timestamp).as_timestamp(), Some(timestamp));

        assert_eq!(Value::Null.as_text(), None);
        assert_eq!(Value::Null.as_integer(), None);
        assert_eq!(Value::Null.as_decimal(), None);
        assert_eq!(Value::Null.as_boolean(), None);
        assert_eq!(Value::Null.as_money(), None);
        assert_eq!(Value::Null.as_date(), None);
        assert_eq!(Value::Null.as_timestamp(), None);
    }

    #[test]
    fn formula_truthiness_respects_zero_and_empty_values() {
        let eur = crate::CurrencyCode::parse("EUR").expect("currency");
        let zero = Decimal::parse("0.00").expect("decimal");
        let negative = Decimal::parse("-0.01").expect("decimal");

        for (value, expected) in [
            (Value::Null, false),
            (Value::Boolean(false), false),
            (Value::Boolean(true), true),
            (Value::Integer(0), false),
            (Value::Integer(-1), true),
            (Value::Decimal(zero), false),
            (Value::Decimal(negative), true),
            (
                Value::Currency(Money::from_minor(eur, 0).expect("money")),
                false,
            ),
            (
                Value::Currency(Money::from_minor(eur, -1).expect("money")),
                true,
            ),
            (Value::from(""), false),
            (Value::from("Jones"), true),
            (Value::Select(String::new()), false),
            (Value::Select("open".into()), true),
            (Value::AttachmentRef(String::new()), false),
            (Value::AttachmentRef("invoice.pdf".into()), true),
            (
                Value::Date(SlabDate::from_days_since_epoch(0).expect("date")),
                true,
            ),
            (Value::Timestamp(SlabTimestamp::from_millis(0)), true),
        ] {
            assert_eq!(value.is_truthy(), expected, "{value:?}");
        }
    }
}
