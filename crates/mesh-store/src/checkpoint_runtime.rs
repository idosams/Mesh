//! Non-default runtime composition for the nine recovery-preservation triggers.
//!
//! This coordinator owns no scheduler and selects no byte or interval value. Callers inject all
//! three parameters, supply elapsed durations as trigger facts, and supply only values already
//! produced by the recovery verifier or durable commit sequence.

use core::fmt;
use std::time::Duration;

use crate::{
    PendingMeaningfulSave, PrivateSaved, RecoveryBoundaryEvidence, RecoveryMachine,
    RecoveryMachineError, RecoveryPreserved, RecoverySequence, RecoveryStamp,
    RecoveryStatePersistence, RecoveryTransition, RecoveryTrigger, RecoveryTriggerInput,
    TriggerEffect,
};

/// Settling interval selected by TASK-347's frozen macOS/Linux corpus.
pub const SELECTED_CHECKPOINT_IDLE_INTERVAL: Duration = Duration::from_millis(50);

/// Recovery-preservation byte bound selected by TASK-347's frozen corpus.
pub const SELECTED_MAXIMUM_UNCHECKPOINTED_BYTES: u64 = 65_536;

/// Recovery-preservation time bound selected by TASK-347's frozen corpus.
pub const SELECTED_MAXIMUM_UNCHECKPOINTED_INTERVAL: Duration = Duration::from_millis(25);

/// Unresolved runtime parameters. Every member is required; there are no fallback values.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CheckpointRuntimeParameters {
    /// Required inactivity interval before a complete window may become meaningful.
    pub idle_interval: Option<Duration>,
    /// Required maximum uncheckpointed byte count.
    pub maximum_uncheckpointed_bytes: Option<u64>,
    /// Required maximum interval before recovery bytes must be preserved.
    pub maximum_uncheckpointed_interval: Option<Duration>,
}

impl CheckpointRuntimeParameters {
    /// The measured cross-platform values ratified by ADR-0042.
    ///
    /// [`Default`] deliberately remains an unresolved configuration for tests and embedders that
    /// need to prove missing values fail closed. Production composition opts into these measured
    /// values explicitly rather than silently turning an absent field into a guessed number.
    #[must_use]
    pub const fn selected_defaults() -> Self {
        Self {
            idle_interval: Some(SELECTED_CHECKPOINT_IDLE_INTERVAL),
            maximum_uncheckpointed_bytes: Some(SELECTED_MAXIMUM_UNCHECKPOINTED_BYTES),
            maximum_uncheckpointed_interval: Some(SELECTED_MAXIMUM_UNCHECKPOINTED_INTERVAL),
        }
    }
}

/// Validated non-default runtime configuration.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CheckpointRuntimeConfig {
    idle_interval: Duration,
    maximum_uncheckpointed_bytes: u64,
    maximum_uncheckpointed_interval: Duration,
}

impl CheckpointRuntimeConfig {
    /// Validate all injected parameters.
    pub fn from_parameters(
        parameters: CheckpointRuntimeParameters,
    ) -> Result<Self, CheckpointRuntimeConfigError> {
        let idle_interval = parameters
            .idle_interval
            .ok_or(CheckpointRuntimeConfigError::MissingIdleInterval)?;
        let maximum_uncheckpointed_bytes = parameters
            .maximum_uncheckpointed_bytes
            .ok_or(CheckpointRuntimeConfigError::MissingMaximumBytes)?;
        let maximum_uncheckpointed_interval = parameters
            .maximum_uncheckpointed_interval
            .ok_or(CheckpointRuntimeConfigError::MissingMaximumInterval)?;
        if idle_interval.is_zero() {
            return Err(CheckpointRuntimeConfigError::ZeroIdleInterval);
        }
        if maximum_uncheckpointed_bytes == 0 {
            return Err(CheckpointRuntimeConfigError::ZeroMaximumBytes);
        }
        if maximum_uncheckpointed_interval.is_zero() {
            return Err(CheckpointRuntimeConfigError::ZeroMaximumInterval);
        }
        Ok(Self {
            idle_interval,
            maximum_uncheckpointed_bytes,
            maximum_uncheckpointed_interval,
        })
    }

