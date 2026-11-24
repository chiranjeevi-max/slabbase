//! Sync extension seam.
//!
//! v1.0 ships a hybrid logical clock, a change set shape, and a transport
//! trait. There is no network implementation. The clock is wired into the
//! storage engine so audit entries are already ordered for a future transport.

use std::collections::VecDeque;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use slab_core::error::codes;
use slab_core::{Clock, ErrorCode, Hlc, RowId, SlabError};

/// A single change carried by a change set.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Change {
    /// Table name.
    pub table: String,
    /// Row id, when the change affects a row.
    pub row_id: Option<RowId>,
    /// Operation label.
    pub operation: String,
    /// JSON before image.
    pub before: Option<serde_json::Value>,
    /// JSON after image.
    pub after: Option<serde_json::Value>,
    /// Clock reading for the change.
    pub hlc: Hlc,
}

/// An ordered batch of changes from one origin.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ChangeSet {
    /// Identifier assigned by the origin.
    pub id: String,
    /// Origin node.
    pub origin: String,
    /// Clock reading of the first change.
    pub base: Option<Hlc>,
    /// Clock reading of the last change.
    pub head: Hlc,
    /// Changes in order.
    pub changes: Vec<Change>,
}

/// Acknowledgement returned by a transport push.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SyncAck {
    /// Clock reading the receiver has accepted.
    pub head: Hlc,
    /// Number of changes applied.
    pub applied: usize,
}

/// A transport that can push and pull change sets.
pub trait SyncTransport {
    /// Pushes a change set to the remote and returns its accepted position.
    fn push(&mut self, changes: &ChangeSet) -> Result<SyncAck, SyncError>;

    /// Pulls change sets newer than `since`, up to `limit`.
    fn pull(&mut self, since: Option<&Hlc>, limit: usize) -> Result<Vec<ChangeSet>, SyncError>;
}

/// A transport used before a network implementation exists. It rejects pushes.
#[derive(Clone, Copy, Debug, Default)]
pub struct NoopTransport;

impl SyncTransport for NoopTransport {
    fn push(&mut self, _changes: &ChangeSet) -> Result<SyncAck, SyncError> {
        Err(SyncError::Unavailable)
    }

    fn pull(&mut self, _since: Option<&Hlc>, _limit: usize) -> Result<Vec<ChangeSet>, SyncError> {
        Ok(Vec::new())
    }
}

/// An in-process transport used by tests and dry runs.
#[derive(Clone, Debug, Default)]
pub struct LoopbackTransport {
    sets: VecDeque<ChangeSet>,
}

impl LoopbackTransport {
    /// Creates an empty loopback transport.
    pub fn new() -> Self {
        Self::default()
    }

    /// The change sets currently held.
    pub fn sets(&self) -> &VecDeque<ChangeSet> {
        &self.sets
    }
}

impl SyncTransport for LoopbackTransport {
    fn push(&mut self, changes: &ChangeSet) -> Result<SyncAck, SyncError> {
        self.sets.push_back(changes.clone());
        Ok(SyncAck {
            head: changes.head.clone(),
            applied: changes.changes.len(),
        })
    }

    fn pull(&mut self, since: Option<&Hlc>, limit: usize) -> Result<Vec<ChangeSet>, SyncError> {
        let mut result = Vec::new();
        for set in &self.sets {
            if result.len() >= limit {
                break;
            }
            if since.is_some_and(|since| &set.head <= since) {
                continue;
            }
            result.push(set.clone());
        }
        Ok(result)
    }
}

/// A hybrid logical clock that tracks a local node and observes remote reads.
#[derive(Clone, Debug)]
pub struct HybridLogicalClock {
    node: String,
    last: Hlc,
}

impl HybridLogicalClock {
    /// Creates a clock for a node. The node name should be stable per device.
    pub fn new(node: impl Into<String>) -> Self {
        Self {
            node: node.into(),
            last: Hlc::new(0, 0, ""),
        }
    }

