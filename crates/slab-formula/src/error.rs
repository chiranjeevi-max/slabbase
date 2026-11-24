//! Errors for parsing, typechecking, ordering, and evaluating formulas.

use slab_core::error::codes;
use slab_core::{CoreError, ErrorCode, SlabError};

/// Errors produced by the formula engine.
#[derive(Debug, thiserror::Error)]
pub enum FormulaError {
    /// The expression did not parse.
    #[error("formula syntax error at byte {position}: {message}")]
    Parse {
        /// Human-readable reason.
        message: String,
        /// Byte offset in the source text.
        position: usize,
    },
    /// A table did not exist in the schema.
    #[error("unknown table: {0}")]
    UnknownTable(String),
    /// A bare column reference did not resolve.
    #[error("unknown column reference: {0}")]
    UnknownColumn(String),
    /// A relation or child column in an aggregate did not resolve.
    #[error("unknown relation or column: {relation}.{column}")]
    UnknownRelation {
        /// Relation name.
        relation: String,
        /// Child column name.
        column: String,
    },
    /// A function name was not recognized.
    #[error("unknown function: {0}")]
    UnknownFunction(String),
    /// A function was called with the wrong number of arguments.
    #[error("{function} expects {expected}, found {found}")]
    Arity {
        /// Function name.
        function: String,
        /// Expected description.
        expected: String,
        /// Actual count.
        found: usize,
    },
    /// The expression combined types that do not mix.
    #[error("formula type error: {0}")]
    Type(String),
    /// A formula dependency cycle exists.
    #[error("cyclic formula dependency: {path}")]
    Cycle {
        /// Deterministic cycle path.
        path: String,
    },
    /// The expression nested deeper than the parser allows.
    #[error("formula nesting exceeds {limit} levels")]
    Depth {
        /// Maximum nesting depth.
        limit: u32,
    },
    /// Evaluation failed against actual values.
    #[error("formula evaluation failed: {0}")]
    Eval(String),
    /// An exact arithmetic operation failed.
    #[error(transparent)]
    Core(#[from] CoreError),
}

impl SlabError for FormulaError {
    fn code(&self) -> ErrorCode {
        match self {
            Self::Parse { .. } => codes::FORMULA_PARSE,
            Self::UnknownTable(_) | Self::UnknownColumn(_) | Self::UnknownRelation { .. } => {
                codes::FORMULA_REF
            }
            Self::UnknownFunction(_) => codes::FORMULA_REF,
            Self::Arity { .. } => codes::FORMULA_ARITY,
            Self::Type(_) => codes::FORMULA_TYPE,
            Self::Cycle { .. } => codes::CYCLE,
            Self::Depth { .. } => codes::FORMULA_DEPTH,
            Self::Eval(_) => codes::FORMULA_EVAL,
            Self::Core(error) => error.code(),
        }
    }
}

impl FormulaError {
    /// The stable error code string.
    pub fn code_str(&self) -> &'static str {
        self.code().as_str()
    }
}
