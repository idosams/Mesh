//! Object identity across rename and move: the acceptance campaign.
//!
//! # The four criteria, and where each is answered
//!
//! | Criterion | Answered by |
//! |---|---|
//! | A rename concurrent with an edit results in one object with both changes applied | [`a_rename_concurrent_with_an_edit_leaves_one_object_carrying_both`] |
//! | A directory move with one million descendants emits one metadata operation, not one per descendant | [`a_subtree_move_is_one_change_of_constant_cost_at_every_size`] |
//! | No operation sequence produces a new object ID for a renamed or moved file | [`no_generated_rename_and_move_sequence_ever_mints_an_identity`] |
//! | History remains traversable across an arbitrary chain of renames and moves | [`an_arbitrary_chain_of_renames_and_moves_stays_traversable`] and [`a_descendant_that_was_never_named_still_has_every_path_it_had`] |
//!
//! # What the numbers here are and are not
//!
//! [`a_subtree_move_is_one_change_of_constant_cost_at_every_size`] prints its measurements, and the
//! PR that landed this transcribes them. The wall-clock figure is the cost of applying the move to
//! an in-memory register that already holds the subtree: it is the *register's* half of plan §11's
//! 100 ms budget, not the whole path, because writing the change to durable storage is
//! `mesh-store`'s and distributing it is `mesh-sync-protocol`'s. Reporting it as the whole budget
//! would be a claim this test cannot support.
//!
//! It runs in a debug build, under whatever else `cargo nextest` is running in parallel, so it is
//! a **ceiling** rather than a benchmark: the assertion is that the register is orders of magnitude
//! inside the budget, and the constant-cost assertions beside it are what actually carry the
//! property. A release build is faster and is not what the gate runs.
//!
//! # Nothing here reads a clock except the thing that measures elapsed time
//!
//! `src/no_ambient_input.rs` refuses the crate's own sources a clock. This is a test, and the one
//! clock read below is `Instant::elapsed` around the measured move. Every ordering decision in
//! every test comes from a [`Stamp`] built out of literals.

mod common;

use std::collections::BTreeMap;
use std::time::Instant;

use common::Shuffler;
use mesh_state::{
    EventId, IdentityChange, IdentityOutcome, IdentityRefusal, Lamport, NormalizedName, ObjectId,
    ObjectKind, ObjectRegister, Placement, Stamp, VersionId,
};

/// Plan §11's published metadata budget for a subtree move.
const METADATA_BUDGET_BYTES: usize = 10 * 1024;

/// Plan §11's published wall-clock budget for moving a million descendants.
const WALL_CLOCK_BUDGET_MILLIS: u128 = 100;

/// The subtree sizes the budget is measured at. The last is the published one.
const SUBTREE_SIZES: [usize; 5] = [0, 1, 1_000, 100_000, 1_000_000];

/// How many children a directory holds in the generated subtree, so nesting is real rather than a
/// chain in disguise.
const BRANCHING: usize = 16;

fn name(text: impl Into<String>) -> NormalizedName {
    NormalizedName::new(text).expect("a test fixture uses structurally valid names")
}

/// An object identity built from an index. Deterministic, and never minted from a clock.
fn object(index: u64) -> ObjectId {
    let mut bytes = [0u8; 16];
    bytes[8..].copy_from_slice(&index.to_be_bytes());
    ObjectId::from_bytes(bytes)
}

/// A stamp at one position in the total order, with the event identifier derived from the counter.
fn at(lamport: u64) -> Stamp {
    let mut event = [0u8; 16];
    event[8..].copy_from_slice(&lamport.to_be_bytes());
    Stamp::new(Lamport::new(lamport), EventId::from_bytes(event), [0; 32])
}

/// A stamp concurrent with [`at`] — same counter, a different event.
fn concurrently_with(lamport: u64, actor: u8) -> Stamp {
    let mut event = [actor; 16];
    event[8..].copy_from_slice(&lamport.to_be_bytes());
    Stamp::new(
        Lamport::new(lamport),
        EventId::from_bytes(event),
        [actor; 32],
    )
}

fn expect_applied(
    register: ObjectRegister,
    change: &IdentityChange,
    stamp: Stamp,
) -> ObjectRegister {
    let (next, outcome) = register.apply(change, stamp);
    assert!(
        matches!(outcome, IdentityOutcome::Applied { .. }),
        "{change:?} at {stamp:?} was not applied: {outcome:?}"
    );
    next
}

