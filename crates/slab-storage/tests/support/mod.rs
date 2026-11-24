#![allow(dead_code)]

use std::time::{SystemTime, UNIX_EPOCH};

use slab_core::{Clock, Hlc};

/// Monotonic clock for integration tests.
pub(crate) struct TestClock {
    last: i64,
    counter: u32,
}

impl TestClock {
    pub(crate) fn new() -> Self {
        Self {
            last: 0,
            counter: 0,
        }
    }
}

impl Clock for TestClock {
    fn now(&mut self) -> Hlc {
        let wall = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| i64::try_from(duration.as_millis()).unwrap_or(i64::MAX))
            .unwrap_or(0)
            .max(self.last);
        if wall == self.last {
            self.counter = self.counter.saturating_add(1);
        } else {
            self.last = wall;
            self.counter = 0;
        }
        Hlc::new(self.last, self.counter, "test")
    }
}
