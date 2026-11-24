//! Policy hooks for field-level redaction and access control, reserved for v1.2.

use crate::value::Value;

/// A hook the storage layer consults before returning a value.
///
/// The default implementation is a no-op. A future release will load policies
/// from the workspace; the trait is shipped now so the read path does not need
/// to change when that happens.
pub trait PolicyHook: Send + Sync {
    /// Returns a replacement value, or `None` to leave the value unchanged.
    fn redact(&self, table: &str, column: &str, value: &Value) -> Option<Value> {
        let _ = (table, column, value);
        None
    }

    /// Whether the caller may read the given table at all.
    fn can_read_table(&self, table: &str) -> bool {
        let _ = table;
        true
    }
}

/// A policy that permits everything and redacts nothing.
#[derive(Clone, Copy, Debug, Default)]
pub struct NoopPolicy;

impl PolicyHook for NoopPolicy {}
