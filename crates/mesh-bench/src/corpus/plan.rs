//! What a generator emits: one ordered stream of items, described before it is
//! written.
//!
//! A workload dataset is not bytes. It is a **description** of bytes, plus the
//! ordered activity performed against them. Keeping the two apart is what makes
//! plan §12.2's larger workloads checkable at all: W2 is a hundred gigabytes,
//! and no test run is going to write a hundred gigabytes to prove that two
//! machines agree on what they contain. The description is small, exact, and
//! can be digested in seconds; the bytes are a deterministic function of it.
//!
//! Everything a generator produces goes through [`Item`], in one order, so the
//! whole workload has exactly one digest and exactly one replay order. Note
//! what is *not* in any variant: a timestamp. Order here is the position in this
//! stream — a logical step — because event order in this program is never a
//! wall clock.

use crate::json::{Json, JsonObject};

/// How a file's bytes are generated, and therefore how they behave.
///
/// These are **byte profiles, not file formats**. A [`ContentKind::Container`]
/// file is not a valid `.docx`; it is high-entropy bytes that behave the way a
/// deflate-compressed container behaves under chunking and delta — which is the
/// only property a storage benchmark can read off it. Anything that needs a
/// parseable document needs a real one, and this module does not pretend to
/// supply it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum ContentKind {
    /// Prose or source: repetitive, compressible, delta-friendly.
    Text,
    /// Comma-separated records: structured, line-oriented, partly repetitive.
    Csv,
    /// A binary asset with a stable header and an incompressible body.
    Binary,
    /// A compressed container profile: maximum entropy, no exploitable structure.
    Container,
}

impl ContentKind {
    /// The name recorded in descriptors and digests.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            ContentKind::Text => "text",
            ContentKind::Csv => "csv",
            ContentKind::Binary => "binary",
            ContentKind::Container => "container",
        }
    }

    /// Whether this profile counts as text for the "mostly text" shape facts.
    #[must_use]
    pub const fn is_text(self) -> bool {
        matches!(self, ContentKind::Text | ContentKind::Csv)
    }

    /// The digest tag. Stable forever; changing one invalidates every manifest.
    const fn tag(self) -> u8 {
        match self {
            ContentKind::Text => 1,
            ContentKind::Csv => 2,
            ContentKind::Binary => 3,
            ContentKind::Container => 4,
        }
    }
}

/// One file, described without generating a byte of it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileSpec {
    /// Path relative to the corpus root, `/`-separated, never absolute.
    pub path: String,
    /// Logical size in bytes.
    pub bytes: u64,
    /// The byte profile.
    pub kind: ContentKind,
    /// The derived stream that generates the bytes.
    pub stream: u64,
}

/// What an edit does to a large file (W5).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EditOp {
    /// Overwrite `length` bytes in place at `offset`.
    Overwrite {
        /// Byte offset of the first replaced byte.
        offset: u64,
        /// How many bytes are replaced.
        length: u64,
    },
    /// Splice `length` new bytes in at `offset`, shifting everything after it.
    Insert {
        /// Byte offset the new bytes land at.
        offset: u64,
        /// How many bytes are inserted.
        length: u64,
    },
    /// Add `length` bytes at the end.
    Append {
        /// How many bytes are appended.
        length: u64,
    },
}

impl EditOp {
    /// The name recorded in descriptors.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            EditOp::Overwrite { .. } => "overwrite",
            EditOp::Insert { .. } => "insert",
            EditOp::Append { .. } => "append",
        }
    }

    /// How many bytes the edit writes.
    #[must_use]
    pub const fn length(self) -> u64 {
        match self {
            EditOp::Overwrite { length, .. }
            | EditOp::Insert { length, .. }
            | EditOp::Append { length } => length,
        }
    }
}

/// The seven failure kinds plan §12.2's W6 enumerates. All seven, or the
/// workload is not W6.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum FaultKind {
    /// Two halves of the peer set stop hearing each other.
    NetworkPartition,
    /// A message is delivered more than once.
    DuplicateMessage,
    /// Messages arrive in an order the sender did not use.
    MessageReorder,
    /// A process dies without unwinding.
    ProcessCrash,
    /// A write fails because the volume is full.
    DiskFull,
    /// A stored chunk no longer matches its digest.
    CorruptChunk,
    /// A peer that has been away for a long time reconnects.
    StalePeerReconnect,
}

/// Every fault kind, in the order plan §12.2 lists them.
pub const FAULT_KINDS: [FaultKind; 7] = [
    FaultKind::NetworkPartition,
    FaultKind::DuplicateMessage,
    FaultKind::MessageReorder,
    FaultKind::ProcessCrash,
    FaultKind::DiskFull,
    FaultKind::CorruptChunk,
    FaultKind::StalePeerReconnect,
];

