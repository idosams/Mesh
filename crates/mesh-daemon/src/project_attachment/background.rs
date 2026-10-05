//! Single-worker reconciliation for an existing project. No event stream is treated as complete.

use super::{external_store, ObservationLimits, ProjectAttachment, SavedAttachmentVersion};
use crate::ipc::Json;
use crate::root_authority::PinnedWorkspaceRoot;
use crate::CheckpointSigner;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

#[cfg(target_os = "macos")]
#[path = "signals_macos.rs"]
mod signals_macos;

#[cfg(any(target_os = "macos", test))]
#[path = "signals_worker.rs"]
mod signals_worker;

/// Native scheduling policy. Scans remain bounded by the observer's separate resource limits.
#[derive(Clone, Copy, Debug)]
pub struct CaptureSchedule {
    /// Request available native wakeup signals; reconciliation still runs if disabled or unavailable.
    pub native_signals: bool,
    /// Delay after a completed attempt, from 250 ms to five minutes. Signals may wake it sooner.
    pub reconciliation_interval: Duration,
    /// Per-attempt entry and byte limits; incomplete traversal never produces a saved version.
    pub limits: ObservationLimits,
}
impl Default for CaptureSchedule {
    fn default() -> Self {
        Self {
            native_signals: true,
            reconciliation_interval: Duration::from_secs(5),
            limits: ObservationLimits::default(),
        }
    }
}

/// Current worker activity, separate from the last completed attempt's result.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CapturePhase {
    /// Registered identities are pinned; initial history recovery has not completed.
    Starting,
    /// Reading a bounded capture of the original project.
    Scanning,
    /// Signing or writing captured bytes into external history.
    Saving,
    /// Waiting for a signal or the next reconciliation interval.
    Waiting,
    /// Stop requested; an in-flight operation may still be finishing.
    Stopping,
    /// Worker exited and will perform no further captures.
    Stopped,
    /// Worker unwound unexpectedly; no automatic replacement was launched.
    Failed,
}

/// Redacted capture health. No filesystem paths, content, signer errors or secrets are retained.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CaptureOutcome {
    /// No attempt has completed in this service instance.
    Pending,
    /// New immutable history was acknowledged.
    Saved,
    /// Complete captured content already matches the saved version.
    Unchanged,
    /// The scan could not admit a complete bounded capture.
    Incomplete,
    /// The original registered source is unavailable or has changed identity.
    SourceUnavailable,
    /// The external store is unavailable or has changed identity.
    StoreUnavailable,
    /// Signing, policy checks or durable saving failed; the last acknowledged version is retained.
    SaveUnavailable,
    /// Stop was requested before this attempt acknowledged a saved capture.
    Cancelled,
}

/// Optional native monitoring has its own lifecycle; capture can stop before native cleanup ends.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NativeSignalState {
    /// Native signals were not requested.
    Disabled,
    /// Native registration is pending; periodic capture is available.
    Starting,
    /// A lossy native stream is registered.
    Active,
    /// Platform, capacity or registration failure leaves periodic capture available.
    Unavailable,
    /// Stop requested; a native registration or cleanup call may still be pending.
    Stopping,
    /// Native helper cleanup has completed.
    Stopped,
}
impl NativeSignalState {
    fn text(self) -> &'static str {
        match self {
            Self::Disabled => "disabled",
            Self::Starting => "starting",
            Self::Active => "active",
            Self::Unavailable => "unavailable",
            Self::Stopping => "stopping",
            Self::Stopped => "stopped",
        }
    }
}

/// Live, redacted status. Ages are measured with a monotonic clock in this process only.
#[derive(Clone, Debug)]
pub struct CaptureStatus {
    /// Native monitoring state, independent of capture-worker termination.
    pub native_signal_state: NativeSignalState,
    /// A native event stream is active; it is lossy and does not attest to content or authorship.
    pub native_events: bool,
    /// Count of coalesced native callback batches, not file edits or saved versions.
    pub event_signals: u64,
    /// Monotonic status revision for waiting clients.
    pub revision: u64,
    /// Current worker activity.
    pub phase: CapturePhase,
    /// Last completed attempt; a new scan does not erase the prior result.
    pub last_outcome: CaptureOutcome,
    /// Last acknowledged/recovered immutable version, not a claim about current live bytes.
    pub saved_version: Option<SavedAttachmentVersion>,
    /// Attempts started in this service instance.
    pub attempts: u64,
    /// Newly acknowledged versions in this instance, excluding unchanged captures.
    pub versions_saved: u64,
    /// Duration of the most recent attempt, including signing and storage when attempted.
    pub last_attempt_duration: Option<Duration>,
    /// Age of the most recent completed attempt; never inferred from a stale wall-clock timestamp.
    pub last_attempt_age: Option<Duration>,
    /// Age since the start of the most recent attempt that acknowledged a complete capture.
    pub last_complete_capture_age: Option<Duration>,
}

