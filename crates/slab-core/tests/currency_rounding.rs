#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use slab_core::{CoreError, CurrencyCode, Decimal, Money, RoundingMode};

struct Generator(u64);

impl Generator {
    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.0
    }

    fn amount(&mut self, limit: i64) -> i64 {
        let value = i64::try_from(self.next() % u64::try_from(limit).unwrap_or(1)).unwrap_or(0);
        if self.next() & 1 == 0 { value } else { -value }
    }
}

#[test]
fn ten_thousand_random_currency_sums_are_exact() {
    let currency = CurrencyCode::parse("USD").unwrap();
    let mut generator = Generator(0x5EED_1234_ABCD_9876);
    for _ in 0..10_000 {
        let count = 1 + (generator.next() % 40) as usize;
        let mut minor: Vec<i64> = Vec::with_capacity(count);
        let mut expected: i128 = 0;
        for _ in 0..count {
            let value = generator.amount(1_000_000_000);
            expected += i128::from(value);
            minor.push(value);
        }
        let total = Money::checked_sum(
            minor
                .iter()
                .map(|value| Money::from_minor(currency, *value).unwrap()),
        )
        .unwrap()
        .unwrap();
        assert_eq!(i128::from(total.minor_units()), expected);
        let parts = total.allocate(&[1, 2, 3, 4, 5]).unwrap();
        let recombined = Money::checked_sum(parts.iter().copied()).unwrap().unwrap();
        assert_eq!(recombined, total);
    }
}

#[test]
fn decimal_addition_is_associative_on_integer_units() {
    let mut generator = Generator(0xDEAD_BEEF_1234_5678);
    for _ in 0..10_000 {
        let a = Decimal::new(generator.amount(100_000_000), 3).unwrap();
        let b = Decimal::new(generator.amount(100_000_000), 5).unwrap();
        let c = Decimal::new(generator.amount(100_000_000), 2).unwrap();
        let left = a.checked_add(b).unwrap().checked_add(c).unwrap();
        let right = a.checked_add(b.checked_add(c).unwrap()).unwrap();
        assert_eq!(left, right);
    }
}

#[test]
fn half_even_rounding_breaks_ties_to_the_even_neighbour() {
    let currency = CurrencyCode::parse("USD").unwrap();
    let cases = [
        ("0.005", "0.00"),
        ("0.015", "0.02"),
        ("0.025", "0.02"),
        ("2.675", "2.68"),
        ("-0.005", "0.00"),
        ("-0.015", "-0.02"),
    ];
    for (input, expected) in cases {
        let money = Money::from_decimal(currency, Decimal::parse(input).unwrap()).unwrap();
        assert_eq!(money.amount().to_string(), expected, "input {input}");
    }
    let rounded = Decimal::parse("1.2345")
        .unwrap()
        .round(3, RoundingMode::HalfEven)
        .unwrap();
    assert_eq!(rounded.to_string(), "1.234");
}

#[test]
fn currency_codes_require_three_ascii_letters() {
    for input in ["US", "USDX", "U1D", "U€"] {
        assert!(matches!(
            CurrencyCode::parse(input),
            Err(CoreError::InvalidCurrencyCode(rejected)) if rejected == input
        ));
    }
}

#[test]
fn three_decimal_currencies_keep_their_minor_units_in_display_text() {
    let currency = CurrencyCode::parse(" kwd ").unwrap();
    let money = Money::from_minor(currency, 1_234).unwrap();
    assert_eq!(currency.to_string(), "KWD");
    assert_eq!(money.scale(), 3);
    assert_eq!(money.to_string(), "KWD 1.234");
    assert_eq!(Money::parse("kwd 1.234", currency).unwrap(), money);
}

#[test]
fn money_parsing_distinguishes_malformed_text_and_a_foreign_currency() {
    let usd = CurrencyCode::parse("USD").unwrap();
    for input in ["100 1.25", "USDX 1.25", "U1D 1.25"] {
        assert!(matches!(
            Money::parse(input, usd),
            Err(CoreError::InvalidDecimal { input: rejected }) if rejected == input
        ));
    }
    assert!(matches!(
        Money::parse("eur 1.25", usd),
        Err(CoreError::CurrencyMismatch { left, right }) if left == "EUR" && right == "USD"
    ));
}

#[test]
fn money_arithmetic_rejects_different_currencies() {
    let usd = Money::from_minor(CurrencyCode::parse("USD").unwrap(), 100).unwrap();
    let eur = Money::from_minor(CurrencyCode::parse("EUR").unwrap(), 100).unwrap();
    for result in [usd.checked_add(eur), usd.checked_sub(eur)] {
        assert!(matches!(
            result,
            Err(CoreError::CurrencyMismatch { left, right }) if left == "USD" && right == "EUR"
        ));
    }
}

#[test]
fn money_ratio_multiplication_rounds_and_rejects_zero_denominators() {
    let money = Money::from_minor(CurrencyCode::parse("USD").unwrap(), 103).unwrap();
    assert_eq!(money.checked_mul_ratio(1, 2).unwrap().minor_units(), 52);
    assert!(matches!(
        money.checked_mul_ratio(1, 0),
        Err(CoreError::DivisionByZero)
    ));
}

#[test]
fn allocation_awards_leftover_minor_units_to_the_largest_remainder() {
    let usd = CurrencyCode::parse("USD").unwrap();
    for (units, expected) in [(2, [0, 0, 1, 1]), (-2, [0, 0, -1, -1])] {
        let money = Money::from_minor(usd, units).unwrap();
        let shares = money.allocate(&[0, 1, 2, 3]).unwrap();
        let shares: Vec<i64> = shares.into_iter().map(Money::minor_units).collect();
        assert_eq!(shares, expected);
    }
}
