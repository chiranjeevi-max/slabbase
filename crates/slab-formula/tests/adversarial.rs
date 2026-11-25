#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use proptest::prelude::*;
use slab_core::{Aggregate, CurrencyCode, Decimal, Money, SlabDate, SlabTimestamp, Value};
use slab_formula::{
    BinaryOp, EvalContext, Expr, FormulaError, evaluate, evaluate_expression, parse,
    rewrite_column_source,
};

struct Context {
    values: Vec<(&'static str, Value)>,
}

impl Context {
    fn new(values: Vec<(&'static str, Value)>) -> Self {
        Self { values }
    }
}

impl EvalContext for Context {
    fn column(&self, name: &str) -> Option<Value> {
        self.values
            .iter()
            .find(|(column, _)| *column == name)
            .map(|(_, value)| value.clone())
    }

    fn aggregate(
        &self,
        _relation: &str,
        _column: &str,
        _aggregate: Aggregate,
    ) -> Result<Value, FormulaError> {
        Ok(Value::Null)
    }

    fn today(&self) -> SlabDate {
        SlabDate::from_ymd(2026, 11, 30).expect("date")
    }

    fn now(&self) -> SlabTimestamp {
        SlabTimestamp::from_parts(2026, 11, 30, 12, 0, 0, 0).expect("timestamp")
    }
}

fn eval(expression: &str, context: &Context) -> Result<Value, FormulaError> {
    evaluate(expression, context)
}

#[test]
fn integer_boundaries_are_structured_errors_not_wraps() {
    let context = Context::new(vec![
        ("max", Value::Integer(i64::MAX)),
        ("min", Value::Integer(i64::MIN)),
        ("one", Value::Integer(1)),
    ]);
    for expression in [
        "[max] + [one]",
        "[min] - [one]",
        "-[min]",
        "ABS([min])",
        "[max] * 2",
        "[min] % -1",
        "[min] / -1",
    ] {
        let result = eval(expression, &context);
        assert!(result.is_err(), "{expression} should fail, got {result:?}");
    }
    assert!(
        eval("[min] / -1", &context)
            .expect_err("overflow")
            .to_string()
            .contains("overflow")
    );
    assert_eq!(
        eval("1 / 0", &context).expect_err("zero").code_str(),
        "SLAB_E_DIV_ZERO"
    );
    assert_eq!(
        eval("1 % 0", &context).expect_err("zero").code_str(),
        "SLAB_E_DIV_ZERO"
    );
    assert!(eval("[min] % 2", &context).is_ok());
}

#[test]
fn null_propagation_and_division_by_zero_are_deterministic() {
    let context = Context::new(vec![("null", Value::Null), ("two", Value::Integer(2))]);
    assert_eq!(eval("[null] + [two]", &context).expect("null"), Value::Null);
    assert_eq!(eval("[null] * [two]", &context).expect("null"), Value::Null);
    assert_eq!(eval("[null] / [two]", &context).expect("null"), Value::Null);
    assert_eq!(eval("[two] / [null]", &context).expect("null"), Value::Null);
    assert_eq!(
        eval("COALESCE([null], 'fallback')", &context).expect("coalesce"),
        Value::Text("fallback".into())
    );
    assert_eq!(
        eval("IF([null], 1, 2)", &context).expect("if"),
        Value::Integer(2)
    );
}

#[test]
fn currency_overflow_and_mismatch_are_structured_errors() {
    let usd = CurrencyCode::parse("USD").expect("usd");
    let eur = CurrencyCode::parse("EUR").expect("eur");
    let context = Context::new(vec![
        (
            "max",
            Value::Currency(Money::from_minor(usd, i64::MAX).expect("money")),
        ),
        (
            "one",
            Value::Currency(Money::from_minor(usd, 1).expect("money")),
        ),
        (
            "eur",
            Value::Currency(Money::from_minor(eur, 1).expect("money")),
        ),
        (
            "zero",
            Value::Currency(Money::from_minor(usd, 0).expect("money")),
        ),
    ]);
    assert!(eval("[max] + [one]", &context).is_err());
    assert!(eval("[max] * 2", &context).is_err());
    assert!(eval("[one] + [eur]", &context).is_err());
    assert!(eval("[one] / [zero]", &context).is_err());
    assert_eq!(
        eval("[one] / 0", &context).expect_err("zero").code_str(),
        "SLAB_E_DIV_ZERO"
    );
    let negative = eval("-3.50", &Context::new(Vec::new())).expect("literal");
    assert_eq!(negative.display_string(), "-3.50");
    let money = Money::from_minor(usd, -350).expect("money");
    assert_eq!(money.amount().to_string(), "-3.50");
}

#[test]
fn deeply_nested_expression_is_rejected_without_stack_overflow() {
    let deep = "(".repeat(10_000) + "1" + &")".repeat(10_000);
    assert!(parse(&deep).is_err());
    let mut expression = Expr::Literal(Value::Integer(1));
    for _ in 0..500 {
        expression = Expr::Unary {
            op: slab_formula::UnaryOp::Neg,
            expr: Box::new(expression),
        };
    }
    let error =
        evaluate_expression(&expression, &Context::new(Vec::new())).expect_err("depth limit");
    assert!(matches!(error, FormulaError::Depth { .. }));
}

#[test]
fn token_span_rewrite_preserves_literals_formatting_and_other_names() {
    let source = "  [rate]*2  + [tax_rate] + LEN(\"rate\") + LEN(\"tax_rate\") ";
    let rewritten = rewrite_column_source(source, None, "rate", "unit_rate").expect("rewrite");
    assert_eq!(
        rewritten,
        "  [unit_rate]*2  + [tax_rate] + LEN(\"rate\") + LEN(\"tax_rate\") "
    );
    let qualified = rewrite_column_source(
        "SUM([Invoice Lines].[rate]) + SUM([Other Lines].[rate]) + [rate]",
        Some("Invoice Lines"),
        "rate",
        "unit_rate",
    )
    .expect("qualified rewrite");
    assert_eq!(
        qualified,
        "SUM([Invoice Lines].[unit_rate]) + SUM([Other Lines].[rate]) + [rate]"
    );
    let untouched = rewrite_column_source("SUM([Lines].[rate])", Some("Other"), "rate", "x")
        .expect("untouched");
    assert_eq!(untouched, "SUM([Lines].[rate])");
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(2_000))]