impl CaptureStatus {
    /// Stable redacted native projection for a future UI or event subscriber. This is not a live
    /// folder snapshot: the saved identity and observation age remain explicit and independent.
    pub fn to_json(&self) -> Json {
        let phase = match self.phase {
            CapturePhase::Starting => "starting",
            CapturePhase::Scanning => "scanning",
            CapturePhase::Saving => "saving",
            CapturePhase::Waiting => "waiting",
            CapturePhase::Stopping => "stopping",
            CapturePhase::Stopped => "stopped",
            CapturePhase::Failed => "failed",
        };
        let outcome = match self.last_outcome {
            CaptureOutcome::Pending => "pending",
            CaptureOutcome::Saved => "saved",
            CaptureOutcome::Unchanged => "unchanged",
            CaptureOutcome::Incomplete => "incomplete",
            CaptureOutcome::SourceUnavailable => "source-unavailable",
            CaptureOutcome::StoreUnavailable => "store-unavailable",
            CaptureOutcome::SaveUnavailable => "save-unavailable",
            CaptureOutcome::Cancelled => "cancelled",
        };
        let millis = |duration: Option<Duration>| {
            duration.map_or(Json::Null, |duration| {
                Json::Number(duration.as_millis().min(u128::from(u64::MAX)) as u64)
            })
        };
        Json::object([
            ("schema", Json::text("mesh.attachment-capture/v1")),
            ("revision", Json::Number(self.revision)),
            ("phase", Json::text(phase)),
            ("last_outcome", Json::text(outcome)),
            (
                "saved_version",
                self.saved_version.map_or(Json::Null, |version| {
                    Json::text(version.operation().to_string())
                }),
            ),
            ("native_events", Json::Bool(self.native_events)),
            (
                "native_signal_state",
                Json::text(self.native_signal_state.text()),
            ),
            ("event_signals", Json::Number(self.event_signals)),
            ("attempts", Json::Number(self.attempts)),
            ("versions_saved", Json::Number(self.versions_saved)),
            (
                "last_attempt_duration_ms",
                millis(self.last_attempt_duration),
            ),
            ("last_attempt_age_ms", millis(self.last_attempt_age)),
            (
                "last_complete_capture_age_ms",
                millis(self.last_complete_capture_age),
            ),
            ("attribution", Json::text("unknown")),
            ("atomic_snapshot", Json::Bool(false)),
        ])
    }
}

struct State {
    status: CaptureStatus,
    stop: bool,
    pending: bool,
    filesystem_pending: bool,
    last_attempt: Option<Instant>,
    last_complete: Option<Instant>,
}
struct Shared {
    state: Mutex<State>,
    changed: Condvar,
}
impl Shared {
    fn lock(&self) -> MutexGuard<'_, State> {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
    fn change(&self, update: impl FnOnce(&mut State)) {
        let mut state = self.lock();
        update(&mut state);
        if state.stop
            && !matches!(
                state.status.phase,
                CapturePhase::Stopped | CapturePhase::Failed
            )
        {
            state.status.phase = CapturePhase::Stopping;
        }
        state.status.revision = state.status.revision.saturating_add(1);
        self.changed.notify_all();
    }
    fn stopped(&self) -> bool {
        self.lock().stop
    }
    fn snapshot(state: &State) -> CaptureStatus {
        let mut status = state.status.clone();
        status.last_attempt_age = state.last_attempt.map(|time| time.elapsed());
        status.last_complete_capture_age = state.last_complete.map(|time| time.elapsed());
        status
    }
}