    /// The node name.
    pub fn node(&self) -> &str {
        &self.node
    }

    /// Observes a remote reading, moving the local clock past it.
    pub fn observe(&mut self, remote: &Hlc) {
        let physical = physical_now();
        let wall = physical.max(remote.wall_ms).max(self.last.wall_ms);
        let counter = if wall == self.last.wall_ms && wall == remote.wall_ms {
            self.last.counter.max(remote.counter).saturating_add(1)
        } else if wall == self.last.wall_ms {
            self.last.counter.saturating_add(1)
        } else if wall == remote.wall_ms {
            remote.counter.saturating_add(1)
        } else {
            0
        };
        self.last = Hlc::new(wall, counter, self.node.clone());
    }
}

impl Default for HybridLogicalClock {
    fn default() -> Self {
        Self::new("local")
    }
}

impl Clock for HybridLogicalClock {
    fn now(&mut self) -> Hlc {
        let physical = physical_now();
        if physical > self.last.wall_ms {
            self.last = Hlc::new(physical, 0, self.node.clone());
        } else {
            self.last = Hlc::new(
                self.last.wall_ms,
                self.last.counter.saturating_add(1),
                self.node.clone(),
            );
        }
        self.last.clone()
    }
}

fn physical_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| i64::try_from(duration.as_millis()).unwrap_or(i64::MAX))
        .unwrap_or(0)
}

/// Errors produced by a transport.
#[derive(Debug, thiserror::Error)]
pub enum SyncError {
    /// No transport is configured.
    #[error("no sync transport is configured")]
    Unavailable,
    /// The remote rejected the change set.
    #[error("remote rejected the change set: {0}")]
    Rejected(String),
    /// The remote is unreachable.
    #[error("remote is unreachable: {0}")]
    Unreachable(String),
}

impl SlabError for SyncError {
    fn code(&self) -> ErrorCode {
        match self {
            Self::Unavailable => codes::UNSUPPORTED,
            Self::Rejected(_) => codes::COMMAND,
            Self::Unreachable(_) => codes::WORKSPACE,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clock_readings_are_strictly_increasing() {
        let mut clock = HybridLogicalClock::new("studio");
        let mut previous = clock.now();
        for _ in 0..1_000 {
            let next = clock.now();
            assert!(next > previous);
            previous = next;
        }
    }

    #[test]
    fn observing_a_future_reading_advances_the_clock() {
        let mut clock = HybridLogicalClock::new("studio");
        let remote = Hlc::new(4_000_000_000_000, 12, "laptop");
        clock.observe(&remote);
        let next = clock.now();
        assert!(next > remote);
        assert_eq!(next.node, "studio");
    }

    #[test]
    fn loopback_transport_round_trips() {
        let mut transport = LoopbackTransport::new();
        let change = Change {
            table: "Clients".into(),
            row_id: Some(1),
            operation: "insert".into(),
            before: None,
            after: Some(serde_json::json!({"Name": "Northlight Studio"})),
            hlc: Hlc::new(1, 0, "studio"),
        };
        let set = ChangeSet {
            id: "set-1".into(),
            origin: "studio".into(),
            base: None,
            head: change.hlc.clone(),
            changes: vec![change],
        };
        let ack = transport.push(&set).expect("push");
        assert_eq!(ack.applied, 1);
        let pulled = transport.pull(None, 10).expect("pull");
        assert_eq!(pulled.len(), 1);
        let empty = transport.pull(Some(&set.head), 10).expect("pull");
        assert!(empty.is_empty());
    }

    #[test]
    fn noop_transport_rejects_push() {
        let mut transport = NoopTransport;
        let set = ChangeSet {
            id: "set".into(),
            origin: "studio".into(),
            base: None,
            head: Hlc::new(1, 0, "studio"),
            changes: Vec::new(),
        };
        assert!(matches!(transport.push(&set), Err(SyncError::Unavailable)));
    }
}
