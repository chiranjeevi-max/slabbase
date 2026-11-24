#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use slab_core::{CurrencyCode, Decimal, Money, RoundingMode};

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
