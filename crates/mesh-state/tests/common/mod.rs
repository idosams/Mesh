//! What both test binaries need: a digest to plug into the seam, a reproducible shuffle, and a
//! generator for concurrent histories.
//!
//! # The digest here is a test double and is named as one
//!
//! `mesh-state` ships no [`HeadDigest`] implementation on purpose — `src/digest.rs` says why. The
//! one below exists so that the crate's *behaviour* can be tested without an algorithm. It is
//! FNV-1a over 128 bits, widened to the thirty-two bytes a head identifier occupies. It is not
//! collision-resistant against a chosen input and nothing here needs it to be: every property
//! under test is "two runs agree" or "two runs differ", and the adversary is a bug in the fold.
//!
//! The one thing it must not do is *hide* a bug by colliding accidentally. Across the histories
//! generated here — a few thousand distinct inputs — a 128-bit accidental collision is not a
//! failure mode worth guarding, and `distinct_causal_sets_have_distinct_heads` in `heads.rs`
//! checks the assumption rather than assuming it.
//!
//! # Nothing here reads a clock
//!
//! The shuffle is a seeded xorshift. Every campaign names its seeds as literals, so a failure is
//! reproducible by rerunning the same test rather than by catching the same millisecond.

// Each integration test binary compiles this module in full and uses a subset of it, so an item
// only `delivery.rs` needs reads as dead code while `heads.rs` is compiling. The alternative —
// splitting the module per binary — would duplicate the digest and the generator, which is how two
// campaigns end up testing subtly different folds.
#![allow(dead_code)]

use mesh_state::{
    ActorId, ChangeSetId, DeliveredChangeSet, HeadAdvancement, HeadDigest, HeadId, Reception,
};

/// The digest the tests plug into the seam. A double, not the protocol digest.
#[derive(Clone, Copy, Debug)]
pub struct TestDigest(u128);

impl TestDigest {
    const OFFSET_BASIS: u128 = 0x6c62_272e_07bb_0142_62b8_2175_6295_c58d;
    const PRIME: u128 = 0x0000_0000_0100_0000_0000_0000_0000_013b;
}

impl HeadDigest for TestDigest {
    fn start() -> Self {
        Self(Self::OFFSET_BASIS)
    }

    fn absorb(&mut self, bytes: &[u8]) {
        let mut state = self.0;
        for byte in bytes {
            state ^= u128::from(*byte);
            state = state.wrapping_mul(Self::PRIME);
        }
        self.0 = state;
    }

    fn finish(self) -> HeadId {
        let mut out = [0u8; 32];
        out[..16].copy_from_slice(&self.0.to_be_bytes());
        out[16..].copy_from_slice(&self.0.rotate_left(37).to_be_bytes());
        HeadId::from_bytes(out)
    }
}

/// The advancement under test, with the seam filled in.
pub type Advancement = HeadAdvancement<TestDigest>;

/// A distinct actor identifier.
#[must_use]
pub fn actor(tag: u8) -> ActorId {
    ActorId::from_bytes([tag; 32])
}

/// A distinct ChangeSet identifier whose byte order is deliberately unrelated to `sequence`.
///
/// The causal order breaks ties on the identifier. If identifiers ascended with authoring order,
/// every tie would break the way the history was written and a fold that ordered by arrival would
/// pass the convergence tests by accident.
#[must_use]
pub fn changeset_id(sequence: u64) -> ChangeSetId {
    let mut bytes = [0u8; 32];
    let mut state = sequence.wrapping_add(0x9E37_79B9_7F4A_7C15);
    for chunk in bytes.chunks_exact_mut(8) {
        state = split_mix(state);
        chunk.copy_from_slice(&state.to_be_bytes());
    }
    ChangeSetId::from_bytes(bytes)
}

/// One round of SplitMix64.
fn split_mix(seed: u64) -> u64 {
    let mut z = seed.wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// A seeded xorshift, so every campaign is reproducible from the seed printed in its failure.
pub struct Shuffler(u64);

impl Shuffler {
    /// Start from a seed. Zero is folded away because xorshift has a fixed point there.
    #[must_use]
    pub const fn new(seed: u64) -> Self {
        Self(if seed == 0 {
            0x2545_F491_4F6C_DD1D
        } else {
            seed
        })
    }

    /// The next value.
    pub fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }

    /// A value below `bound`.
    pub fn below(&mut self, bound: usize) -> usize {
        if bound == 0 {
            0
        } else {
            usize::try_from(self.next_u64() % bound as u64).unwrap_or(0)
        }
    }

    /// Fisher-Yates, in place.
    pub fn shuffle<T>(&mut self, items: &mut [T]) {
        for index in (1..items.len()).rev() {
            let swap = self.below(index + 1);
            items.swap(index, swap);
        }
    }
}