struct CaptureTarget {
    attachment: ProjectAttachment,
    store: PinnedWorkspaceRoot,
    metadata: PathBuf,
    registered: Option<(super::AttachmentStorage, super::ProvisionedAttachment)>,
}

/// One native background controller. Starting it explicitly authorizes captures with the supplied
/// host signer. It provisions no agent and never writes, locks or takes over the original project.
/// Dropping requests stop without blocking; use `stop_and_join` to wait for capture termination. Native helper cleanup is reported separately.
pub struct AttachmentCaptureService {
    shared: Arc<Shared>,
    worker: Option<JoinHandle<()>>,
}
impl AttachmentCaptureService {
    /// Start a worker with retained catalog authority. Ordinary projects remain ordinary;
    /// enrolled lanes use exact dependency recovery and capture on every attempt.
    pub fn start_registered(
        storage: &super::AttachmentStorage,
        selected: &super::ProvisionedAttachment,
        signer: Arc<dyn CheckpointSigner>,
        schedule: CaptureSchedule,
    ) -> io::Result<Self> {
        let selected = storage.exact_registered_work(selected)?;
        let target = CaptureTarget {
            attachment: selected.project().clone(),
            store: selected.store.clone(),
            metadata: selected.metadata_path().to_path_buf(),
            registered: Some((storage.clone(), selected)),
        };
        Self::start_target_with_signals(target, signer, schedule, |shared, path| {
            #[cfg(target_os = "macos")]
            signals_worker::start(shared, path);
            #[cfg(not(target_os = "macos"))]
            {
                let _ = path;
                shared.change(|state| {
                    state.status.native_signal_state = NativeSignalState::Unavailable
                });
            }
        })
    }

    /// Pin an existing registration and its external metadata, then start one bounded worker.
    /// The host owns key setup and lifetime. No key is generated or persisted by this service.
    pub fn start(
        metadata: &Path,
        signer: Arc<dyn CheckpointSigner>,
        schedule: CaptureSchedule,
    ) -> io::Result<Self> {
        let attachment = ProjectAttachment::reopen(metadata)?;
        let store = external_store(metadata, &attachment)?;
        Self::start_pinned(metadata, attachment, store, signer, schedule)
    }

    pub(super) fn start_pinned(
        metadata: &Path,
        attachment: ProjectAttachment,
        store: PinnedWorkspaceRoot,
        signer: Arc<dyn CheckpointSigner>,
        schedule: CaptureSchedule,
    ) -> io::Result<Self> {
        Self::start_pinned_with_signals(
            metadata,
            attachment,
            store,
            signer,
            schedule,
            |shared, path| {
                #[cfg(target_os = "macos")]
                signals_worker::start(shared, path);
                #[cfg(not(target_os = "macos"))]
                {
                    let _ = path;
                    shared.change(|state| {
                        state.status.native_signal_state = NativeSignalState::Unavailable
                    });
                }
            },
        )
    }

    fn start_pinned_with_signals<F>(
        metadata: &Path,
        attachment: ProjectAttachment,
        store: PinnedWorkspaceRoot,
        signer: Arc<dyn CheckpointSigner>,
        schedule: CaptureSchedule,
        start_signals: F,
    ) -> io::Result<Self>
    where
        F: FnOnce(&Arc<Shared>, &Path) + Send + 'static,
    {
        Self::start_target_with_signals(
            CaptureTarget {
                attachment,
                store,
                metadata: metadata.to_path_buf(),
                registered: None,
            },
            signer,
            schedule,
            start_signals,
        )
    }

