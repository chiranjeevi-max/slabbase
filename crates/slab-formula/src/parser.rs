//! Recursive-descent parser with bounded nesting.

use slab_core::{MAX_EXPRESSION_DEPTH, Value};

use crate::ast::{BinaryOp, Expr, UnaryOp};
use crate::error::FormulaError;
use crate::lexer::{Token, TokenKind, tokenize};

/// Hard ceiling on formula source size. The parser is iterative, but a huge
/// flat chain still allocates a token and node per operator before the depth
/// check runs; reject it before tokenizing.
const MAX_FORMULA_BYTES: usize = 16 * 1024;

/// Parses a formula into an expression tree.
pub fn parse(input: &str) -> Result<Expr, FormulaError> {
    if input.len() > MAX_FORMULA_BYTES {
        return Err(FormulaError::Parse {
            message: format!("formula exceeds {MAX_FORMULA_BYTES} bytes"),
            position: MAX_FORMULA_BYTES,
        });
    }
    let tokens = tokenize(input)?;
    let mut parser = Parser {
        tokens,
        index: 0,
        depth: 0,
    };
    let expression = parser.parse_expression()?;
    if !matches!(parser.peek(), TokenKind::Eof) {
        return Err(FormulaError::Parse {
            message: format!("unexpected {}", parser.peek().describe()),
            position: parser.position(),
        });
    }
    // Parenthesis and unary nesting is bounded while parsing, but a flat
    // operator chain (`1+1+…`) builds a left-deep tree iteratively. Enforce
    // the same depth bound on the finished tree before any recursive consumer
    // (typecheck, eval, rendering) or the drop path sees it.
    if expression.expression_depth() > MAX_EXPRESSION_DEPTH {
        return Err(FormulaError::Depth {
            limit: MAX_EXPRESSION_DEPTH,
        });
    }
    Ok(expression)
}

struct Parser {
    tokens: Vec<Token>,
    index: usize,
    depth: u32,
}

impl Parser {
    fn peek(&self) -> &TokenKind {
        self.tokens
            .get(self.index)
            .map(|token| &token.kind)
            .unwrap_or(&TokenKind::Eof)
    }

    fn position(&self) -> usize {
        self.tokens
            .get(self.index)
            .map(|token| token.position)
            .unwrap_or(0)
    }

    fn advance(&mut self) -> Token {
        let token = self.tokens.get(self.index).cloned().unwrap_or(Token {
            kind: TokenKind::Eof,
            position: 0,
        });
        self.index += 1;
        token
    }

    fn enter(&mut self) -> Result<(), FormulaError> {
        self.depth += 1;
        if self.depth > MAX_EXPRESSION_DEPTH {
            return Err(FormulaError::Depth {
                limit: MAX_EXPRESSION_DEPTH,
            });
        }
        Ok(())
    }

    fn leave(&mut self) {
        self.depth = self.depth.saturating_sub(1);
    }

    fn parse_expression(&mut self) -> Result<Expr, FormulaError> {
        self.parse_or()
    }

    fn parse_or(&mut self) -> Result<Expr, FormulaError> {
        let mut left = self.parse_and()?;
        while matches!(self.peek(), TokenKind::Or) {
            self.advance();
            let right = self.parse_and()?;
            left = Expr::Binary {
                op: BinaryOp::Or,
                left: Box::new(left),
                right: Box::new(right),
            };
        }
        Ok(left)
    }

    fn parse_and(&mut self) -> Result<Expr, FormulaError> {
        let mut left = self.parse_not()?;
        while matches!(self.peek(), TokenKind::And) {
            self.advance();
            let right = self.parse_not()?;
            left = Expr::Binary {
                op: BinaryOp::And,
                left: Box::new(left),
                right: Box::new(right),
            };
        }
        Ok(left)
    }