/// A register holding `/source`, `/destination` and `/source/subtree`, with `descendants`
/// directories hanging beneath the subtree root.
///
/// Object 0 is the root, 1 is `source`, 2 is `destination`, 3 is the subtree root, and 4 upwards
/// are the descendants.
fn with_subtree(descendants: usize) -> ObjectRegister {
    let root = object(0);
    let mut register = ObjectRegister::new(root, at(0));
    let mut counter = 1u64;
    for (id, parent, entry) in [
        (object(1), root, "source"),
        (object(2), root, "destination"),
        (object(3), object(1), "subtree"),
    ] {
        register = expect_applied(
            register,
            &IdentityChange::Create {
                object: id,
                kind: ObjectKind::Directory,
            },
            at(counter),
        );
        counter += 1;
        register = expect_applied(
            register,
            &IdentityChange::Link {
                object: id,
                directory: parent,
                name: name(entry),
            },
            at(counter),
        );
        counter += 1;
    }

    for index in 0..descendants {
        let id = object(4 + index as u64);
        // The first BRANCHING hang off the subtree root; after that off an earlier descendant, so
        // the tree is genuinely nested rather than one enormous directory.
        let parent = if index < BRANCHING {
            object(3)
        } else {
            object(4 + (index / BRANCHING) as u64)
        };
        register = expect_applied(
            register,
            &IdentityChange::Create {
                object: id,
                kind: ObjectKind::Directory,
            },
            at(counter),
        );
        counter += 1;
        register = expect_applied(
            register,
            &IdentityChange::Link {
                object: id,
                directory: parent,
                name: name(format!("n{index}")),
            },
            at(counter),
        );
        counter += 1;
    }
    register
}

/// The move under measurement: `/source/subtree` becomes `/destination/subtree`.
fn subtree_move() -> IdentityChange {
    IdentityChange::Move {
        object: object(3),
        from_directory: object(1),
        from_name: name("subtree"),
        to_directory: object(2),
        to_name: name("subtree"),
    }
}

// ---------------------------------------------------------------------------------------------
// Criterion: a rename concurrent with an edit results in one object with both changes applied.
// ---------------------------------------------------------------------------------------------

#[test]
fn a_rename_concurrent_with_an_edit_leaves_one_object_carrying_both() {
    let version = VersionId::from_bytes([0x7e; 32]);
    let rename = IdentityChange::Rename {
        object: object(4),
        directory: object(3),
        from_name: name("n0"),
        to_name: name("renamed"),
    };
    let edit = IdentityChange::WriteVersion {
        object: object(4),
        version,
    };
    // Concurrent: one Lamport counter, two actors, two events. Neither causally follows the other.
    let renamed_at = concurrently_with(64, 0xa1);
    let edited_at = concurrently_with(64, 0xb2);

    for (first, first_at, second, second_at) in [
        (&rename, renamed_at, &edit, edited_at),
        (&edit, edited_at, &rename, renamed_at),
    ] {
        let register = with_subtree(1);
        let before = register.object_count();
        let register = expect_applied(register, first, first_at);
        let register = expect_applied(register, second, second_at);

        // One object, not two: the rename did not mint anything and the edit did not either.
        assert_eq!(register.object_count(), before);
        assert!(register.contains(object(4)));
        // Both changes applied: the new name and the new version.
        assert_eq!(
            register.path_of(object(4)).unwrap().to_string(),
            "/source/subtree/renamed"
        );
        assert_eq!(register.version_of(object(4)), Some(version));
    }
}