    fn start_target_with_signals<F>(
        target: CaptureTarget,
        signer: Arc<dyn CheckpointSigner>,
        schedule: CaptureSchedule,
        start_signals: F,
    ) -> io::Result<Self>
    where
        F: FnOnce(&Arc<Shared>, &Path) + Send + 'static,
    {
        let CaptureTarget {
            attachment, store, ..
        } = &target;
        attachment.ensure_current()?;
        store.ensure_namespace_identity()?;
        super::detachment::ensure_attached(store)?;
        schedule.limits.validate()?;
        if !(Duration::from_millis(250)..=Duration::from_secs(300))
            .contains(&schedule.reconciliation_interval)
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "capture interval must be between 250 ms and five minutes",
            ));
        }
        let shared = Arc::new(Shared {
            state: Mutex::new(State {
                status: CaptureStatus {
                    native_signal_state: if schedule.native_signals {
                        NativeSignalState::Starting
                    } else {
                        NativeSignalState::Disabled
                    },
                    native_events: false,
                    event_signals: 0,
                    revision: 0,
                    phase: CapturePhase::Starting,
                    last_outcome: CaptureOutcome::Pending,
                    saved_version: None,
                    attempts: 0,
                    versions_saved: 0,
                    last_attempt_duration: None,
                    last_attempt_age: None,
                    last_complete_capture_age: None,
                },
                stop: false,
                pending: true,
                filesystem_pending: false,
                last_attempt: None,
                last_complete: None,
            }),
            changed: Condvar::new(),
        });
        let running = shared.clone();
        let worker = thread::Builder::new()
            .name("mesh-attachment-capture".to_owned())
            .spawn(move || {
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    run(&running, target, signer, schedule, start_signals);
                }));
                running.change(|state| {
                    stop_native_signals(state);
                    state.stop = true;
                    state.status.native_events = false;
                    state.status.phase = if result.is_ok() {
                        CapturePhase::Stopped
                    } else {
                        CapturePhase::Failed
                    }
                });
            })?;
        Ok(Self {
            shared,
            worker: Some(worker),
        })
    }

    /// A redacted view of current activity and the last known saved point.
    pub fn status(&self) -> CaptureStatus {
        Shared::snapshot(&self.shared.lock())
    }

    /// Coalesce a missed-event or explicit checkpoint signal into at most one pending rescan.
    /// Returns false after stop is requested. No source path or file bytes are accepted here.
    pub fn request_capture(&self) -> bool {
        let mut state = self.shared.lock();
        if state.stop
            || matches!(
                state.status.phase,
                CapturePhase::Stopped | CapturePhase::Failed
            )
        {
            return false;
        }
        state.pending = true;
        self.shared.changed.notify_all();
        true
    }

    /// Wait for a status revision or timeout. Spurious wakes do not masquerade as progress.
    pub fn wait_for_update(&self, revision: u64, timeout: Duration) -> CaptureStatus {
        let (state, _) = self
            .shared
            .changed
            .wait_timeout_while(self.shared.lock(), timeout, |state| {
                state.status.revision <= revision
            })
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        Shared::snapshot(&state)
    }

    /// Stop future scans and wake a waiting worker. An in-flight durable commit may still finish.
    pub fn request_stop(&self) {
        self.shared.change(|state| {
            stop_native_signals(state);
            state.stop = true;
            state.pending = false;
        });
    }

    /// Wait for the current bounded attempt to exit. Slow filesystem I/O or an external signer can
    /// delay this wait; stop does not claim to interrupt those operations or roll back a commit.
    pub fn stop_and_join(mut self) -> io::Result<CaptureStatus> {
        self.stop_capture_and_join()
    }

    /// Join the capture worker while retaining live status for optional native cleanup.
    /// A stopped capture cannot save again; native monitoring may still report stopping.
    pub fn stop_capture_and_join(&mut self) -> io::Result<CaptureStatus> {
        self.request_stop();
        if let Some(worker) = self.worker.take() {
            worker
                .join()
                .map_err(|_| io::Error::other("attachment capture worker failed"))?;
        }
        Ok(self.status())
    }
}
impl Drop for AttachmentCaptureService {
    fn drop(&mut self) {
        self.request_stop();
    }
}

fn stop_native_signals(state: &mut State) {
    if matches!(
        state.status.native_signal_state,
        NativeSignalState::Starting | NativeSignalState::Active
    ) {
        state.status.native_signal_state = NativeSignalState::Stopping;
    }
    state.status.native_events = false;
}

