//! Which identifier family names an actor — the question ADR-0003 settled.
//!
//! `docs/protocol.md` §3.1 listed `actor` among the entities named by an `entity ID`, a minted
//! UUIDv7, while plan §4.2 declares `ActorId` as a thirty-two-byte value derived from the actor's
//! key. Both were normative and they could not both be followed. The plan won: an actor is named
//! by a `record ID` derived from its `actor key` and from nothing else.
//!
//! Every test below pins one half of that ruling. The first four pin what it buys — one key is one
//! actor, everywhere, with no registry to consult and nothing mutable able to rename it. The fifth
//! pins what it costs, which is the part a later reader is most likely to have forgotten: there is
//! no re-keying. Rotating an `actor key` names a *different* actor, and every record that named the
//! old one keeps naming it.

use std::collections::HashSet;

use mesh_types::{
    Actor, ActorId, ActorKind, Blake3, ContentDigest, ObjectId, PublicKey, SessionId, Timestamp,
    WorkspaceId,
};

/// The domain the actor derivation absorbs first, restated here on purpose.
///
/// `mesh_types::actor` holds the same string in a private constant. Duplicating it is what makes
/// this file an independent check: editing the derivation's domain without editing this line fails
/// the test rather than silently renaming every actor in existence.
const ACTOR_KEY_DOMAIN: &str = "mesh.v0.actor-key";

/// A key from one repeated byte. Distinct seeds give distinct keys, which is all any test here
/// needs from them.
fn key(seed: u8) -> PublicKey {
    PublicKey::from_bytes([seed; 32])
}

fn actor(kind: ActorKind, public_key: PublicKey) -> Actor {
    Actor::new(
        kind,
        public_key,
        "display name".to_owned(),
        Timestamp::from_unix_millis(1_700_000_000_000),
    )
}

/// The derivation, recomputed by a path the implementation does not take.
///
/// `PublicKey::actor_id` feeds the hasher incrementally; this feeds one flat buffer to
/// `Blake3::digest_bytes`, which the `Blake3` implementation overrides with its own one-shot
/// routine. Two code paths agreeing on the same bytes is worth more than one path agreeing with
/// itself.
fn recomputed(public_key: &PublicKey) -> ActorId {
    let mut input = Vec::with_capacity(ACTOR_KEY_DOMAIN.len() + 32);
    input.extend_from_slice(ACTOR_KEY_DOMAIN.as_bytes());
    input.extend_from_slice(public_key.as_bytes());
    ActorId::from_digest(Blake3::digest_bytes(&input))
}

/// The settled answer, stated as an equation: identifier = digest(domain ‖ key).
#[test]
fn an_actor_identifier_is_the_digest_of_the_key_and_nothing_else() {
    let mut seen = HashSet::new();
    for seed in 0..=u8::MAX {
        let public_key = key(seed);
        let derived = public_key.actor_id::<Blake3>();
        assert_eq!(
            derived,
            recomputed(&public_key),
            "the actor derivation absorbs something other than the domain tag and the key"
        );
        assert_eq!(actor(ActorKind::Human, public_key).id::<Blake3>(), derived);
        seen.insert(derived);
    }
    assert_eq!(seen.len(), 256, "two keys produced one actor identifier");
}

/// No field of the actor record other than the key reaches the identifier.
///
/// This is the property the ruling rests on. If a display name could move an identifier, renaming
/// an actor would rename its whole authored history, and attribution would be editable by whoever
/// can edit a label.
#[test]
fn no_mutable_part_of_an_actor_record_changes_its_identifier() {
    let public_key = key(0x2a);
    let baseline = public_key.actor_id::<Blake3>();
    let sponsor = key(0x2b).actor_id::<Blake3>();

    let mut variants = vec![actor(ActorKind::Human, public_key)];
    for kind in ActorKind::ALL {
        variants.push(actor(kind, public_key));
    }
    variants.push(actor(ActorKind::Agent, public_key).with_display_name(String::new()));
    variants.push(actor(ActorKind::Agent, public_key).with_display_name("renamed".to_owned()));
    variants.push(actor(ActorKind::Agent, public_key).with_sponsor_human(sponsor));
    variants.push(actor(ActorKind::Agent, public_key).disabled_at(Timestamp::from_unix_millis(9)));
    variants.push(Actor::new(
        ActorKind::Validator,
        public_key,
        "another name entirely".to_owned(),
        Timestamp::from_unix_millis(0),
    ));

    for variant in &variants {
        assert_eq!(
            variant.id::<Blake3>(),
            baseline,
            "a mutable field renamed the actor: {variant:?}"
        );
    }
}

