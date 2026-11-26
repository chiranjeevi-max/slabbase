//! Exact decimal and currency arithmetic backed by integer minor units.
//!
//! `Decimal` stores an `i64` coefficient and an explicit scale in `0..=9`.
//! `Money` pairs a `Decimal` amount with a three-letter currency code. No
//! operation converts through `f64`.

use std::fmt;

use serde::de::{self, Visitor};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::error::CoreError;

/// Maximum number of fractional decimal digits supported by `Decimal`.
pub const MAX_SCALE: u8 = 9;

const POW10: [i128; 10] = [
    1,
    10,
    100,
    1_000,
    10_000,
    100_000,
    1_000_000,
    10_000_000,
    100_000_000,
    1_000_000_000,
];

/// Raises ten to `exponent`, rejecting values that do not fit an `i128`.
fn pow10(exponent: u32) -> Result<i128, CoreError> {
    if exponent > 38 {
        return Err(CoreError::NumericOverflow {
            operation: "scaling by a power of ten",
        });
    }
    Ok(10_i128.pow(exponent))
}

/// How a rounded division resolves the discarded remainder.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(
    feature = "ts",
    ts(export, export_to = "../../../apps/desktop/src/bindings/")
)]
pub enum RoundingMode {
    /// Round to the nearest value, ties to the even neighbour. The default for money.
    HalfEven,
    /// Round to the nearest value, ties away from zero.
    HalfAwayFromZero,
    /// Discard the fractional part.
    TowardZero,
    /// Move away from zero when any remainder exists.
    AwayFromZero,
    /// Move toward positive infinity.
    Up,
    /// Move toward negative infinity.
    Down,
}

/// An exact fixed-precision decimal.
#[derive(Clone, Copy, Debug)]
pub struct Decimal {
    units: i64,
    scale: u8,
}

impl Decimal {
    /// Builds a decimal from a coefficient and scale.
    pub fn new(units: i64, scale: u8) -> Result<Self, CoreError> {
        if scale > MAX_SCALE {
            return Err(CoreError::ScaleOutOfRange {
                scale,
                max: MAX_SCALE,
            });
        }
        Ok(Self { units, scale })
    }

    /// Zero with the given scale.
    pub fn zero(scale: u8) -> Result<Self, CoreError> {
        Self::new(0, scale)
    }

    /// An integer with scale zero.
    pub const fn from_integer(value: i64) -> Self {
        Self {
            units: value,
            scale: 0,
        }
    }

    /// Parses an optionally signed decimal literal. Exponents are rejected.
    pub fn parse(input: &str) -> Result<Self, CoreError> {
        let invalid = || CoreError::InvalidDecimal {
            input: input.to_owned(),
        };
        let trimmed = input.trim();
        if trimmed.is_empty() {
            return Err(invalid());
        }
        let (negative, body) = match trimmed.as_bytes().first() {
            Some(b'-') => (true, &trimmed[1..]),
            Some(b'+') => (false, &trimmed[1..]),
            _ => (false, trimmed),
        };
        if body.is_empty() {
            return Err(invalid());
        }
        if body.bytes().any(|byte| byte == b'e' || byte == b'E') {
            return Err(invalid());
        }
        let mut pieces = body.split('.');
        let integer = pieces.next().ok_or_else(invalid)?;
        let fraction = pieces.next().unwrap_or("");
        if pieces.next().is_some() {
            return Err(invalid());
        }
        if integer.is_empty() && fraction.is_empty() {
            return Err(invalid());
        }
        if !integer.bytes().all(|byte| byte.is_ascii_digit())
            || !fraction.bytes().all(|byte| byte.is_ascii_digit())
        {
            return Err(invalid());
        }
        let scale = u8::try_from(fraction.len()).map_err(|_| CoreError::ScaleOutOfRange {
            scale: MAX_SCALE.saturating_add(1),
            max: MAX_SCALE,
        })?;
        if scale > MAX_SCALE {
            return Err(CoreError::ScaleOutOfRange {
                scale,
                max: MAX_SCALE,
            });
        }
        let integer_units: i128 = if integer.is_empty() {
            0
        } else {
            integer.parse::<i128>().map_err(|_| invalid())?
        };
        let fraction_units: i128 = if fraction.is_empty() {
            0
        } else {
            fraction.parse::<i128>().map_err(|_| invalid())?
        };
        let factor = POW10[usize::from(scale)];
        let magnitude = integer_units
            .checked_mul(factor)
            .and_then(|value| value.checked_add(fraction_units))
            .ok_or(CoreError::NumericOverflow {
                operation: "parsing a decimal",
            })?;
        let signed = if negative { -magnitude } else { magnitude };
        let units = i64::try_from(signed).map_err(|_| CoreError::NumericOverflow {
            operation: "parsing a decimal",
        })?;
        Ok(Self { units, scale })
    }