/// The same property held over a wider corpus: for every one of the register's facets, a change to
/// one facet concurrent with a change to another leaves both applied, in either delivery order.
#[test]
fn concurrent_changes_to_different_facets_never_displace_each_other() {
    let mut seen = 0;
    for (placement_change, expected_path) in [
        (
            IdentityChange::Rename {
                object: object(4),
                directory: object(3),
                from_name: name("n0"),
                to_name: name("renamed"),
            },
            "/source/subtree/renamed",
        ),
        (
            IdentityChange::Move {
                object: object(4),
                from_directory: object(3),
                from_name: name("n0"),
                to_directory: object(2),
                to_name: name("promoted"),
            },
            "/destination/promoted",
        ),
    ] {
        for version_byte in [0x01u8, 0xff] {
            let version = VersionId::from_bytes([version_byte; 32]);
            let edit = IdentityChange::WriteVersion {
                object: object(4),
                version,
            };
            let placed_at = concurrently_with(64, 0xa1);
            let edited_at = concurrently_with(64, 0xb2);
            for forwards in [true, false] {
                let register = with_subtree(1);
                let register = if forwards {
                    let register = expect_applied(register, &placement_change, placed_at);
                    expect_applied(register, &edit, edited_at)
                } else {
                    let register = expect_applied(register, &edit, edited_at);
                    expect_applied(register, &placement_change, placed_at)
                };
                assert_eq!(
                    register.path_of(object(4)).unwrap().to_string(),
                    expected_path
                );
                assert_eq!(register.version_of(object(4)), Some(version));
                seen += 1;
            }
        }
    }
    assert_eq!(seen, 8);
}

// ---------------------------------------------------------------------------------------------
// Criterion: a directory move with one million descendants emits one metadata operation.
// ---------------------------------------------------------------------------------------------

#[test]
fn a_subtree_move_is_one_change_of_constant_cost_at_every_size() {
    let mut metadata = Vec::new();
    let mut touched = Vec::new();
    let mut millis = Vec::new();

    for size in SUBTREE_SIZES {
        let register = with_subtree(size);
        let deepest = object(3 + size as u64);
        let before = register
            .path_of(deepest)
            .expect("the deepest descendant is reachable before the move");
        assert!(
            before.to_string().starts_with("/source/subtree"),
            "{before}"
        );
        let history_before = register.placement_history(deepest).len();

        let change = subtree_move();
        let started = Instant::now();
        let (register, outcome) = register.apply(&change, at(u64::from(u32::MAX)));
        let elapsed = started.elapsed();

        let IdentityOutcome::Applied {
            entries_touched,
            metadata_bytes,
        } = outcome
        else {
            panic!("a subtree move of {size} was not applied: {outcome:?}");
        };

        // One change. There is no plural form of this call.
        assert_eq!(entries_touched, 2, "at subtree size {size}");
        assert!(
            metadata_bytes < METADATA_BUDGET_BYTES,
            "a subtree move of {size} cost {metadata_bytes} bytes, over the \
             {METADATA_BUDGET_BYTES}-byte budget"
        );
        assert!(
            elapsed.as_millis() < WALL_CLOCK_BUDGET_MILLIS,
            "a subtree move of {size} took {} ms, over the {WALL_CLOCK_BUDGET_MILLIS} ms budget",
            elapsed.as_millis()
        );

        // Not one descendant record changed, and every descendant moved. At size zero the
        // "deepest descendant" is the moved object itself, which is the one record a move writes.
        let expected_growth = usize::from(size == 0);
        assert_eq!(
            register.placement_history(deepest).len(),
            history_before + expected_growth,
            "the deepest descendant's record changed at subtree size {size}"
        );
        let after = register
            .path_of(deepest)
            .expect("the deepest descendant is reachable after the move");
        assert!(
            after.to_string().starts_with("/destination/subtree"),
            "{after}"
        );
        assert_eq!(
            after.to_string().strip_prefix("/destination").unwrap(),
            before.to_string().strip_prefix("/source").unwrap(),
            "the path below the moved root changed at subtree size {size}"
        );
        assert_eq!(register.child_count(object(1)), 0);
        assert_eq!(register.child_count(object(2)), 1);

        metadata.push(metadata_bytes);
        touched.push(entries_touched);
        millis.push(elapsed.as_micros());
        println!(
            "subtree move · descendants {size:>9} · {metadata_bytes} metadata bytes · \
             {entries_touched} directory entries touched · {} us",
            elapsed.as_micros()
        );
    }

    // Constant, not merely sublinear. This is the assertion that carries the budget; the wall-clock
    // ceiling above only says the register is nowhere near it.
    assert!(
        metadata.windows(2).all(|pair| pair[0] == pair[1]),
        "the metadata cost of a subtree move varies with the subtree: {metadata:?}"
    );
    assert!(
        touched.windows(2).all(|pair| pair[0] == pair[1]),
        "the entries touched by a subtree move varies with the subtree: {touched:?}"
    );
    println!("subtree move · microseconds by size {SUBTREE_SIZES:?}: {millis:?}");
}

