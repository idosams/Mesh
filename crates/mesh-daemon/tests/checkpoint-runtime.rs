//! Daemon facts route to the store coordinator without changing trigger semantics.

use mesh_daemon::{route_boundary, BoundaryRouting, RecoveryRuntimeSignal};
use mesh_materializer::{
    BoundaryObservationV1, BoundaryReasonV1, CheckpointCandidateV1, EventSequence,
    RenameEvidenceUnavailable, ViewId,
};
use mesh_store::{BoundaryEvidenceKind, RecoverySequence, RecoveryTrigger};

#[test]
fn daemon_recovery_signals_cover_the_four_immediate_runtime_triggers() {
    assert_eq!(
        [
            RecoveryRuntimeSignal::IntegratedFlush,
            RecoveryRuntimeSignal::ActorProcessExited,
            RecoveryRuntimeSignal::ReviewOpened,
            RecoveryRuntimeSignal::ActorDisconnected,
        ]
        .map(RecoveryRuntimeSignal::trigger),
        [
            RecoveryTrigger::IntegratedAgentRequestsFlush,
            RecoveryTrigger::ActorProcessExited,
            RecoveryTrigger::UserOpenedReview,
            RecoveryTrigger::ActorDisconnected,
        ]
    );
}

#[test]
fn adapter_candidates_route_exactly_and_unsupported_stays_explicit() {
    for (reason, trigger, kind) in [
        (
            BoundaryReasonV1::Closed,
            RecoveryTrigger::ModifiedFileHandleClosed,
            BoundaryEvidenceKind::Closed,
        ),
        (
            BoundaryReasonV1::Synced,
            RecoveryTrigger::FsyncCompleted,
            BoundaryEvidenceKind::Synced,
        ),
        (
            BoundaryReasonV1::RenamedIntoPlace,
            RecoveryTrigger::AtomicReplacementCompleted,
            BoundaryEvidenceKind::RenamedIntoPlace,
        ),
    ] {
        let observation = BoundaryObservationV1::Candidate(CheckpointCandidateV1::new(
            ViewId::new(1),
            EventSequence::new(7),
            reason,
        ));
        assert_eq!(
            route_boundary(&observation),
            BoundaryRouting::Evidence {
                trigger,
                evidence: mesh_store::RecoveryBoundaryEvidence::new(
                    RecoverySequence::new(7).unwrap(),
                    kind,
                ),
            }
        );
    }

    assert_eq!(
        route_boundary(&BoundaryObservationV1::None),
        BoundaryRouting::None
    );
    assert_eq!(
        route_boundary(&BoundaryObservationV1::Unsupported(
            RenameEvidenceUnavailable::all()
        )),
        BoundaryRouting::Unsupported
    );
    assert_eq!(
        route_boundary(&BoundaryObservationV1::Candidate(
            CheckpointCandidateV1::new(
                ViewId::new(1),
                EventSequence::new(0),
                BoundaryReasonV1::Closed,
            )
        )),
        BoundaryRouting::Unsupported
    );
}