    /// Configured inactivity interval.
    #[must_use]
    pub const fn idle_interval(self) -> Duration {
        self.idle_interval
    }

    /// Configured recovery byte bound.
    #[must_use]
    pub const fn maximum_uncheckpointed_bytes(self) -> u64 {
        self.maximum_uncheckpointed_bytes
    }

    /// Configured recovery interval bound.
    #[must_use]
    pub const fn maximum_uncheckpointed_interval(self) -> Duration {
        self.maximum_uncheckpointed_interval
    }
}

/// Why injected runtime configuration was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CheckpointRuntimeConfigError {
    /// No inactivity interval was supplied.
    MissingIdleInterval,
    /// No byte bound was supplied.
    MissingMaximumBytes,
    /// No interval bound was supplied.
    MissingMaximumInterval,
    /// A zero inactivity interval would close every window immediately.
    ZeroIdleInterval,
    /// A zero byte bound would fire before any work exists.
    ZeroMaximumBytes,
    /// A zero interval bound would fire before any work exists.
    ZeroMaximumInterval,
}

impl fmt::Display for CheckpointRuntimeConfigError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::MissingIdleInterval => "idle interval is required",
            Self::MissingMaximumBytes => "maximum uncheckpointed bytes is required",
            Self::MissingMaximumInterval => "maximum uncheckpointed interval is required",
            Self::ZeroIdleInterval => "idle interval must be greater than zero",
            Self::ZeroMaximumBytes => "maximum uncheckpointed bytes must be greater than zero",
            Self::ZeroMaximumInterval => {
                "maximum uncheckpointed interval must be greater than zero"
            }
        })
    }
}

impl std::error::Error for CheckpointRuntimeConfigError {}

/// Opening or driving the coordinator failed.
#[derive(Debug, PartialEq, Eq)]
pub enum CheckpointRuntimeError<E> {
    /// Required non-default configuration was invalid.
    Configuration(CheckpointRuntimeConfigError),
    /// The recovery-preservation state machine refused the transition.
    Recovery(RecoveryMachineError<E>),
    /// The caller attempted to route a trigger through the wrong runtime entry point.
    WrongTriggerClass,
    /// The observed byte count overflowed rather than silently wrapping.
    ByteCountOverflow,
    /// There is no open window to settle.
    NoOpenWindow,
    /// The settled extent has no durable save acknowledgement bound to it.
    NoDurableAcknowledgement,
    /// The independently verified acknowledgement disagreed with the persisted pending witness.
    PendingAcknowledgementMismatch,
}

/// Proof that the configured inactivity interval elapsed for one exact window extent.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SettledWindow {
    through: RecoverySequence,
}

impl SettledWindow {
    /// Exact final event established before the inactivity interval elapsed.
    #[must_use]
    pub const fn through(self) -> RecoverySequence {
        self.through
    }
}

impl<E: fmt::Display> fmt::Display for CheckpointRuntimeError<E> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Configuration(error) => error.fmt(formatter),
            Self::Recovery(error) => error.fmt(formatter),
            Self::WrongTriggerClass => {
                formatter.write_str("trigger used through the wrong entry point")
            }
            Self::ByteCountOverflow => formatter.write_str("uncheckpointed byte count overflowed"),
            Self::NoOpenWindow => formatter.write_str("no activity window is open"),
            Self::NoDurableAcknowledgement => {
                formatter.write_str("the settled window has no durable acknowledgement to recover")
            }
            Self::PendingAcknowledgementMismatch => formatter.write_str(
                "the verified acknowledgement does not match the pending settled window",
            ),
        }
    }
}

impl<E: fmt::Debug + fmt::Display> std::error::Error for CheckpointRuntimeError<E> {}