    /// The integer coefficient.
    pub const fn units(self) -> i64 {
        self.units
    }

    /// The number of fractional digits.
    pub const fn scale(self) -> u8 {
        self.scale
    }

    /// True when the coefficient is zero.
    pub const fn is_zero(self) -> bool {
        self.units == 0
    }

    /// True when the coefficient is negative.
    pub const fn is_negative(self) -> bool {
        self.units < 0
    }

    /// Converts to the given scale, rounding with `mode` when digits are lost.
    pub fn rescale(self, target: u8, mode: RoundingMode) -> Result<Self, CoreError> {
        if target > MAX_SCALE {
            return Err(CoreError::ScaleOutOfRange {
                scale: target,
                max: MAX_SCALE,
            });
        }
        if target == self.scale {
            return Ok(self);
        }
        if target > self.scale {
            let factor = POW10[usize::from(target - self.scale)];
            let scaled =
                i128::from(self.units)
                    .checked_mul(factor)
                    .ok_or(CoreError::NumericOverflow {
                        operation: "increasing decimal scale",
                    })?;
            let units = i64::try_from(scaled).map_err(|_| CoreError::NumericOverflow {
                operation: "increasing decimal scale",
            })?;
            return Ok(Self {
                units,
                scale: target,
            });
        }
        let divisor = POW10[usize::from(self.scale - target)];
        let rounded = div_round(i128::from(self.units), divisor, mode)?;
        let units = i64::try_from(rounded).map_err(|_| CoreError::NumericOverflow {
            operation: "reducing decimal scale",
        })?;
        Ok(Self {
            units,
            scale: target,
        })
    }

    /// Rounds to the given scale.
    pub fn round(self, target: u8, mode: RoundingMode) -> Result<Self, CoreError> {
        self.rescale(target, mode)
    }

    /// Removes trailing fractional zeros without changing the value.
    pub fn normalized(self) -> Self {
        let mut units = i128::from(self.units);
        let mut scale = self.scale;
        while scale > 0 && units % 10 == 0 {
            units /= 10;
            scale -= 1;
        }
        let units = i64::try_from(units).unwrap_or(self.units);
        Self { units, scale }
    }

    /// Checked addition. The result uses the larger input scale.
    pub fn checked_add(self, other: Self) -> Result<Self, CoreError> {
        let scale = self.scale.max(other.scale);
        let left = i128::from(self.rescale(scale, RoundingMode::HalfEven)?.units);
        let right = i128::from(other.rescale(scale, RoundingMode::HalfEven)?.units);
        let sum = left.checked_add(right).ok_or(CoreError::NumericOverflow {
            operation: "adding decimals",
        })?;
        Ok(Self {
            units: i64::try_from(sum).map_err(|_| CoreError::NumericOverflow {
                operation: "adding decimals",
            })?,
            scale,
        })
    }

    /// Checked subtraction. The result uses the larger input scale.
    pub fn checked_sub(self, other: Self) -> Result<Self, CoreError> {
        self.checked_add(Self {
            units: other
                .units
                .checked_neg()
                .ok_or(CoreError::NumericOverflow {
                    operation: "negating a decimal",
                })?,
            scale: other.scale,
        })
    }

