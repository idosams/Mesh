//! One native maintenance owner; the worker never retains the daemon while sleeping.
use super::*;
use std::io;
use std::thread::{self, JoinHandle};

/// Redacted result of the most recent scheduled maintenance attempt.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OrphanCleanupOutcome {
    /// No attempt has run yet.
    Waiting,
    /// No active workspace was open.
    NoWorkspace,
    /// A native writer, reader or assigned agent prevented admission.
    Deferred,
    /// An admitted pass completed; only unreferenced arrivals can be removed.
    Collected(usize),
    /// Identity, recovery, history or I/O validation refused the pass.
    Refused,
}

/// In-memory maintenance evidence, containing no paths, contents or credentials.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OrphanCleanupStatus {
    /// Completed attempts, including deferred and refused attempts.
    pub attempts: u64,
    /// Most recent result; failures never authorize retry with weaker checks.
    pub outcome: OrphanCleanupOutcome,
}

#[derive(Debug)]
pub(super) struct WorkerState {
    control: Mutex<Control>,
    wake: Condvar,
}

#[derive(Debug)]
struct Control {
    stop: bool,
    status: OrphanCleanupStatus,
}

/// Sole owner of a periodic native cleanup worker.
///
/// Drop wakes its wait and joins an in-flight pass before returning. Admitted filesystem I/O
/// can delay shutdown; cancellation never abandons custody or an in-flight journal rewrite.
#[derive(Debug)]
pub struct OrphanCleanupWorker {
    state: Arc<WorkerState>,
    worker: Option<JoinHandle<()>>,
}

impl OrphanCleanupWorker {
    /// Read redacted native maintenance evidence without inspecting a workspace.
    #[must_use]
    pub fn status(&self) -> OrphanCleanupStatus {
        self.state
            .control
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .status
    }

    #[cfg(test)]
    pub(super) fn wait_after(&self, attempts: u64) -> OrphanCleanupStatus {
        let control = self.state.control.lock().unwrap();
        let (control, _) = self
            .state
            .wake
            .wait_timeout_while(control, Duration::from_secs(5), |c| {
                c.status.attempts <= attempts
            })
            .unwrap();
        assert!(
            control.status.attempts > attempts,
            "cleanup worker did not complete an attempt"
        );
        control.status
    }
}

impl Drop for OrphanCleanupWorker {
    fn drop(&mut self) {
        self.state
            .control
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .stop = true;
        self.state.wake.notify_all();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

impl LiveDaemon {
    /// Start one cleanup owner with a 60-second delay between completed attempts.
    ///
    /// Only the currently open workspace is considered; assigned/busy workspaces are skipped.
    /// All recorded versions are retained. Keep the returned owner alive while serving.
    ///
    /// # Errors
    /// Refuses a second owner or a failed native thread creation.
    pub fn start_orphan_cleanup(self: &Arc<Self>) -> io::Result<OrphanCleanupWorker> {
        self.start_orphan_cleanup_interval(Duration::from_secs(60))
    }

    pub(super) fn start_orphan_cleanup_interval(
        self: &Arc<Self>,
        interval: Duration,
    ) -> io::Result<OrphanCleanupWorker> {
        if interval.is_zero() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "cleanup interval must be positive",
            ));
        }
        let mut registered = self
            .orphan_cleanup
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if registered.upgrade().is_some() {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "cleanup already has an owner",
            ));
        }
        let state = Arc::new(WorkerState {
            control: Mutex::new(Control {
                stop: false,
                status: OrphanCleanupStatus {
                    attempts: 0,
                    outcome: OrphanCleanupOutcome::Waiting,
                },
            }),
            wake: Condvar::new(),
        });
        let running = Arc::clone(&state);
        let daemon = Arc::downgrade(self);
        let worker = thread::Builder::new()
            .name("mesh-orphan-cleanup".into())
            .spawn(move || loop {
                let control = running
                    .control
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner);
                let (control, _) = running
                    .wake
                    .wait_timeout_while(control, interval, |c| !c.stop)
                    .unwrap_or_else(PoisonError::into_inner);
                if control.stop {
                    break;
                }
                drop(control);
                let Some(daemon) = daemon.upgrade() else {
                    break;
                };
                let outcome = daemon.try_cleanup_active_workspace();
                drop(daemon);
                let mut control = running
                    .control
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner);
                control.status.attempts = control.status.attempts.saturating_add(1);
                control.status.outcome = outcome;
                running.wake.notify_all();
            })?;
        *registered = Arc::downgrade(&state);
        Ok(OrphanCleanupWorker {
            state,
            worker: Some(worker),
        })
    }

    fn try_cleanup_active_workspace(&self) -> OrphanCleanupOutcome {
        let Some(held) = Self::try_cleanup_lock(&self.open) else {
            return OrphanCleanupOutcome::Deferred;
        };
        let Some(open) = held.as_ref() else {
            return OrphanCleanupOutcome::NoWorkspace;
        };
        let root = open.root().as_path().to_path_buf();
        let digest = open.digest().to_string();
        let installation = open.installation();
        drop(held);
        let Some(root) = root.to_str() else {
            return OrphanCleanupOutcome::Refused;
        };
        match self.try_collect_workspace_orphans(
            root,
            &digest,
            &installation,
            mesh_cas::CollectionMode::Delete,
        ) {
            Ok(Some(collected)) => OrphanCleanupOutcome::Collected(collected.collected().len()),
            Ok(None) => OrphanCleanupOutcome::Deferred,
            Err(_) => OrphanCleanupOutcome::Refused,
        }
    }
}
