//! Shared fixtures: a seeded generator, a workspace builder, and a way to see the exact bytes.
//!
//! No dependency, so no `rand` and no `proptest`. The generator is a SplitMix64, which reproduces
//! exactly from a `u64` seed — a failing campaign prints its seed and the seed replays it.
//!
//! Each test binary compiles its own copy of this module, so a helper used by one of them is dead
//! code in the others — hence the crate-wide allow, following `mesh-cas`, `mesh-conflicts` and
//! every other shared fixture module in this workspace.

#![allow(dead_code)]

use std::cell::RefCell;
use std::rc::Rc;

use mesh_approval::{
    Absorb, ActorId, CanonicalRecord, Content, DiffPresentation, Digest32, DigestHasher,
    DigestWriter, DomainTag, HeadId, NormalizedName, ObjectId, ReviewBundle, VersionId,
    WorkspaceState,
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

/// A name, or a panic — every literal in these tests is one.
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

/// A head identifier from one byte.
pub fn head(byte: u8) -> HeadId {
    HeadId::from_bytes([byte; 32])
}

/// An actor identifier from one byte.
pub fn actor_id(byte: u8) -> ActorId {
    ActorId::from_bytes([byte; 32])
}

/// One text version, with one line per element.
pub fn text(byte: u8, lines: &[&str]) -> Content {
    Content::Text {
        version: version(byte),
        lines: lines.iter().map(|line| (*line).to_owned()).collect(),
    }
}

/// One binary version.
pub fn binary(byte: u8, length: u64) -> Content {
    Content::Binary {
        version: version(byte),
        digest: [byte; 32],
        byte_length: length,
    }
}

/// The root every generated workspace hangs from.
pub fn root() -> ObjectId {
    object(0)
}

/// A generated workspace: a tree of directories with files in them, and no cycle by construction.
pub struct Generated {
    pub state: WorkspaceState,
    pub directories: Vec<ObjectId>,
    pub files: Vec<ObjectId>,
}

/// Build a workspace of `directories` directories and `files` files from `seed`.
///
/// Every directory's parent is a directory that already exists, so the tree is acyclic by
/// construction rather than by check — a generator that could emit a cycle would be testing the
/// refusal path instead of the property under test.
pub fn generate(seed: &mut Seeded, directories: usize, files: usize) -> Generated {
    let mut state = WorkspaceState::new(root());
    let mut dirs = vec![root()];

    for index in 0..directories {
        let id = object(u8::try_from(1 + index).expect("a small directory count"));
        let parent = dirs[seed.below(dirs.len())];
        state = state.with_directory(id, parent, name(&format!("dir{index}")));
        dirs.push(id);
    }

    let mut file_ids = Vec::new();
    for index in 0..files {
        let id = object(u8::try_from(64 + index).expect("a small file count"));
        let parent = dirs[seed.below(dirs.len())];
        let content = if seed.below(4) == 0 {
            binary(u8::try_from(index % 251).expect("a byte"), 1 + index as u64)
        } else {
            text(
                u8::try_from(index % 251).expect("a byte"),
                &["alpha", "beta", "gamma"],
            )
        };
        state = state.with_file(id, parent, name(&format!("file{index}.md")), content);
        file_ids.push(id);
    }

    Generated {
        state,
        directories: dirs,
        files: file_ids,
    }
}

/// A later state derived from `from` by mutating files only.
///
/// Directories are left where they are: moving one could create a cycle, and a cycle is the
/// refusal path rather than the property under test. Returns `None` when the mutations happened to
/// cancel out, which the caller treats as "generate another".
pub fn mutate(seed: &mut Seeded, from: &Generated, rounds: usize) -> Option<WorkspaceState> {
    let mut state = from.state.clone();
    let mut changed = false;

    for round in 0..rounds {
        if from.files.is_empty() {
            break;
        }
        let file = from.files[seed.below(from.files.len())];
        let Some(held) = state.object(file).cloned() else {
            continue;
        };
        let directory = held.directory().expect("a generated file hangs somewhere");
        let entry = held.name().expect("a generated file has a name").clone();
        state = match seed.below(4) {
            0 => state.with_file(
                file,
                directory,
                name(&format!("renamed{round}.md")),
                held.content()
                    .expect("a generated file holds content")
                    .clone(),
            ),
            1 => {
                let target = from.directories[seed.below(from.directories.len())];
                state.with_file(
                    file,
                    target,
                    entry,
                    held.content()
                        .expect("a generated file holds content")
                        .clone(),
                )
            }
            2 => state.with_file(
                file,
                directory,
                entry,
                text(
                    u8::try_from(128 + round % 100).expect("a byte"),
                    &["rewritten", "by", "an", "agent"],
                ),
            ),
            _ => state.without(file),
        };
        changed = true;
    }

    // One creation, so a campaign always exercises the create path too.
    let fresh = object(u8::try_from(200 + seed.below(40)).expect("a byte"));
    if state.object(fresh).is_none() {
        let parent = from.directories[seed.below(from.directories.len())];
        state = state.with_file(fresh, parent, name("created.md"), text(250, &["new"]));
        changed = true;
    }

    if changed && state != from.state {
        Some(state)
    } else {
        None
    }
}

/// Records every byte a record absorbs, so a test can compare byte streams and not only digests.
///
/// The acceptance criterion is a *byte-identical* bundle. A digest is evidence for that and not the
/// thing itself, so this exists to make the literal claim checkable — including across processes,
/// where the stream is printed as hex and compared.
#[derive(Clone, Default)]
pub struct Recorder {
    absorbed: Rc<RefCell<Vec<u8>>>,
}

impl DigestHasher for Recorder {
    fn update(&mut self, bytes: &[u8]) {
        self.absorbed.borrow_mut().extend_from_slice(bytes);
    }

    fn finalize(self) -> Digest32 {
        Digest32::from_bytes([0; 32])
    }
}

/// The exact byte stream a bundle absorbs, domain tag included.
pub fn canonical_bytes(bundle: &ReviewBundle) -> Vec<u8> {
    let recorder = Recorder::default();
    let absorbed = Rc::clone(&recorder.absorbed);
    let mut writer = DigestWriter::new(ReviewBundle::DOMAIN, recorder);
    bundle.absorb(&mut writer);
    let _ = writer.finish();
    let bytes = absorbed.borrow().clone();
    bytes
}

/// The domain the presentation recorder frames under.
///
/// The crate's own presentation domain is private, and it does not need to be public: what is
/// compared across processes is the byte stream the entries absorb, and a constant prefix that both
/// sides share cannot make two different renderings look alike.
const PRESENTATION_PROBE: DomainTag = DomainTag::new("test.v0.diff-presentation-probe");

/// The exact byte stream a rendered diff absorbs.
///
/// The acceptance criterion is that the same bundle always renders the same diff. A digest is
/// evidence for that; the bytes are the thing itself, so they are what the cross-process check in
/// `tests/diff.rs` compares.
pub fn presentation_bytes(presentation: &DiffPresentation) -> Vec<u8> {
    let recorder = Recorder::default();
    let absorbed = Rc::clone(&recorder.absorbed);
    let mut writer = DigestWriter::new(PRESENTATION_PROBE, recorder);
    presentation.absorb(&mut writer);
    let _ = writer.finish();
    let bytes = absorbed.borrow().clone();
    bytes
}

/// Those bytes as lowercase hex, for printing across a process boundary.
pub fn hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}
