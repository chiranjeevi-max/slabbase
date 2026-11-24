//! A worker thread that exclusively owns the SQLite workspace.

use std::sync::Mutex;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread::{self, JoinHandle};

use serde::Serialize;
use serde::de::DeserializeOwned;
use slab_storage::Workspace;
use slab_sync::HybridLogicalClock;

use crate::dto::WorkspaceInfoDto;
use crate::error::ApiError;

type Work = Box<dyn FnOnce(&mut Option<Workspace>) -> Result<ApiErrorOrJson, ApiError> + Send>;
type ApiErrorOrJson = String;

struct Job {
    work: Work,
    reply: Sender<Result<String, ApiError>>,
}

/// Handle to the database worker.
pub struct DbHandle {
    sender: Mutex<Option<Sender<Job>>>,
    join: Mutex<Option<JoinHandle<()>>>,
}

impl DbHandle {
    /// Starts the worker thread.
    pub fn new() -> Result<Self, ApiError> {
        let (sender, receiver) = mpsc::channel::<Job>();
        let join = thread::Builder::new()
            .name("slabbase-db".into())
            .spawn(move || run_worker(receiver))
            .map_err(|error| {
                ApiError::new(
                    "SLAB_E_INTERNAL",
                    format!("could not start the database thread: {error}"),
                )
            })?;
        Ok(Self {
            sender: Mutex::new(Some(sender)),
            join: Mutex::new(Some(join)),
        })
    }

    /// Opens a workspace, replacing any current one.
    pub fn open(&self, path: String) -> Result<WorkspaceInfoDto, ApiError> {
        self.submit(move |slot| {
            let workspace = Workspace::open(&path, Box::new(HybridLogicalClock::new("desktop")))
                .map_err(ApiError::from)?;
            let info = WorkspaceInfoDto::from(workspace.info().map_err(ApiError::from)?);
            close_slot(slot)?;
            *slot = Some(workspace);
            Ok(info)
        })
    }

    /// Creates a workspace, replacing any current one.
    pub fn create(&self, path: String, name: String) -> Result<WorkspaceInfoDto, ApiError> {
        self.submit(move |slot| {
            close_slot(slot)?;
            let workspace =
                Workspace::create(&path, &name, Box::new(HybridLogicalClock::new("desktop")))
                    .map_err(ApiError::from)?;
            let info = WorkspaceInfoDto::from(workspace.info().map_err(ApiError::from)?);
            *slot = Some(workspace);
            Ok(info)
        })
    }

    /// Closes the current workspace.
    pub fn close(&self) -> Result<(), ApiError> {
        self.submit(|slot| {
            close_slot(slot)?;
            Ok(())
        })
    }

    /// Returns workspace info when one is open.
    pub fn info(&self) -> Result<Option<WorkspaceInfoDto>, ApiError> {
        self.submit(|slot| match slot.as_ref() {
            Some(workspace) => Ok(Some(WorkspaceInfoDto::from(
                workspace.info().map_err(ApiError::from)?,
            ))),
            None => Ok(None),
        })
    }

    /// Loads the sample workspace.
    pub fn load_sample(&self) -> Result<WorkspaceInfoDto, ApiError> {
        self.run(|workspace| {
            workspace.seed_studio().map_err(ApiError::from)?;
            Ok(WorkspaceInfoDto::from(
                workspace.info().map_err(ApiError::from)?,
            ))
        })
    }

    /// Runs a closure against an open workspace on the worker thread.
    pub fn run<T, F>(&self, work: F) -> Result<T, ApiError>
    where
        T: Serialize + DeserializeOwned + Send + 'static,
        F: FnOnce(&mut Workspace) -> Result<T, ApiError> + Send + 'static,
    {
        self.submit(move |slot| {
            let workspace = slot.as_mut().ok_or_else(ApiError::no_workspace)?;
            work(workspace)
        })
    }

    fn submit<T, F>(&self, work: F) -> Result<T, ApiError>
    where
        T: Serialize + DeserializeOwned + Send + 'static,
        F: FnOnce(&mut Option<Workspace>) -> Result<T, ApiError> + Send + 'static,
    {
        let (reply, response) = mpsc::channel();
        {
            let guard = self
                .sender
                .lock()
                .map_err(|_| ApiError::internal("database channel lock is poisoned"))?;
            let sender = guard
                .as_ref()
                .ok_or_else(|| ApiError::internal("database worker is closed"))?;
            sender
                .send(Job {
                    work: Box::new(move |slot| {
                        let value = work(slot)?;
                        serde_json::to_string(&value).map_err(ApiError::from)
                    }),
                    reply,
                })
                .map_err(|_| ApiError::internal("database worker is not running"))?;
        }
        let json = response
            .recv()
            .map_err(|_| ApiError::internal("database worker stopped before replying"))??;
        serde_json::from_str(&json).map_err(ApiError::from)
    }
}

fn close_slot(slot: &mut Option<Workspace>) -> Result<(), ApiError> {
    if let Some(workspace) = slot.take() {
        workspace.close().map_err(ApiError::from)?;
    }
    Ok(())
}

fn run_worker(receiver: Receiver<Job>) {
    let mut workspace: Option<Workspace> = None;
    while let Ok(job) = receiver.recv() {
        let result = (job.work)(&mut workspace);
        let _ = job.reply.send(result);
    }
    if let Some(workspace) = workspace.take() {
        let _ = workspace.close();
    }
}

impl Drop for DbHandle {
    fn drop(&mut self) {
        if let Ok(mut guard) = self.sender.lock() {
            guard.take();
        }
        if let Ok(mut join) = self.join.lock()
            && let Some(handle) = join.take()
        {
            let _ = handle.join();
        }
    }
}
