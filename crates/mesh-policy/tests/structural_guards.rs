//! Guards over this crate's own source, so the structural properties are enforced by the merge
//! gate and not only by the prose that claims them.
//!
//! # Why a source scan rather than a compile-fail harness
//!
//! `crates/mesh-policy/src/lib.rs` carries two `compile_fail` doctests — an agent's capability
//! offered to the publication guard (`E0308`) and a widened delegation (`E0599`). Those are the
//! real negative compile tests and `cargo test -p mesh-policy --doc` runs them.
//!
//! `npm test` does not: `verify:rust` is `cargo nextest run --workspace`, and **nextest does not
//! run doctests**. So a lane that deleted either proof would not turn the merge gate red. This file
//! closes that: it asserts the proofs are still in the source, and asserts the four structural
//! properties the prose claims, under a target nextest does run.
//!
//! A `trybuild`-style harness would be the tidier answer and is not available — a third-party
//! dependency in this workspace is bounded by ADR-0014 to an audited cryptographic primitive, and a
//! test harness is not one.

const LIB: &str = include_str!("../src/lib.rs");
const DECISION: &str = include_str!("../src/decision.rs");
const LEDGER: &str = include_str!("../src/ledger.rs");
const PUBLICATION: &str = include_str!("../src/publication.rs");
const PRINCIPAL: &str = include_str!("../src/principal.rs");
const EPOCH: &str = include_str!("../src/epoch.rs");
const REVOCATION: &str = include_str!("../src/revocation.rs");
const SESSION: &str = include_str!("../src/session.rs");

const SOURCES: [(&str, &str); 8] = [
    ("lib.rs", LIB),
    ("decision.rs", DECISION),
    ("ledger.rs", LEDGER),
    ("publication.rs", PUBLICATION),
    ("principal.rs", PRINCIPAL),
    ("epoch.rs", EPOCH),
    ("revocation.rs", REVOCATION),
    ("session.rs", SESSION),
];

