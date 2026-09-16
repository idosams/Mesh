//! Derive the protected shared version from durable publication authority.
//!
//! Legacy Ed25519 reviewer keys remain readable but cannot advance this head. Production authority
//! is a v1 ES256 receipt whose public credential was enrolled by the native host, whose statement
//! binds the exact recomputed review context, and whose one-time challenge and approval record are
//! both durable. Any malformed, untrusted, contradictory, or replayed authority poisons the answer
//! fail-closed rather than being skipped.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, PoisonError, RwLock};

use mesh_approval::{
    verify_human_approval_receipt, Digest32, ExpectedHumanApproval, HeadId, HumanApprovalContext,
    HumanApprovalCredential, HumanApprovalReceipt,
};
use mesh_store::{RecordDigest, ReviewRecord, ReviewVerdict, StoredRecord};
use mesh_types::PublicKey;

/// The explicit parent before the first shared publication.
pub const GENESIS_SHARED_HEAD: HeadId = HeadId::from_bytes([0; 32]);

/// Human reviewer keys the daemon is allowed to trust for shared publication.
///
/// An empty set means trust has not been configured, not that every key is trusted.
#[derive(Clone, Debug, Default)]
pub struct TrustedReviewers {
    state: Arc<RwLock<TrustedReviewerState>>,
}

#[derive(Debug, Default)]
struct TrustedReviewerState {
    keys: BTreeSet<PublicKey>,
    human_credentials: BTreeMap<Digest32, HumanApprovalCredential>,
}

impl TrustedReviewers {
    /// Build an explicit trust set. Duplicate keys collapse to one identity.
    #[must_use]
    pub fn new(keys: impl IntoIterator<Item = PublicKey>) -> Self {
        Self {
            state: Arc::new(RwLock::new(TrustedReviewerState {
                keys: keys.into_iter().collect(),
                human_credentials: BTreeMap::new(),
            })),
        }
    }

    /// Build trust from OS-enrolled, user-verifying human credentials.
    #[must_use]
    pub fn with_human_credentials(
        credentials: impl IntoIterator<Item = HumanApprovalCredential>,
    ) -> Self {
        let trust = Self::default();
        for credential in credentials {
            trust.enroll_human_credential(credential);
        }
        trust
    }

    /// Add the public half of a credential that was enrolled by the native platform adapter.
    pub fn enroll_human_credential(&self, credential: HumanApprovalCredential) {
        self.state
            .write()
            .unwrap_or_else(PoisonError::into_inner)
            .human_credentials
            .insert(credential.id(), credential);
    }

    /// Whether a daemon operator supplied at least one trusted reviewer key.
    #[must_use]
    pub fn is_configured(&self) -> bool {
        let state = self.state.read().unwrap_or_else(PoisonError::into_inner);
        !state.keys.is_empty() || !state.human_credentials.is_empty()
    }

    pub(crate) fn has_human_credentials(&self) -> bool {
        !self
            .state
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .human_credentials
            .is_empty()
    }

    pub(crate) fn human_credential(&self, id: Digest32) -> Option<HumanApprovalCredential> {
        self.state
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .human_credentials
            .get(&id)
            .cloned()
    }
}

/// The shared-version answer derived from one immutable record sequence.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SharedVersion {
    /// No trust root was supplied, so the daemon must refuse to claim an answer.
    TrustNotConfigured,
    /// Trust exists, but no valid durable user-verified approval can be folded.
    HumanAuthorityUnavailable,
    /// At least one exact user-verified receipt produced a valid linear advance.
    Available(HeadId),
}

impl SharedVersion {
    pub(crate) const fn head(self) -> Option<HeadId> {
        match self {
            Self::TrustNotConfigured => None,
            Self::HumanAuthorityUnavailable => None,
            Self::Available(head) => Some(head),
        }
    }

    pub(crate) const fn is_answered(self) -> bool {
        matches!(self, Self::Available(_))
    }
}

