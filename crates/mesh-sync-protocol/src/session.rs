//! The authentication seam, and the honest statement of what is unauthenticated today.
//!
//! # State this plainly, because the alternative is a false claim
//!
//! **Nothing in Mesh can produce a signature in production yet.** `mesh-crypto` verifies real
//! Ed25519 and holds no secret half: key custody is a platform backend that has not been built,
//! and the signer that exists there is a test dependency that ships in no binary. So a peer today
//! cannot present evidence of who it is, and no amount of protocol design changes that.
//!
//! What this module does is make that visible instead of implicit:
//!
//! * [`PeerAuthenticator`] is the seam a real verifier plugs into. This crate ships no verifier —
//!   it cannot, it has no dependency and no cryptography.
//! * [`NoAuthenticator`] is the only implementation here, and it answers
//!   [`AuthenticationOutcome::Unverified`] for every peer, never [`AuthenticationOutcome::Verified`].
//!   It is not a stub that pretends; it is a truthful answer to "is this peer verified", and the
//!   answer today is no.
//! * [`AuthenticationPolicy::RequireVerifiedPeers`] therefore admits *nothing* on the current tree.
//!   That is the point. A deployment that wants authenticated replication sets that policy and
//!   gets a refusal until a real [`PeerAuthenticator`] is supplied, rather than silently getting
//!   an unauthenticated session.
//!
//! A session under [`AuthenticationPolicy::AdmitUnverifiedPeers`] replicates between peers whose
//! identity is asserted and not checked. That is a local-development and single-user posture, and
//! it is the posture the two-actor exchange in `tests/two_actor_exchange.rs` runs under — stated
//! there too, so nobody reads that test as evidence of authenticated replication.

use crate::error::{ErrorCode, ProtocolError};
use crate::ids::ActorId;
use crate::message::{SyncMessage, PROTOCOL_VERSION};
use crate::plane::MessageKind;

/// What a verifier concluded about a peer.
///
/// Exactly one of `verified`, `unverified` and `refused`. `unverified` and `refused` are different
/// facts: the first is "no evidence was checked", the second is "evidence was checked and was
/// wrong". Collapsing them would make a missing signer indistinguishable from a forgery.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum AuthenticationOutcome {
    /// Evidence was checked and it holds: this peer is the actor it claims to be.
    Verified,
    /// No evidence was checked. The peer may be exactly who it says it is; nothing knows.
    Unverified,
    /// Evidence was checked and it does not hold.
    Refused,
}

/// Whether a session admits peers whose identity has not been verified.
///
/// Exactly one of `require verified peers` and `admit unverified peers`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum AuthenticationPolicy {
    /// Only a verified peer may send anything past the handshake. On the current tree this admits
    /// nothing at all, because no [`PeerAuthenticator`] that can answer
    /// [`AuthenticationOutcome::Verified`] exists yet.
    RequireVerifiedPeers,
    /// An unverified peer may replicate. A refused peer still may not: a failed check is worse
    /// than no check.
    AdmitUnverifiedPeers,
}

/// The seam a real verifier plugs into.
///
/// The implementation belongs to a composition root that can reach `mesh-crypto`; this crate has
/// no dependency and therefore no cryptography of its own. A verifier that always answers
/// [`AuthenticationOutcome::Verified`] is a defect, not a convenience — [`NoAuthenticator`] shows
/// the shape a truthful non-verifier takes.
pub trait PeerAuthenticator {
    /// What can be concluded about `actor` from `signature` over `challenge`.
    fn verify(
        &self,
        actor: &ActorId,
        challenge: &[u8; 32],
        signature: &[u8],
    ) -> AuthenticationOutcome;
}

/// The only [`PeerAuthenticator`] this crate ships: it verifies nothing and says so.
///
/// Every peer is [`AuthenticationOutcome::Unverified`]. It never answers
/// [`AuthenticationOutcome::Verified`] and never answers [`AuthenticationOutcome::Refused`],
/// because it checks nothing and so has learned nothing either way.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct NoAuthenticator;

impl PeerAuthenticator for NoAuthenticator {
    fn verify(
        &self,
        _actor: &ActorId,
        _challenge: &[u8; 32],
        _signature: &[u8],
    ) -> AuthenticationOutcome {
        AuthenticationOutcome::Unverified
    }
}

