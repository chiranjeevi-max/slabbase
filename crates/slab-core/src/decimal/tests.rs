use super::*;

#[test]
fn powers_of_ten_check_the_i128_limit() {
    assert_eq!(
        pow10(38).expect("largest representable power of ten"),
        100_000_000_000_000_000_000_000_000_000_000_000_000
    );
    assert!(matches!(pow10(39), Err(CoreError::NumericOverflow { .. })));
}

#[test]
fn parses_and_formats_decimal() {
    let parsed = Decimal::parse("-1234.50").expect("valid");
    assert_eq!(parsed.units(), -123_450);
    assert_eq!(parsed.scale(), 2);
    assert_eq!(parsed.to_string(), "-1234.50");
    assert_eq!(Decimal::parse("0.005").expect("valid").to_string(), "0.005");
    assert_eq!(Decimal::parse(".5").expect("valid").to_string(), "0.5");
    assert_eq!(Decimal::parse(" +.50 ").expect("valid").to_string(), "0.50");
}

#[test]
fn rejects_exponents_and_embedded_signs_in_decimal_literals() {
    for input in ["1e3", "1E3", "1,000", "1.+5", "1.-5", "--1"] {
        assert!(matches!(
            Decimal::parse(input),
            Err(CoreError::InvalidDecimal { input: rejected }) if rejected == input
        ));
    }
}

#[test]
fn decimal_sign_is_independent_of_scale() {
    for (units, zero, negative) in [(25, false, false), (0, true, false), (-25, false, true)] {
        let decimal = Decimal::new(units, 2).expect("valid");
        assert_eq!(decimal.is_zero(), zero);
        assert_eq!(decimal.is_negative(), negative);
    }
}

#[test]
fn rescaling_moves_the_decimal_point_by_the_scale_difference() {
    for (input, target, expected) in [
        ("-1.25", 5, "-1.25000"),
        ("4.1234", 1, "4.1"),
        ("0.1", 9, "0.100000000"),
        ("1.00", 2, "1.00"),
    ] {
        let decimal = Decimal::parse(input).expect("valid");
        let scaled = decimal
            .rescale(target, RoundingMode::HalfEven)
            .expect("rescale");
        assert_eq!(scaled.to_string(), expected);
    }
}

#[test]
fn rescaling_rejects_an_unsupported_target_or_coefficient_overflow() {
    assert!(matches!(
        Decimal::from_integer(1).rescale(10, RoundingMode::HalfEven),
        Err(CoreError::ScaleOutOfRange { scale: 10, max: 9 })
    ));
    assert!(matches!(
        Decimal::from_integer(i64::MAX).rescale(1, RoundingMode::HalfEven),
        Err(CoreError::NumericOverflow { .. })
    ));
}

#[test]
fn normalization_removes_only_fractional_trailing_zeros() {
    for (input, expected) in [("-1.2500", "-1.25"), ("1200", "1200"), ("0.000", "0")] {
        let decimal = Decimal::parse(input).expect("valid");
        assert_eq!(decimal.normalized().to_string(), expected);
    }
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
    let divisor = Decimal::from_integer(20_000);
    for (dividend, expected) in [(1, "0.0000"), (3, "0.0002"), (-3, "-0.0002")] {
        let quotient = Decimal::from_integer(dividend)
            .checked_div(divisor)
            .expect("div");
        assert_eq!(quotient.to_string(), expected);
    }
}

#[test]
fn division_accounts_for_the_dividend_scale() {
    let dividend = Decimal::parse("0.25").expect("valid");
    let divisor = Decimal::parse("0.8").expect("valid");
    assert_eq!(
        dividend.checked_div(divisor).expect("divide").to_string(),
        "0.312500"
    );
}

#[test]
fn directed_rounding_accounts_for_both_operand_signs() {
    for (numerator, denominator, up, down) in [
        (5, 2, 3, 2),
        (-5, 2, -2, -3),
        (5, -2, -2, -3),
        (-5, -2, 3, 2),
    ] {
        assert_eq!(
            div_round(numerator, denominator, RoundingMode::Up).expect("round up"),
            up
        );
        assert_eq!(
            div_round(numerator, denominator, RoundingMode::Down).expect("round down"),
            down
        );
    }
}

#[test]
fn absolute_value_preserves_scale_and_checks_overflow() {
    for input in ["-2.50", "2.50"] {
        let decimal = Decimal::parse(input).expect("valid");
        assert_eq!(
            decimal.checked_abs().expect("absolute value").to_string(),
            "2.50"
        );
    }
    assert!(matches!(
        Decimal::from_integer(i64::MIN).checked_abs(),
        Err(CoreError::NumericOverflow { .. })
    ));
}

#[test]
fn coefficient_widens_the_signed_minor_units() {
    let decimal = Decimal::new(i64::MIN, 9).expect("valid");
    assert_eq!(decimal.coefficient(), -9_223_372_036_854_775_808);
}

#[test]
fn decimal_comparison_aligns_either_operand() {
    use std::cmp::Ordering::{Equal, Greater, Less};

    let one = Decimal::from_integer(1);
    let exact = Decimal::parse("1.00").expect("valid");
    let higher = Decimal::parse("1.01").expect("valid");
    assert_eq!(one.partial_cmp(&exact), Some(Equal));
    assert_eq!(exact.partial_cmp(&one), Some(Equal));
    assert_eq!(one.partial_cmp(&higher), Some(Less));
    assert_eq!(higher.partial_cmp(&one), Some(Greater));
}

#[test]
fn decimal_json_rejects_fractional_numbers_with_a_string_hint() {
    let error =
        serde_json::from_str::<Decimal>("0.15").expect_err("requires an exact decimal string");
    assert!(error.to_string().contains("a decimal string"));
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
    let money = Money::from_minor(CurrencyCode::parse("USD").expect("usd"), 100).expect("money");
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
fn scaled_equality_ignores_scale() {
    assert_eq!(
        Decimal::parse("1.0").expect("valid"),
        Decimal::parse("1.00").expect("valid")
    );
}
