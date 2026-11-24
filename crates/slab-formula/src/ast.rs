//! Expression tree and canonical rendering.

use std::fmt;

use slab_core::Value;

/// A unary operator.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnaryOp {
    /// Arithmetic negation.
    Neg,
    /// Logical negation.
    Not,
}

impl UnaryOp {
    /// The source spelling.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Neg => "-",
            Self::Not => "NOT",
        }
    }
}

/// A binary operator.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BinaryOp {
    /// Addition.
    Add,
    /// Subtraction.
    Sub,
    /// Multiplication.
    Mul,
    /// Division.
    Div,
    /// Remainder.
    Rem,
    /// Equality.
    Eq,
    /// Inequality.
    NotEq,
    /// Less than.
    Lt,
    /// Less than or equal.
    Lte,
    /// Greater than.
    Gt,
    /// Greater than or equal.
    Gte,
    /// Logical conjunction.
    And,
    /// Logical disjunction.
    Or,
}

impl BinaryOp {
    /// The source spelling.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Add => "+",
            Self::Sub => "-",
            Self::Mul => "*",
            Self::Div => "/",
            Self::Rem => "%",
            Self::Eq => "=",
            Self::NotEq => "!=",
            Self::Lt => "<",
            Self::Lte => "<=",
            Self::Gt => ">",
            Self::Gte => ">=",
            Self::And => "AND",
            Self::Or => "OR",
        }
    }
}

/// A parsed formula expression.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Expr {
    /// A literal value.
    Literal(Value),
    /// A column reference. `table` is only present for aggregate arguments.
    Column {
        /// Relation name, when qualified.
        table: Option<String>,
        /// Column name.
        name: String,
    },
    /// A unary operation.
    Unary {
        /// Operator.
        op: UnaryOp,
        /// Operand.
        expr: Box<Expr>,
    },
    /// A binary operation.
    Binary {
        /// Operator.
        op: BinaryOp,
        /// Left operand.
        left: Box<Expr>,
        /// Right operand.
        right: Box<Expr>,
    },
    /// A function call. Names are stored uppercase.
    Call {
        /// Function name.
        name: String,
        /// Arguments.
        args: Vec<Expr>,
    },
}

impl Expr {
    /// True when this is a qualified column reference.
    pub fn qualified_column(&self) -> Option<(&str, &str)> {
        match self {
            Self::Column {
                table: Some(table),
                name,
            } => Some((table, name)),
            _ => None,
        }
    }

    /// Collects bare (unqualified) column references in first-seen order.
    pub fn bare_columns(&self) -> Vec<String> {
        let mut found: Vec<String> = Vec::new();
        self.walk(&mut |expr| {
            if let Self::Column { table: None, name } = expr
                && !found.iter().any(|seen| seen == name)
            {
                found.push(name.clone());
            }
        });
        found
    }

    /// Collects qualified column references as `(relation, column)` pairs.
    pub fn qualified_columns(&self) -> Vec<(String, String)> {
        let mut found: Vec<(String, String)> = Vec::new();
        self.walk(&mut |expr| {
            if let Self::Column {
                table: Some(table),
                name,
            } = expr
                && !found
                    .iter()
                    .any(|(seen_table, seen_name)| seen_table == table && seen_name == name)
            {
                found.push((table.clone(), name.clone()));
            }
        });
        found
    }

    fn walk(&self, visit: &mut impl FnMut(&Expr)) {
        visit(self);
        match self {
            Self::Literal(_) | Self::Column { .. } => {}
            Self::Unary { expr, .. } => expr.walk(visit),
            Self::Binary { left, right, .. } => {
                left.walk(visit);
                right.walk(visit);
            }
            Self::Call { args, .. } => {
                for arg in args {
                    arg.walk(visit);
                }
            }
        }
    }
}

fn render_string(value: &str) -> String {
    let mut rendered = String::with_capacity(value.len() + 2);
    rendered.push('"');
    for ch in value.chars() {
        match ch {
            '"' => rendered.push_str("\\\""),
            '\\' => rendered.push_str("\\\\"),
            '\n' => rendered.push_str("\\n"),
            '\t' => rendered.push_str("\\t"),
            other => rendered.push(other),
        }
    }
    rendered.push('"');
    rendered
}

impl fmt::Display for Expr {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Literal(Value::Null) => formatter.write_str("NULL"),
            Self::Literal(Value::Text(text))
            | Self::Literal(Value::Select(text))
            | Self::Literal(Value::AttachmentRef(text)) => {
                formatter.write_str(&render_string(text))
            }
            Self::Literal(Value::Integer(value)) => write!(formatter, "{value}"),
            Self::Literal(Value::Decimal(value)) => write!(formatter, "{value}"),
            Self::Literal(Value::Boolean(value)) => {
                formatter.write_str(if *value { "TRUE" } else { "FALSE" })
            }
            Self::Literal(other) => formatter.write_str(&other.display_string()),
            Self::Column { table: None, name } => write!(formatter, "[{name}]"),
            Self::Column {
                table: Some(table),
                name,
            } => write!(formatter, "[{table}].[{name}]"),
            Self::Unary { op, expr } => write!(formatter, "({} {expr})", op.as_str()),
            Self::Binary { op, left, right } => {
                write!(formatter, "({left} {} {right})", op.as_str())
            }
            Self::Call { name, args } => {
                formatter.write_str(name)?;
                formatter.write_str("(")?;
                for (index, arg) in args.iter().enumerate() {
                    if index > 0 {
                        formatter.write_str(", ")?;
                    }
                    write!(formatter, "{arg}")?;
                }
                formatter.write_str(")")
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_canonical_text() {
        let expr = Expr::Binary {
            op: BinaryOp::Add,
            left: Box::new(Expr::Column {
                table: None,
                name: "Subtotal".into(),
            }),
            right: Box::new(Expr::Literal(Value::Decimal(
                slab_core::Decimal::parse("0.08").expect("decimal"),
            ))),
        };
        assert_eq!(expr.to_string(), "([Subtotal] + 0.08)");
    }
}
