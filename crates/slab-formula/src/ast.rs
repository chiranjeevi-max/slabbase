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

    /// Rewrites column references in place. `table` is `None` for bare
    /// references and `Some(relation)` for aggregate references.
    pub fn rewrite_column(&mut self, table: Option<&str>, from: &str, to: &str) {
        let mut stack: Vec<&mut Expr> = vec![self];
        while let Some(node) = stack.pop() {
            match node {
                Self::Column { table: owner, name } => {
                    if owner.as_deref() == table && name == from {
                        *name = to.to_owned();
                    }
                }
                Self::Unary { expr, .. } => stack.push(expr),
                Self::Binary { left, right, .. } => {
                    stack.push(right);
                    stack.push(left);
                }
                Self::Call { args, .. } => stack.extend(args.iter_mut()),
                Self::Literal(_) => {}
            }
        }
    }

    fn walk(&self, visit: &mut impl FnMut(&Expr)) {
        // Iterative so a formula with a long operator chain cannot overflow
        // the stack while collecting references.
        let mut stack: Vec<&Expr> = vec![self];
        while let Some(node) = stack.pop() {
            visit(node);
            match node {
                Self::Literal(_) | Self::Column { .. } => {}
                Self::Unary { expr, .. } => stack.push(expr),
                Self::Binary { left, right, .. } => {
                    stack.push(right);
                    stack.push(left);
                }
                Self::Call { args, .. } => stack.extend(args.iter().rev()),
            }
        }
    }

    /// The maximum node depth of the tree, computed without recursion.
    pub(crate) fn expression_depth(&self) -> u32 {
        let mut maximum = 0;
        let mut stack: Vec<(&Expr, u32)> = vec![(self, 1)];
        while let Some((node, depth)) = stack.pop() {
            maximum = maximum.max(depth);
            let child_depth = depth.saturating_add(1);
            match node {
                Self::Literal(_) | Self::Column { .. } => {}
                Self::Unary { expr, .. } => stack.push((expr, child_depth)),
                Self::Binary { left, right, .. } => {
                    stack.push((right, child_depth));
                    stack.push((left, child_depth));
                }
                Self::Call { args, .. } => {
                    for arg in args {
                        stack.push((arg, child_depth));
                    }
                }
            }
        }
        maximum
    }

    /// Detaches every child into `stack`, leaving this node childless.
    fn drain_children(&mut self, stack: &mut Vec<Expr>) {
        match self {
            Self::Unary { expr, .. } => {
                stack.push(std::mem::replace(&mut **expr, Self::Literal(Value::Null)));
            }
            Self::Binary { left, right, .. } => {
                stack.push(std::mem::replace(&mut **left, Self::Literal(Value::Null)));
                stack.push(std::mem::replace(&mut **right, Self::Literal(Value::Null)));
            }
            Self::Call { args, .. } => stack.extend(std::mem::take(args)),
            Self::Literal(_) | Self::Column { .. } => {}
        }
    }
}

/// Drops the tree iteratively.
///
/// A flat expression such as `1+1+…` is built as a left-deep tree. The derived
/// recursive drop would exhaust the stack for a long formula (the parser is
/// iterative, so parsing itself succeeds). Dismantling the tree with an
/// explicit work list keeps drop O(n) and stack-constant; each node is dropped
/// only after its children have been detached, so its own drop is shallow.
impl Drop for Expr {
    fn drop(&mut self) {
        let mut stack: Vec<Expr> = Vec::new();
        self.drain_children(&mut stack);
        while let Some(mut node) = stack.pop() {
            node.drain_children(&mut stack);
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
    use crate::parse;

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

    #[test]
    fn column_references_are_deduplicated_in_first_seen_order() {
        let expression = parse(
            "IF(NOT [Hidden], [Rate] + [Rate] + [Qty], SUM([Lines].[Rate]) + SUM([Lines].[Qty]) + SUM([Other].[Rate]) + SUM([Lines].[Rate]))",
        )
        .expect("formula");

        assert_eq!(expression.bare_columns(), ["Hidden", "Rate", "Qty"]);
        assert_eq!(
            expression.qualified_columns(),
            [
                ("Lines".into(), "Rate".into()),
                ("Lines".into(), "Qty".into()),
                ("Other".into(), "Rate".into())
            ]
        );
    }

    #[test]
    fn rewriting_tree_changes_only_matching_owner_and_column() {
        let mut expression =
            parse("SUM([Lines].[Rate]) + SUM([Lines].[Qty]) + SUM([Other].[Rate]) + [Rate]")
                .expect("formula");

        expression.rewrite_column(Some("Lines"), "Rate", "Unit Rate");

        assert_eq!(
            expression,
            parse("SUM([Lines].[Unit Rate]) + SUM([Lines].[Qty]) + SUM([Other].[Rate]) + [Rate]")
                .expect("expected formula")
        );
    }

    #[test]
    fn dropping_a_deep_tree_uses_bounded_stack_space() {
        std::thread::Builder::new()
            .stack_size(128 * 1024)
            .spawn(|| {
                let mut expression = Expr::Literal(Value::Integer(0));
                for _ in 0..10_000 {
                    expression = Expr::Unary {
                        op: UnaryOp::Neg,
                        expr: Box::new(expression),
                    };
                }
                drop(expression);
            })
            .expect("start test thread")
            .join()
            .expect("drop expression");
    }
}
