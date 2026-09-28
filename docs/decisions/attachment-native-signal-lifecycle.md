# Optional native signals cannot block capture

Status: correction under validation for [issue #37](https://github.com/idosams/Mesh/issues/37).
This does not establish packaged graphical acceptance or fleet latency guarantees.

## Evidence and decision

Two native-event tests repeatedly timed out before their initial capture in the local full gate.
A separate eight-process diagnostic reproduced 15 failures in 16 unchanged test executions. A
failing capture worker's sampled stack remained in `FSEventStreamStart` registration RPC for all
801 samples. The test thread was waiting for its first status update. That establishes a blocking
native call in the sampled failure; it does not establish why the host service was delayed.

Native events are optional, lossy hints. Initial reconciliation, periodic capture and capture stop
must not depend on native registration completing. A separate helper owns registration, callbacks
and destruction on one thread. The identity-checked capture worker retains exclusive responsibility
for bounded observations, signing and history. The helper holds a path and generation-specific
signal state, without signing, history-store or source-write authority. Event paths never become
content, identity evidence or authorship.

## Ownership and bounds

At most 32 native helpers can be live in one process. A permit remains held through registration
and native destruction, including a blocked OS call. Capacity exhaustion, startup failure or unwind
leaves periodic capture available. There is no automatic retry loop or unbounded replacement thread.
A later explicit capture session can try registration again; periodic-only sessions do not
silently accumulate registration retries. This bounds helpers per process, not across all
independent harness processes or the operating system's event service.

A stopped generation cannot be revived by late registration or a callback. Stop sets the generation's
stop flag and clears active-native status. A registration completing after that point is destroyed
without activating capture. Activation before stop wakes a fresh identity-checked reconciliation to
cover edits during registration. This wakeup is not counted as a native callback batch.

`stop_and_join` and `stop_capture_and_join` wait for the capture worker and its in-flight durable
attempt. They do not pretend to cancel an in-progress OS registration call. Native cleanup may remain
pending afterward. The desktop keeps the stopped controller's live status while detached, so it can
observe cleanup completing. Resuming creates a new capture generation; any old helper remains
stopped and consumes its existing process permit until native cleanup finishes.

## Projection and compatibility

`mesh.attachment-capture/v1` gains the additive `native_signal_state` field: `disabled`, `starting`,
`active`, `unavailable`, `stopping`, or `stopped`. `native_events` remains for existing readers and
is true only for an active stream on a live capture generation. Capture phase and native signal
state describe separate lifetimes. `capture.phase=stopped` means no further captures can run; it
does not imply that `native_signal_state=stopping` has finished cleanup. Fresh catalog restoration
has no helper from that prior process and reports native signals stopped.

Older replies without this field retain the previous active/periodic fallback presentation. New
unknown state values or contradictory activity refuse at the presentation boundary. English and Hebrew UI copy
separates pending native setup, periodic checks and pending cleanup, preserving literal path display.
No durable receipt, journal, approval, or source identity format changes. There is no new agent
write capability or public IPC method.

## Verification

Controlled native tests park registration and cleanup separately. They check initial and periodic
capture, capture stop while registration is held, bounded capacity with periodic fallback, late
registration after stop, eventual permit release, registration failure/unwind, and reconciliation
on activation. These tests exercise the real capture worker and immutable history with controlled
optional monitoring; they are not evidence of real FSEvents delivery.

The actual macOS event tests still require a registered stream, callback batches, saved nested edits,
atomic replacement and root replacement refusal, under their unchanged eight-second wait bounds.
They now reserve the full nextest worker pool because registration is a shared host service and the
recorded failure was a blocking service RPC during unrelated suite filesystem load. Readiness for
native hints is observed independently of first capture. This test scheduling is not a throughput
claim; the separate controlled tests enforce capture availability even when registration never
becomes ready during the observation window. The executable proof likewise waits for an actually
active stream before editing and still requires a callback batch afterward.

Required before completion: focused and failing-before regression evidence, real native event tests,
canonical full checks and CI, the revision-bound executable journey, and review/merged delivery.
Four-worker fleet measurements and full packaged graphical journeys remain in the full fleet plan.
