//! Concurrent database state for the Tauri shell.
//!
//! Writes are serialized behind one writer mutex. Reads lease a read-only
//! `Workspace` from a bounded pool, so pagination, search, history, and PDF
//! reads run on their own WAL connections while an import or schema write is
//! still in flight. `Workspace` itself remains single-threaded; the shell owns
//! the synchronization boundary.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, RwLock};

use slab_storage::{QueryCancel, Workspace};
use slab_sync::HybridLogicalClock;

use crate::dto::WorkspaceInfoDto;
use crate::error::ApiError;

const READER_POOL_SIZE: usize = 4;

/// Handle to the shared workspace state.
pub struct DbHandle {
    writer: Mutex<Option<Workspace>>,
    readers: Mutex<Vec<Workspace>>,
    path: Mutex<Option<PathBuf>>,
    lifecycle: RwLock<()>,
    cancel: Mutex<Arc<QueryCancel>>,
    search_cancel: Mutex<Arc<QueryCancel>>,
    generation: AtomicU64,
    read_gate: ReadGate,
}

impl DbHandle {
    /// Creates an empty handle.
    pub fn new() -> Result<Self, ApiError> {
        Ok(Self {
            writer: Mutex::new(None),
            readers: Mutex::new(Vec::new()),
            path: Mutex::new(None),
            lifecycle: RwLock::new(()),
            cancel: Mutex::new(Arc::new(QueryCancel::new())),
            search_cancel: Mutex::new(Arc::new(QueryCancel::new())),
            generation: AtomicU64::new(0),
            read_gate: ReadGate::new(),
        })
    }

    /// The cancellation handle for the current workspace generation.
    ///
    /// Every open or create installs a fresh handle. Readers lease the handle
    /// they were opened with, so an interrupt aimed at a previous workspace can
    /// never reach the workspace now on screen.
    pub fn cancel_handle(&self) -> Arc<QueryCancel> {
        match self.cancel.lock() {
            Ok(guard) => Arc::clone(&guard),
            Err(poisoned) => Arc::clone(&poisoned.into_inner()),
        }
    }

    /// The search cancellation handle for the current workspace generation.
    pub fn search_cancel_handle(&self) -> Arc<QueryCancel> {
        match self.search_cancel.lock() {
            Ok(guard) => Arc::clone(&guard),
            Err(poisoned) => Arc::clone(&poisoned.into_inner()),
        }
    }

    /// Interrupts a superseded page query without taking the writer lock.
    pub fn cancel_query(&self) -> bool {
        self.cancel_handle().interrupt()
    }

    /// Interrupts only a superseded full-text search.
    ///
    /// Pages and portals use a separate handle, so a palette search can never
    /// cancel a grid read that is still loading.
    pub fn cancel_search_query(&self) -> bool {
        self.search_cancel_handle().interrupt()
    }

    fn install_cancels(&self, cancel: Arc<QueryCancel>, search_cancel: Arc<QueryCancel>) {
        *lock(&self.cancel) = cancel;
        *lock(&self.search_cancel) = search_cancel;
    }

    /// Installs a replacement writer and closes the outgoing one.
    ///
    /// The replacement is already built and its info was read; a failed
    /// checkpoint on the old workspace must not discard it or leave the path
    /// pointing at a writer-less workspace.
    fn replace_writer(&self, workspace: Workspace) {
        let previous = lock(&self.writer).replace(workspace);
        if let Some(previous) = previous {
            let _ = previous.close();
        }
    }

    /// Opens a workspace, replacing any current one.
    pub fn open(&self, path: String) -> Result<WorkspaceInfoDto, ApiError> {
        let _gate = self
            .lifecycle
            .write()
            .map_err(|_| ApiError::internal("workspace lifecycle lock is poisoned"))?;
        let mut workspace = Workspace::open(&path, Box::new(HybridLogicalClock::new("desktop")))
            .map_err(ApiError::from)?;
        let cancel = Arc::new(QueryCancel::new());
        let search_cancel = Arc::new(QueryCancel::new());
        workspace.set_cancel_handle(Arc::clone(&cancel));
        workspace.set_search_cancel_handle(Arc::clone(&search_cancel));
        let info = WorkspaceInfoDto::from(workspace.info().map_err(ApiError::from)?);
        self.clear_readers();
        *lock(&self.path) = Some(PathBuf::from(&path));
        self.install_cancels(Arc::clone(&cancel), Arc::clone(&search_cancel));
        self.replace_writer(workspace);
        self.fill_readers(Path::new(&path), &cancel, &search_cancel);
        Ok(info)
    }