/// One side of one CWP session: who the peer claims to be, what was concluded about that claim,
/// and whether the handshake has completed.
///
/// Immutable. Every transition returns a new session, so a session can be held beside the message
/// that produced it and the pair replayed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Session {
    policy: AuthenticationPolicy,
    peer: Option<ActorId>,
    outcome: AuthenticationOutcome,
    established: bool,
}

impl Session {
    /// A session that has not yet seen a `HELLO`.
    #[must_use]
    pub const fn opening(policy: AuthenticationPolicy) -> Self {
        Self {
            policy,
            peer: None,
            outcome: AuthenticationOutcome::Unverified,
            established: false,
        }
    }

    /// The policy this session was opened under.
    #[must_use]
    pub const fn policy(&self) -> AuthenticationPolicy {
        self.policy
    }

    /// The actor the peer claims to be, once `HELLO` has been accepted.
    #[must_use]
    pub const fn peer(&self) -> Option<ActorId> {
        self.peer
    }

    /// What was concluded about the peer's claim.
    #[must_use]
    pub const fn outcome(&self) -> AuthenticationOutcome {
        self.outcome
    }

    /// Whether the handshake has completed, so messages past it may flow.
    #[must_use]
    pub const fn is_established(&self) -> bool {
        self.established
    }

    /// Whether this session's peer is verified. False on the current tree, always.
    #[must_use]
    pub const fn is_verified(&self) -> bool {
        matches!(self.outcome, AuthenticationOutcome::Verified)
    }

    /// Accept a handshake message, producing the session that follows it.
    ///
    /// `HELLO` records the claim and establishes the session if the policy allows an unverified
    /// peer. `AUTHENTICATE` runs the verifier and establishes the session if the outcome allows.
    ///
    /// # Errors
    ///
    /// [`ProtocolError`] when the message fails its own preconditions, when `AUTHENTICATE` arrives
    /// before `HELLO`, when the two name different actors or echo a different challenge, or when
    /// the policy refuses the outcome.
    pub fn accept_handshake(
        &self,
        message: &SyncMessage,
        authenticator: &dyn PeerAuthenticator,
    ) -> Result<Self, ProtocolError> {
        message.check()?;
        match message {
            SyncMessage::Hello { actor, .. } => Ok(Self {
                policy: self.policy,
                peer: Some(*actor),
                outcome: AuthenticationOutcome::Unverified,
                established: self.policy == AuthenticationPolicy::AdmitUnverifiedPeers,
            }),
            SyncMessage::Authenticate {
                actor,
                challenge,
                signature,
            } => {
                let claimed = self.peer.ok_or_else(|| {
                    ProtocolError::new(
                        ErrorCode::UnauthenticatedSession,
                        MessageKind::Authenticate,
                        "AUTHENTICATE before HELLO: there is no claim to authenticate",
                    )
                })?;
                if claimed != *actor {
                    return Err(ProtocolError::new(
                        ErrorCode::UnknownActor,
                        MessageKind::Authenticate,
                        "the signature is offered for an actor this session did not claim",
                    ));
                }
                let outcome = authenticator.verify(actor, challenge, signature);
                if outcome == AuthenticationOutcome::Refused {
                    return Err(ProtocolError::new(
                        ErrorCode::UnverifiedPeer,
                        MessageKind::Authenticate,
                        "the signature was checked and does not hold",
                    ));
                }
                Ok(Self {
                    policy: self.policy,
                    peer: Some(*actor),
                    outcome,
                    established: self.admits(outcome),
                })
            }
            other => Err(ProtocolError::new(
                ErrorCode::MalformedMessage,
                other.kind(),
                "only HELLO and AUTHENTICATE establish a session",
            )),
        }
    }