    /// Checked multiplication. The result scale is the sum of input scales,
    /// rounded to `MAX_SCALE` when necessary.
    pub fn checked_mul(self, other: Self) -> Result<Self, CoreError> {
        let product = i128::from(self.units)
            .checked_mul(i128::from(other.units))
            .ok_or(CoreError::NumericOverflow {
                operation: "multiplying decimals",
            })?;
        let raw_scale = u16::from(self.scale) + u16::from(other.scale);
        if raw_scale <= u16::from(MAX_SCALE) {
            let scale = u8::try_from(raw_scale).map_err(|_| CoreError::NumericOverflow {
                operation: "multiplying decimals",
            })?;
            return Ok(Self {
                units: i64::try_from(product).map_err(|_| CoreError::NumericOverflow {
                    operation: "multiplying decimals",
                })?,
                scale,
            });
        }
        let excess = raw_scale - u16::from(MAX_SCALE);
        let divisor =
            POW10[usize::from(
                u8::try_from(excess).map_err(|_| CoreError::NumericOverflow {
                    operation: "multiplying decimals",
                })?,
            )];
        let rounded = div_round(product, divisor, RoundingMode::HalfEven)?;
        Ok(Self {
            units: i64::try_from(rounded).map_err(|_| CoreError::NumericOverflow {
                operation: "multiplying decimals",
            })?,
            scale: MAX_SCALE,
        })
    }

    /// Checked division. The result keeps at least four fractional digits and
    /// never fewer than the left operand, capped at `MAX_SCALE`.
    pub fn checked_div(self, other: Self) -> Result<Self, CoreError> {
        if other.is_zero() {
            return Err(CoreError::DivisionByZero);
        }
        let result_scale = self.scale.saturating_add(4).max(other.scale).min(MAX_SCALE);
        let exponent = i32::from(other.scale) + i32::from(result_scale) - i32::from(self.scale);
        let factor = pow10(exponent.unsigned_abs())?;
        let (numerator, denominator) = if exponent >= 0 {
            (
                i128::from(self.units)
                    .checked_mul(factor)
                    .ok_or(CoreError::NumericOverflow {
                        operation: "dividing decimals",
                    })?,
                i128::from(other.units),
            )
        } else {
            (
                i128::from(self.units),
                i128::from(other.units)
                    .checked_mul(factor)
                    .ok_or(CoreError::NumericOverflow {
                        operation: "dividing decimals",
                    })?,
            )
        };
        let rounded = div_round(numerator, denominator, RoundingMode::HalfEven)?;
        Ok(Self {
            units: i64::try_from(rounded).map_err(|_| CoreError::NumericOverflow {
                operation: "dividing decimals",
            })?,
            scale: result_scale,
        })
    }

    /// Checked remainder, computed at the larger input scale.
    pub fn checked_rem(self, other: Self) -> Result<Self, CoreError> {
        if other.is_zero() {
            return Err(CoreError::DivisionByZero);
        }
        let scale = self.scale.max(other.scale);
        // Widen before aligning. Rescaling into an `i64` first rejected
        // representable remainders (and panicked on `i64::MIN % -1`).
        let left = i128::from(self.units)
            .checked_mul(pow10(u32::from(scale - self.scale))?)
            .ok_or(CoreError::NumericOverflow {
                operation: "aligning a decimal remainder",
            })?;
        let right = i128::from(other.units)
            .checked_mul(pow10(u32::from(scale - other.scale))?)
            .ok_or(CoreError::NumericOverflow {
                operation: "aligning a decimal remainder",
            })?;
        let units = i64::try_from(left % right).map_err(|_| CoreError::NumericOverflow {
            operation: "taking a decimal remainder",
        })?;
        Ok(Self { units, scale })
    }

