//! Stable error codes and the core error enum.

use std::fmt;

/// Machine-readable error code carried across the IPC boundary.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ErrorCode(&'static str);

impl ErrorCode {
    /// Wraps a stable `SLAB_E_*` string.
    pub const fn new(code: &'static str) -> Self {
        Self(code)
    }

    /// The stable string.
    pub const fn as_str(self) -> &'static str {
        self.0
    }
}

impl fmt::Display for ErrorCode {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.0)
    }
}

/// Codes shared across crates.
pub mod codes {
    use super::ErrorCode;

    /// A decimal literal was malformed.
    pub const DECIMAL: ErrorCode = ErrorCode::new("SLAB_E_DECIMAL");
    /// A scale was outside the supported range.
    pub const SCALE: ErrorCode = ErrorCode::new("SLAB_E_SCALE");
    /// Integer arithmetic overflowed.
    pub const OVERFLOW: ErrorCode = ErrorCode::new("SLAB_E_NUMERIC_OVERFLOW");
    /// Division by zero.
    pub const DIV_ZERO: ErrorCode = ErrorCode::new("SLAB_E_DIV_ZERO");
    /// Currency mismatch.
    pub const CURRENCY: ErrorCode = ErrorCode::new("SLAB_E_CURRENCY");
    /// Invalid currency code.
    pub const CURRENCY_CODE: ErrorCode = ErrorCode::new("SLAB_E_CURRENCY_CODE");
    /// Wrong value type.
    pub const TYPE: ErrorCode = ErrorCode::new("SLAB_E_TYPE");
    /// Invalid identifier.
    pub const NAME: ErrorCode = ErrorCode::new("SLAB_E_NAME");
    /// Invalid layout.
    pub const LAYOUT: ErrorCode = ErrorCode::new("SLAB_E_LAYOUT");
    /// Invalid column.
    pub const COLUMN: ErrorCode = ErrorCode::new("SLAB_E_COLUMN");
    /// Invalid allocation.
    pub const ALLOCATION: ErrorCode = ErrorCode::new("SLAB_E_ALLOCATION");
    /// Invalid date.
    pub const DATE: ErrorCode = ErrorCode::new("SLAB_E_DATE");
    /// Formula syntax error.
    pub const FORMULA_PARSE: ErrorCode = ErrorCode::new("SLAB_E_FORMULA_PARSE");
    /// Formula type error.
    pub const FORMULA_TYPE: ErrorCode = ErrorCode::new("SLAB_E_FORMULA_TYPE");
    /// Formula reference error.
    pub const FORMULA_REF: ErrorCode = ErrorCode::new("SLAB_E_FORMULA_REF");
    /// Formula arity error.
    pub const FORMULA_ARITY: ErrorCode = ErrorCode::new("SLAB_E_FORMULA_ARITY");
    /// Formula nesting too deep.
    pub const FORMULA_DEPTH: ErrorCode = ErrorCode::new("SLAB_E_FORMULA_DEPTH");
    /// Formula evaluation error.
    pub const FORMULA_EVAL: ErrorCode = ErrorCode::new("SLAB_E_FORMULA_EVAL");
    /// Circular formula dependency.
    pub const CYCLE: ErrorCode = ErrorCode::new("SLAB_E_CYCLE");
    /// Storage failure.
    pub const STORAGE: ErrorCode = ErrorCode::new("SLAB_E_STORAGE");
    /// Foreign key violation.
    pub const FK_VIOLATION: ErrorCode = ErrorCode::new("SLAB_E_FK_VIOLATION");
    /// Unique constraint violation.
    pub const UNIQUE: ErrorCode = ErrorCode::new("SLAB_E_UNIQUE");
    /// Not found.
    pub const NOT_FOUND: ErrorCode = ErrorCode::new("SLAB_E_NOT_FOUND");
    /// Invalid query or filter.
    pub const QUERY: ErrorCode = ErrorCode::new("SLAB_E_QUERY");
    /// Import failure.
    pub const IMPORT: ErrorCode = ErrorCode::new("SLAB_E_IMPORT");
    /// Print failure.
    pub const PRINT: ErrorCode = ErrorCode::new("SLAB_E_PRINT");
    /// Workspace file failure.
    pub const WORKSPACE: ErrorCode = ErrorCode::new("SLAB_E_WORKSPACE");
    /// Nothing to undo or redo.
    pub const UNDO_EMPTY: ErrorCode = ErrorCode::new("SLAB_E_UNDO_EMPTY");
    /// A superseded interactive query was interrupted.
    pub const CANCELLED: ErrorCode = ErrorCode::new("SLAB_E_CANCELLED");
    /// Invalid command.
    pub const COMMAND: ErrorCode = ErrorCode::new("SLAB_E_COMMAND");
    /// Internal invariant violated.
    pub const INTERNAL: ErrorCode = ErrorCode::new("SLAB_E_INTERNAL");
    /// The operation is reserved for a later release.
    pub const UNSUPPORTED: ErrorCode = ErrorCode::new("SLAB_E_UNSUPPORTED");
}

/// Implemented by every crate error so the shell can report one shape.
pub trait SlabError: std::error::Error + Send + Sync {
    /// The stable machine-readable code.
    fn code(&self) -> ErrorCode;
}