    /// Whether this session may carry `message` right now.
    ///
    /// # Errors
    ///
    /// [`ProtocolError`] when the message needs an established session and this one is not
    /// established, when the peer is not admitted by the policy, or when the message fails its own
    /// preconditions.
    pub fn admit(&self, message: &SyncMessage) -> Result<(), ProtocolError> {
        let kind = message.kind();
        if kind.needs_established_session() {
            if !self.established {
                return Err(ProtocolError::new(
                    ErrorCode::UnauthenticatedSession,
                    kind,
                    "the session is not established: send HELLO first",
                ));
            }
            if !self.admits(self.outcome) {
                return Err(ProtocolError::new(
                    ErrorCode::UnverifiedPeer,
                    kind,
                    "this session requires verified peers and nothing has verified this one",
                ));
            }
        }
        message.check()
    }

    /// Whether the policy admits an outcome.
    const fn admits(&self, outcome: AuthenticationOutcome) -> bool {
        match self.policy {
            AuthenticationPolicy::RequireVerifiedPeers => {
                matches!(outcome, AuthenticationOutcome::Verified)
            }
            AuthenticationPolicy::AdmitUnverifiedPeers => {
                !matches!(outcome, AuthenticationOutcome::Refused)
            }
        }
    }

    /// The `HELLO` this side sends, at this crate's version and record encoding profile.
    #[must_use]
    pub fn hello(actor: ActorId, challenge: [u8; 32]) -> SyncMessage {
        SyncMessage::Hello {
            protocol_version: PROTOCOL_VERSION,
            encoding_profile: crate::RECORD_ENCODING_PROFILE.to_owned(),
            actor,
            challenge,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ids::ActorSequence;

    /// A verifier that answers whatever a test needs, so the policy logic can be exercised for
    /// outcomes no production code can produce today.
    struct FixedOutcome(AuthenticationOutcome);

    impl PeerAuthenticator for FixedOutcome {
        fn verify(
            &self,
            _actor: &ActorId,
            _challenge: &[u8; 32],
            _signature: &[u8],
        ) -> AuthenticationOutcome {
            self.0
        }
    }

    fn actor(byte: u8) -> ActorId {
        ActorId::from_bytes([byte; 32])
    }

    fn head_update() -> SyncMessage {
        SyncMessage::UpdateActorHead {
            actor: actor(1),
            head: crate::HeadId::from_bytes([1; 32]),
            sequence: ActorSequence::new(1),
        }
    }

    /// The claim this whole module exists to keep honest.
    #[test]
    fn the_only_shipped_authenticator_never_verifies_anybody() {
        let outcome = NoAuthenticator.verify(&actor(1), &[0; 32], &[1, 2, 3]);
        assert_eq!(outcome, AuthenticationOutcome::Unverified);
        assert_ne!(outcome, AuthenticationOutcome::Verified);
    }

    #[test]
    fn requiring_verified_peers_admits_nothing_on_this_tree() {
        let session = Session::opening(AuthenticationPolicy::RequireVerifiedPeers)
            .accept_handshake(&Session::hello(actor(1), [4; 32]), &NoAuthenticator)
            .expect("HELLO is accepted");
        assert!(!session.is_established());
        assert!(!session.is_verified());

        let after = session
            .accept_handshake(
                &SyncMessage::Authenticate {
                    actor: actor(1),
                    challenge: [4; 32],
                    signature: vec![9; 64],
                },
                &NoAuthenticator,
            )
            .expect("an unverified outcome is not a refusal");
        assert!(!after.is_established());
        assert_eq!(
            after.admit(&head_update()).expect_err("refused").code(),
            ErrorCode::UnauthenticatedSession
        );
    }

    #[test]
    fn admitting_unverified_peers_establishes_the_session_at_hello() {
        let session = Session::opening(AuthenticationPolicy::AdmitUnverifiedPeers)
            .accept_handshake(&Session::hello(actor(1), [4; 32]), &NoAuthenticator)
            .expect("HELLO is accepted");
        assert!(session.is_established());
        assert!(!session.is_verified());
        assert_eq!(session.peer(), Some(actor(1)));
        assert_eq!(session.admit(&head_update()), Ok(()));
    }

    #[test]
    fn a_verified_peer_is_admitted_under_the_strict_policy() {
        let verifier = FixedOutcome(AuthenticationOutcome::Verified);
        let session = Session::opening(AuthenticationPolicy::RequireVerifiedPeers)
            .accept_handshake(&Session::hello(actor(1), [4; 32]), &verifier)
            .and_then(|session| {
                session.accept_handshake(
                    &SyncMessage::Authenticate {
                        actor: actor(1),
                        challenge: [4; 32],
                        signature: vec![9; 64],
                    },
                    &verifier,
                )
            })
            .expect("a verified peer is admitted");
        assert!(session.is_established());
        assert!(session.is_verified());
        assert_eq!(session.admit(&head_update()), Ok(()));
    }

    #[test]
    fn a_refused_signature_is_refused_under_either_policy() {
        let verifier = FixedOutcome(AuthenticationOutcome::Refused);
        for policy in [
            AuthenticationPolicy::RequireVerifiedPeers,
            AuthenticationPolicy::AdmitUnverifiedPeers,
        ] {
            let session = Session::opening(policy)
                .accept_handshake(&Session::hello(actor(1), [4; 32]), &verifier)
                .expect("HELLO is accepted");
            let refusal = session
                .accept_handshake(
                    &SyncMessage::Authenticate {
                        actor: actor(1),
                        challenge: [4; 32],
                        signature: vec![9; 64],
                    },
                    &verifier,
                )
                .expect_err("a checked-and-wrong signature is refused");
            assert_eq!(refusal.code(), ErrorCode::UnverifiedPeer);
        }
    }

    #[test]
    fn authenticate_before_hello_is_refused() {
        let refusal = Session::opening(AuthenticationPolicy::AdmitUnverifiedPeers)
            .accept_handshake(
                &SyncMessage::Authenticate {
                    actor: actor(1),
                    challenge: [4; 32],
                    signature: vec![9; 64],
                },
                &NoAuthenticator,
            )
            .expect_err("there is no claim yet");
        assert_eq!(refusal.code(), ErrorCode::UnauthenticatedSession);
    }

    #[test]
    fn a_signature_offered_for_another_actor_is_refused() {
        let session = Session::opening(AuthenticationPolicy::AdmitUnverifiedPeers)
            .accept_handshake(&Session::hello(actor(1), [4; 32]), &NoAuthenticator)
            .expect("HELLO is accepted");
        let refusal = session
            .accept_handshake(
                &SyncMessage::Authenticate {
                    actor: actor(2),
                    challenge: [4; 32],
                    signature: vec![9; 64],
                },
                &NoAuthenticator,
            )
            .expect_err("the actor does not match the claim");
        assert_eq!(refusal.code(), ErrorCode::UnknownActor);
    }

    #[test]
    fn a_message_before_the_handshake_is_refused() {
        let opening = Session::opening(AuthenticationPolicy::AdmitUnverifiedPeers);
        assert_eq!(
            opening.admit(&head_update()).expect_err("refused").code(),
            ErrorCode::UnauthenticatedSession
        );
    }

    #[test]
    fn an_error_may_always_be_sent() {
        let opening = Session::opening(AuthenticationPolicy::RequireVerifiedPeers);
        let refusal = SyncMessage::Error(ProtocolError::new(
            ErrorCode::Busy,
            MessageKind::ChunkBatch,
            "later",
        ));
        assert_eq!(opening.admit(&refusal), Ok(()));
    }

    #[test]
    fn a_non_handshake_message_cannot_establish_a_session() {
        let refusal = Session::opening(AuthenticationPolicy::AdmitUnverifiedPeers)
            .accept_handshake(&head_update(), &NoAuthenticator)
            .expect_err("only HELLO and AUTHENTICATE establish");
        assert_eq!(refusal.code(), ErrorCode::MalformedMessage);
    }

    #[test]
    fn a_hello_that_fails_its_own_precondition_never_establishes() {
        let refusal = Session::opening(AuthenticationPolicy::AdmitUnverifiedPeers)
            .accept_handshake(
                &SyncMessage::Hello {
                    protocol_version: PROTOCOL_VERSION + 7,
                    encoding_profile: crate::RECORD_ENCODING_PROFILE.to_owned(),
                    actor: actor(1),
                    challenge: [0; 32],
                },
                &NoAuthenticator,
            )
            .expect_err("the version is refused");
        assert_eq!(refusal.code(), ErrorCode::UnsupportedVersion);
    }
}
