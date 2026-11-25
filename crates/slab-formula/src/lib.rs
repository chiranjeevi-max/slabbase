#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

//! A small, bounded formula language for derived columns.
//!
//! The crate parses expressions, infers their type, evaluates them against
//! row values, and orders formula columns by dependency. It performs no I/O
//! and does not execute user text as code.

pub mod ast;
pub mod dag;
pub mod error;
pub mod eval;
pub mod lexer;
pub mod parser;
pub mod types;

pub use ast::{BinaryOp, Expr, UnaryOp};
pub use dag::{FormulaCheck, FormulaDag};
pub use error::FormulaError;
pub use eval::{EvalContext, evaluate, evaluate_expression};
pub use parser::parse;
pub use types::{FormulaType, TypeEnv, typecheck, validate_expression};

#[cfg(test)]
mod property_tests {
    use proptest::prelude::*;
    use slab_core::{Column, ColumnType, Schema, Table};

    use crate::ast::{BinaryOp, Expr, UnaryOp};
    use crate::{FormulaDag, parser, types};

    fn cyclic_schema(length: usize) -> Schema {
        let columns = (0..length)
            .map(|index| {
                let next = (index + 1) % length;
                Column {
                    id: format!("c{index}"),
                    table: "Cycle".into(),
                    name: format!("C{index}"),
                    ty: ColumnType::Formula {
                        expression: format!("[C{next}] + 1"),
                    },
                    required: false,
                    unique: false,
                    default: None,
                    position: u32::try_from(index).unwrap_or(0),
                }
            })
            .collect();
        Schema {
            tables: vec![Table {
                id: "t_cycle".into(),
                name: "Cycle".into(),
                columns,
                created_at: 0,
            }],
            relations: Vec::new(),
            layouts: Vec::new(),
        }
    }

    fn column_strategy() -> impl Strategy<Value = Expr> {
        prop::sample::select(vec!["Qty", "Price", "Subtotal", "Name", "Start"]).prop_map(|name| {
            Expr::Column {
                table: None,
                name: name.to_owned(),
            }
        })
    }

    fn expression_strategy() -> impl Strategy<Value = Expr> {
        let leaf = prop_oneof![
            (0_i64..=i64::MAX).prop_map(|value| Expr::Literal(slab_core::Value::Integer(value))),
            (1_i64..10_000).prop_map(|value| {
                Expr::Literal(slab_core::Value::Decimal(
                    slab_core::Decimal::new(value, 2).expect("scale"),
                ))
            }),
            "[a-zA-Z ]{0,12}".prop_map(|text| Expr::Literal(slab_core::Value::Text(text))),
            any::<bool>().prop_map(|value| Expr::Literal(slab_core::Value::Boolean(value))),
            column_strategy(),
        ];
        leaf.prop_recursive(5, 64, 6, |inner| {
            prop_oneof![
                inner.clone().prop_map(|expr| Expr::Unary {
                    op: UnaryOp::Neg,
                    expr: Box::new(expr),
                }),
                (inner.clone(), inner.clone()).prop_map(|(left, right)| Expr::Binary {
                    op: BinaryOp::Add,
                    left: Box::new(left),
                    right: Box::new(right),
                }),
                (inner.clone(), inner.clone()).prop_map(|(left, right)| Expr::Binary {
                    op: BinaryOp::Mul,
                    left: Box::new(left),
                    right: Box::new(right),
                }),
                (inner.clone(), inner.clone()).prop_map(|(condition, value)| Expr::Call {
                    name: "IF".into(),
                    args: vec![
                        Expr::Binary {
                            op: BinaryOp::Gt,
                            left: Box::new(condition),
                            right: Box::new(Expr::Literal(slab_core::Value::Integer(0))),
                        },
                        value,
                        Expr::Literal(slab_core::Value::Null),
                    ],
                }),
                inner.prop_map(|value| Expr::Call {
                    name: "CONCAT".into(),
                    args: vec![value, Expr::Literal(slab_core::Value::Text("x".into()))],
                }),
            ]
        })
    }

    proptest! {
        #[test]
        fn arbitrary_text_never_panics(input in ".{0,200}") {
            let _ = parser::parse(&input);
        }

        #[test]
        fn canonical_rendering_round_trips(expression in expression_strategy()) {
            let rendered = expression.to_string();
            let reparsed = parser::parse(&rendered).expect("canonical text should parse");
            prop_assert_eq!(reparsed, expression);
        }

        #[test]
        fn guaranteed_cycles_are_rejected_deterministically(length in 1_usize..=8) {
            let schema = cyclic_schema(length);
            let first = FormulaDag::build(&schema).expect_err("cycle must be rejected");
            let second = FormulaDag::build(&schema).expect_err("cycle must be rejected");
            prop_assert_eq!(first.code_str(), "SLAB_E_CYCLE");
            prop_assert_eq!(first.to_string(), second.to_string());
            let dag = FormulaDag::default();
            let check = dag.check(&schema, "Cycle", "C0", "[C0] + 1")
                .expect_err("proposed cycle must be rejected");
            prop_assert_eq!(check.code_str(), "SLAB_E_CYCLE");
            prop_assert_eq!(dag.formula_columns("Cycle").count(), 0);
        }

        #[test]
        fn typecheck_never_panics(input in ".{0,120}") {
            if let Ok(expression) = parser::parse(&input) {
                let env = types::TypeEnv::default()
                    .with_column("Qty", types::FormulaType::Integer)
                    .with_column("Price", types::FormulaType::Decimal);
                let _ = types::typecheck(&expression, &env);
            }
        }
    }

    #[test]
    fn every_cycle_length_is_rejected_before_mutation() {
        for length in 1..=8 {
            let schema = cyclic_schema(length);
            let error = FormulaDag::build(&schema).expect_err("cycle");
            assert_eq!(error.code_str(), "SLAB_E_CYCLE");
        }
    }
}
