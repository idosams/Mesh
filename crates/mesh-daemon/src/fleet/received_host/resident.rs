//! Native resident loop. Connection handles carry requests, never ownership of providers.
use super::*;
use crate::fleet::RemoteAdmissionReceipt;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, SyncSender};
use std::time::Duration;

const QUEUE_CAPACITY: usize = 32;
const POLL_INTERVAL: Duration = Duration::from_millis(50);

/// Native-only commands. Neither transport bytes nor renderer input can construct a handoff.
/// Replies use nonblocking delivery: a lost or full reply channel cannot stop worker observation.
pub enum ReceivedWorkerRequest {
    /// Transfer an original handoff into resident ownership. Reply loss does not undo startup.
    Start {
        /// Original single-use receiving authority.
        handoff: Box<RemoteReceivedHandoff>,
        /// Independently admitted native execution configuration.
        launch: Box<ReceivedWorkerLaunch>,
        /// Correlated native result, never another launch permit.
        reply: SyncSender<Result<RemoteAdmissionReceipt, Unavailable>>,
    },
    /// Read native facts for an exact retained admission.
    Snapshot {
        /// Exact retained admission facts.
        admission: RemoteAdmissionReceipt,
        /// Snapshot or explicit unavailability.
        reply: SyncSender<Result<Json, Unavailable>>,
    },
    /// Request cancellation without releasing ownership or claiming termination.
    Cancel {
        /// Exact retained admission facts.
        admission: RemoteAdmissionReceipt,
        /// Result of persisting the native cancellation request.
        reply: SyncSender<Result<(), Unavailable>>,
    },
}

/// Fixed-capacity native mailbox. Its receiver cannot be replaced with an unbounded channel.
pub struct ReceivedWorkerMailbox(Receiver<ReceivedWorkerRequest>);
impl ReceivedWorkerMailbox {
    /// Create a mailbox for at most 32 pending native requests. Connection threads should use
    /// `try_send`; a full/disconnected send returns the original request, including any handoff,
    /// which they must retain for reconciliation instead of reconstructing launch authority.
    pub fn bounded() -> (SyncSender<ReceivedWorkerRequest>, Self) {
        let (sender, receiver) = mpsc::sync_channel(QUEUE_CAPACITY);
        (sender, Self(receiver))
    }
}

impl ReceivedWorkerSupervisor {
    /// Run on the resident native thread, independently of broker connection threads. Poll before
    /// every request and at idle intervals; continuous request traffic cannot suppress polling.
    /// Native start/storage work can still block this thread: the interval is not a latency promise.
    ///
    /// Closing all command senders or observation receivers does not stop the loop. Only the
    /// separately owned native stop flag does; returning preserves this supervisor's original
    /// owners and all occupied slots. Stopping observation is not cancellation or termination.
    /// Pending mailbox requests remain in the caller-owned mailbox after return.
    ///
    /// Observations are bounded best-effort facts: a full channel drops this sample rather than
    /// blocking. Consumers must inspect each worker's observation time; the durable native history
    /// remains authoritative. The observer must drain the channel to obtain newer samples.
    pub fn serve(
        &mut self,
        mailbox: &ReceivedWorkerMailbox,
        stop: &AtomicBool,
        observations: &SyncSender<Vec<ReceivedWorkerObservation>>,
    ) {
        while !stop.load(Ordering::Acquire) {
            let _ = observations.try_send(self.poll());
            match mailbox.0.recv_timeout(POLL_INTERVAL) {
                Ok(request) => {
                    // Complete an already dequeued request even if stop changes during its handling.
                    // Never drop a consumed handoff or turn it into a second grant. Native stop
                    // takes effect at the next loop boundary, not as request cancellation.
                    match request {
                        ReceivedWorkerRequest::Start {
                            handoff,
                            launch,
                            reply,
                        } => {
                            let _ = reply.try_send(self.start_received(*handoff, *launch));
                        }
                        ReceivedWorkerRequest::Snapshot { admission, reply } => {
                            let _ = reply.try_send(self.snapshot(&admission));
                        }
                        ReceivedWorkerRequest::Cancel { admission, reply } => {
                            let _ = reply.try_send(self.request_cancel(&admission));
                        }
                    }
                }
                Err(RecvTimeoutError::Timeout) => {}
                // recv_timeout returns immediately once disconnected. Keep polling without spinning.
                Err(RecvTimeoutError::Disconnected) => std::thread::sleep(POLL_INTERVAL),
            }
        }
    }
}