    /// Checked negation.
    pub fn checked_neg(self) -> Result<Self, CoreError> {
        Ok(Self {
            units: self.units.checked_neg().ok_or(CoreError::NumericOverflow {
                operation: "negating a decimal",
            })?,
            scale: self.scale,
        })
    }

    /// Checked absolute value.
    pub fn checked_abs(self) -> Result<Self, CoreError> {
        if self.units < 0 {
            self.checked_neg()
        } else {
            Ok(self)
        }
    }

    /// The value as an `i128` coefficient at its own scale. Used by arithmetic
    /// tests and by adapters that need an integer view.
    pub const fn coefficient(self) -> i128 {
        self.units as i128
    }
}

impl std::str::FromStr for Decimal {
    type Err = CoreError;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        Self::parse(input)
    }
}

impl PartialEq for Decimal {
    fn eq(&self, other: &Self) -> bool {
        self.cmp_value(*other).is_eq()
    }
}

impl Eq for Decimal {}

impl PartialOrd for Decimal {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Decimal {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.cmp_value(*other)
    }
}

impl Decimal {
    fn cmp_value(self, other: Self) -> std::cmp::Ordering {
        // Both coefficients are exact `i64` values aligned by at most nine
        // decimal places, so the widened comparison always fits an `i128`.
        // Rescaling into an `i64` first silently reported equal values when
        // the aligned coefficient overflowed.
        let scale = self.scale.max(other.scale);
        let left = i128::from(self.units) * POW10[usize::from(scale - self.scale)];
        let right = i128::from(other.units) * POW10[usize::from(scale - other.scale)];
        left.cmp(&right)
    }
}

impl fmt::Display for Decimal {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let magnitude = i128::from(self.units).unsigned_abs().to_string();
        let scale = usize::from(self.scale);
        if self.units < 0 {
            formatter.write_str("-")?;
        }
        if scale == 0 {
            return formatter.write_str(&magnitude);
        }
        let padded = if magnitude.len() <= scale {
            format!("{magnitude:0>width$}", width = scale + 1)
        } else {
            magnitude
        };
        let split = padded.len() - scale;
        formatter.write_str(&padded[..split])?;
        formatter.write_str(".")?;
        formatter.write_str(&padded[split..])
    }
}

impl Serialize for Decimal {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for Decimal {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct DecimalVisitor;

        impl Visitor<'_> for DecimalVisitor {
            type Value = Decimal;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("a decimal string such as \"123.45\"")
            }

            fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                Decimal::parse(value).map_err(E::custom)
            }

            fn visit_i64<E>(self, value: i64) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                Ok(Decimal::from_integer(value))
            }

            fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                let units = i64::try_from(value).map_err(E::custom)?;
                Ok(Decimal::from_integer(units))
            }
        }

        deserializer.deserialize_any(DecimalVisitor)
    }
}

/// Rounding helper shared by decimal, currency, and formula code.
pub fn div_round(
    numerator: i128,
    denominator: i128,
    mode: RoundingMode,
) -> Result<i128, CoreError> {
    if denominator == 0 {
        return Err(CoreError::DivisionByZero);
    }
    let quotient = numerator
        .checked_div(denominator)
        .ok_or(CoreError::NumericOverflow {
            operation: "dividing",
        })?;
    let remainder = numerator
        .checked_rem(denominator)
        .ok_or(CoreError::NumericOverflow {
            operation: "dividing",
        })?;
    if remainder == 0 {
        return Ok(quotient);
    }
    let negative = (numerator < 0) ^ (denominator < 0);
    let step_away = |value: i128| -> Result<i128, CoreError> {
        if negative {
            value.checked_sub(1)
        } else {
            value.checked_add(1)
        }
        .ok_or(CoreError::NumericOverflow {
            operation: "rounding",
        })
    };
    match mode {
        RoundingMode::TowardZero => Ok(quotient),
        RoundingMode::AwayFromZero => step_away(quotient),
        RoundingMode::Up => {
            if negative {
                Ok(quotient)
            } else {
                step_away(quotient)
            }
        }
        RoundingMode::Down => {
            if negative {
                step_away(quotient)
            } else {
                Ok(quotient)
            }
        }
        RoundingMode::HalfEven | RoundingMode::HalfAwayFromZero => {
            let doubled = remainder
                .checked_abs()
                .and_then(|value| value.checked_mul(2))
                .ok_or(CoreError::NumericOverflow {
                    operation: "rounding",
                })?;
            let divisor = denominator.unsigned_abs();
            let doubled = u128::try_from(doubled).unwrap_or(u128::MAX);
            match doubled.cmp(&divisor) {
                std::cmp::Ordering::Less => Ok(quotient),
                std::cmp::Ordering::Greater => step_away(quotient),
                std::cmp::Ordering::Equal => {
                    if mode == RoundingMode::HalfAwayFromZero {
                        return step_away(quotient);
                    }
                    if quotient % 2 == 0 {
                        Ok(quotient)
                    } else {
                        step_away(quotient)
                    }
                }
            }
        }
    }
}