    fn parse_not(&mut self) -> Result<Expr, FormulaError> {
        if matches!(self.peek(), TokenKind::Not) {
            self.advance();
            self.enter()?;
            let expression = self.parse_not();
            self.leave();
            return Ok(Expr::Unary {
                op: UnaryOp::Not,
                expr: Box::new(expression?),
            });
        }
        self.parse_comparison()
    }

    fn parse_comparison(&mut self) -> Result<Expr, FormulaError> {
        let left = self.parse_additive()?;
        let op = match self.peek() {
            TokenKind::Eq => Some(BinaryOp::Eq),
            TokenKind::NotEq => Some(BinaryOp::NotEq),
            TokenKind::Lt => Some(BinaryOp::Lt),
            TokenKind::Lte => Some(BinaryOp::Lte),
            TokenKind::Gt => Some(BinaryOp::Gt),
            TokenKind::Gte => Some(BinaryOp::Gte),
            _ => None,
        };
        let Some(op) = op else {
            return Ok(left);
        };
        self.advance();
        let right = self.parse_additive()?;
        if matches!(
            self.peek(),
            TokenKind::Eq
                | TokenKind::NotEq
                | TokenKind::Lt
                | TokenKind::Lte
                | TokenKind::Gt
                | TokenKind::Gte
        ) {
            return Err(FormulaError::Parse {
                message: "comparison operators cannot be chained without parentheses".into(),
                position: self.position(),
            });
        }
        Ok(Expr::Binary {
            op,
            left: Box::new(left),
            right: Box::new(right),
        })
    }

    fn parse_additive(&mut self) -> Result<Expr, FormulaError> {
        let mut left = self.parse_multiplicative()?;
        loop {
            let op = match self.peek() {
                TokenKind::Plus => BinaryOp::Add,
                TokenKind::Minus => BinaryOp::Sub,
                _ => break,
            };
            self.advance();
            let right = self.parse_multiplicative()?;
            left = Expr::Binary {
                op,
                left: Box::new(left),
                right: Box::new(right),
            };
        }
        Ok(left)
    }

    fn parse_multiplicative(&mut self) -> Result<Expr, FormulaError> {
        let mut left = self.parse_unary()?;
        loop {
            let op = match self.peek() {
                TokenKind::Star => BinaryOp::Mul,
                TokenKind::Slash => BinaryOp::Div,
                TokenKind::Percent => BinaryOp::Rem,
                _ => break,
            };
            self.advance();
            let right = self.parse_unary()?;
            left = Expr::Binary {
                op,
                left: Box::new(left),
                right: Box::new(right),
            };
        }
        Ok(left)
    }

    fn parse_unary(&mut self) -> Result<Expr, FormulaError> {
        let op = match self.peek() {
            TokenKind::Minus => Some(UnaryOp::Neg),
            TokenKind::Plus => None,
            _ => return self.parse_primary(),
        };
        let had_operator = matches!(op, Some(UnaryOp::Neg));
        self.advance();
        self.enter()?;
        let expression = self.parse_unary();
        self.leave();
        let expression = expression?;
        if had_operator {
            Ok(Expr::Unary {
                op: UnaryOp::Neg,
                expr: Box::new(expression),
            })
        } else {
            Ok(expression)
        }
    }

    fn parse_primary(&mut self) -> Result<Expr, FormulaError> {
        self.enter()?;
        let result = self.parse_primary_inner();
        self.leave();
        result
    }

