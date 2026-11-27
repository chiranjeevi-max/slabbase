//! Tokenizer for the formula language.

use std::iter::{Enumerate, Peekable};
use std::str::Bytes;

use crate::error::FormulaError;

/// One lexical token.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Token {
    pub kind: TokenKind,
    pub position: usize,
}

/// Token kinds. Keywords are case-insensitive.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum TokenKind {
    Ident(String),
    String(String),
    Number(String),
    LParen,
    RParen,
    Comma,
    Dot,
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    Eq,
    NotEq,
    Lt,
    Lte,
    Gt,
    Gte,
    True,
    False,
    Null,
    And,
    Or,
    Not,
    Eof,
}

impl TokenKind {
    pub(crate) const fn describe(&self) -> &'static str {
        match self {
            Self::Ident(_) => "identifier",
            Self::String(_) => "string",
            Self::Number(_) => "number",
            Self::LParen => "(",
            Self::RParen => ")",
            Self::Comma => ",",
            Self::Dot => ".",
            Self::Plus => "+",
            Self::Minus => "-",
            Self::Star => "*",
            Self::Slash => "/",
            Self::Percent => "%",
            Self::Eq => "=",
            Self::NotEq => "!=",
            Self::Lt => "<",
            Self::Lte => "<=",
            Self::Gt => ">",
            Self::Gte => ">=",
            Self::True => "TRUE",
            Self::False => "FALSE",
            Self::Null => "NULL",
            Self::And => "AND",
            Self::Or => "OR",
            Self::Not => "NOT",
            Self::Eof => "end of formula",
        }
    }
}

type SourceBytes<'a> = Peekable<Enumerate<Bytes<'a>>>;

/// Converts source text into tokens, ending with `Eof`.
pub(crate) fn tokenize(input: &str) -> Result<Vec<Token>, FormulaError> {
    let mut bytes = input.bytes().enumerate().peekable();
    let mut tokens = Vec::new();
    while let Some((start, byte)) = bytes.next() {
        let kind = match byte {
            byte if byte.is_ascii_whitespace() => continue,
            b'(' => TokenKind::LParen,
            b')' => TokenKind::RParen,
            b'[' => {
                let Some((end, _)) = bytes.find(|&(_, byte)| byte == b']') else {
                    return Err(FormulaError::Parse {
                        message: "unterminated [column] reference".into(),
                        position: start,
                    });
                };
                let name = &input[start + 1..end];
                if name.is_empty() {
                    return Err(FormulaError::Parse {
                        message: "empty [column] reference".into(),
                        position: start,
                    });
                }
                TokenKind::Ident(name.to_owned())
            }
            b',' => TokenKind::Comma,
            b'.' => TokenKind::Dot,
            b'+' => TokenKind::Plus,
            b'-' => TokenKind::Minus,
            b'*' => TokenKind::Star,
            b'/' => TokenKind::Slash,
            b'%' => TokenKind::Percent,
            b'=' => {
                consume_byte(&mut bytes, b'=');
                TokenKind::Eq
            }
            b'!' => {
                if !consume_byte(&mut bytes, b'=') {
                    return Err(FormulaError::Parse {
                        message: "expected !=".into(),
                        position: start,
                    });
                }
                TokenKind::NotEq
            }
            b'<' => {
                if consume_byte(&mut bytes, b'=') {
                    TokenKind::Lte
                } else if consume_byte(&mut bytes, b'>') {
                    TokenKind::NotEq
                } else {
                    TokenKind::Lt
                }
            }
            b'>' => {
                if consume_byte(&mut bytes, b'=') {
                    TokenKind::Gte
                } else {
                    TokenKind::Gt
                }
            }
            quote @ (b'"' | b'\'') => {
                TokenKind::String(tokenize_string(input, &mut bytes, start, quote)?)
            }
            byte if byte.is_ascii_digit() => {
                while bytes.next_if(|&(_, byte)| byte.is_ascii_digit()).is_some() {}
                if consume_byte(&mut bytes, b'.') {
                    while bytes.next_if(|&(_, byte)| byte.is_ascii_digit()).is_some() {}
                }
                let end = bytes.peek().map_or(input.len(), |&(index, _)| index);
                TokenKind::Number(input[start..end].to_owned())
            }
            byte if byte.is_ascii_alphabetic() || byte == b'_' => {
                while bytes
                    .next_if(|&(_, byte)| byte.is_ascii_alphanumeric() || byte == b'_')
                    .is_some()
                {}
                let end = bytes.peek().map_or(input.len(), |&(index, _)| index);
                let word = &input[start..end];
                match word.to_ascii_uppercase().as_str() {
                    "AND" => TokenKind::And,
                    "OR" => TokenKind::Or,
                    "NOT" => TokenKind::Not,
                    "TRUE" => TokenKind::True,
                    "FALSE" => TokenKind::False,
                    "NULL" => TokenKind::Null,
                    _ => TokenKind::Ident(word.to_owned()),
                }
            }
            other => {
                return Err(FormulaError::Parse {
                    message: format!("unexpected character {:?}", other as char),
                    position: start,
                });
            }
        };
        tokens.push(Token {
            kind,
            position: start,
        });
    }
    tokens.push(Token {
        kind: TokenKind::Eof,
        position: input.len(),
    });
    Ok(tokens)
}

