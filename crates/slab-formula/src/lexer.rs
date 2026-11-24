//! Tokenizer for the formula language.

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

/// Converts source text into tokens, ending with `Eof`.
pub(crate) fn tokenize(input: &str) -> Result<Vec<Token>, FormulaError> {
    let bytes = input.as_bytes();
    let mut tokens = Vec::new();
    let mut index = 0;
    while index < bytes.len() {
        let byte = bytes[index];
        if byte.is_ascii_whitespace() {
            index += 1;
            continue;
        }
        let start = index;
        let kind = match byte {
            b'(' => {
                index += 1;
                TokenKind::LParen
            }
            b')' => {
                index += 1;
                TokenKind::RParen
            }
            b'[' => {
                index += 1;
                let name_start = index;
                while index < bytes.len() && bytes[index] != b']' {
                    index += 1;
                }
                if index >= bytes.len() {
                    return Err(FormulaError::Parse {
                        message: "unterminated [column] reference".into(),
                        position: start,
                    });
                }
                let name = input[name_start..index].to_owned();
                index += 1;
                if name.is_empty() {
                    return Err(FormulaError::Parse {
                        message: "empty [column] reference".into(),
                        position: start,
                    });
                }
                TokenKind::Ident(name)
            }
            b',' => {
                index += 1;
                TokenKind::Comma
            }
            b'.' => {
                index += 1;
                TokenKind::Dot
            }
            b'+' => {
                index += 1;
                TokenKind::Plus
            }
            b'-' => {
                index += 1;
                TokenKind::Minus
            }
            b'*' => {
                index += 1;
                TokenKind::Star
            }
            b'/' => {
                index += 1;
                TokenKind::Slash
            }
            b'%' => {
                index += 1;
                TokenKind::Percent
            }
            b'=' => {
                index += 1;
                if index < bytes.len() && bytes[index] == b'=' {
                    index += 1;
                }
                TokenKind::Eq
            }
            b'!' => {
                index += 1;
                if index < bytes.len() && bytes[index] == b'=' {
                    index += 1;
                    TokenKind::NotEq
                } else {
                    return Err(FormulaError::Parse {
                        message: "expected !=".into(),
                        position: start,
                    });
                }
            }
            b'<' => {
                index += 1;
                if index < bytes.len() {
                    match bytes[index] {
                        b'=' => {
                            index += 1;
                            TokenKind::Lte
                        }
                        b'>' => {
                            index += 1;
                            TokenKind::NotEq
                        }
                        _ => TokenKind::Lt,
                    }
                } else {
                    TokenKind::Lt
                }
            }
            b'>' => {
                index += 1;
                if index < bytes.len() && bytes[index] == b'=' {
                    index += 1;
                    TokenKind::Gte
                } else {
                    TokenKind::Gt
                }
            }
            b'"' | b'\'' => {
                let quote = byte;
                index += 1;
                let mut value = String::new();
                let mut segment_start = index;
                loop {
                    if index >= bytes.len() {
                        return Err(FormulaError::Parse {
                            message: "unterminated string literal".into(),
                            position: start,
                        });
                    }
                    if bytes[index] == quote {
                        value.push_str(&input[segment_start..index]);
                        index += 1;
                        break;
                    }
                    if bytes[index] == b'\\' {
                        value.push_str(&input[segment_start..index]);
                        index += 1;
                        if index >= bytes.len() {
                            return Err(FormulaError::Parse {
                                message: "unterminated escape sequence".into(),
                                position: start,
                            });
                        }
                        let escaped = match bytes[index] {
                            b'n' => '\n',
                            b't' => '\t',
                            b'\\' => '\\',
                            b'"' => '"',
                            b'\'' => '\'',
                            other => {
                                return Err(FormulaError::Parse {
                                    message: format!("unsupported escape \\{}", other as char),
                                    position: index,
                                });
                            }
                        };
                        value.push(escaped);
                        index += 1;
                        segment_start = index;
                        continue;
                    }
                    index += 1;
                }
                TokenKind::String(value)
            }
            byte if byte.is_ascii_digit() => {
                while index < bytes.len() && bytes[index].is_ascii_digit() {
                    index += 1;
                }
                if index < bytes.len() && bytes[index] == b'.' {
                    index += 1;
                    while index < bytes.len() && bytes[index].is_ascii_digit() {
                        index += 1;
                    }
                }
                TokenKind::Number(input[start..index].to_owned())
            }
            byte if byte.is_ascii_alphabetic() || byte == b'_' => {
                while index < bytes.len()
                    && (bytes[index].is_ascii_alphanumeric() || bytes[index] == b'_')
                {
                    index += 1;
                }
                let word = &input[start..index];
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
}