    #[test]
    fn arbitrary_formula_text_never_panics(input in any::<String>()) {
        let context = Context::new(vec![
            ("a", Value::Integer(7)),
            ("b", Value::Integer(-3)),
            ("text", Value::Text("Renée — €".into())),
        ]);
        let _ = evaluate(&input, &context);
        let _ = rewrite_column_source(&input, None, "a", "z");
    }

    #[test]
    fn adversarial_punctuation_never_panics(input in "[\\[\\](){}.,;:'\"*+\\-/%<>=!&|^_a-zA-Z0-9 ]{0,256}") {
        let context = Context::new(vec![("a", Value::Integer(1))]);
        let _ = evaluate(&input, &context);
    }
}

#[test]
fn nested_parentheses_within_limit_evaluate() {
    let expression = format!("{}1{}", "(".repeat(100), ")".repeat(100));
    assert_eq!(
        eval(&expression, &Context::new(Vec::new())).expect("evaluate"),
        Value::Integer(1)
    );
    let decimals = Decimal::parse("123456789.123456789").expect("decimal");
    assert_eq!(decimals.to_string(), "123456789.123456789");
    assert!(eval("9223372036854775808", &Context::new(Vec::new())).is_err());
    assert!(eval("1e10", &Context::new(Vec::new())).is_err());
    let _ = BinaryOp::Add;
}

#[test]
fn renaming_to_a_non_bare_name_brackets_the_reference() {
    let rewritten = slab_formula::rewrite_column_source("Qty * Price", None, "Price", "Unit Price")
        .expect("rewrite");
    assert_eq!(rewritten, "Qty * [Unit Price]");
    assert!(
        slab_formula::parse(&rewritten).is_ok(),
        "rewritten formula no longer parses"
    );

    let already_bracketed =
        slab_formula::rewrite_column_source("[Qty] * [Price]", None, "Price", "Unit Price")
            .expect("rewrite");
    assert_eq!(already_bracketed, "[Qty] * [Unit Price]");

    let keyword =
        slab_formula::rewrite_column_source("Qty * Price", None, "Price", "And").expect("rewrite");
    assert_eq!(keyword, "Qty * [And]");
    assert!(slab_formula::parse(&keyword).is_ok());

    let qualified = slab_formula::rewrite_column_source(
        "[Items].[Price]",
        Some("Items"),
        "Price",
        "Unit Price",
    )
    .expect("rewrite");
    assert_eq!(qualified, "[Items].[Unit Price]");
}