/// A three-letter ISO 4217 style currency code.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CurrencyCode([u8; 3]);

impl CurrencyCode {
    /// Validates and normalizes a currency code to uppercase.
    pub fn parse(input: &str) -> Result<Self, CoreError> {
        let bytes = input.trim().as_bytes();
        if bytes.len() != 3 || !bytes.iter().all(u8::is_ascii_alphabetic) {
            return Err(CoreError::InvalidCurrencyCode(input.to_owned()));
        }
        let mut normalized = [0_u8; 3];
        for (index, byte) in bytes.iter().enumerate() {
            normalized[index] = byte.to_ascii_uppercase();
        }
        Ok(Self(normalized))
    }

    /// Builds a code from three known-uppercase ASCII bytes without validation.
    ///
    /// Use this only for compile-time constants such as `*b"USD"`.
    pub const fn from_static(bytes: [u8; 3]) -> Self {
        Self(bytes)
    }

    /// The uppercase code as a string slice.
    pub fn as_str(&self) -> &str {
        std::str::from_utf8(&self.0).unwrap_or("XXX")
    }

    /// The conventional minor-unit scale for the currency.
    pub const fn default_scale(self) -> u8 {
        match &self.0 {
            b"JPY" | b"KRW" | b"VND" | b"CLP" | b"ISK" => 0,
            b"BHD" | b"KWD" | b"OMR" | b"TND" | b"JOD" => 3,
            _ => 2,
        }
    }
}

impl fmt::Display for CurrencyCode {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl Serialize for CurrencyCode {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for CurrencyCode {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw = String::deserialize(deserializer)?;
        Self::parse(&raw).map_err(de::Error::custom)
    }
}

/// An exact monetary amount: a decimal amount plus its currency.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(
    feature = "ts",
    ts(export, export_to = "../../../apps/desktop/src/bindings/")
)]
pub struct Money {
    /// ISO 4217 style code.
    #[cfg_attr(feature = "ts", ts(type = "string"))]
    pub currency: CurrencyCode,
    /// Amount at the currency's minor-unit scale.
    #[cfg_attr(feature = "ts", ts(type = "string"))]
    pub amount: Decimal,
}

impl Money {
    /// Builds a monetary amount from minor units at the currency's default scale.
    pub fn from_minor(currency: CurrencyCode, minor_units: i64) -> Result<Self, CoreError> {
        Ok(Self {
            currency,
            amount: Decimal::new(minor_units, currency.default_scale())?,
        })
    }

    /// Builds a monetary amount from minor units with an explicit scale.
    pub fn from_minor_scaled(
        currency: CurrencyCode,
        minor_units: i64,
        scale: u8,
    ) -> Result<Self, CoreError> {
        Ok(Self {
            currency,
            amount: Decimal::new(minor_units, scale)?,
        })
    }

