//! Evaluation of parsed formulas against actual values.

use std::cmp::Ordering;

use slab_core::{
    Aggregate, CurrencyCode, Decimal, Money, RoundingMode, SlabDate, SlabTimestamp, Value,
};

use crate::ast::{BinaryOp, Expr, UnaryOp};
use crate::error::FormulaError;
use crate::parser;

/// Values supplied to the evaluator for one row.
pub trait EvalContext {
    /// Returns a stored or previously computed column value.
    fn column(&self, name: &str) -> Option<Value>;

    /// Computes an aggregate over a child relation.
    fn aggregate(
        &self,
        relation: &str,
        column: &str,
        aggregate: Aggregate,
    ) -> Result<Value, FormulaError>;

    /// The current local date.
    fn today(&self) -> SlabDate;

    /// The current UTC timestamp.
    fn now(&self) -> SlabTimestamp;
}

/// Parses and evaluates a formula.
pub fn evaluate(input: &str, context: &dyn EvalContext) -> Result<Value, FormulaError> {
    let expression = parser::parse(input)?;
    evaluate_expression(&expression, context)
}

/// Evaluates a parsed expression.
pub fn evaluate_expression(
    expression: &Expr,
    context: &dyn EvalContext,
) -> Result<Value, FormulaError> {
    eval(expression, context, 0)
}

fn eval(expression: &Expr, context: &dyn EvalContext, depth: u32) -> Result<Value, FormulaError> {
    if depth > slab_core::MAX_EXPRESSION_DEPTH {
        return Err(FormulaError::Depth {
            limit: slab_core::MAX_EXPRESSION_DEPTH,
        });
    }
    match expression {
        Expr::Literal(value) => Ok(value.clone()),
        Expr::Column { table: None, name } => context
            .column(name)
            .ok_or_else(|| FormulaError::UnknownColumn(name.clone())),
        Expr::Column { table: Some(_), .. } => Err(FormulaError::Type(
            "qualified columns are only allowed inside aggregate functions".into(),
        )),
        Expr::Unary { op, expr } => {
            let value = eval(expr, context, depth + 1)?;
            match op {
                UnaryOp::Neg => negate(&value),
                UnaryOp::Not => Ok(Value::Boolean(!value.is_truthy())),
            }
        }
        Expr::Binary { op, left, right } => {
            let left = eval(left, context, depth + 1)?;
            match op {
                BinaryOp::And if !left.is_truthy() => Ok(Value::Boolean(false)),
                BinaryOp::Or if left.is_truthy() => Ok(Value::Boolean(true)),
                _ => {
                    let right = eval(right, context, depth + 1)?;
                    apply_binary(*op, &left, &right)
                }
            }
        }
        Expr::Call { name, args } => eval_call(name, args, context, depth),
    }
}

fn eval_call(
    name: &str,
    args: &[Expr],
    context: &dyn EvalContext,
    depth: u32,
) -> Result<Value, FormulaError> {
    match name {
        "IF" => {
            if args.len() != 3 {
                return arity(name, "3 arguments", args.len());
            }
            let condition = eval(&args[0], context, depth + 1)?;
            if condition.is_truthy() {
                eval(&args[1], context, depth + 1)
            } else {
                eval(&args[2], context, depth + 1)
            }
        }
        "COALESCE" => {
            if args.is_empty() {
                return arity(name, "at least one argument", 0);
            }
            for arg in args {
                let value = eval(arg, context, depth + 1)?;
                if !value.is_null() {
                    return Ok(value);
                }
            }
            Ok(Value::Null)
        }
        "COUNT" | "SUM" | "AVG" | "MIN" | "MAX" => {
            if args.len() != 1 {
                return arity(name, "1 argument", args.len());
            }
            let Some((relation, column)) = args[0].qualified_column() else {
                return Err(FormulaError::Type(format!(
                    "{name} expects a qualified child column"
                )));
            };
            let aggregate = match name {
                "COUNT" => Aggregate::Count,
                "SUM" => Aggregate::Sum,
                "AVG" => Aggregate::Avg,
                "MIN" => Aggregate::Min,
                _ => Aggregate::Max,
            };
            context.aggregate(relation, column, aggregate)
        }
        _ => {
            let mut values = Vec::with_capacity(args.len());
            for arg in args {
                values.push(eval(arg, context, depth + 1)?);
            }
            apply_function(name, &values, context)
        }
    }
}

