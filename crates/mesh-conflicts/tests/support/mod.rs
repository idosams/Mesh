//! Shared fixtures for the campaigns: a seeded generator and a workspace to run it against.
//!
//! No dependency, so no `rand` and no `proptest`. The generator below is a SplitMix64, which is
//! forty lines and reproduces exactly from a `u64` seed — which is what a campaign needs anyway. A
//! failure prints its seed and the seed replays it.
//!
//! Each test binary compiles its own copy of this module, so a helper used by one of them is dead
//! code in the others — hence the crate-wide allow, following `mesh-cas`, `mesh-state` and every
//! other shared fixture module in this workspace.

#![allow(dead_code)]

use mesh_conflicts::{
    ActorId, Change, Content, Effect, EventId, Lamport, NormalizedName, ObjectId, ObjectKind,
    Snapshot, Stamp, VersionId,
};

/// A seeded generator. SplitMix64, as published.
pub struct Seeded {
    state: u64,
}

impl Seeded {
    /// The generator for this seed.
    pub const fn new(seed: u64) -> Self {
        Self { state: seed }
    }

    /// The next value.
    pub fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }

    /// A value below `bound`.
    pub fn below(&mut self, bound: usize) -> usize {
        assert!(bound > 0, "a bound of zero has no values below it");
        usize::try_from(self.next_u64() % bound as u64).expect("a value below a usize bound")
    }

    /// A shuffled copy. Fisher-Yates, so every permutation is reachable.
    pub fn shuffled<T: Clone>(&mut self, items: &[T]) -> Vec<T> {
        let mut out = items.to_vec();
        for at in (1..out.len()).rev() {
            out.swap(at, self.below(at + 1));
        }
        out
    }
}

/// The identifiers the generated campaigns work over.
pub struct Workspace {
    pub base: Snapshot,
    pub root: ObjectId,
    pub directories: Vec<ObjectId>,
    pub files: Vec<ObjectId>,
}

/// A name, or a panic — every literal in this file is one.
pub fn name(text: &str) -> NormalizedName {
    NormalizedName::new(text).expect("a test name is a name")
}

/// An object identifier from one byte.
pub fn object(byte: u8) -> ObjectId {
    ObjectId::from_bytes([byte; 16])
}

/// A version identifier from one byte.
pub fn version(byte: u8) -> VersionId {
    VersionId::from_bytes([byte; 32])
}

/// A stamp at this counter with this event byte.
pub fn at(lamport: u64, event: u8) -> Stamp {
    Stamp::new(
        Lamport::new(lamport),
        EventId::from_bytes([event; 16]),
        [0; 32],
    )
}

/// Lines from string literals.
pub fn lines(text: &[&str]) -> Vec<String> {
    text.iter().map(|line| (*line).to_owned()).collect()
}

/// Two directories and four files — two text, two bytes — under one root.
///
/// Deliberately small. A campaign over four objects and a dozen changes hits every row of the
/// table many times over; a campaign over four hundred hits the same rows and takes a hundred
/// times as long to tell you so.
pub fn workspace() -> Workspace {
    let root = object(0);
    let alpha = object(1);
    let beta = object(2);
    let text_one = object(3);
    let text_two = object(4);
    let bytes_one = object(5);
    let bytes_two = object(6);

    let base = Snapshot::new(root)
        .with_directory(alpha, root, name("alpha"))
        .with_directory(beta, root, name("beta"))
        .with_file(
            text_one,
            alpha,
            name("one.md"),
            Content::Text {
                version: version(0x11),
                lines: lines(&["a", "b", "c", "d", "e", "f", "g", "h"]),
            },
        )
        .with_file(
            text_two,
            beta,
            name("two.md"),
            Content::Text {
                version: version(0x12),
                lines: lines(&["p", "q", "r", "s", "t", "u", "v", "w"]),
            },
        )
        .with_file(
            bytes_one,
            alpha,
            name("one.bin"),
            Content::Binary {
                version: version(0x13),
                digest: [0x13; 32],
                byte_length: 64,
            },
        )
        .with_file(
            bytes_two,
            beta,
            name("two.bin"),
            Content::Binary {
                version: version(0x14),
                digest: [0x14; 32],
                byte_length: 64,
            },
        );

    Workspace {
        base,
        root,
        directories: vec![alpha, beta],
        files: vec![text_one, text_two, bytes_one, bytes_two],
    }
}

/// A set of concurrent changes over `workspace`, drawn from this seed.
///
/// Every change is given one of three Lamport counters, so a large fraction of any generated set
/// is genuinely concurrent rather than sequential — which is the only interesting case and the one
/// a naive generator misses.
pub fn generate(seeded: &mut Seeded, space: &Workspace, count: usize) -> Vec<Change> {
    let actors = [
        ActorId::from_bytes([1; 32]),
        ActorId::from_bytes([2; 32]),
        ActorId::from_bytes([3; 32]),
    ];
    let mut changes = Vec::with_capacity(count);
    for index in 0..count {
        let actor = actors[seeded.below(actors.len())];
        let stamp = at(
            u64::try_from(seeded.below(3)).expect("a counter below three"),
            u8::try_from(index % 251).expect("an event byte"),
        );
        changes.push(Change::new(stamp, actor, effect(seeded, space, index)));
    }
    changes
}

/// One generated effect.
fn effect(seeded: &mut Seeded, space: &Workspace, index: usize) -> Effect {
    let file = space.files[seeded.below(space.files.len())];
    let directory = space.directories[seeded.below(space.directories.len())];
    let fresh = object(u8::try_from(0x40 + index % 64).expect("a fresh object byte"));
    let fresh_version = version(u8::try_from(0x80 + index % 96).expect("a fresh version byte"));

    match seeded.below(8) {
        0 => Effect::Create {
            object: fresh,
            kind: ObjectKind::File,
            directory,
            name: name("collision.md"),
        },
        1 => Effect::Rename {
            object: file,
            name: name(&format!("renamed-{index}.md")),
        },
        2 => Effect::Reparent {
            object: if seeded.below(2) == 0 {
                file
            } else {
                directory
            },
            directory: space.directories[seeded.below(space.directories.len())],
        },
        3 | 4 => Effect::WriteText {
            object: file,
            version: fresh_version,
            lines: mutated_lines(seeded, index),
        },
        5 => Effect::WriteBinary {
            object: file,
            version: fresh_version,
            digest: [u8::try_from(index % 251).expect("a digest byte"); 32],
            byte_length: 64,
        },
        6 => Effect::Delete { object: file },
        _ => Effect::WriteText {
            object: fresh,
            version: fresh_version,
            lines: mutated_lines(seeded, index),
        },
    }
}

/// Eight lines with one or two of them rewritten, so generated text edits both merge and collide.
fn mutated_lines(seeded: &mut Seeded, index: usize) -> Vec<String> {
    let mut out: Vec<String> = "abcdefgh"
        .chars()
        .map(|letter| letter.to_string())
        .collect();
    let touched = 1 + seeded.below(2);
    for _ in 0..touched {
        let at = seeded.below(out.len());
        out[at] = format!("{index}-{at}");
    }
    out
}
