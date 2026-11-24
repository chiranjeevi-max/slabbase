//! Hybrid logical clock value shared by storage and sync.

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::error::CoreError;

/// A hybrid logical clock reading: physical milliseconds, a logical counter,
/// and the node that produced it.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(
    feature = "ts",
    ts(export, export_to = "../../../apps/desktop/src/bindings/")
)]
pub struct Hlc {
    /// Physical time in Unix milliseconds.
    pub wall_ms: i64,
    /// Logical counter for events within the same millisecond.
    pub counter: u32,
    /// Node identifier, unique per device.
    pub node: String,
}

impl Hlc {
    /// Builds a clock reading.
    pub fn new(wall_ms: i64, counter: u32, node: impl Into<String>) -> Self {
        Self {
            wall_ms,
            counter,
            node: node.into(),
        }
    }

    /// Parses the textual form produced by `Display`.
    pub fn parse(input: &str) -> Result<Self, CoreError> {
        let mut pieces = input.splitn(3, '-');
        let wall = pieces.next().ok_or_else(|| invalid(input))?;
        let counter = pieces.next().ok_or_else(|| invalid(input))?;
        let node = pieces.next().ok_or_else(|| invalid(input))?;
        if node.is_empty() {
            return Err(invalid(input));
        }
        Ok(Self {
            wall_ms: wall.parse().map_err(|_| invalid(input))?,
            counter: counter.parse().map_err(|_| invalid(input))?,
            node: node.to_owned(),
        })
    }
}

fn invalid(input: &str) -> CoreError {
    CoreError::InvalidColumn {
        reason: format!("invalid HLC timestamp: {input}"),
    }
}

impl fmt::Display for Hlc {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{:016}-{:06}-{}",
            self.wall_ms, self.counter, self.node
        )
    }
}

/// A clock the storage engine uses to stamp audit rows.
///
/// The concrete hybrid logical clock lives in `slab-sync`; storage only needs
/// this trait, which keeps the dependency graph acyclic.
pub trait Clock: Send {
    /// Returns the next strictly increasing reading.
    fn now(&mut self) -> Hlc;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hlc_text_is_sortable() {
        let early = Hlc::new(1_700_000_000_000, 2, "studio");
        let late = Hlc::new(1_700_000_000_001, 0, "studio");
        assert!(early.to_string() < late.to_string());
        assert_eq!(Hlc::parse(&early.to_string()).expect("parse"), early);
    }
}