fn apply_function(
    name: &str,
    values: &[Value],
    context: &dyn EvalContext,
) -> Result<Value, FormulaError> {
    match name {
        "CONCAT" => {
            if values.is_empty() {
                return arity(name, "at least one argument", 0);
            }
            let mut output = String::new();
            for value in values {
                output.push_str(&value.display_string());
            }
            Ok(Value::Text(output))
        }
        "LEN" => {
            if values.len() != 1 {
                return arity(name, "1 argument", values.len());
            }
            let Some(text) = values[0].as_text() else {
                return Err(FormulaError::Eval("LEN expects text".into()));
            };
            let length = i64::try_from(text.chars().count()).unwrap_or(i64::MAX);
            Ok(Value::Integer(length))
        }
        "UPPER" | "LOWER" | "TRIM" => {
            if values.len() != 1 {
                return arity(name, "1 argument", values.len());
            }
            let Some(text) = values[0].as_text() else {
                return Err(FormulaError::Eval(format!("{name} expects text")));
            };
            let output = match name {
                "UPPER" => text.to_uppercase(),
                "LOWER" => text.to_lowercase(),
                _ => text.trim().to_owned(),
            };
            Ok(Value::Text(output))
        }
        "ABS" => {
            if values.len() != 1 {
                return arity(name, "1 argument", values.len());
            }
            absolute(&values[0])
        }
        "ROUND" => {
            if values.len() != 2 {
                return arity(name, "2 arguments", values.len());
            }
            let Some(digits) = values[1].as_integer() else {
                return Err(FormulaError::Eval(
                    "ROUND expects an integer digit count".into(),
                ));
            };
            if !(0..=i64::from(slab_core::MAX_SCALE)).contains(&digits) {
                return Err(FormulaError::Eval(format!(
                    "ROUND digits must be between 0 and {}",
                    slab_core::MAX_SCALE
                )));
            }
            let digits = u8::try_from(digits)
                .map_err(|_| FormulaError::Eval("ROUND digit count out of range".into()))?;
            round(&values[0], digits)
        }
        "DATE" => {
            if values.len() != 1 {
                return arity(name, "1 argument", values.len());
            }
            date_value(&values[0])
        }
        "TIMESTAMP" => {
            if values.len() != 1 {
                return arity(name, "1 argument", values.len());
            }
            timestamp_value(&values[0])
        }
        "TODAY" => {
            if !values.is_empty() {
                return arity(name, "no arguments", values.len());
            }
            Ok(Value::Date(context.today()))
        }
        "NOW" => {
            if !values.is_empty() {
                return arity(name, "no arguments", values.len());
            }
            Ok(Value::Timestamp(context.now()))
        }
        "YEAR" | "MONTH" | "DAY" => {
            if values.len() != 1 {
                return arity(name, "1 argument", values.len());
            }
            let date = match &values[0] {
                Value::Date(date) => *date,
                Value::Timestamp(stamp) => stamp.date(),
                _ => {
                    return Err(FormulaError::Eval(format!(
                        "{name} expects a date or timestamp"
                    )));
                }
            };
            let part = match name {
                "YEAR" => i64::from(date.year()),
                "MONTH" => i64::from(date.month()),
                _ => i64::from(date.day()),
            };
            Ok(Value::Integer(part))
        }
        other => Err(FormulaError::UnknownFunction(other.to_owned())),
    }
}

fn arity(name: &str, expected: &str, found: usize) -> Result<Value, FormulaError> {
    Err(FormulaError::Arity {
        function: name.to_owned(),
        expected: expected.to_owned(),
        found,
    })
}

fn apply_binary(op: BinaryOp, left: &Value, right: &Value) -> Result<Value, FormulaError> {
    match op {
        BinaryOp::Add => add(left, right),
        BinaryOp::Sub => subtract(left, right),
        BinaryOp::Mul => multiply(left, right),
        BinaryOp::Div => divide(left, right),
        BinaryOp::Rem => remainder(left, right),
        BinaryOp::Eq => Ok(Value::Boolean(equals(left, right))),
        BinaryOp::NotEq => Ok(Value::Boolean(!equals(left, right))),
        BinaryOp::Lt | BinaryOp::Lte | BinaryOp::Gt | BinaryOp::Gte => {
            let ordering = compare(left, right)?.ok_or_else(|| {
                FormulaError::Eval(format!(
                    "cannot order {} and {}",
                    left.type_name(),
                    right.type_name()
                ))
            })?;
            let result = match op {
                BinaryOp::Lt => ordering == Ordering::Less,
                BinaryOp::Lte => ordering != Ordering::Greater,
                BinaryOp::Gt => ordering == Ordering::Greater,
                _ => ordering != Ordering::Less,
            };
            Ok(Value::Boolean(result))
        }
        BinaryOp::And | BinaryOp::Or => Ok(Value::Boolean(match op {
            BinaryOp::And => left.is_truthy() && right.is_truthy(),
            _ => left.is_truthy() || right.is_truthy(),
        })),
    }
}