    /// Creates a workspace, replacing any current one.
    pub fn create(&self, path: String, name: String) -> Result<WorkspaceInfoDto, ApiError> {
        let _gate = self
            .lifecycle
            .write()
            .map_err(|_| ApiError::internal("workspace lifecycle lock is poisoned"))?;
        // Build the replacement before tearing down the current workspace. A
        // refused create (an existing non-empty file, a bad directory) must
        // leave the workspace on screen open and usable.
        let mut workspace =
            Workspace::create(&path, &name, Box::new(HybridLogicalClock::new("desktop")))
                .map_err(ApiError::from)?;
        let cancel = Arc::new(QueryCancel::new());
        let search_cancel = Arc::new(QueryCancel::new());
        workspace.set_cancel_handle(Arc::clone(&cancel));
        workspace.set_search_cancel_handle(Arc::clone(&search_cancel));
        let info = WorkspaceInfoDto::from(workspace.info().map_err(ApiError::from)?);
        self.clear_readers();
        *lock(&self.path) = Some(PathBuf::from(&path));
        self.install_cancels(Arc::clone(&cancel), Arc::clone(&search_cancel));
        self.replace_writer(workspace);
        self.fill_readers(Path::new(&path), &cancel, &search_cancel);
        Ok(info)
    }

    /// Closes the current workspace.
    pub fn close(&self) -> Result<(), ApiError> {
        let _gate = self
            .lifecycle
            .write()
            .map_err(|_| ApiError::internal("workspace lifecycle lock is poisoned"))?;
        self.clear_readers();
        close_writer(&self.writer)?;
        *lock(&self.path) = None;
        self.install_cancels(Arc::new(QueryCancel::new()), Arc::new(QueryCancel::new()));
        Ok(())
    }

    /// Returns workspace info when one is open.
    pub fn info(&self) -> Result<Option<WorkspaceInfoDto>, ApiError> {
        let open = self
            .path
            .lock()
            .map_err(|_| ApiError::internal("workspace path lock is poisoned"))?
            .is_some();
        if !open {
            return Ok(None);
        }
        Ok(Some(self.read(|workspace| {
            Ok(WorkspaceInfoDto::from(workspace.info()?))
        })?))
    }

    /// Loads the sample workspace. It creates tables, so it excludes readers.
    pub fn load_sample(&self) -> Result<WorkspaceInfoDto, ApiError> {
        self.write_schema(|workspace| {
            workspace.seed_studio().map_err(ApiError::from)?;
            Ok(WorkspaceInfoDto::from(
                workspace.info().map_err(ApiError::from)?,
            ))
        })
    }

    /// Runs a read against a leased read-only workspace.
    pub fn read<T, F>(&self, work: F) -> Result<T, ApiError>
    where
        T: Send,
        F: FnOnce(&Workspace) -> Result<T, ApiError> + Send,
    {
        let _gate = self
            .lifecycle
            .read()
            .map_err(|_| ApiError::internal("workspace lifecycle lock is poisoned"))?;
        let _permit = self.read_gate.acquire();
        let path = self
            .path
            .lock()
            .map_err(|_| ApiError::internal("workspace path lock is poisoned"))?
            .clone()
            .ok_or_else(ApiError::no_workspace)?;
        let generation = self.generation.load(Ordering::Acquire);
        let workspace = self.checkout_reader(&path)?;
        let outcome = work(&workspace);
        self.checkin_reader(workspace, generation);
        outcome
    }

    /// Runs a data write against the serialized writer workspace.
    ///
    /// Reads may continue from their WAL snapshot while an import or row write
    /// is active.
    pub fn write<T, F>(&self, work: F) -> Result<T, ApiError>
    where
        T: Send,
        F: FnOnce(&mut Workspace) -> Result<T, ApiError> + Send,
    {
        let _gate = self
            .lifecycle
            .read()
            .map_err(|_| ApiError::internal("workspace lifecycle lock is poisoned"))?;
        let outcome = {
            let mut writer = self
                .writer
                .lock()
                .map_err(|_| ApiError::internal("writer lock is poisoned"))?;
            let workspace = writer.as_mut().ok_or_else(ApiError::no_workspace)?;
            work(workspace)
        };
        self.clear_readers();
        self.generation.fetch_add(1, Ordering::AcqRel);
        outcome
    }

