//! Bounded optional native monitoring. This helper owns no source-write or signing authority.
use super::{NativeSignalState, Shared};
use std::io;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
#[cfg(target_os = "macos")]
use std::sync::OnceLock;
use std::thread::{self, JoinHandle};

pub(super) struct Budget {
    active: AtomicUsize,
    maximum: usize,
}
impl Budget {
    pub(super) fn new(maximum: usize) -> Arc<Self> {
        Arc::new(Self {
            active: AtomicUsize::new(0),
            maximum,
        })
    }
    #[cfg(test)]
    pub(super) fn occupied(&self) -> usize {
        self.active.load(Ordering::Acquire)
    }
    fn acquire(self: &Arc<Self>) -> io::Result<Permit> {
        self.active
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |active| {
                (active < self.maximum).then_some(active + 1)
            })
            .map_err(|_| io::Error::other("native monitoring capacity unavailable"))?;
        Ok(Permit(Arc::clone(self)))
    }
}
struct Permit(Arc<Budget>);
impl Drop for Permit {
    fn drop(&mut self) {
        self.0.active.fetch_sub(1, Ordering::AcqRel);
    }
}

#[cfg(target_os = "macos")]
pub(super) fn start(shared: &Arc<Shared>, path: &std::path::Path) {
    static BUDGET: OnceLock<Arc<Budget>> = OnceLock::new();
    let path = path.to_owned();
    let signals = Arc::clone(shared);
    let _ = start_with(
        shared,
        Arc::clone(BUDGET.get_or_init(|| Budget::new(32))),
        move || super::signals_macos::Events::start(&path, &signals),
    );
}

pub(super) fn start_with<F, T>(
    shared: &Arc<Shared>,
    budget: Arc<Budget>,
    register: F,
) -> io::Result<JoinHandle<()>>
where
    F: FnOnce() -> io::Result<T> + Send + 'static,
{
    let permit = match budget.acquire() {
        Ok(permit) => permit,
        Err(error) => {
            shared
                .change(|state| state.status.native_signal_state = NativeSignalState::Unavailable);
            return Err(error);
        }
    };
    let running = Arc::clone(shared);
    let result = thread::Builder::new()
        .name("mesh-attachment-signals".to_owned())
        .spawn(move || {
            // The permit survives native destruction, including blocked stop/registration calls.
            let _permit = permit;
            if running.stopped() {
                running
                    .change(|state| state.status.native_signal_state = NativeSignalState::Stopped);
                return;
            }
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| -> io::Result<()> {
                let events = register()?;
                {
                    let mut state = running.lock();
                    if !state.stop {
                        state.status.native_events = true;
                        state.status.native_signal_state = NativeSignalState::Active;
                        state.filesystem_pending = true;
                        state.status.revision = state.status.revision.saturating_add(1);
                        running.changed.notify_all();
                    }
                    let mut state = running
                        .changed
                        .wait_while(state, |state| !state.stop)
                        .unwrap_or_else(std::sync::PoisonError::into_inner);
                    state.status.native_events = false;
                    state.status.native_signal_state = NativeSignalState::Stopping;
                    state.status.revision = state.status.revision.saturating_add(1);
                    running.changed.notify_all();
                }
                // Never hold the signal-state lock while calling into native cleanup.
                drop(events);
                Ok(())
            }));
            running.change(|state| {
                state.status.native_events = false;
                state.status.native_signal_state = if state.stop {
                    NativeSignalState::Stopped
                } else {
                    NativeSignalState::Unavailable
                };
            });
        });
    if result.is_err() {
        shared.change(|state| state.status.native_signal_state = NativeSignalState::Unavailable);
    }
    result
}