fn equals(left: &Value, right: &Value) -> bool {
    match (left, right) {
        (Value::Null, Value::Null) => true,
        _ => compare(left, right)
            .ok()
            .flatten()
            .is_some_and(|ordering| ordering == Ordering::Equal),
    }
}

fn compare(left: &Value, right: &Value) -> Result<Option<Ordering>, FormulaError> {
    match (left, right) {
        (Value::Integer(left), Value::Integer(right)) => Ok(Some(left.cmp(right))),
        (Value::Decimal(left), Value::Decimal(right)) => Ok(Some(left.cmp(right))),
        (Value::Integer(left), Value::Decimal(right)) => {
            Ok(Some(Decimal::from_integer(*left).cmp(right)))
        }
        (Value::Decimal(left), Value::Integer(right)) => {
            Ok(Some(left.cmp(&Decimal::from_integer(*right))))
        }
        (Value::Currency(left), Value::Currency(right)) => {
            if left.currency() != right.currency() {
                return Err(FormulaError::Core(slab_core::CoreError::CurrencyMismatch {
                    left: left.currency().to_string(),
                    right: right.currency().to_string(),
                }));
            }
            Ok(Some(left.amount().cmp(&right.amount())))
        }
        (
            Value::Text(left) | Value::Select(left) | Value::AttachmentRef(left),
            Value::Text(right) | Value::Select(right) | Value::AttachmentRef(right),
        ) => Ok(Some(left.cmp(right))),
        (Value::Boolean(left), Value::Boolean(right)) => Ok(Some(left.cmp(right))),
        (Value::Date(left), Value::Date(right)) => Ok(Some(left.cmp(right))),
        (Value::Timestamp(left), Value::Timestamp(right)) => Ok(Some(left.cmp(right))),
        _ => Ok(None),
    }
}

enum Number {
    Integer(i64),
    Decimal(Decimal),
}

fn number(value: &Value) -> Option<Number> {
    match value {
        Value::Integer(value) => Some(Number::Integer(*value)),
        Value::Decimal(value) => Some(Number::Decimal(*value)),
        _ => None,
    }
}

fn number_decimal(value: Number) -> Decimal {
    match value {
        Number::Integer(value) => Decimal::from_integer(value),
        Number::Decimal(value) => value,
    }
}

fn number_value(value: Number) -> Value {
    match value {
        Number::Integer(value) => Value::Integer(value),
        Number::Decimal(value) => Value::Decimal(value),
    }
}

fn add(left: &Value, right: &Value) -> Result<Value, FormulaError> {
    if left.is_null() || right.is_null() {
        return Ok(Value::Null);
    }
    if let (Value::Currency(left), Value::Currency(right)) = (left, right) {
        return Ok(Value::Currency(left.checked_add(*right)?));
    }
    if let (Some(left), Some(right)) = (number(left), number(right)) {
        let result = match (left, right) {
            (Number::Integer(left), Number::Integer(right)) => Number::Integer(
                left.checked_add(right)
                    .ok_or_else(|| overflow("adding integers"))?,
            ),
            (left, right) => {
                Number::Decimal(number_decimal(left).checked_add(number_decimal(right))?)
            }
        };
        return Ok(number_value(result));
    }
    Err(FormulaError::Eval(format!(
        "cannot add {} and {}",
        left.type_name(),
        right.type_name()
    )))
}

fn subtract(left: &Value, right: &Value) -> Result<Value, FormulaError> {
    if left.is_null() || right.is_null() {
        return Ok(Value::Null);
    }
    if let (Value::Currency(left), Value::Currency(right)) = (left, right) {
        return Ok(Value::Currency(left.checked_sub(*right)?));
    }
    if let (Some(left), Some(right)) = (number(left), number(right)) {
        let result = match (left, right) {
            (Number::Integer(left), Number::Integer(right)) => Number::Integer(
                left.checked_sub(right)
                    .ok_or_else(|| overflow("subtracting integers"))?,
            ),
            (left, right) => {
                Number::Decimal(number_decimal(left).checked_sub(number_decimal(right))?)
            }
        };
        return Ok(number_value(result));
    }
    Err(FormulaError::Eval(format!(
        "cannot subtract {} from {}",
        right.type_name(),
        left.type_name()
    )))
}