    /// Runs a schema-changing write while excluding concurrent read leases.
    ///
    /// A read loads metadata and then prepares its data SQL on that snapshot.
    /// If a rename or table rebuild committed between those two steps, the
    /// prepared statement could name a column that no longer exists. Schema
    /// changes take the lifecycle write lock, so every lease observes either
    /// the old schema for its whole call or the new one, never a mixture.
    pub fn write_schema<T, F>(&self, work: F) -> Result<T, ApiError>
    where
        T: Send,
        F: FnOnce(&mut Workspace) -> Result<T, ApiError> + Send,
    {
        let _gate = self
            .lifecycle
            .write()
            .map_err(|_| ApiError::internal("workspace lifecycle lock is poisoned"))?;
        let outcome = {
            let mut writer = self
                .writer
                .lock()
                .map_err(|_| ApiError::internal("writer lock is poisoned"))?;
            let workspace = writer.as_mut().ok_or_else(ApiError::no_workspace)?;
            work(workspace)
        };
        self.clear_readers();
        self.generation.fetch_add(1, Ordering::AcqRel);
        outcome
    }

    fn checkout_reader(&self, path: &Path) -> Result<Workspace, ApiError> {
        let pooled = self
            .readers
            .lock()
            .map_err(|_| ApiError::internal("reader pool lock is poisoned"))?
            .pop();
        match pooled {
            Some(workspace) => Ok(workspace),
            None => {
                let cancel = self.cancel_handle();
                let search_cancel = self.search_cancel_handle();
                self.open_reader(path, &cancel, &search_cancel)
            }
        }
    }

    fn checkin_reader(&self, workspace: Workspace, generation: u64) {
        if self.generation.load(Ordering::Acquire) != generation {
            return;
        }
        if let Ok(mut readers) = self.readers.lock()
            && readers.len() < READER_POOL_SIZE
        {
            readers.push(workspace);
        }
    }

    fn open_reader(
        &self,
        path: &Path,
        cancel: &Arc<QueryCancel>,
        search_cancel: &Arc<QueryCancel>,
    ) -> Result<Workspace, ApiError> {
        let mut workspace =
            Workspace::open_read_only(path, Box::new(HybridLogicalClock::new("desktop-read")))
                .map_err(ApiError::from)?;
        workspace.set_cancel_handle(Arc::clone(cancel));
        workspace.set_search_cancel_handle(Arc::clone(search_cancel));
        Ok(workspace)
    }

    fn fill_readers(
        &self,
        path: &Path,
        cancel: &Arc<QueryCancel>,
        search_cancel: &Arc<QueryCancel>,
    ) {
        if let Ok(mut readers) = self.readers.lock() {
            while readers.len() < READER_POOL_SIZE {
                match self.open_reader(path, cancel, search_cancel) {
                    Ok(workspace) => readers.push(workspace),
                    Err(_) => break,
                }
            }
        }
    }

    fn clear_readers(&self) {
        if let Ok(mut readers) = self.readers.lock() {
            readers.clear();
        }
    }
}

fn close_writer(slot: &Mutex<Option<Workspace>>) -> Result<(), ApiError> {
    let workspace = lock(slot).take();
    if let Some(workspace) = workspace {
        workspace.close().map_err(ApiError::from)?;
    }
    Ok(())
}

/// Locks a mutex, recovering the data if a previous holder panicked.
fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Bounds concurrent read leases so the reader pool never grows past four.
struct ReadGate {
    available: Mutex<usize>,
    ready: Condvar,
}

impl ReadGate {
    fn new() -> Self {
        Self {
            available: Mutex::new(READER_POOL_SIZE),
            ready: Condvar::new(),
        }
    }

    fn acquire(&self) -> ReadPermit<'_> {
        let mut available = self
            .available
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        while *available == 0 {
            available = self
                .ready
                .wait(available)
                .unwrap_or_else(std::sync::PoisonError::into_inner);
        }
        *available = available.saturating_sub(1);
        ReadPermit { gate: self }
    }
}

struct ReadPermit<'a> {
    gate: &'a ReadGate,
}

impl Drop for ReadPermit<'_> {
    fn drop(&mut self) {
        let mut available = self
            .gate
            .available
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        *available = available.saturating_add(1);
        self.gate.ready.notify_one();
    }
}

impl Drop for DbHandle {
    fn drop(&mut self) {
        let _ = self.close();
    }
}