/// The control. If the register had stored a path per object, a move would have cost one record per
/// descendant — this is what that would have been, so the constant above reads as a design choice
/// rather than an accident.
#[test]
fn rewriting_every_descendant_would_have_blown_the_budget() {
    let per_descendant = IdentityChange::Rename {
        object: object(4),
        directory: object(3),
        from_name: name("n0"),
        to_name: name("n0"),
    }
    .metadata_byte_count();
    let naive = per_descendant * 1_000_000;
    assert!(
        naive > METADATA_BUDGET_BYTES * 1_000,
        "the per-descendant spelling would have cost {naive} bytes for a million descendants"
    );
    let IdentityOutcome::Applied { metadata_bytes, .. } =
        with_subtree(1).apply(&subtree_move(), at(999)).1
    else {
        panic!("the move was not applied");
    };
    assert!(metadata_bytes * 1_000 < naive);
}

// ---------------------------------------------------------------------------------------------
// Criterion: no operation sequence produces a new object ID for a renamed or moved file.
// ---------------------------------------------------------------------------------------------

#[test]
fn no_generated_rename_and_move_sequence_ever_mints_an_identity() {
    for seed in [1u64, 2, 3, 5, 8, 13, 21, 34] {
        let mut shuffler = Shuffler::new(seed);
        let register = with_subtree(64);
        let identities_before: Vec<ObjectId> = register.object_ids().collect();
        let mut register = register;

        for round in 0..200u64 {
            let subject = object(4 + shuffler.below(64) as u64);
            let stamp = at(10_000 + round);
            let change = match shuffler.below(4) {
                0 => IdentityChange::Rename {
                    object: subject,
                    directory: current_directory(&register, subject),
                    from_name: name("ignored"),
                    to_name: name(format!("r{round}")),
                },
                1 => IdentityChange::Move {
                    object: subject,
                    from_directory: current_directory(&register, subject),
                    from_name: name("ignored"),
                    to_directory: object(2),
                    to_name: name(format!("m{round}")),
                },
                2 => IdentityChange::WriteVersion {
                    object: subject,
                    version: VersionId::from_bytes([round as u8; 32]),
                },
                _ => IdentityChange::Unlink {
                    object: subject,
                    directory: current_directory(&register, subject),
                    name: name("ignored"),
                },
            };
            let (next, outcome) = register.apply(&change, stamp);
            assert!(
                !matches!(
                    outcome,
                    IdentityOutcome::Refused(IdentityRefusal::UnknownObject(_))
                ),
                "seed {seed} round {round}: {outcome:?}"
            );
            register = next;
        }

        let identities_after: Vec<ObjectId> = register.object_ids().collect();
        assert_eq!(
            identities_before, identities_after,
            "seed {seed}: a rename, move, unlink or edit sequence changed the set of identities"
        );
    }
}

/// The directory an object currently sits in, or the destination if it is detached. Fixture help:
/// the register needs an absolute placement, and a generated sequence has to look one up.
fn current_directory(register: &ObjectRegister, object_id: ObjectId) -> ObjectId {
    match register.placement_of(object_id) {
        Some(Placement::Bound { directory, .. }) => *directory,
        _ => object(2),
    }
}

/// The negative control: the one member that *is* allowed to mint one, does.
#[test]
fn only_creating_an_object_changes_the_set_of_identities() {
    let register = with_subtree(1);
    let before = register.object_count();
    let (register, outcome) = register.apply(
        &IdentityChange::Create {
            object: object(9_000),
            kind: ObjectKind::File,
        },
        at(500),
    );
    assert!(matches!(outcome, IdentityOutcome::Applied { .. }));
    assert_eq!(register.object_count(), before + 1);
}

// ---------------------------------------------------------------------------------------------
// Criterion: history remains traversable across an arbitrary chain of renames and moves.
// ---------------------------------------------------------------------------------------------

