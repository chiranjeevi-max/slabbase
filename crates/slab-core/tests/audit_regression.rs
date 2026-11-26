//! Regression tests for defects found during the failure-mode audit.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use slab_core::{CurrencyCode, Decimal, Money};

#[test]
fn divides_decimals_across_full_scale_spread() {
    let one = Decimal::parse("1").expect("valid");
    let tiny = Decimal::parse("0.000000001").expect("valid");
    let quotient = one.checked_div(tiny).expect("division must not panic");
    assert_eq!(quotient.to_string(), "1000000000.000000000");
}

#[test]
fn compares_large_decimals_with_different_scales_exactly() {
    let whole = Decimal::parse("10000000000").expect("valid");
    let fractional = Decimal::parse("9000000000.000000001").expect("valid");
    assert_ne!(whole, fractional);
    assert_eq!(whole.cmp(&fractional), std::cmp::Ordering::Greater);
}

#[test]
fn timestamp_parse_rejects_multibyte_text_without_panicking() {
    for input in [
        "2026-11-25T00:00:\u{1F525}",
        "2026-11-25T00:00:00.\u{1F525}",
        "2026-11-25T00:00:00.12\u{1F525}",
    ] {
        assert!(
            slab_core::SlabTimestamp::parse(input).is_err(),
            "{input:?} should be rejected"
        );
    }
}

#[test]
fn decimal_min_remainder_by_negative_one_is_exact() {
    let minimum = Decimal::new(i64::MIN, 0).expect("valid");
    let remainder = minimum
        .checked_rem(Decimal::from_integer(-1))
        .expect("remainder must not panic");
    assert_eq!(remainder, Decimal::from_integer(0));
}

fn usd(minor_units: i64) -> Money {
    Money::from_minor(CurrencyCode::parse("USD").expect("usd"), minor_units).expect("money")
}

#[test]
fn money_multiplication_rounds_once_at_the_money_scale() {
    let product = usd(3)
        .checked_mul_decimal(Decimal::parse("0.499999999").expect("quantity"))
        .expect("multiply");
    assert_eq!(
        product.minor_units(),
        1,
        "$0.03 * 0.499999999 must round to $0.01"
    );
}

#[test]
fn money_multiplication_does_not_false_overflow() {
    let product = usd(i64::MAX)
        .checked_mul_decimal(Decimal::parse("1.000000000").expect("quantity"))
        .expect("multiply");
    assert_eq!(product.minor_units(), i64::MAX);
}

#[test]
fn money_division_rounds_once_at_the_money_scale() {
    let quotient = usd(3)
        .checked_div_decimal(Decimal::parse("2.000000001").expect("divisor"))
        .expect("divide");
    assert_eq!(
        quotient.minor_units(),
        1,
        "$0.03 / 2.000000001 must round to $0.01"
    );
}

#[test]
fn remainder_widens_before_aligning() {
    let remainder = Decimal::from_integer(10_000_000_000)
        .checked_rem(Decimal::new(1, 9).expect("divisor"))
        .expect("remainder");
    assert_eq!(remainder, Decimal::from_integer(0));
}

#[test]
fn allocation_handles_the_minimum_amount() {
    let parts = usd(i64::MIN).allocate(&[1]).expect("allocate");
    assert_eq!(parts.len(), 1);
    assert_eq!(parts[0].minor_units(), i64::MIN);
}

#[test]
fn money_parses_display_text_and_rejects_a_foreign_code() {
    let currency = CurrencyCode::parse("USD").expect("usd");
    assert_eq!(
        Money::parse("USD 123.45", currency)
            .expect("display form")
            .minor_units(),
        12_345
    );
    assert_eq!(
        Money::parse("123.45", currency)
            .expect("bare amount")
            .minor_units(),
        12_345
    );
    assert!(Money::parse("EUR 123.45", currency).is_err());
    assert_eq!(
        Money::parse("1.005", currency)
            .expect("rounded")
            .minor_units(),
        100
    );
    assert!(
        Money::parse_exact("1.005", currency).is_err(),
        "an exact parse must reject sub-scale precision"
    );
}