    /// Parses `Value::Currency` display text: `"USD 123.45"` or `"123.45"`,
    /// rounded to the currency's minor-unit scale.
    pub fn parse(text: &str, currency: CurrencyCode) -> Result<Self, CoreError> {
        Self::from_decimal(currency, Self::parse_amount(text, currency)?)
    }

    /// Parses display text and rejects precision beyond the currency scale.
    ///
    /// Import rounds to the currency scale; a filter must not, because
    /// `12.304` is not `12.30` for an equality or range test.
    pub fn parse_exact(text: &str, currency: CurrencyCode) -> Result<Self, CoreError> {
        let amount = Self::parse_amount(text, currency)?;
        let money = Self::from_decimal(currency, amount)?;
        if money.amount() != amount {
            return Err(CoreError::InvalidDecimal {
                input: text.to_owned(),
            });
        }
        Ok(money)
    }

    fn parse_amount(text: &str, currency: CurrencyCode) -> Result<Decimal, CoreError> {
        let trimmed = text.trim();
        let amount = match trimmed.split_once(char::is_whitespace) {
            Some((code, rest))
                if code.len() == 3 && code.bytes().all(|byte| byte.is_ascii_alphabetic()) =>
            {
                if !code.eq_ignore_ascii_case(currency.as_str()) {
                    return Err(CoreError::CurrencyMismatch {
                        left: code.to_ascii_uppercase(),
                        right: currency.to_string(),
                    });
                }
                rest.trim()
            }
            _ => trimmed,
        };
        Decimal::parse(amount)
    }

    /// Converts a decimal to money at the currency's default scale, rounding
    /// half to even.
    pub fn from_decimal(currency: CurrencyCode, amount: Decimal) -> Result<Self, CoreError> {
        Ok(Self {
            currency,
            amount: amount.rescale(currency.default_scale(), RoundingMode::HalfEven)?,
        })
    }

    /// The currency code.
    pub const fn currency(self) -> CurrencyCode {
        self.currency
    }

    /// The amount.
    pub const fn amount(self) -> Decimal {
        self.amount
    }

    /// The amount in minor units, when the scale matches the currency default.
    pub fn minor_units(self) -> i64 {
        self.amount.units()
    }

    /// The scale of the amount.
    pub const fn scale(self) -> u8 {
        self.amount.scale()
    }

    /// Checked addition. Both operands must share a currency.
    pub fn checked_add(self, other: Self) -> Result<Self, CoreError> {
        self.ensure_currency(other)?;
        Ok(Self {
            currency: self.currency,
            amount: self.amount.checked_add(other.amount)?,
        })
    }

    /// Checked subtraction. Both operands must share a currency.
    pub fn checked_sub(self, other: Self) -> Result<Self, CoreError> {
        self.ensure_currency(other)?;
        Ok(Self {
            currency: self.currency,
            amount: self.amount.checked_sub(other.amount)?,
        })
    }

    /// Multiplies by an exact decimal quantity and rounds to the money scale.
    pub fn checked_mul_decimal(self, quantity: Decimal) -> Result<Self, CoreError> {
        // Round once at the money scale. Going through `Decimal::checked_mul`
        // first round-trips via MAX_SCALE and can turn 0.01499999997 into
        // 0.02 instead of 0.01 (and falsely overflow a representable result).
        let product = i128::from(self.amount.units())
            .checked_mul(i128::from(quantity.units()))
            .ok_or(CoreError::NumericOverflow {
                operation: "multiplying money",
            })?;
        let rounded = div_round(
            product,
            pow10(u32::from(quantity.scale()))?,
            RoundingMode::HalfEven,
        )?;
        Ok(Self {
            currency: self.currency,
            amount: Decimal::new(
                i64::try_from(rounded).map_err(|_| CoreError::NumericOverflow {
                    operation: "multiplying money",
                })?,
                self.amount.scale(),
            )?,
        })
    }