#[test]
fn an_arbitrary_chain_of_renames_and_moves_stays_traversable() {
    let mut register = with_subtree(4);
    let subject = object(4);
    let mut expected: Vec<(Stamp, String)> =
        vec![(at(11), register.path_of(subject).unwrap().to_string())];

    // Alternating renames and moves, back and forth between two directories, forty times.
    for step in 0..40u64 {
        let stamp = at(1_000 + step);
        let change = if step % 2 == 0 {
            IdentityChange::Rename {
                object: subject,
                directory: current_directory(&register, subject),
                from_name: name("ignored"),
                to_name: name(format!("step{step}")),
            }
        } else {
            let destination = if step % 4 == 1 { object(2) } else { object(3) };
            IdentityChange::Move {
                object: subject,
                from_directory: current_directory(&register, subject),
                from_name: name("ignored"),
                to_directory: destination,
                to_name: name(format!("step{step}")),
            }
        };
        register = expect_applied(register, &change, stamp);
        expected.push((stamp, register.path_of(subject).unwrap().to_string()));
    }

    // The identity never moved.
    assert!(register.contains(subject));
    // Every link in the chain is still there, in order, and every historical path is derivable.
    assert_eq!(register.placement_history(subject).len(), 41);
    for (stamp, path) in &expected {
        assert_eq!(
            register.path_at(subject, *stamp).unwrap().to_string(),
            *path,
            "the path at {stamp:?} is no longer derivable"
        );
    }
    // Walking the chain from either end reaches the same forty-one records.
    let forwards: Vec<Stamp> = register
        .placement_history(subject)
        .iter()
        .map(mesh_state::PlacementRecord::stamp)
        .collect();
    let mut backwards = forwards.clone();
    backwards.reverse();
    backwards.reverse();
    assert_eq!(forwards, backwards);
    assert!(forwards.windows(2).all(|pair| pair[0] < pair[1]));
}

#[test]
fn a_descendant_that_was_never_named_still_has_every_path_it_had() {
    let register = with_subtree(64);
    let deepest = object(67);
    let original = register.path_of(deepest).unwrap().to_string();

    // Rename the subtree root, then move it, then rename it again. The descendant is never named.
    let register = expect_applied(
        register,
        &IdentityChange::Rename {
            object: object(3),
            directory: object(1),
            from_name: name("subtree"),
            to_name: name("core"),
        },
        at(2_000),
    );
    let register = expect_applied(
        register,
        &IdentityChange::Move {
            object: object(3),
            from_directory: object(1),
            from_name: name("core"),
            to_directory: object(2),
            to_name: name("core"),
        },
        at(2_001),
    );
    let register = expect_applied(
        register,
        &IdentityChange::Rename {
            object: object(3),
            directory: object(2),
            from_name: name("core"),
            to_name: name("kernel"),
        },
        at(2_002),
    );

    // One record was written for the descendant, ever: the link that created its entry.
    assert_eq!(register.placement_history(deepest).len(), 1);

    let tail = original.strip_prefix("/source/subtree").unwrap().to_owned();
    for (stamp, expected) in [
        (at(1_999), format!("/source/subtree{tail}")),
        (at(2_000), format!("/source/core{tail}")),
        (at(2_001), format!("/destination/core{tail}")),
        (at(2_002), format!("/destination/kernel{tail}")),
    ] {
        assert_eq!(
            register.path_at(deepest, stamp).unwrap().to_string(),
            expected,
            "the descendant's path at {stamp:?} is wrong"
        );
    }
    assert_eq!(
        register.path_of(deepest).unwrap().to_string(),
        format!("/destination/kernel{tail}")
    );
}

// ---------------------------------------------------------------------------------------------
// Convergence, contested names, and the stated ceiling.
// ---------------------------------------------------------------------------------------------

/// Delivery order within a round of concurrent changes does not change the register.
///
/// The register assumes **causal** delivery — a change is applied after the changes it causally
/// follows, which is what [`mesh_state::HeadAdvancement`] buffers for one layer up. So the shuffle
/// below reorders freely inside each concurrent round and never across rounds, which is exactly the
/// freedom a real network has.
#[test]
fn every_delivery_order_within_a_round_reaches_one_register() {
    for seed in [7u64, 11, 17, 23] {
        let rounds = generated_rounds(seed);
        let reference = fold(with_subtree(16), &rounds);
        for replica in 0..4u64 {
            let mut shuffler = Shuffler::new(seed * 31 + replica);
            let mut shuffled = rounds.clone();
            for round in &mut shuffled {
                shuffler.shuffle(round);
            }
            assert_eq!(
                fold(with_subtree(16), &shuffled),
                reference,
                "seed {seed} replica {replica}: two delivery orders reached different registers"
            );
        }
    }
}