/// One per-view automatic checkpoint coordinator.
#[derive(Debug)]
pub struct CheckpointCoordinator<P: RecoveryStatePersistence> {
    config: CheckpointRuntimeConfig,
    machine: RecoveryMachine<P>,
    uncheckpointed_bytes: u64,
}

impl<P: RecoveryStatePersistence> CheckpointCoordinator<P> {
    /// Open from durable recovery state with explicit configuration.
    pub fn open(
        persistence: P,
        parameters: CheckpointRuntimeParameters,
    ) -> Result<Self, CheckpointRuntimeError<P::Error>> {
        let config = CheckpointRuntimeConfig::from_parameters(parameters)
            .map_err(CheckpointRuntimeError::Configuration)?;
        let machine =
            RecoveryMachine::open(persistence).map_err(CheckpointRuntimeError::Recovery)?;
        Ok(Self {
            config,
            machine,
            uncheckpointed_bytes: 0,
        })
    }

    /// Observe one event and extend the open window.
    pub fn observe(
        &mut self,
        sequence: RecoverySequence,
        changed_bytes: u64,
    ) -> Result<(), CheckpointRuntimeError<P::Error>> {
        let next_bytes = self.validated_next_bytes(sequence, changed_bytes)?;
        self.machine.observe(sequence).map_err(|error| {
            CheckpointRuntimeError::Recovery(RecoveryMachineError::State(error))
        })?;
        self.uncheckpointed_bytes = next_bytes;
        Ok(())
    }

    /// Observe an event whose canonical journal append has already become durable.
    ///
    /// The open-window update is atomically persisted. A persistence failure leaves both the
    /// in-memory window and byte counter unchanged, allowing the composition root to report that
    /// journal truth exists while no checkpoint acknowledgement was returned.
    pub fn observe_durable(
        &mut self,
        sequence: RecoverySequence,
        changed_bytes: u64,
        stamp: RecoveryStamp,
        acknowledgement: PrivateSaved,
    ) -> Result<(), CheckpointRuntimeError<P::Error>> {
        self.validate_strict_observation_sequence(sequence)?;
        let next_bytes = self.validated_next_bytes(sequence, changed_bytes)?;
        self.machine
            .observe_durable(sequence, stamp, acknowledgement)
            .map_err(CheckpointRuntimeError::Recovery)?;
        self.uncheckpointed_bytes = next_bytes;
        Ok(())
    }

    /// Validate one prospective observation without changing memory or durable state.
    ///
    /// Composition roots use this before an irreversible save, then call [`Self::observe`] only
    /// after the save's immutable journal append succeeded. Holding the coordinator exclusively
    /// across both calls makes the second validation identical to the first.
    pub fn validate_observation(
        &self,
        sequence: RecoverySequence,
        changed_bytes: u64,
    ) -> Result<(), CheckpointRuntimeError<P::Error>> {
        self.validate_strict_observation_sequence(sequence)?;
        self.validated_next_bytes(sequence, changed_bytes).map(drop)
    }

    fn validate_strict_observation_sequence(
        &self,
        sequence: RecoverySequence,
    ) -> Result<(), CheckpointRuntimeError<P::Error>> {
        let snapshot = self.machine.snapshot();
        let latest = [
            snapshot.open_window().map(|window| window.last()),
            snapshot
                .latest_recovery()
                .map(|recovery| recovery.through()),
            snapshot
                .last_meaningful()
                .map(|checkpoint| checkpoint.through()),
        ]
        .into_iter()
        .flatten()
        .max();
        if latest.is_some_and(|latest| sequence <= latest) {
            return Err(CheckpointRuntimeError::Recovery(
                RecoveryMachineError::State(crate::RecoveryStateError::SequenceOutsideWindow),
            ));
        }
        Ok(())
    }