fn consume_byte(bytes: &mut SourceBytes<'_>, expected: u8) -> bool {
    bytes.next_if(|&(_, byte)| byte == expected).is_some()
}

fn tokenize_string(
    input: &str,
    bytes: &mut SourceBytes<'_>,
    start: usize,
    quote: u8,
) -> Result<String, FormulaError> {
    let mut value = String::new();
    let mut segment_start = start + 1;
    while let Some((position, byte)) = bytes.next() {
        // The ASCII quote and backslash delimit complete UTF-8 segments.
        if byte == quote {
            value.push_str(&input[segment_start..position]);
            return Ok(value);
        }
        if byte == b'\\' {
            value.push_str(&input[segment_start..position]);
            let Some((position, escaped)) = bytes.next() else {
                return Err(FormulaError::Parse {
                    message: "unterminated escape sequence".into(),
                    position: start,
                });
            };
            let escaped = match escaped {
                b'n' => '\n',
                b't' => '\t',
                b'\\' => '\\',
                b'"' => '"',
                b'\'' => '\'',
                other => {
                    return Err(FormulaError::Parse {
                        message: format!("unsupported escape \\{}", other as char),
                        position,
                    });
                }
            };
            value.push(escaped);
            segment_start = position + 1;
        }
    }
    Err(FormulaError::Parse {
        message: "unterminated string literal".into(),
        position: start,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokenizes_keywords_case_insensitively() {
        let tokens = tokenize("and AND Or true NULL").expect("tokens");
        assert!(matches!(tokens[0].kind, TokenKind::And));
        assert!(matches!(tokens[1].kind, TokenKind::And));
        assert!(matches!(tokens[2].kind, TokenKind::Or));
        assert!(matches!(tokens[3].kind, TokenKind::True));
        assert!(matches!(tokens[4].kind, TokenKind::Null));
    }

    #[test]
    fn tokenizes_bracket_names_with_spaces() {
        let tokens = tokenize("[Invoice Line Items].[Unit Price]").expect("tokens");
        assert_eq!(
            tokens[0].kind,
            TokenKind::Ident("Invoice Line Items".into())
        );
        assert_eq!(tokens[1].kind, TokenKind::Dot);
        assert_eq!(tokens[2].kind, TokenKind::Ident("Unit Price".into()));
    }

    #[test]
    fn rejects_unterminated_strings() {
        assert!(tokenize("\"abc").is_err());
    }

    #[test]
    fn tokenizes_punctuation_and_comparison_operators() {
        use TokenKind::{
            Comma, Dot, Eof, Eq, Gt, Gte, LParen, Lt, Lte, Minus, NotEq, Percent, Plus, RParen,
            Slash, Star,
        };

        for (input, expected) in [
            (
                " ( ) , . + - * / % ",
                vec![
                    (1, LParen),
                    (3, RParen),
                    (5, Comma),
                    (7, Dot),
                    (9, Plus),
                    (11, Minus),
                    (13, Star),
                    (15, Slash),
                    (17, Percent),
                ],
            ),
            (
                " = == != < <= <> > >=",
                vec![
                    (1, Eq),
                    (3, Eq),
                    (6, NotEq),
                    (9, Lt),
                    (11, Lte),
                    (14, NotEq),
                    (17, Gt),
                    (19, Gte),
                ],
            ),
            ("=", vec![(0, Eq)]),
            ("<", vec![(0, Lt)]),
            (">", vec![(0, Gt)]),
        ] {
            let mut expected: Vec<_> = expected
                .into_iter()
                .map(|(position, kind)| Token { kind, position })
                .collect();
            expected.push(Token {
                kind: Eof,
                position: input.len(),
            });
            assert_eq!(tokenize(input).expect("tokens"), expected, "{input:?}");
        }
    }

    #[test]
    fn preserves_unicode_bracket_names_strings_and_byte_offsets() {
        let input = r#"[Café ☕]+"été\n\t\\\"\'fin""#;
        assert_eq!(
            tokenize(input).expect("tokens"),
            vec![
                Token {
                    kind: TokenKind::Ident("Café ☕".into()),
                    position: 0,
                },
                Token {
                    kind: TokenKind::Plus,
                    position: 11,
                },
                Token {
                    kind: TokenKind::String("été\n\t\\\"'fin".into()),
                    position: 12,
                },
                Token {
                    kind: TokenKind::Eof,
                    position: input.len(),
                },
            ],
        );
    }

    #[test]
    fn malformed_tokens_report_their_byte_offsets() {
        for (input, expected_position, expected_message) in [
            (" [", 1, "unterminated [column] reference"),
            (" []", 1, "empty [column] reference"),
            (" !", 1, "expected !="),
            (" !3", 1, "expected !="),
            (" 'é", 1, "unterminated string literal"),
            (" 'é\\", 1, "unterminated escape sequence"),
            (r" 'é\q'", 5, "unsupported escape \\q"),
        ] {
            let error = tokenize(input).expect_err("malformed token");
            assert!(
                matches!(error, FormulaError::Parse { position, ref message }
                    if position == expected_position && message == expected_message),
                "{input:?} produced {error:?}"
            );
        }
    }
}