/// Errors produced by pure domain operations.
#[derive(Debug, thiserror::Error)]
pub enum CoreError {
    /// A decimal string was malformed.
    #[error("invalid decimal literal: {input}")]
    InvalidDecimal {
        /// The rejected input.
        input: String,
    },
    /// A scale outside `0..=MAX_SCALE` was requested.
    #[error("decimal scale {scale} is outside 0..={max}")]
    ScaleOutOfRange {
        /// Requested scale.
        scale: u8,
        /// Maximum accepted scale.
        max: u8,
    },
    /// Integer arithmetic overflowed.
    #[error("numeric overflow while {operation}")]
    NumericOverflow {
        /// Description of the failed operation.
        operation: &'static str,
    },
    /// Division by zero.
    #[error("division by zero")]
    DivisionByZero,
    /// Two monetary values used different currencies.
    #[error("currency mismatch: {left} and {right}")]
    CurrencyMismatch {
        /// Left currency.
        left: String,
        /// Right currency.
        right: String,
    },
    /// A currency code was not three ASCII letters.
    #[error("invalid currency code: {0}")]
    InvalidCurrencyCode(String),
    /// A value did not match the expected type.
    #[error("expected {expected}, found {found}")]
    TypeMismatch {
        /// Expected type name.
        expected: &'static str,
        /// Actual type name.
        found: &'static str,
    },
    /// An identifier was empty, too long, reserved, or malformed.
    #[error("invalid identifier: {reason}")]
    InvalidName {
        /// Why the identifier was rejected.
        reason: String,
    },
    /// A layout referenced a missing table, column, or relation, or fell outside the grid.
    #[error("invalid layout: {reason}")]
    InvalidLayout {
        /// Why the layout was rejected.
        reason: String,
    },
    /// A column definition was incomplete or inconsistent.
    #[error("invalid column: {reason}")]
    InvalidColumn {
        /// Why the column was rejected.
        reason: String,
    },
    /// A monetary allocation could not preserve the total.
    #[error("invalid allocation: {reason}")]
    Allocation {
        /// Why the allocation was rejected.
        reason: String,
    },
    /// A date or timestamp was out of range or malformed.
    #[error("invalid date value: {0}")]
    InvalidDate(String),
}

impl SlabError for CoreError {
    fn code(&self) -> ErrorCode {
        match self {
            Self::InvalidDecimal { .. } => codes::DECIMAL,
            Self::ScaleOutOfRange { .. } => codes::SCALE,
            Self::NumericOverflow { .. } => codes::OVERFLOW,
            Self::DivisionByZero => codes::DIV_ZERO,
            Self::CurrencyMismatch { .. } => codes::CURRENCY,
            Self::InvalidCurrencyCode(_) => codes::CURRENCY_CODE,
            Self::TypeMismatch { .. } => codes::TYPE,
            Self::InvalidName { .. } => codes::NAME,
            Self::InvalidLayout { .. } => codes::LAYOUT,
            Self::InvalidColumn { .. } => codes::COLUMN,
            Self::Allocation { .. } => codes::ALLOCATION,
            Self::InvalidDate(_) => codes::DATE,
        }
    }
}

impl CoreError {
    /// Shorthand for the stable code string.
    pub fn code_str(&self) -> &'static str {
        self.code().as_str()
    }
}

/// Validates a table, column, relation, or layout identifier.
///
/// Identifiers start with an ASCII letter, contain ASCII letters, digits,
/// single spaces, or underscores, and do not end with a space. The reserved
/// system prefix and the primary key name are rejected.
pub fn validate_identifier(name: &str) -> Result<(), CoreError> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err(CoreError::InvalidName {
            reason: "name is empty".into(),
        });
    }
    if trimmed.len() > 64 {
        return Err(CoreError::InvalidName {
            reason: "name exceeds 64 bytes".into(),
        });
    }
    if trimmed != name {
        return Err(CoreError::InvalidName {
            reason: "name has leading or trailing whitespace".into(),
        });
    }
    if trimmed.starts_with(crate::SYSTEM_PREFIX) {
        return Err(CoreError::InvalidName {
            reason: format!("names beginning with {} are reserved", crate::SYSTEM_PREFIX),
        });
    }
    if trimmed.eq_ignore_ascii_case(crate::ROW_ID_COLUMN) {
        return Err(CoreError::InvalidName {
            reason: "id is the reserved primary key column".into(),
        });
    }
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return Err(CoreError::InvalidName {
            reason: "name is empty".into(),
        });
    };
    if !first.is_ascii_alphabetic() {
        return Err(CoreError::InvalidName {
            reason: "name must start with an ASCII letter".into(),
        });
    }
    if name.ends_with(' ') {
        return Err(CoreError::InvalidName {
            reason: "name must not end with a space".into(),
        });
    }
    for ch in chars {
        let valid = ch.is_ascii_alphanumeric() || ch == ' ' || ch == '_';
        if !valid {
            return Err(CoreError::InvalidName {
                reason: format!("character {ch:?} is not allowed"),
            });
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_reasonable_identifiers() {
        assert!(validate_identifier("Clients").is_ok());
        assert!(validate_identifier("Invoice Line Items").is_ok());
        assert!(validate_identifier("job_2026").is_ok());
    }

    #[test]
    fn rejects_system_and_reserved_names() {
        for name in ["", " id ", "id", "_slab_meta", "1clients", "Client.Notes"] {
            assert!(validate_identifier(name).is_err(), "{name} should fail");
        }
    }
}
