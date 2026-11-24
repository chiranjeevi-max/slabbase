use std::error::Error;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use slab_core::{Clock, Hlc};
use slab_storage::Workspace;

struct FixtureClock {
    last: i64,
    counter: u32,
}

impl FixtureClock {
    fn new() -> Self {
        Self {
            last: 0,
            counter: 0,
        }
    }
}

impl Clock for FixtureClock {
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
        Hlc::new(self.last, self.counter, "fixture")
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    let path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "fixtures/design-studio.slab".to_owned());
    let path = Path::new(&path);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    for suffix in ["", "-wal", "-shm"] {
        let candidate = format!("{}{suffix}", path.display());
        match std::fs::remove_file(candidate) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(Box::new(error)),
        }
    }
    let mut workspace = Workspace::create(path, "Design Studio", Box::new(FixtureClock::new()))?;
    workspace.seed_studio()?;
    workspace.close()?;
    Ok(())
}