    fn validated_next_bytes(
        &self,
        sequence: RecoverySequence,
        changed_bytes: u64,
    ) -> Result<u64, CheckpointRuntimeError<P::Error>> {
        if self
            .machine
            .snapshot()
            .open_window()
            .is_some_and(|window| sequence < window.last())
        {
            return Err(CheckpointRuntimeError::Recovery(
                RecoveryMachineError::State(crate::RecoveryStateError::SequenceOutsideWindow),
            ));
        }
        self.uncheckpointed_bytes
            .checked_add(changed_bytes)
            .ok_or(CheckpointRuntimeError::ByteCountOverflow)
    }

    /// Record one of the three candidate boundary triggers without moving a durable pointer.
    pub fn record_boundary(
        &mut self,
        trigger: RecoveryTrigger,
        evidence: RecoveryBoundaryEvidence,
    ) -> Result<RecoveryTransition, CheckpointRuntimeError<P::Error>> {
        if trigger.effect() != TriggerEffect::EvidenceOnly {
            return Err(CheckpointRuntimeError::WrongTriggerClass);
        }
        self.machine
            .apply(trigger, RecoveryTriggerInput::Evidence(evidence))
            .map_err(CheckpointRuntimeError::Recovery)
    }

    /// Atomically admit a native mutation's activity, boundary evidence, and verified recovery.
    pub fn observe_evidence_and_recover(
        &mut self,
        sequence: RecoverySequence,
        changed_bytes: u64,
        evidence_trigger: RecoveryTrigger,
        evidence: RecoveryBoundaryEvidence,
        recovery_trigger: RecoveryTrigger,
        recovery: RecoveryPreserved,
    ) -> Result<RecoveryTransition, CheckpointRuntimeError<P::Error>> {
        self.validate_strict_observation_sequence(sequence)?;
        self.validated_next_bytes(sequence, changed_bytes)?;
        let transition = self
            .machine
            .observe_evidence_and_recover(
                sequence,
                evidence_trigger,
                evidence,
                recovery_trigger,
                recovery,
            )
            .map_err(CheckpointRuntimeError::Recovery)?;
        if matches!(transition, RecoveryTransition::RecoveryPreserved(_)) {
            self.uncheckpointed_bytes = 0;
        }
        Ok(transition)
    }

    /// Preserve verified recovery bytes for an immediate recovery-only trigger.
    pub fn preserve(
        &mut self,
        trigger: RecoveryTrigger,
        recovery: RecoveryPreserved,
    ) -> Result<RecoveryTransition, CheckpointRuntimeError<P::Error>> {
        if trigger.effect() != TriggerEffect::RecoveryOnly
            || trigger == RecoveryTrigger::MaximumUncheckpointedBytesOrTime
        {
            return Err(CheckpointRuntimeError::WrongTriggerClass);
        }
        let transition = self
            .machine
            .apply(trigger, RecoveryTriggerInput::Recovery(recovery))
            .map_err(CheckpointRuntimeError::Recovery)?;
        if matches!(
            transition,
            RecoveryTransition::RecoveryPreserved(_) | RecoveryTransition::Duplicate
        ) {
            self.uncheckpointed_bytes = 0;
        }
        Ok(transition)
    }

    /// Preserve verified recovery bytes when either configured maximum has been reached.
    pub fn preserve_at_maximum(
        &mut self,
        elapsed: Duration,
        recovery: RecoveryPreserved,
    ) -> Result<Option<RecoveryTransition>, CheckpointRuntimeError<P::Error>> {
        if self.uncheckpointed_bytes < self.config.maximum_uncheckpointed_bytes
            && elapsed < self.config.maximum_uncheckpointed_interval
        {
            return Ok(None);
        }
        let transition = self
            .machine
            .apply(
                RecoveryTrigger::MaximumUncheckpointedBytesOrTime,
                RecoveryTriggerInput::Recovery(recovery),
            )
            .map_err(CheckpointRuntimeError::Recovery)?;
        if matches!(
            transition,
            RecoveryTransition::RecoveryPreserved(_) | RecoveryTransition::Duplicate
        ) {
            self.uncheckpointed_bytes = 0;
        }
        Ok(Some(transition))
    }

