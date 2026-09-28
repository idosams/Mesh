//! Durable correlation only: native transport must authenticate the peer and verify bytes.
//! An expired deadline is never evidence of termination or permission to reassign execution.
use super::{id_valid, refuse, Error};
use mesh_store::RecordDigest;

/// Immutable remote input/worker correlation plus a monotonically advancing lease.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RemoteAssignment {
    /// Objective-unique assignment identity, never reused for another lane or attempt.
    pub id: String,
    /// Canonical public-key identity authenticated separately by native transport.
    pub worker_key: String,
    /// Exact lane input version.
    pub input: RecordDigest,
    /// Integrity identity of the immutable transfer bundle, verified separately by native code.
    pub bundle: RecordDigest,
    /// Starts at one; only exact compare-and-advance renewal may change it.
    pub lease_sequence: u64,
    /// Native-authorized Unix expiry in milliseconds; expiry alone never releases ownership.
    pub lease_until_ms: u64,
}
impl RemoteAssignment {
    pub(super) fn validate(&self) -> Result<(), Error> {
        id_valid(&self.id)?;
        if self.id.len() > 96
            || self.lease_sequence != 1
            || self.lease_until_ms == 0
            || !RecordDigest::parse_hex(&self.worker_key)
                .is_ok_and(|key| key.to_string() == self.worker_key)
        {
            return refuse("remote-assignment-invalid");
        }
        Ok(())
    }
    pub(super) fn advance(
        &mut self,
        id: &str,
        worker_key: &str,
        expected: u64,
        until: u64,
    ) -> Result<(), Error> {
        if self.id != id || self.worker_key != worker_key {
            return refuse("remote-assignment-mismatch");
        }
        if self.lease_sequence != expected || until <= self.lease_until_ms {
            return refuse("remote-lease-stale");
        }
        let next = expected
            .checked_add(1)
            .ok_or(Error::Refused("remote-lease-exhausted"))?;
        self.lease_sequence = next;
        self.lease_until_ms = until;
        Ok(())
    }
}

#[test]
fn exhausted_remote_lease_does_not_mutate_retained_assignment() {
    let mut assignment = RemoteAssignment {
        id: "assignment".into(),
        worker_key: "ab".repeat(32),
        input: RecordDigest::from_bytes([1; 32]),
        bundle: RecordDigest::from_bytes([2; 32]),
        lease_sequence: u64::MAX,
        lease_until_ms: 1000,
    };
    let before = assignment.clone();
    assert!(matches!(
        assignment.advance("assignment", &"ab".repeat(32), u64::MAX, 2000),
        Err(Error::Refused("remote-lease-exhausted"))
    ));
    assert_eq!(assignment, before);
}