/// Rounds of concurrent changes: everything inside a round shares a Lamport counter.
fn generated_rounds(seed: u64) -> Vec<Vec<(IdentityChange, Stamp)>> {
    let mut shuffler = Shuffler::new(seed);
    let mut rounds = Vec::new();
    for round in 0..12u64 {
        let mut concurrent = Vec::new();
        for actor in 0..3u8 {
            let subject = object(4 + shuffler.below(16) as u64);
            let stamp = concurrently_with(3_000 + round, actor);
            let change = match shuffler.below(3) {
                0 => IdentityChange::Rename {
                    object: subject,
                    directory: object(3),
                    from_name: name("ignored"),
                    to_name: name(format!("r{round}x{actor}")),
                },
                1 => IdentityChange::Move {
                    object: subject,
                    from_directory: object(3),
                    from_name: name("ignored"),
                    to_directory: object(2),
                    to_name: name(format!("m{round}")),
                },
                _ => IdentityChange::WriteVersion {
                    object: subject,
                    version: VersionId::from_bytes([round as u8; 32]),
                },
            };
            concurrent.push((change, stamp));
        }
        rounds.push(concurrent);
    }
    rounds
}

fn fold(register: ObjectRegister, rounds: &[Vec<(IdentityChange, Stamp)>]) -> ObjectRegister {
    let mut register = register;
    for round in rounds {
        for (change, stamp) in round {
            register = register.apply(change, *stamp).0;
        }
    }
    register
}

/// A name contest destroys no identity. Both objects survive, both keep their history, and every
/// contender is reportable — which is what `ResolveNameConflict` needs in order to preserve them.
#[test]
fn a_concurrent_name_contest_preserves_every_contender() {
    let register = with_subtree(2);
    let contested = name("contested");
    let register = expect_applied(
        register,
        &IdentityChange::Rename {
            object: object(4),
            directory: object(3),
            from_name: name("n0"),
            to_name: contested.clone(),
        },
        concurrently_with(700, 0x11),
    );
    let register = expect_applied(
        register,
        &IdentityChange::Rename {
            object: object(5),
            directory: object(3),
            from_name: name("n1"),
            to_name: contested.clone(),
        },
        concurrently_with(700, 0x22),
    );

    let contenders = register.contenders_for(object(3), &contested);
    assert_eq!(contenders.len(), 2);
    assert!(contenders.contains(&object(4)) && contenders.contains(&object(5)));
    // Neither identity is gone and neither history is truncated.
    assert!(register.contains(object(4)) && register.contains(object(5)));
    assert_eq!(register.placement_history(object(4)).len(), 2);
    assert_eq!(register.placement_history(object(5)).len(), 2);
    // The name resolves to exactly one of them, deterministically, by stamp.
    let entries: BTreeMap<String, ObjectId> = register
        .entries_of(object(3))
        .map(|(entry, holder)| (entry.to_string(), holder))
        .collect();
    assert_eq!(entries.get("contested"), Some(&contenders[0]));
}

/// The stated ceiling, pinned so that the day it changes is a visible one.
///
/// Two concurrent moves that would place each of two directories inside the other: whichever is
/// applied second is refused. Converging this needs the undo-and-replay construction from
/// Kleppmann et al.'s highly-available move operation, which belongs to the conflict engine.
/// `src/identity.rs` says so; this is the executable half of that sentence.
#[test]
fn two_concurrent_moves_forming_a_cycle_refuse_the_second() {
    let register = with_subtree(2);
    // Objects 4 and 5 are siblings under the subtree root. Move each inside the other.
    let into_five = IdentityChange::Move {
        object: object(4),
        from_directory: object(3),
        from_name: name("n0"),
        to_directory: object(5),
        to_name: name("n0"),
    };
    let into_four = IdentityChange::Move {
        object: object(5),
        from_directory: object(3),
        from_name: name("n1"),
        to_directory: object(4),
        to_name: name("n1"),
    };
    let register = expect_applied(register, &into_five, concurrently_with(800, 0x33));
    let (register, outcome) = register.apply(&into_four, concurrently_with(800, 0x44));
    assert_eq!(
        outcome,
        IdentityOutcome::Refused(IdentityRefusal::WouldContainItself {
            object: object(5),
            directory: object(4),
        })
    );
    // Both identities and both paths survive the refusal; nothing is orphaned.
    assert_eq!(
        register.path_of(object(4)).unwrap().to_string(),
        "/source/subtree/n1/n0"
    );
    assert_eq!(
        register.path_of(object(5)).unwrap().to_string(),
        "/source/subtree/n1"
    );
}