impl FaultKind {
    /// The name recorded in descriptors and shape facts.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            FaultKind::NetworkPartition => "network_partition",
            FaultKind::DuplicateMessage => "duplicate_message",
            FaultKind::MessageReorder => "message_reorder",
            FaultKind::ProcessCrash => "process_crash",
            FaultKind::DiskFull => "disk_full",
            FaultKind::CorruptChunk => "corrupt_chunk",
            FaultKind::StalePeerReconnect => "stale_peer_reconnect",
        }
    }

    /// The digest tag. Stable forever.
    const fn tag(self) -> u8 {
        match self {
            FaultKind::NetworkPartition => 1,
            FaultKind::DuplicateMessage => 2,
            FaultKind::MessageReorder => 3,
            FaultKind::ProcessCrash => 4,
            FaultKind::DiskFull => 5,
            FaultKind::CorruptChunk => 6,
            FaultKind::StalePeerReconnect => 7,
        }
    }
}

/// One injected failure, positioned by logical step.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FaultSpec {
    /// Position in the schedule. A step index, never a clock reading.
    pub step: u64,
    /// What goes wrong.
    pub kind: FaultKind,
    /// Which peer, actor or object it happens to.
    pub subject: String,
    /// The kind-specific magnitude: peers cut off, bytes corrupted, messages replayed.
    pub magnitude: u64,
}

/// One file an actor changes (W3).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChangeSpec {
    /// Which actor made it, `actor-0000`-style.
    pub actor: String,
    /// The path it touched.
    pub path: String,
    /// Whether the path came from the pool actors share.
    pub shared: bool,
    /// How many bytes the change rewrites.
    pub bytes: u64,
}

/// A checkpoint one actor takes after a run of changes (W3).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CheckpointSpec {
    /// Whose checkpoint it is.
    pub actor: String,
    /// How many of that actor's changes it covers.
    pub covers_changes: u64,
}

/// Everything a generator emits, in one order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Item {
    /// A file in the corpus.
    File(FileSpec),
    /// A path the workload reads (W2's 5% accessed subset).
    Access {
        /// The path that is read.
        path: String,
    },
    /// A named activity replayed against the corpus (W1's install, build, test).
    Activity {
        /// The activity's name.
        name: String,
    },
    /// An actor's change to one file (W3).
    Change(ChangeSpec),
    /// An actor's checkpoint (W3).
    Checkpoint(CheckpointSpec),
    /// An edit to the large file (W5).
    Edit {
        /// The file the edit applies to.
        path: String,
        /// What the edit does.
        op: EditOp,
        /// The stream generating the replacement bytes.
        stream: u64,
    },
    /// An injected failure (W6).
    Fault(FaultSpec),
}

impl Item {
    /// The digest tag. Stable forever; changing one invalidates every manifest.
    pub(crate) const fn tag(&self) -> u8 {
        match self {
            Item::File(_) => 1,
            Item::Access { .. } => 2,
            Item::Activity { .. } => 3,
            Item::Change(_) => 4,
            Item::Checkpoint(_) => 5,
            Item::Edit { .. } => 6,
            Item::Fault(_) => 7,
        }
    }

    /// The file behind this item, if it is one.
    #[must_use]
    pub const fn as_file(&self) -> Option<&FileSpec> {
        match self {
            Item::File(file) => Some(file),
            _ => None,
        }
    }

    /// The item as JSON, for `mesh-bench corpus describe`.
    #[must_use]
    pub fn to_json(&self) -> Json {
        let object = match self {
            Item::File(file) => JsonObject::new()
                .with("item", Json::string("file"))
                .with("path", Json::string(&file.path))
                .with("bytes", Json::Uint(file.bytes))
                .with("kind", Json::string(file.kind.name())),
            Item::Access { path } => JsonObject::new()
                .with("item", Json::string("access"))
                .with("path", Json::string(path)),
            Item::Activity { name } => JsonObject::new()
                .with("item", Json::string("activity"))
                .with("name", Json::string(name)),
            Item::Change(change) => JsonObject::new()
                .with("item", Json::string("change"))
                .with("actor", Json::string(&change.actor))
                .with("path", Json::string(&change.path))
                .with("shared", Json::Bool(change.shared))
                .with("bytes", Json::Uint(change.bytes)),
            Item::Checkpoint(checkpoint) => JsonObject::new()
                .with("item", Json::string("checkpoint"))
                .with("actor", Json::string(&checkpoint.actor))
                .with("covers_changes", Json::Uint(checkpoint.covers_changes)),
            Item::Edit { path, op, .. } => JsonObject::new()
                .with("item", Json::string("edit"))
                .with("path", Json::string(path))
                .with("op", Json::string(op.name()))
                .with("offset", Json::Uint(edit_offset(*op)))
                .with("length", Json::Uint(op.length())),
            Item::Fault(fault) => JsonObject::new()
                .with("item", Json::string("fault"))
                .with("step", Json::Uint(fault.step))
                .with("kind", Json::string(fault.kind.name()))
                .with("subject", Json::string(&fault.subject))
                .with("magnitude", Json::Uint(fault.magnitude)),
        };
        Json::Object(object)
    }
}