/// One key is one actor, whatever kind of participant holds it.
///
/// The kind is not absorbed, so it cannot separate two actors — which means an `agent` and each of
/// its `agent-run` executions must hold *distinct keys* to be distinct actors. Under the losing
/// reading a minted identifier per run would have separated them for free; here it is a
/// requirement on whoever mints keys, and this test is where that requirement is written down.
#[test]
fn one_key_is_one_actor_whatever_kind_holds_it() {
    let public_key = key(0x5c);
    let identifiers: HashSet<ActorId> = ActorKind::ALL
        .iter()
        .map(|kind| actor(*kind, public_key).id::<Blake3>())
        .collect();
    assert_eq!(
        identifiers.len(),
        1,
        "the actor kind reached the identifier, so one key names several actors"
    );

    let per_run: HashSet<ActorId> = (0..8)
        .map(|run| actor(ActorKind::AgentRun, key(0x80 + run)).id::<Blake3>())
        .collect();
    assert_eq!(
        per_run.len(),
        8,
        "distinct run keys must give distinct agent-run actors"
    );
}

/// The cost of the ruling, pinned so nobody rediscovers it in production.
///
/// There is no re-keying. A compromised or expired key does not get replaced inside one actor —
/// the new key names a new actor, and continuity between the two is an edge somebody has to author
/// and sign, not a field somebody can update. A test that asserted the opposite would be asserting
/// that identity can be transferred, which is exactly what the derivation exists to prevent.
#[test]
fn rotating_the_actor_key_names_a_new_actor() {
    let before = actor(ActorKind::Human, key(0x11));
    let after = Actor::new(
        before.kind(),
        key(0x12),
        before.display_name().to_owned(),
        before.created_at(),
    );

    assert_eq!(before.display_name(), after.display_name());
    assert_eq!(before.kind(), after.kind());
    assert_ne!(
        before.id::<Blake3>(),
        after.id::<Blake3>(),
        "a rotated key kept the old actor identifier, so identity outlived the key it is derived from"
    );
}

/// A `record ID` is not an `entity ID`, at the only boundary where the two could be confused.
///
/// The type system already makes the mistake unrepresentable in Rust — `ActorId` has no UUID
/// constructor and `entity_id.rs` carries `compile_fail` proofs that no entity identifier can be
/// built from a digest. This test covers the other direction: text arriving from outside. An actor
/// identifier's text form must never be accepted where a version-7 UUID is expected.
#[test]
fn an_actor_identifier_is_never_accepted_as_an_entity_identifier() {
    for seed in 0..=u8::MAX {
        let text = key(seed).actor_id::<Blake3>().to_string();
        assert_eq!(text.len(), 64, "an actor identifier is 64 hex characters");
        assert_eq!(
            ActorId::parse_hex(&text)
                .expect("an actor identifier round-trips through its hex form"),
            key(seed).actor_id::<Blake3>()
        );
        assert!(WorkspaceId::parse(&text).is_err());
        assert!(SessionId::parse(&text).is_err());
        assert!(ObjectId::parse(&text).is_err());
    }
}

/// One derivation, pinned to a constant, so drift is a test failure rather than a silent rename.
///
/// This is a stability pin and not an independent verification of BLAKE3 — `blake3_reference_vectors`
/// is that, against the published vectors. What moves this constant is a change to *this*
/// derivation: a different domain tag, a reordering, an extra absorbed field. Any of those renames
/// every actor that has ever existed, which is a compatibility event and not a test to update.
#[test]
fn the_actor_derivation_is_pinned() {
    const KEY_07_ACTOR_ID: &str =
        "1819d4707f2760856da798278b177f3aacddf9f7b672ed4dda63b9b48128b956";

    assert_eq!(key(7).actor_id::<Blake3>().to_string(), KEY_07_ACTOR_ID);
}