/// Where one seed's ChangeSet identifiers begin.
///
/// Each seed gets a disjoint block of a hundred thousand, and the hand-written fixtures use the
/// block below every seed. Without that, two histories built with different seeds would reuse an
/// identifier, and a fold that keys on the identifier — as this one does, because a record
/// identifier is a digest of the record — would correctly treat them as one ChangeSet. That is not
/// a bug in the fold, but it is a way for a test to prove the opposite of what it says: the first
/// draft of `a_parent_that_never_arrives_leaves_its_child_visible_and_held` failed exactly here,
/// because the "unrelated" history it delivered happened to contain the missing parent.
const fn identifier_space(seed: u64) -> u64 {
    100_000 * (seed + 1)
}

/// A generated concurrent history and the head every actor that applies all of it must reach.
pub struct History {
    /// Every authored ChangeSet, in an order that respects causality.
    pub changesets: Vec<DeliveredChangeSet>,
    /// The head over the whole set.
    pub head: HeadId,
}

/// Generate a history in which several actors author concurrently and merge each other's work.
///
/// The interleaving is the point, and it is made **structural rather than probabilistic**. Every
/// actor authors first, from its own empty head, so the history begins with exactly `actors`
/// concurrent ChangeSets — none an ancestor of any other — and the first cross-delivery therefore
/// produces a genuine merge. An earlier draft relied on chance for that and seed 13 produced a
/// history with no merge at all, which would have narrowed every convergence claim in
/// `tests/delivery.rs` to the serial case without failing anything.
///
/// After that opening, each round picks an actor, delivers it a few of the other actors'
/// ChangeSets — some of whose own causal parents it will not hold yet, which exercises buffering
/// during generation as well as during delivery — and has it author.
#[must_use]
pub fn generate_history(seed: u64, actors: usize, rounds: usize) -> History {
    assert!(actors > 1, "one actor cannot be concurrent with itself");
    assert!(rounds > actors, "an empty history proves nothing");
    let mut shuffler = Shuffler::new(seed);
    let mut advancements: Vec<Advancement> = (0..actors)
        .map(|index| Advancement::new(actor(u8::try_from(index + 1).unwrap_or(1))))
        .collect();
    let mut authored: Vec<DeliveredChangeSet> = Vec::new();
    let mut next_id = identifier_space(seed);

    // The concurrent opening: every actor authors from the empty head, following nothing.
    for advancement in &mut advancements {
        let (advanced, changeset) = advancement
            .author(changeset_id(next_id))
            .expect("an actor with no history can always author");
        next_id += 1;
        *advancement = advanced;
        authored.push(changeset);
    }

    for _ in actors..rounds {
        let who = shuffler.below(actors);
        for _ in 0..=shuffler.below(3) {
            let pick = shuffler.below(authored.len());
            let (advanced, _) = advancements[who].deliver(authored[pick].clone());
            advancements[who] = advanced;
        }
        let (advanced, changeset) = advancements[who]
            .author(changeset_id(next_id))
            .expect("authoring from an actor's own applied tips is always derivable");
        next_id += 1;
        advancements[who] = advanced;
        authored.push(changeset);
    }

    let mut referee = Advancement::new(actor(0xff));
    for changeset in &authored {
        let (advanced, reception) = referee.deliver(changeset.clone());
        assert!(
            matches!(reception, Reception::Applied { .. }),
            "authoring order is a linear extension of causality, so nothing should buffer: \
             {reception:?}"
        );
        referee = advanced;
    }
    assert_eq!(referee.applied().len(), authored.len());
    assert!(referee.known_missing().is_empty());

    History {
        changesets: authored,
        head: referee.head(),
    }
}

/// Deliver a whole stream to one actor and return the actor.
#[must_use]
pub fn deliver_all(mut advancement: Advancement, stream: &[DeliveredChangeSet]) -> Advancement {
    for changeset in stream {
        let (advanced, _) = advancement.deliver(changeset.clone());
        advancement = advanced;
    }
    advancement
}