fn multiply(left: &Value, right: &Value) -> Result<Value, FormulaError> {
    if left.is_null() || right.is_null() {
        return Ok(Value::Null);
    }
    match (left, right) {
        (Value::Currency(money), other) if number(other).is_some() => {
            let quantity = number_decimal(
                number(other).ok_or_else(|| FormulaError::Eval("expected a number".into()))?,
            );
            Ok(Value::Currency(money.checked_mul_decimal(quantity)?))
        }
        (other, Value::Currency(money)) if number(other).is_some() => {
            let quantity = number_decimal(
                number(other).ok_or_else(|| FormulaError::Eval("expected a number".into()))?,
            );
            Ok(Value::Currency(money.checked_mul_decimal(quantity)?))
        }
        _ => {
            let (Some(left), Some(right)) = (number(left), number(right)) else {
                return Err(FormulaError::Eval(format!(
                    "cannot multiply {} by {}",
                    left.type_name(),
                    right.type_name()
                )));
            };
            let result = match (left, right) {
                (Number::Integer(left), Number::Integer(right)) => Number::Integer(
                    left.checked_mul(right)
                        .ok_or_else(|| overflow("multiplying integers"))?,
                ),
                (left, right) => {
                    Number::Decimal(number_decimal(left).checked_mul(number_decimal(right))?)
                }
            };
            Ok(number_value(result))
        }
    }
}

fn divide(left: &Value, right: &Value) -> Result<Value, FormulaError> {
    if left.is_null() || right.is_null() {
        return Ok(Value::Null);
    }
    match (left, right) {
        (Value::Currency(money), other) if number(other).is_some() => {
            let divisor = number_decimal(
                number(other).ok_or_else(|| FormulaError::Eval("expected a number".into()))?,
            );
            Ok(Value::Currency(money.checked_div_decimal(divisor)?))
        }
        (Value::Currency(left), Value::Currency(right)) => {
            if left.currency() != right.currency() {
                return Err(FormulaError::Core(slab_core::CoreError::CurrencyMismatch {
                    left: left.currency().to_string(),
                    right: right.currency().to_string(),
                }));
            }
            Ok(Value::Decimal(left.amount().checked_div(right.amount())?))
        }
        _ => {
            let (Some(left), Some(right)) = (number(left), number(right)) else {
                return Err(FormulaError::Eval(format!(
                    "cannot divide {} by {}",
                    left.type_name(),
                    right.type_name()
                )));
            };
            let result = number_decimal(left).checked_div(number_decimal(right))?;
            Ok(Value::Decimal(result))
        }
    }
}

fn remainder(left: &Value, right: &Value) -> Result<Value, FormulaError> {
    if left.is_null() || right.is_null() {
        return Ok(Value::Null);
    }
    let (Some(left), Some(right)) = (number(left), number(right)) else {
        return Err(FormulaError::Eval(format!(
            "remainder expects numbers, found {} and {}",
            left.type_name(),
            right.type_name()
        )));
    };
    match (left, right) {
        (Number::Integer(left), Number::Integer(right)) => {
            let remainder = left.checked_rem(right).ok_or_else(|| {
                if right == 0 {
                    FormulaError::Core(slab_core::CoreError::DivisionByZero)
                } else {
                    overflow("taking an integer remainder")
                }
            })?;
            Ok(Value::Integer(remainder))
        }
        (left, right) => Ok(Value::Decimal(
            number_decimal(left).checked_rem(number_decimal(right))?,
        )),
    }
}

fn negate(value: &Value) -> Result<Value, FormulaError> {
    if value.is_null() {
        return Ok(Value::Null);
    }
    match value {
        Value::Integer(value) => Ok(Value::Integer(
            value
                .checked_neg()
                .ok_or_else(|| overflow("negating an integer"))?,
        )),
        Value::Decimal(value) => Ok(Value::Decimal(value.checked_neg()?)),
        Value::Currency(money) => Ok(Value::Currency(Money::from_minor_scaled(
            money.currency(),
            money.amount().checked_neg()?.units(),
            money.scale(),
        )?)),
        _ => Err(FormulaError::Eval(format!(
            "cannot negate {}",
            value.type_name()
        ))),
    }
}