/// Preserve the legacy Ed25519 surface as read-only, non-authoritative compatibility behavior.
pub(crate) fn fold<F: mesh_cas::DurableFs>(
    _records: &[StoredRecord],
    _store: &mesh_cas::Cas<F, mesh_cas::Blake3>,
    trusted: &TrustedReviewers,
) -> SharedVersion {
    if !trusted.is_configured() {
        return SharedVersion::TrustNotConfigured;
    }
    SharedVersion::HumanAuthorityUnavailable
}

/// Derive a protected head from exact v1 receipts and independently recomputed review contexts.
///
/// A malformed or contradictory approval poisons the answer rather than being skipped. The
/// journal remains readable, but no shared head is claimed from a sequence containing ambiguous
/// authority.
pub(crate) fn fold_human<F: mesh_cas::DurableFs>(
    records: &[StoredRecord],
    store: &mesh_cas::Cas<F, mesh_cas::Blake3>,
    trusted: &TrustedReviewers,
    mut context_for: impl FnMut(&ReviewRecord, HeadId) -> Option<HumanApprovalContext>,
) -> SharedVersion {
    if !trusted.is_configured() {
        return SharedVersion::TrustNotConfigured;
    }

    let mut reviews = BTreeMap::<RecordDigest, ReviewRecord>::new();
    let mut used_challenges = BTreeSet::<[u8; 32]>::new();
    let mut current = GENESIS_SHARED_HEAD;
    let mut advanced = false;

    for record in records {
        match record {
            StoredRecord::Review(review) => {
                reviews.insert(review.bundle, *review);
            }
            StoredRecord::Approval(approval) if approval.verdict == ReviewVerdict::Approved => {
                let Some(review) = reviews.get(&approval.bundle) else {
                    return SharedVersion::HumanAuthorityUnavailable;
                };
                let Ok(bytes) = store.read(&mesh_cas::Digest32::from_bytes(
                    *approval.approval.as_bytes(),
                )) else {
                    return SharedVersion::HumanAuthorityUnavailable;
                };
                let Ok(receipt) = HumanApprovalReceipt::from_canonical_bytes(&bytes) else {
                    return SharedVersion::HumanAuthorityUnavailable;
                };
                let carried = receipt.draft().expected();
                let credential_id = carried.credential().id();
                let Some(credential) = trusted.human_credential(credential_id) else {
                    return SharedVersion::HumanAuthorityUnavailable;
                };
                let Some(context) = context_for(review, current) else {
                    return SharedVersion::HumanAuthorityUnavailable;
                };
                let challenge = *carried.challenge();
                let expected = ExpectedHumanApproval::new(context, credential, challenge);
                if expected.context().expected_canonical_head() != current
                    || approval.bundle.as_bytes()
                        != expected.context().review_bundle().digest().as_bytes()
                    || approval.approver.as_bytes() != credential_id.as_bytes()
                    || challenge == [0; 32]
                    || !used_challenges.insert(challenge)
                    || verify_human_approval_receipt(&bytes, &expected).is_err()
                {
                    return SharedVersion::HumanAuthorityUnavailable;
                }
                current = expected.context().reviewed_actor_head();
                advanced = true;
            }
            _ => {}
        }
    }

    if advanced {
        SharedVersion::Available(current)
    } else {
        SharedVersion::HumanAuthorityUnavailable
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> std::path::PathBuf {
        let mut path = std::env::temp_dir();
        path.push(format!("mesh-publication-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        path
    }

    #[test]
    fn configured_keys_still_do_not_claim_humanheld_authority() {
        let root = scratch("trust-state");
        let store = mesh_cas::Cas::open(&root).expect("CAS");
        assert_eq!(
            fold(&[], &store, &TrustedReviewers::default()),
            SharedVersion::TrustNotConfigured
        );
        assert_eq!(
            fold(
                &[],
                &store,
                &TrustedReviewers::new([PublicKey::from_bytes([7; 32])])
            ),
            SharedVersion::HumanAuthorityUnavailable
        );
        let configured = fold(
            &[],
            &store,
            &TrustedReviewers::new([PublicKey::from_bytes([7; 32])]),
        );
        assert_eq!(configured.head(), None);
        assert!(!configured.is_answered());
        let _ = std::fs::remove_dir_all(root);
    }
}