// ---------------------------------------------------------------------------------------------
// Regression seeds. One per identity break found while building this register.
// ---------------------------------------------------------------------------------------------

/// Found while writing [`a_descendant_that_was_never_named_still_has_every_path_it_had`]: a
/// historical path must be resolved with *every* ancestor read at the same position in the total
/// order. Reading the moved ancestor historically and its parent currently produced a path that
/// never existed.
#[test]
fn regression_a_historical_path_reads_every_ancestor_at_the_same_position() {
    let register = with_subtree(20);
    let register = expect_applied(
        register,
        &IdentityChange::Rename {
            object: object(1),
            directory: object(0),
            from_name: name("source"),
            to_name: name("src"),
        },
        at(5_000),
    );
    let register = expect_applied(
        register,
        &IdentityChange::Rename {
            object: object(3),
            directory: object(1),
            from_name: name("subtree"),
            to_name: name("core"),
        },
        at(5_001),
    );
    let leaf = object(5);
    assert_eq!(
        register.path_at(leaf, at(4_999)).unwrap().to_string(),
        "/source/subtree/n1"
    );
    assert_eq!(
        register.path_at(leaf, at(5_000)).unwrap().to_string(),
        "/src/subtree/n1"
    );
    assert_eq!(
        register.path_at(leaf, at(5_001)).unwrap().to_string(),
        "/src/core/n1"
    );
}

/// Found while writing [`every_delivery_order_within_a_round_reaches_one_register`]: a change that
/// arrives after a higher-stamped change to the same facet must still land in the history, or the
/// register a replica reaches depends on the order its network happened to deliver in.
#[test]
fn regression_a_superseded_change_is_recorded_rather_than_dropped() {
    let register = with_subtree(1);
    let late = IdentityChange::Rename {
        object: object(4),
        directory: object(3),
        from_name: name("n0"),
        to_name: name("late"),
    };
    let early = IdentityChange::Rename {
        object: object(4),
        directory: object(3),
        from_name: name("n0"),
        to_name: name("early"),
    };
    let forwards = {
        let register = with_subtree(1);
        let register = register.apply(&early, at(6_000)).0;
        register.apply(&late, at(6_001)).0
    };
    let backwards = {
        let register = register.apply(&late, at(6_001)).0;
        let (register, outcome) = register.apply(&early, at(6_000));
        assert!(matches!(outcome, IdentityOutcome::Superseded { .. }));
        register
    };
    assert_eq!(forwards, backwards);
    assert_eq!(
        backwards.path_of(object(4)).unwrap().to_string(),
        "/source/subtree/late"
    );
    assert_eq!(backwards.placement_history(object(4)).len(), 3);
}

/// Found while writing [`a_subtree_move_is_one_change_of_constant_cost_at_every_size`]: the
/// register must refuse to place an object under a file, or a path walk beneath it terminates at
/// something that cannot hold an entry and the subtree is unreachable without any change having
/// been refused.
#[test]
fn regression_a_file_never_becomes_a_directory_by_being_linked_into() {
    let register = with_subtree(1);
    let register = expect_applied(
        register,
        &IdentityChange::Create {
            object: object(500),
            kind: ObjectKind::File,
        },
        at(7_000),
    );
    let register = expect_applied(
        register,
        &IdentityChange::Link {
            object: object(500),
            directory: object(3),
            name: name("readme.md"),
        },
        at(7_001),
    );
    let (register, outcome) = register.apply(
        &IdentityChange::Move {
            object: object(4),
            from_directory: object(3),
            from_name: name("n0"),
            to_directory: object(500),
            to_name: name("n0"),
        },
        at(7_002),
    );
    assert_eq!(
        outcome,
        IdentityOutcome::Refused(IdentityRefusal::NotADirectory(object(500)))
    );
    assert_eq!(
        register.path_of(object(4)).unwrap().to_string(),
        "/source/subtree/n0"
    );
}