    fn parse_primary_inner(&mut self) -> Result<Expr, FormulaError> {
        let token = self.advance();
        match token.kind {
            TokenKind::Number(text) => parse_number(&text, token.position),
            TokenKind::String(text) => Ok(Expr::Literal(Value::Text(text))),
            TokenKind::True => Ok(Expr::Literal(Value::Boolean(true))),
            TokenKind::False => Ok(Expr::Literal(Value::Boolean(false))),
            TokenKind::Null => Ok(Expr::Literal(Value::Null)),
            TokenKind::LParen => {
                let expression = self.parse_expression()?;
                match self.advance().kind {
                    TokenKind::RParen => Ok(expression),
                    other => Err(FormulaError::Parse {
                        message: format!("expected ), found {}", other.describe()),
                        position: self.position(),
                    }),
                }
            }
            TokenKind::Ident(name) => {
                if matches!(self.peek(), TokenKind::LParen) {
                    return self.parse_call(name);
                }
                if matches!(self.peek(), TokenKind::Dot) {
                    self.advance();
                    let column = self.expect_name("column name")?;
                    return Ok(Expr::Column {
                        table: Some(name),
                        name: column,
                    });
                }
                Ok(Expr::Column { table: None, name })
            }
            other => Err(FormulaError::Parse {
                message: format!("expected an expression, found {}", other.describe()),
                position: token.position,
            }),
        }
    }

    fn parse_call(&mut self, name: String) -> Result<Expr, FormulaError> {
        self.advance();
        let mut args = Vec::new();
        if matches!(self.peek(), TokenKind::RParen) {
            self.advance();
            return Ok(Expr::Call {
                name: name.to_ascii_uppercase(),
                args,
            });
        }
        loop {
            args.push(self.parse_expression()?);
            match self.peek() {
                TokenKind::Comma => {
                    self.advance();
                }
                TokenKind::RParen => {
                    self.advance();
                    break;
                }
                other => {
                    return Err(FormulaError::Parse {
                        message: format!("expected , or ) in {name}(), found {}", other.describe()),
                        position: self.position(),
                    });
                }
            }
        }
        Ok(Expr::Call {
            name: name.to_ascii_uppercase(),
            args,
        })
    }

    fn expect_name(&mut self, label: &str) -> Result<String, FormulaError> {
        let token = self.advance();
        match token.kind {
            TokenKind::Ident(name) => Ok(name),
            other => Err(FormulaError::Parse {
                message: format!("expected {label}, found {}", other.describe()),
                position: token.position,
            }),
        }
    }
}

fn parse_number(text: &str, position: usize) -> Result<Expr, FormulaError> {
    if text.contains('.') {
        let decimal = slab_core::Decimal::parse(text).map_err(|error| FormulaError::Parse {
            message: error.to_string(),
            position,
        })?;
        return Ok(Expr::Literal(Value::Decimal(decimal)));
    }
    let integer = text.parse::<i64>().map_err(|_| FormulaError::Parse {
        message: format!("integer literal {text} is out of range"),
        position,
    })?;
    Ok(Expr::Literal(Value::Integer(integer)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn multiplication_binds_tighter_than_addition() {
        let expr = parse("1 + 2 * 3").expect("parse");
        assert_eq!(expr.to_string(), "(1 + (2 * 3))");
    }

    #[test]
    fn parentheses_override_precedence() {
        let expr = parse("(1 + 2) * 3").expect("parse");
        assert_eq!(expr.to_string(), "((1 + 2) * 3)");
    }

    #[test]
    fn parses_qualified_aggregates_and_calls() {
        let expr =
            parse("SUM([Line Items].[Amount]) / COUNT([Line Items].[Amount])").expect("parse");
        assert_eq!(
            expr.to_string(),
            "(SUM([Line Items].[Amount]) / COUNT([Line Items].[Amount]))"
        );
    }

    #[test]
    fn parses_strings_and_escapes() {
        let expr = parse(r#"CONCAT("a\"b", 'c\nd')"#).expect("parse");
        assert_eq!(expr.to_string(), "CONCAT(\"a\\\"b\", \"c\\nd\")");
    }

    #[test]
    fn rejects_unbalanced_parentheses() {
        assert!(parse("(1 + 2").is_err());
        assert!(parse("1 + 2)").is_err());
    }

    #[test]
    fn rejects_chained_comparisons() {
        assert!(parse("1 < 2 < 3").is_err());
    }

    #[test]
    fn rejects_excessive_nesting() {
        let deep = "(".repeat(200) + "1" + &")".repeat(200);
        assert!(parse(&deep).is_err());
    }
}