/// Source with every run of whitespace collapsed, so a signature that `rustfmt` wrapped across
/// lines still matches the text a test looks for.
fn flattened(source: &str) -> String {
    source.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The two negative compile proofs are still in the crate documentation.
#[test]
fn the_compile_fail_proofs_are_present() {
    assert_eq!(
        LIB.matches("```compile_fail").count(),
        2,
        "a `compile_fail` proof was removed from lib.rs; nextest would not have noticed"
    );
    assert!(LIB.contains("```compile_fail,E0308"), "the tier proof");
    assert!(LIB.contains("```compile_fail,E0599"), "the widening proof");
    assert!(
        flattened(LIB).contains("DelegatedAction::AdvanceCanonicalHead"),
        "the widening proof must ask for the variant that does not exist"
    );
}

/// The publication guard is monomorphic in the human tier and takes an enrolled human. Both, in the
/// signature, so an agent's values cannot be passed.
#[test]
fn the_publication_guard_takes_a_human_and_a_human_held_capability() {
    let ledger = flattened(LEDGER);
    let at = ledger
        .find("pub fn authorize_publication(")
        .expect("the ledger's publication door");
    let signature = &ledger[at..at + 300];
    assert!(
        signature.contains("approver: &HumanPrincipal"),
        "the guard must take an enrolled human: {signature}"
    );
    assert!(
        signature.contains("capability: &Capability<HumanHeld>"),
        "the guard must be monomorphic in the human tier: {signature}"
    );
    assert!(
        !signature.contains("<T: AuthorityTier>"),
        "a generic publication guard accepts a delegated capability: {signature}"
    );
}

/// The gates are reachable only through the ledger, so no decision escapes the record.
///
/// The free functions that decide are `pub(crate)`; the only public doors are the two
/// `DecisionLedger` methods, and each returns the extended ledger beside the outcome.
#[test]
fn the_gates_are_not_public() {
    assert!(
        flattened(DECISION).contains("pub(crate) fn authorize<T: AuthorityTier>("),
        "the general gate must be crate-private"
    );
    assert!(
        flattened(DECISION).contains("pub(crate) fn check_preconditions<T: AuthorityTier>("),
        "the shared precondition check must be crate-private"
    );
    assert!(
        flattened(PUBLICATION).contains("pub(crate) fn authorize_publication("),
        "the publication gate must be crate-private"
    );
    assert!(
        !flattened(LIB).contains("pub use crate::decision::authorize"),
        "the crate root must not re-export a gate"
    );
    assert!(
        !flattened(LIB).contains("pub use crate::publication::authorize_publication"),
        "the crate root must not re-export the publication gate"
    );
    // Both public doors return the ledger, so a decision cannot be reached without its record.
    let ledger = flattened(LEDGER);
    for door in [
        "pub fn authorize<T: AuthorityTier>(",
        "pub fn authorize_publication(",
    ] {
        let at = ledger
            .find(door)
            .unwrap_or_else(|| panic!("{door} is the public door"));
        let signature = &ledger[at..at + 320];
        assert!(
            signature.contains("self,") && signature.contains("(Self,"),
            "{door} must consume the ledger and return the extended one: {signature}"
        );
    }
}

/// The authority witnesses have no constructor a caller outside this crate can reach, and no
/// `Default`. A public constructor is a way to hold authority nobody granted.
#[test]
fn the_authority_witnesses_have_no_public_constructor() {
    // `Grant::new` exists so the gate can build one, and it is crate-private.
    let decision = flattened(DECISION);
    let grant_impl = decision
        .find("impl<T: AuthorityTier> Grant<T> {")
        .expect("the Grant impl block");
    let grant = &decision[grant_impl..];
    assert!(
        grant.starts_with("impl<T: AuthorityTier> Grant<T> { pub(crate) const fn new("),
        "Grant's constructor must be the first item in its impl and crate-private"
    );

    // `PublicationAuthority` has no constructor at all: the guard builds it with a struct literal
    // from inside the module, which no other crate can write.
    let publication = flattened(PUBLICATION);
    assert!(
        !publication.contains("pub fn new(") && !publication.contains("pub const fn new("),
        "PublicationAuthority must have no public constructor"
    );

    for (name, source) in SOURCES {
        for witness in ["Grant", "PublicationAuthority", "HumanPrincipal", "Denial"] {
            assert!(
                !flattened(source).contains(&format!("impl Default for {witness}")),
                "{name} gives {witness} a Default, which is authority from nothing"
            );
        }
    }

    // The only constructor of a human principal is the fallible one, and there is no `From`.
    assert!(flattened(PRINCIPAL)
        .contains("pub fn enrol(principal: Principal) -> Result<Self, PrincipalError>"));
    assert!(!flattened(PRINCIPAL).contains("impl From<Principal> for HumanPrincipal"));
}

/// No trait in this crate hands an implementor a permissive default, and nothing returns a bare
/// `true` from a default method. Fail closed means an implementor cannot inherit "yes".
#[test]
fn no_default_method_answers_yes() {
    for (name, source) in SOURCES {
        assert!(
            !flattened(source).contains("pub trait "),
            "{name} declares a trait; a trait here is a seam an implementor can answer `true` at, \
             and this crate has no such seam by design"
        );
        assert!(
            !flattened(source).contains("-> bool { true }"),
            "{name} has a method that always answers yes"
        );
    }
}

/// No wall clock anywhere. `now` is an argument on every path that needs one.
#[test]
fn nothing_reads_a_clock() {
    for (name, source) in SOURCES {
        for needle in [
            "SystemTime",
            "Instant::now",
            "UNIX_EPOCH",
            "std::time",
            "chrono",
        ] {
            assert!(
                !source.contains(needle),
                "{name} names `{needle}`; ordering is lamport then event id then content hash, \
                 and validity is decided against a `now` the caller supplies"
            );
        }
    }
}

/// No ambient input or output. This is a `core-services` crate: it decides, it does not reach out.
#[test]
fn nothing_reaches_the_operating_system() {
    for (name, source) in SOURCES {
        for needle in ["std::fs", "std::net", "std::process", "std::os", "unsafe "] {
            assert!(!source.contains(needle), "{name} names `{needle}`");
        }
    }
}

/// The delegable vocabulary this crate gates over names nothing canonical. The same guard
/// `mesh-crypto` keeps over its own enum, restated over the operation table that consumes it.
#[test]
fn no_delegable_operation_names_canonical_state() {
    for action in mesh_crypto::DelegatedAction::ALL {
        assert!(
            !action.as_str().contains("canonical"),
            "`{action}` is delegable and names canonical state"
        );
    }
    let advancing: Vec<_> = mesh_policy::Operation::ALL
        .into_iter()
        .filter(|operation| operation.delegable_action().is_none())
        .collect();
    assert_eq!(advancing.len(), 1);
    assert!(advancing[0].advances_canonical_state());
}

/// Admission before catch-up is unrepresentable, and stays that way.
///
/// `src/session.rs` carries a third `compile_fail` proof, and nextest does not run doctests — the
/// same hole this file exists to close for the two in `lib.rs`. So the property is asserted over
/// the source: [`PendingSession`] has no `admit`, [`AdmittingSession`] has no public constructor,
/// and the only function that returns one consumes a pending session.
#[test]
fn admission_before_catch_up_is_unrepresentable() {
    let session = flattened(SESSION);

    assert!(
        session.contains("```compile_fail,E0599"),
        "the proof that `PendingSession` has no `admit` was removed"
    );

    let pending = session
        .find("impl PendingSession {")
        .expect("the PendingSession impl block");
    let admitting = session
        .find("impl AdmittingSession {")
        .expect("the AdmittingSession impl block");
    assert!(pending < admitting, "the impl blocks are in source order");
    assert!(
        !session[pending..admitting].contains("fn admit"),
        "PendingSession gained an admit method; the ordering is now a convention"
    );
    assert!(
        session[admitting..].contains("pub fn admit<T: AuthorityTier>("),
        "AdmittingSession must be the type that admits"
    );

    // The only route into the admitting state consumes the pending one.
    assert!(
        session.contains(
            "pub fn catch_up( self, offered: &[EpochRotation], asserted: PolicyHeadAssertion, ) \
             -> Result<AdmittingSession, CatchUpError>"
        ),
        "catch_up must consume the pending session and be the only producer of an AdmittingSession"
    );
    assert!(
        !session[admitting..].contains("pub fn new(")
            && !session[admitting..].contains("pub const fn new("),
        "AdmittingSession must have no public constructor"
    );
    assert!(
        !flattened(LIB).contains("pub use crate::session::AdmittingSession as"),
        "the crate root must not alias a way around the session type"
    );
}

/// A revocation entry can only come from a rotation record a peer verified, and a revocation only
/// ever moves earlier.
///
/// `RevocationLedger::record` is the single mutator and it is crate-private, so no caller outside
/// this crate can add a revocation that no rotation carried — or, far worse, move an existing one
/// forward, which would re-validate the records the first revocation refused.
#[test]
fn a_revocation_is_recorded_only_by_applying_a_rotation() {
    let revocation = flattened(REVOCATION);
    assert!(
        revocation.contains("pub(crate) fn record( self, subject: ActorKey, effective: PolicyEpoch, reason: RotationReason, ) -> Self"),
        "the only mutator of the revocation ledger must be crate-private and consume the ledger"
    );
    assert!(
        !revocation.contains("(&mut self")
            && !revocation.contains("( &mut self")
            && !revocation.contains("fn remove"),
        "the revocation ledger must be append-only and immutable"
    );
    assert!(
        flattened(EPOCH).contains("ledger.record(*subject, rotation.to, rotation.reason)"),
        "revocations must become effective in the epoch the rotation enters, never the one it leaves"
    );

    // Fail closed: the standing that means "this peer cannot tell" is not on the valid side.
    assert!(
        revocation.contains("Self::Stands => true,"),
        "exactly one authorship standing is valid"
    );
    assert!(
        revocation.contains(
            "Self::AuthoredAfterRevocation { .. } | Self::Indeterminate { .. } => false,"
        ),
        "an unobserved epoch must never read as validly authored"
    );
}