fn absolute(value: &Value) -> Result<Value, FormulaError> {
    if value.is_null() {
        return Ok(Value::Null);
    }
    match value {
        Value::Integer(value) => Ok(Value::Integer(
            value
                .checked_abs()
                .ok_or_else(|| overflow("taking an absolute value"))?,
        )),
        Value::Decimal(value) => Ok(Value::Decimal(value.checked_abs()?)),
        Value::Currency(money) => Ok(Value::Currency(Money::from_minor_scaled(
            money.currency(),
            money.amount().checked_abs()?.units(),
            money.scale(),
        )?)),
        _ => Err(FormulaError::Eval(format!(
            "ABS expects a number, found {}",
            value.type_name()
        ))),
    }
}

fn round(value: &Value, digits: u8) -> Result<Value, FormulaError> {
    if value.is_null() {
        return Ok(Value::Null);
    }
    match value {
        Value::Integer(value) => Ok(Value::Integer(*value)),
        Value::Decimal(value) => Ok(Value::Decimal(
            value.rescale(digits, RoundingMode::HalfEven)?,
        )),
        Value::Currency(money) => {
            let amount = money.amount().rescale(digits, RoundingMode::HalfEven)?;
            Ok(Value::Currency(Money::from_minor_scaled(
                money.currency(),
                amount.units(),
                amount.scale(),
            )?))
        }
        _ => Err(FormulaError::Eval(format!(
            "ROUND expects a number, found {}",
            value.type_name()
        ))),
    }
}

fn date_value(value: &Value) -> Result<Value, FormulaError> {
    match value {
        Value::Date(date) => Ok(Value::Date(*date)),
        Value::Timestamp(stamp) => Ok(Value::Date(stamp.date())),
        Value::Text(text) => Ok(Value::Date(SlabDate::parse(text)?)),
        _ => Err(FormulaError::Eval(format!(
            "DATE expects text, found {}",
            value.type_name()
        ))),
    }
}

fn timestamp_value(value: &Value) -> Result<Value, FormulaError> {
    match value {
        Value::Timestamp(stamp) => Ok(Value::Timestamp(*stamp)),
        Value::Date(date) => Ok(Value::Timestamp(SlabTimestamp::from_parts(
            date.year(),
            date.month(),
            date.day(),
            0,
            0,
            0,
            0,
        )?)),
        Value::Text(text) => Ok(Value::Timestamp(SlabTimestamp::parse(text)?)),
        _ => Err(FormulaError::Eval(format!(
            "TIMESTAMP expects text, found {}",
            value.type_name()
        ))),
    }
}

fn overflow(operation: &'static str) -> FormulaError {
    FormulaError::Core(slab_core::CoreError::NumericOverflow { operation })
}