    /// Divides by an exact decimal quantity and rounds to the money scale.
    pub fn checked_div_decimal(self, divisor: Decimal) -> Result<Self, CoreError> {
        if divisor.is_zero() {
            return Err(CoreError::DivisionByZero);
        }
        // Round once at the money scale: (units / 10^scale) ÷ (du / 10^ds)
        // at the same scale is `units * 10^ds / du`.
        let numerator = i128::from(self.amount.units())
            .checked_mul(pow10(u32::from(divisor.scale()))?)
            .ok_or(CoreError::NumericOverflow {
                operation: "dividing money",
            })?;
        let rounded = div_round(
            numerator,
            i128::from(divisor.units()),
            RoundingMode::HalfEven,
        )?;
        Ok(Self {
            currency: self.currency,
            amount: Decimal::new(
                i64::try_from(rounded).map_err(|_| CoreError::NumericOverflow {
                    operation: "dividing money",
                })?,
                self.amount.scale(),
            )?,
        })
    }

    /// Multiplies by `numerator / denominator`, rounding half to even.
    pub fn checked_mul_ratio(self, numerator: i64, denominator: i64) -> Result<Self, CoreError> {
        if denominator == 0 {
            return Err(CoreError::DivisionByZero);
        }
        let scaled = i128::from(self.amount.units())
            .checked_mul(i128::from(numerator))
            .ok_or(CoreError::NumericOverflow {
                operation: "scaling money",
            })?;
        let rounded = div_round(scaled, i128::from(denominator), RoundingMode::HalfEven)?;
        Ok(Self {
            currency: self.currency,
            amount: Decimal::new(
                i64::try_from(rounded).map_err(|_| CoreError::NumericOverflow {
                    operation: "scaling money",
                })?,
                self.amount.scale(),
            )?,
        })
    }

    /// Splits the amount proportionally. The sum of the parts always equals the
    /// original amount; leftover minor units go to the largest remainders.
    pub fn allocate(self, ratios: &[u32]) -> Result<Vec<Self>, CoreError> {
        if ratios.is_empty() {
            return Err(CoreError::Allocation {
                reason: "no ratios supplied".into(),
            });
        }
        let total_ratio: u128 = ratios.iter().map(|ratio| u128::from(*ratio)).sum();
        if total_ratio == 0 {
            return Err(CoreError::Allocation {
                reason: "ratios sum to zero".into(),
            });
        }
        let negative = self.amount.units() < 0;
        let total = i128::from(self.amount.units()).unsigned_abs();
        let mut shares = Vec::with_capacity(ratios.len());
        let mut remainders = Vec::with_capacity(ratios.len());
        let mut assigned = 0_u128;
        for ratio in ratios {
            let weighted = total
                .checked_mul(u128::from(*ratio))
                .ok_or(CoreError::Allocation {
                    reason: "amount times ratio overflowed".into(),
                })?;
            let share = weighted / total_ratio;
            let remainder = weighted % total_ratio;
            assigned += share;
            shares.push(share);
            remainders.push(remainder);
        }
        let mut leftover = total - assigned;
        while leftover > 0 {
            let mut best: Option<usize> = None;
            for (index, remainder) in remainders.iter().enumerate() {
                if *remainder == 0 {
                    continue;
                }
                match best {
                    None => best = Some(index),
                    Some(current) if remainders[current] < *remainder => best = Some(index),
                    Some(_) => {}
                }
            }
            let Some(index) = best else {
                break;
            };
            shares[index] += 1;
            remainders[index] = 0;
            leftover -= 1;
        }
        let mut result = Vec::with_capacity(shares.len());
        for share in shares {
            // A single ratio can own the whole amount, including i64::MIN.
            // Apply the sign in i128 before narrowing once.
            let magnitude = i128::try_from(share).map_err(|_| CoreError::Allocation {
                reason: "share out of range".into(),
            })?;
            let signed = if negative {
                magnitude.checked_neg().ok_or(CoreError::Allocation {
                    reason: "share out of range".into(),
                })?
            } else {
                magnitude
            };
            let units = i64::try_from(signed).map_err(|_| CoreError::Allocation {
                reason: "share out of range".into(),
            })?;
            result.push(Self {
                currency: self.currency,
                amount: Decimal::new(units, self.amount.scale())?,
            });
        }
        Ok(result)
    }