fn run<F>(
    shared: &Arc<Shared>,
    target: CaptureTarget,
    signer: Arc<dyn CheckpointSigner>,
    schedule: CaptureSchedule,
    start_signals: F,
) where
    F: FnOnce(&Arc<Shared>, &Path),
{
    let CaptureTarget {
        attachment,
        store,
        metadata,
        registered,
    } = target;
    if shared.stopped() {
        shared.change(|state| {
            if schedule.native_signals {
                state.status.native_signal_state = NativeSignalState::Stopped;
            }
        });
        return;
    }
    if schedule.native_signals {
        start_signals(shared, attachment.root());
    }
    let actor = signer.public_key();
    if store
        .filesystem()
        .inspect_entry(Path::new(super::history::HISTORY))
        .is_ok()
    {
        let versions = match &registered {
            Some((storage, selected)) => storage.registered_review_versions(selected),
            None => attachment.saved_versions_in_store(&metadata, store.clone()),
        };
        match versions {
            Ok(versions) => {
                shared.change(|state| state.status.saved_version = versions.last().copied())
            }
            Err(_) => {
                shared.change(|state| state.status.last_outcome = CaptureOutcome::SaveUnavailable)
            }
        }
    }
    loop {
        {
            let mut state = shared.lock();
            loop {
                if state.stop {
                    return;
                }
                let now = Instant::now();
                let reconcile = state
                    .last_attempt
                    .map(|time| time + schedule.reconciliation_interval)
                    .unwrap_or(now);
                let event_ready = state
                    .last_attempt
                    .map(|time| time + Duration::from_millis(250))
                    .unwrap_or(now);
                if state.pending
                    || now >= reconcile
                    || (state.filesystem_pending && now >= event_ready)
                {
                    break;
                }
                let deadline = if state.filesystem_pending {
                    reconcile.min(event_ready)
                } else {
                    reconcile
                };
                let (next, _) = shared
                    .changed
                    .wait_timeout(state, deadline.saturating_duration_since(now))
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                state = next;
            }
            state.filesystem_pending = false;
            state.pending = false;
        }
        let started = Instant::now();
        shared.change(|state| {
            state.status.phase = CapturePhase::Scanning;
            state.status.attempts = state.status.attempts.saturating_add(1);
        });
        let mut saved = None;
        let outcome = if store.ensure_namespace_identity().is_err() {
            CaptureOutcome::StoreUnavailable
        } else if attachment.ensure_current().is_err() {
            CaptureOutcome::SourceUnavailable
        } else {
            match attachment.capture_inputs(schedule.limits) {
                Err(_) => CaptureOutcome::Incomplete,
                Ok(input) => {
                    if shared.stopped() {
                        complete_attempt(shared, started, CaptureOutcome::Cancelled, None);
                        return;
                    }
                    shared.change(|state| state.status.phase = CapturePhase::Saving);
                    let sign = |payload: &mesh_crypto::SigningPayload| {
                        if shared.stopped() {
                            return Err("capture stopped".to_owned());
                        }
                        let signature = signer.sign(payload)?;
                        if shared.stopped() {
                            return Err("capture stopped".to_owned());
                        }
                        Ok(signature)
                    };
                    let result = match &registered {
                        Some((storage, selected)) => {
                            storage.save_registered_capture(selected, &input, actor, sign)
                        }
                        None => attachment.save_capture_in_store(
                            &metadata,
                            &input,
                            actor,
                            sign,
                            store.clone(),
                        ),
                    };
                    match result {
                        Ok(version) => {
                            saved = Some(version);
                            if shared.lock().status.saved_version == saved {
                                CaptureOutcome::Unchanged
                            } else {
                                CaptureOutcome::Saved
                            }
                        }
                        Err(_) if shared.stopped() => CaptureOutcome::Cancelled,
                        Err(_) => CaptureOutcome::SaveUnavailable,
                    }
                }
            }
        };
        complete_attempt(shared, started, outcome, saved);
    }
}

fn complete_attempt(
    shared: &Shared,
    started: Instant,
    outcome: CaptureOutcome,
    saved: Option<SavedAttachmentVersion>,
) {
    shared.change(|state| {
        state.status.phase = CapturePhase::Waiting;
        state.status.last_outcome = outcome;
        state.status.last_attempt_duration = Some(started.elapsed());
        state.last_attempt = Some(Instant::now());
        if let Some(saved) = saved {
            state.status.saved_version = Some(saved);
            state.last_complete = Some(started);
            if outcome == CaptureOutcome::Saved {
                state.status.versions_saved = state.status.versions_saved.saturating_add(1);
            }
        }
    });
}

#[cfg(test)]
#[path = "signals_worker_tests.rs"]
mod signals_worker_tests;