/// The offset an edit acts at; an append acts at the end, reported as zero
/// because the end is not a position the descriptor knows before replay.
const fn edit_offset(op: EditOp) -> u64 {
    match op {
        EditOp::Overwrite { offset, .. } | EditOp::Insert { offset, .. } => offset,
        EditOp::Append { .. } => 0,
    }
}

/// The stable tags, exposed so the digest and the tests read the same table.
pub(crate) const fn content_tag(kind: ContentKind) -> u8 {
    kind.tag()
}

/// The stable fault tags, exposed for the same reason.
pub(crate) const fn fault_tag(kind: FaultKind) -> u8 {
    kind.tag()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_fault_kind_is_listed_exactly_once() {
        let mut kinds = FAULT_KINDS.to_vec();
        kinds.sort();
        kinds.dedup();
        assert_eq!(kinds.len(), FAULT_KINDS.len(), "a kind is listed twice");
        assert_eq!(FAULT_KINDS.len(), 7, "plan 12.2 lists seven failure kinds");
    }

    #[test]
    fn fault_names_are_distinct() {
        let mut names: Vec<&str> = FAULT_KINDS.iter().map(|kind| kind.name()).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), 7);
    }

    #[test]
    fn fault_tags_are_distinct() {
        let mut tags: Vec<u8> = FAULT_KINDS.iter().map(|kind| fault_tag(*kind)).collect();
        tags.sort_unstable();
        tags.dedup();
        assert_eq!(tags.len(), 7);
    }

    #[test]
    fn content_tags_are_distinct() {
        let kinds = [
            ContentKind::Text,
            ContentKind::Csv,
            ContentKind::Binary,
            ContentKind::Container,
        ];
        let mut tags: Vec<u8> = kinds.iter().map(|kind| content_tag(*kind)).collect();
        tags.sort_unstable();
        tags.dedup();
        assert_eq!(tags.len(), kinds.len());
    }

    #[test]
    fn text_profiles_are_the_ones_that_count_as_text() {
        assert!(ContentKind::Text.is_text());
        assert!(ContentKind::Csv.is_text());
        assert!(!ContentKind::Binary.is_text());
        assert!(!ContentKind::Container.is_text());
    }

    #[test]
    fn item_tags_are_distinct() {
        let items = [
            Item::File(FileSpec {
                path: "a".to_owned(),
                bytes: 1,
                kind: ContentKind::Text,
                stream: 0,
            }),
            Item::Access {
                path: "a".to_owned(),
            },
            Item::Activity {
                name: "build".to_owned(),
            },
            Item::Change(ChangeSpec {
                actor: "actor-0000".to_owned(),
                path: "a".to_owned(),
                shared: false,
                bytes: 1,
            }),
            Item::Checkpoint(CheckpointSpec {
                actor: "actor-0000".to_owned(),
                covers_changes: 1,
            }),
            Item::Edit {
                path: "a".to_owned(),
                op: EditOp::Append { length: 1 },
                stream: 0,
            },
            Item::Fault(FaultSpec {
                step: 0,
                kind: FaultKind::DiskFull,
                subject: "peer-0".to_owned(),
                magnitude: 1,
            }),
        ];
        let mut tags: Vec<u8> = items.iter().map(Item::tag).collect();
        tags.sort_unstable();
        tags.dedup();
        assert_eq!(tags.len(), items.len());
    }

    #[test]
    fn edit_lengths_are_reported_for_every_operation() {
        assert_eq!(
            EditOp::Overwrite {
                offset: 5,
                length: 7
            }
            .length(),
            7
        );
        assert_eq!(
            EditOp::Insert {
                offset: 5,
                length: 9
            }
            .length(),
            9
        );
        assert_eq!(EditOp::Append { length: 3 }.length(), 3);
        assert_eq!(EditOp::Append { length: 3 }.name(), "append");
    }

    #[test]
    fn a_file_item_serialises_its_shape_and_not_its_stream() {
        let json = Item::File(FileSpec {
            path: "src/lib.rs".to_owned(),
            bytes: 12,
            kind: ContentKind::Text,
            stream: 0xdead_beef,
        })
        .to_json();
        let object = json.as_object().expect("an object");
        assert_eq!(
            object.get("path").and_then(Json::as_str),
            Some("src/lib.rs")
        );
        assert_eq!(object.get("bytes").and_then(Json::as_u64), Some(12));
        assert!(
            object.get("stream").is_none(),
            "the stream seed is an implementation detail of the bytes, not part of the shape"
        );
    }
}