    /// Sums an iterator of money values, requiring one currency.
    pub fn checked_sum<I>(values: I) -> Result<Option<Self>, CoreError>
    where
        I: IntoIterator<Item = Self>,
    {
        let mut iter = values.into_iter();
        let Some(first) = iter.next() else {
            return Ok(None);
        };
        let mut total = first;
        for value in iter {
            total = total.checked_add(value)?;
        }
        Ok(Some(total))
    }

    fn ensure_currency(self, other: Self) -> Result<(), CoreError> {
        if self.currency != other.currency {
            return Err(CoreError::CurrencyMismatch {
                left: self.currency.to_string(),
                right: other.currency.to_string(),
            });
        }
        Ok(())
    }
}

impl fmt::Display for Money {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{} {}", self.currency, self.amount)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_formats_decimal() {
        let parsed = Decimal::parse("-1234.50").expect("valid");
        assert_eq!(parsed.units(), -123_450);
        assert_eq!(parsed.scale(), 2);
        assert_eq!(parsed.to_string(), "-1234.50");
        assert_eq!(Decimal::parse("0.005").expect("valid").to_string(), "0.005");
        assert_eq!(Decimal::parse(".5").expect("valid").to_string(), "0.5");
        assert!(Decimal::parse("1e3").is_err());
        assert!(Decimal::parse("1,000").is_err());
    }

    #[test]
    fn addition_aligns_scales() {
        let total = Decimal::parse("0.1")
            .expect("valid")
            .checked_add(Decimal::parse("0.02").expect("valid"))
            .expect("add");
        assert_eq!(total.to_string(), "0.12");
    }

    #[test]
    fn multiplication_rounds_at_max_scale() {
        let left = Decimal::parse("1.234567891").expect("valid");
        let product = left.checked_mul(left).expect("mul");
        assert_eq!(product.scale(), MAX_SCALE);
        assert_eq!(product.to_string(), "1.524157877");
    }

    #[test]
    fn division_rounds_half_even() {
        let one = Decimal::from_integer(1);
        let eight = Decimal::from_integer(8);
        assert_eq!(one.checked_div(eight).expect("div").to_string(), "0.1250");
    }

    #[test]
    fn money_rounding_is_half_even() {
        let currency = CurrencyCode::parse("USD").expect("usd");
        let half = Decimal::parse("0.005").expect("valid");
        let rounded = Money::from_decimal(currency, half).expect("money");
        assert_eq!(rounded.amount().to_string(), "0.00");
        let three = Decimal::parse("0.015").expect("valid");
        assert_eq!(
            Money::from_decimal(currency, three)
                .expect("money")
                .amount()
                .to_string(),
            "0.02"
        );
    }

    #[test]
    fn allocation_preserves_total() {
        let money =
            Money::from_minor(CurrencyCode::parse("USD").expect("usd"), 100).expect("money");
        let parts = money.allocate(&[1, 1, 1]).expect("allocate");
        let total = Money::checked_sum(parts.iter().copied())
            .expect("sum")
            .expect("nonempty");
        assert_eq!(total, money);
        assert_eq!(parts.len(), 3);
        assert_eq!(parts[0].amount().units(), 34);
        assert_eq!(parts[1].amount().units(), 33);
    }

    #[test]
    fn jpy_has_no_minor_unit() {
        let currency = CurrencyCode::parse("jpy").expect("jpy");
        assert_eq!(currency.as_str(), "JPY");
        assert_eq!(currency.default_scale(), 0);
    }

    #[test]
    fn zero_scaled_equality_ignores_scale() {
        assert_eq!(
            Decimal::parse("1.0").expect("valid"),
            Decimal::parse("1.00").expect("valid")
        );
    }
}