    /// Whether the current open window has reached either configured recovery-only bound.
    ///
    /// Composition roots use this before reading or hashing the immutable recovery source. The
    /// authoritative check still occurs inside [`Self::preserve_at_maximum`]; this predicate only
    /// avoids manufacturing an artifact while neither maximum is due.
    #[must_use]
    pub fn recovery_due(&self, elapsed: Duration) -> bool {
        self.machine.snapshot().open_window().is_some()
            && (self.uncheckpointed_bytes >= self.config.maximum_uncheckpointed_bytes
                || elapsed >= self.config.maximum_uncheckpointed_interval)
    }

    /// Establish that the configured inactivity interval elapsed for the current final event.
    #[must_use]
    pub fn settled_window(&self, elapsed: Duration) -> Option<SettledWindow> {
        if elapsed < self.config.idle_interval {
            return None;
        }
        self.machine
            .snapshot()
            .open_window()
            .map(|window| SettledWindow {
                through: window.last(),
            })
    }

    /// Return the exact persisted save that can settle after the configured idle interval.
    #[must_use]
    pub fn pending_settlement(
        &self,
        elapsed: Duration,
    ) -> Option<(SettledWindow, PendingMeaningfulSave)> {
        let settled = self.settled_window(elapsed)?;
        let pending = self.machine.snapshot().pending_meaningful()?;
        (pending.through() == settled.through).then_some((settled, pending))
    }

    /// Close a pending restart window using an acknowledgement independently reproduced by the
    /// store from immutable records.
    pub fn save_verified_pending(
        &mut self,
        settled: SettledWindow,
        acknowledgement: PrivateSaved,
    ) -> Result<RecoveryTransition, CheckpointRuntimeError<P::Error>> {
        let pending = self
            .machine
            .snapshot()
            .pending_meaningful()
            .ok_or(CheckpointRuntimeError::NoDurableAcknowledgement)?;
        if pending.through() != settled.through
            || pending.index_digest() != acknowledgement.index_digest()
            || pending.counts()
                != (
                    acknowledgement.operations(),
                    acknowledgement.manifests(),
                    acknowledgement.chunks(),
                )
        {
            return Err(CheckpointRuntimeError::PendingAcknowledgementMismatch);
        }
        self.save_settled(settled, pending.stamp(), acknowledgement)
    }

    /// Admit the acknowledgement returned by saving the exact settled extent.
    pub fn save_settled(
        &mut self,
        settled: SettledWindow,
        stamp: RecoveryStamp,
        acknowledgement: PrivateSaved,
    ) -> Result<RecoveryTransition, CheckpointRuntimeError<P::Error>> {
        if self.machine.snapshot().open_window().is_none() {
            return Err(CheckpointRuntimeError::NoOpenWindow);
        }
        if let Some(pending) = self.machine.snapshot().pending_meaningful() {
            if pending.through() != settled.through
                || pending.stamp() != stamp
                || pending.index_digest() != acknowledgement.index_digest()
                || pending.counts()
                    != (
                        acknowledgement.operations(),
                        acknowledgement.manifests(),
                        acknowledgement.chunks(),
                    )
            {
                return Err(CheckpointRuntimeError::PendingAcknowledgementMismatch);
            }
        }
        let transition = self
            .machine
            .apply(
                RecoveryTrigger::ActorBecameIdleAfterSettling,
                RecoveryTriggerInput::Meaningful {
                    stamp,
                    through: settled.through,
                    acknowledgement,
                },
            )
            .map_err(CheckpointRuntimeError::Recovery)?;
        self.uncheckpointed_bytes = 0;
        Ok(transition)
    }

    /// Validated runtime configuration.
    #[must_use]
    pub const fn config(&self) -> CheckpointRuntimeConfig {
        self.config
    }

    /// Current durable and open-window truth.
    #[must_use]
    pub const fn machine(&self) -> &RecoveryMachine<P> {
        &self.machine
    }

    /// Transfer the persistence owner for restart.
    #[must_use]
    pub fn into_persistence(self) -> P {
        self.machine.into_persistence()
    }
}