/// A small helper for tests and adapters that need a currency amount.
pub fn money_from_minor(currency: &str, minor_units: i64) -> Result<Money, FormulaError> {
    Ok(Money::from_minor(
        CurrencyCode::parse(currency)?,
        minor_units,
    )?)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;

    struct TestContext {
        columns: BTreeMap<String, Value>,
        aggregates: BTreeMap<(String, String, Aggregate), Value>,
        today: SlabDate,
        now: SlabTimestamp,
    }

    impl EvalContext for TestContext {
        fn column(&self, name: &str) -> Option<Value> {
            self.columns.get(name).cloned()
        }

        fn aggregate(
            &self,
            relation: &str,
            column: &str,
            aggregate: Aggregate,
        ) -> Result<Value, FormulaError> {
            self.aggregates
                .get(&(relation.to_owned(), column.to_owned(), aggregate))
                .cloned()
                .ok_or_else(|| FormulaError::Eval("missing aggregate".into()))
        }

        fn today(&self) -> SlabDate {
            self.today
        }

        fn now(&self) -> SlabTimestamp {
            self.now
        }
    }

    fn context() -> TestContext {
        let mut columns = BTreeMap::new();
        columns.insert("Qty".into(), Value::Integer(3));
        columns.insert(
            "Price".into(),
            Value::Decimal(Decimal::parse("12.50").expect("price")),
        );
        columns.insert(
            "Subtotal".into(),
            Value::Currency(money_from_minor("USD", 3750).expect("money")),
        );
        columns.insert(
            "Tax".into(),
            Value::Currency(money_from_minor("USD", 300).expect("tax")),
        );
        columns.insert("Name".into(), Value::Text("Ada".into()));
        columns.insert("Active".into(), Value::Boolean(true));
        columns.insert(
            "Start".into(),
            Value::Date(SlabDate::parse("2026-01-15").expect("date")),
        );
        let mut aggregates = BTreeMap::new();
        aggregates.insert(
            ("Lines".into(), "Amount".into(), Aggregate::Sum),
            Value::Currency(money_from_minor("USD", 3750).expect("sum")),
        );
        aggregates.insert(
            ("Lines".into(), "Amount".into(), Aggregate::Count),
            Value::Integer(3),
        );
        TestContext {
            columns,
            aggregates,
            today: SlabDate::parse("2026-11-25").expect("today"),
            now: SlabTimestamp::parse("2026-11-25T10:00:00.000Z").expect("now"),
        }
    }

    #[test]
    fn adds_money_exactly() {
        let value = evaluate("[Subtotal] + [Tax]", &context()).expect("eval");
        assert_eq!(
            value,
            Value::Currency(money_from_minor("USD", 4050).expect("money"))
        );
    }

    #[test]
    fn multiplies_money_by_quantity() {
        let value = evaluate("[Subtotal] * 2", &context()).expect("eval");
        assert_eq!(
            value,
            Value::Currency(money_from_minor("USD", 7500).expect("money"))
        );
    }

    #[test]
    fn evaluates_aggregates() {
        let mut context = context();
        for (aggregate, value) in [
            (Aggregate::Avg, 1250),
            (Aggregate::Min, 500),
            (Aggregate::Max, 2000),
        ] {
            context.aggregates.insert(
                ("Lines".into(), "Amount".into(), aggregate),
                Value::Currency(money_from_minor("USD", value).expect("money")),
            );
        }
        for (formula, expected) in [
            ("SUM([Lines].[Amount])", 3750),
            ("AVG([Lines].[Amount])", 1250),
            ("MIN([Lines].[Amount])", 500),
            ("MAX([Lines].[Amount])", 2000),
        ] {
            let expected = Value::Currency(money_from_minor("USD", expected).expect("money"));
            assert_eq!(evaluate(formula, &context).expect(formula), expected);
        }
        assert_eq!(
            evaluate("COUNT([Lines].[Amount])", &context).expect("count"),
            Value::Integer(3)
        );
    }

    #[test]
    fn short_circuits_if_and_coalesce() {
        assert_eq!(
            evaluate("IF([Active], [Name], \"inactive\")", &context()).expect("eval"),
            Value::Text("Ada".into())
        );
        assert_eq!(
            evaluate("COALESCE(NULL, [Name], \"fallback\")", &context()).expect("eval"),
            Value::Text("Ada".into())
        );
    }

    #[test]
    fn string_functions_work() {
        assert_eq!(
            evaluate("CONCAT(UPPER([Name]), \" / \", LEN([Name]))", &context()).expect("eval"),
            Value::Text("ADA / 3".into())
        );
        assert_eq!(
            evaluate("LOWER(TRIM('  ÉADA  '))", &context()).expect("eval"),
            Value::Text("éada".into())
        );
    }

    #[test]
    fn division_by_zero_is_an_error() {
        let error = evaluate("[Qty] / 0", &context()).expect_err("division by zero");
        assert_eq!(error.code_str(), "SLAB_E_DIV_ZERO");
    }

    #[test]
    fn dates_and_rounding_work() {
        assert_eq!(
            evaluate("YEAR([Start])", &context()).expect("year"),
            Value::Integer(2026)
        );
        assert_eq!(
            evaluate("ROUND(12.346, 2)", &context()).expect("round"),
            Value::Decimal(Decimal::parse("12.35").expect("expected"))
        );
        assert_eq!(
            evaluate("TODAY()", &context()).expect("today"),
            Value::Date(SlabDate::parse("2026-11-25").expect("date"))
        );
    }

    #[test]
    fn timestamp_converts_utc_text() {
        let value =
            evaluate("TIMESTAMP('1969-12-31T23:59:59.999Z')", &context()).expect("timestamp");
        assert_eq!(value, Value::Timestamp(SlabTimestamp::from_millis(-1)));
    }

    #[test]
    fn temporal_functions_return_dates_and_utc_clock_values() {
        let context = context();
        let date = SlabDate::parse("2026-11-25").expect("date");
        let midnight = SlabTimestamp::parse("2026-11-25T00:00:00Z").expect("midnight");
        for (formula, expected) in [
            ("DATE('2026-11-25')", Value::Date(date)),
            ("DATE(TODAY())", Value::Date(date)),
            ("DATE(NOW())", Value::Date(date)),
            ("NOW()", Value::Timestamp(context.now)),
            ("TIMESTAMP(NOW())", Value::Timestamp(context.now)),
            ("TIMESTAMP(TODAY())", Value::Timestamp(midnight)),
            ("MONTH(NOW())", Value::Integer(11)),
            ("DAY(TODAY())", Value::Integer(25)),
        ] {
            assert_eq!(evaluate(formula, &context).expect(formula), expected);
        }
    }

    #[test]
    fn boolean_operators_skip_only_the_unneeded_operand() {
        let context = context();
        for (formula, expected) in [
            ("FALSE AND [Missing]", false),
            ("TRUE OR [Missing]", true),
            ("TRUE AND FALSE", false),
            ("FALSE OR TRUE", true),
            ("FALSE OR FALSE", false),
        ] {
            assert_eq!(
                evaluate(formula, &context).expect(formula),
                Value::Boolean(expected)
            );
        }
        for formula in ["TRUE AND [Missing]", "FALSE OR [Missing]"] {
            assert!(matches!(
                evaluate(formula, &context),
                Err(FormulaError::UnknownColumn(name)) if name == "Missing"
            ));
        }
    }

    #[test]
    fn numeric_operators_preserve_exact_results_and_nulls() {
        let context = context();
        for (formula, expected) in [
            ("[Qty] + 2", Value::Integer(5)),
            ("[Qty] - 2", Value::Integer(1)),
            ("[Qty] * 2", Value::Integer(6)),
            ("[Qty] % 2", Value::Integer(1)),
            ("-[Qty]", Value::Integer(-3)),
            ("ABS(-3)", Value::Integer(3)),
            ("ROUND(3, 0)", Value::Integer(3)),
            (
                "[Subtotal] - [Tax]",
                Value::Currency(money_from_minor("USD", 3450).expect("money")),
            ),
            (
                "2 * [Tax]",
                Value::Currency(money_from_minor("USD", 600).expect("money")),
            ),
            (
                "-[Tax]",
                Value::Currency(money_from_minor("USD", -300).expect("money")),
            ),
            (
                "ABS(-[Tax])",
                Value::Currency(money_from_minor("USD", 300).expect("money")),
            ),
        ] {
            assert_eq!(evaluate(formula, &context).expect(formula), expected);
        }
        for (formula, expected) in [
            ("[Price] + 0.25", "12.75"),
            ("[Price] - 2", "10.50"),
            ("[Price] * 2", "25.00"),
            ("[Price] / 2", "6.25"),
            ("[Price] % 2", "0.50"),
            ("[Subtotal] / [Tax]", "12.5"),
            ("ABS(-[Price])", "12.50"),
        ] {
            let expected = Value::Decimal(Decimal::parse(expected).expect("decimal"));
            assert_eq!(evaluate(formula, &context).expect(formula), expected);
        }
        for formula in [
            "2 + NULL",
            "NULL - 2",
            "2 - NULL",
            "2 * NULL",
            "NULL % 2",
            "2 % NULL",
            "-NULL",
            "ABS(NULL)",
            "ROUND(NULL, 2)",
            "COALESCE(NULL, NULL)",
        ] {
            assert_eq!(evaluate(formula, &context).expect(formula), Value::Null);
        }
    }

    #[test]
    fn compares_numeric_and_temporal_values_without_losing_precision() {
        let context = context();
        for (formula, expected) in [
            ("2 < 3", true),
            ("3 < 3", false),
            ("3 <= 3", true),
            ("3 <= 2", false),
            ("3 > 2", true),
            ("3 > 3", false),
            ("3 >= 3", true),
            ("2 >= 3", false),
            ("3 = 3.00", true),
            ("3.01 > 3", true),
            ("2.90 < 3.0", true),
            ("3.0 > 2.90", true),
            ("3 != 3", false),
            ("NULL = NULL", true),
            ("NULL != 0", true),
            ("0 = NULL", false),
            ("TRUE = FALSE", false),
            ("TRUE = TRUE", true),
            ("[Tax] < [Subtotal]", true),
            ("[Start] < TODAY()", true),
            ("TIMESTAMP(TODAY()) < NOW()", true),
        ] {
            assert_eq!(
                evaluate(formula, &context).expect(formula),
                Value::Boolean(expected)
            );
        }
    }

    #[test]
    fn rejects_incompatible_currency_operations_and_preserves_rounding_scale() {
        let mut context = context();
        context.columns.insert(
            "Foreign".into(),
            Value::Currency(money_from_minor("EUR", 300).expect("money")),
        );
        for formula in ["[Tax] < [Foreign]", "[Tax] / [Foreign]"] {
            let error = evaluate(formula, &context).expect_err(formula);
            assert!(matches!(
                error,
                FormulaError::Core(slab_core::CoreError::CurrencyMismatch { left, right })
                    if left == "USD" && right == "EUR"
            ));
        }
        for (formula, expected) in [
            ("[Tax] * [Foreign]", "cannot multiply currency by currency"),
            ("[Name] * [Tax]", "cannot multiply text by currency"),
            ("[Tax] / [Name]", "cannot divide currency by text"),
        ] {
            assert!(matches!(
                evaluate(formula, &context),
                Err(FormulaError::Eval(message)) if message == expected
            ));
        }
        let rounded = evaluate("ROUND([Tax], 1)", &context).expect("rounded amount");
        assert!(matches!(
            rounded,
            Value::Currency(amount) if amount.amount().units() == 30 && amount.scale() == 1
        ));
    }

    #[test]
    fn reports_wrong_argument_counts_before_evaluating_functions() {
        let context = context();
        for formula in [
            "IF()",
            "COALESCE()",
            "CONCAT()",
            "SUM()",
            "LEN()",
            "TRIM()",
            "ABS()",
            "ROUND(1)",
            "DATE()",
            "TIMESTAMP()",
            "TODAY(1)",
            "NOW(1)",
            "YEAR()",
        ] {
            assert!(
                matches!(evaluate(formula, &context), Err(FormulaError::Arity { .. })),
                "{formula}"
            );
        }
    }

    #[test]
    fn comparisons_and_boolean_logic_work() {
        assert_eq!(
            evaluate("[Qty] >= 3 AND [Active]", &context()).expect("logic"),
            Value::Boolean(true)
        );
        assert_eq!(
            evaluate("NOT ([Name] = \"Ada\")", &context()).expect("logic"),
            Value::Boolean(false)
        );
    }

    #[test]
    fn evaluation_accepts_the_depth_limit_and_rejects_the_next_level() {
        let mut expression = Expr::Literal(Value::Integer(2));
        for _ in 0..slab_core::MAX_EXPRESSION_DEPTH {
            expression = Expr::Unary {
                op: UnaryOp::Neg,
                expr: Box::new(expression),
            };
        }
        let context = context();
        assert_eq!(
            evaluate_expression(&expression, &context).expect("at depth limit"),
            Value::Integer(2)
        );
        let deeper = Expr::Unary {
            op: UnaryOp::Neg,
            expr: Box::new(expression),
        };
        assert!(matches!(
            evaluate_expression(&deeper, &context),
            Err(FormulaError::Depth { .. })
        ));
    }

    #[test]
    fn nested_operator_and_function_operands_count_toward_evaluation_depth() {
        let integer = |value| Expr::Literal(Value::Integer(value));
        let boolean = |value| Expr::Literal(Value::Boolean(value));
        let deep = |mut expression, op| {
            for _ in 0..slab_core::MAX_EXPRESSION_DEPTH {
                expression = Expr::Unary {
                    op,
                    expr: Box::new(expression),
                };
            }
            expression
        };
        let deep_number = || deep(integer(1), UnaryOp::Neg);
        let binary = |op, left, right| Expr::Binary {
            op,
            left: Box::new(left),
            right: Box::new(right),
        };
        let call = |name: &str, args| Expr::Call {
            name: name.into(),
            args,
        };
        let context = context();
        for expression in [
            binary(BinaryOp::Add, deep_number(), integer(1)),
            binary(BinaryOp::Add, integer(1), deep_number()),
            binary(
                BinaryOp::And,
                boolean(true),
                deep(boolean(true), UnaryOp::Not),
            ),
            binary(
                BinaryOp::Or,
                boolean(false),
                deep(boolean(true), UnaryOp::Not),
            ),
            call(
                "IF",
                vec![deep(boolean(true), UnaryOp::Not), integer(1), integer(0)],
            ),
            call("IF", vec![boolean(true), deep_number(), integer(0)]),
            call("IF", vec![boolean(false), integer(1), deep_number()]),
            call("COALESCE", vec![deep_number()]),
            call("ABS", vec![deep_number()]),
        ] {
            assert!(matches!(
                evaluate_expression(&expression, &context),
                Err(FormulaError::Depth { .. })
            ));
        }
    }
}
