//! The portable metadata set: what mesh carries between hosts, and what it deliberately drops.
//!
//! # The one sentence this module is built around
//!
//! **The set mesh drops is published in the same breath as the set it keeps, because a person who
//! does not know a field is dropped will assume it is kept.** Every synchronizing tool loses
//! something. The ones people trust say which something, up front, in a list they can read.
//!
//! # Why this is an enumeration rather than a page of prose
//!
//! A page of prose about metadata goes stale the first time an adapter is written, and nothing
//! fails when it does. Here, `why()` and `Display` are exhaustive matches over every variant, so a
//! field added to either list without a name and a stated reason **does not compile**. The lists
//! [`PreservedMetadata::EVERY`] and [`DroppedMetadata::EVERY`] make the two sets iterable rather
//! than quotable, and the tests below require them to be duplicate-free and to match their
//! declared lengths.
//!
//! The residual, since a check that is described but not bounded is worth less than none: adding a
//! variant and forgetting `EVERY` compiles. The compiler catches the reason and the name; the
//! length assertion catches the list. Nothing here catches a field mesh gained that nobody thought
//! to enumerate at all — that is what review is for.
//!
//! # The rule that decides which list a field goes in
//!
//! A field is **preserved** only if it means the same thing on every host mesh supports and can be
//! reconstructed there exactly. Everything else is dropped, and dropped means *never written and
//! never claimed* — not "written where possible", which is the shape that produces a workspace
//! whose permissions depend on which machine last touched it.
//!
//! Two consequences worth stating, because both look like omissions:
//!
//! * **No timestamps.** Not modification time, not creation time. Plan-wide, order is
//!   `lamport → event ULID → content hash` and never wall-clock; a preserved mtime is a wall clock
//!   in the data model wearing a filesystem's clothes, and the first tool to sort by it would be
//!   sorting by the least trustworthy field mesh has.
//! * **No owner and no group.** A numeric uid means a different person on a different host, so
//!   carrying it faithfully is worse than dropping it: it would restore a plausible wrong answer
//!   instead of an obviously absent one.
//!
//! # The claim ceiling, stated rather than implied
//!
//! This module **declares** the set. It does not implement a filesystem adapter and cannot, from
//! here, demonstrate a round trip through one — no adapter exists on this tree. What it buys is
//! that every adapter written later is written against one published list rather than against
//! whatever each author assumed, and that a field's absence is a recorded decision with a reason
//! attached rather than an oversight nobody can distinguish from one.

use core::fmt;

/// A metadata field mesh carries between hosts.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PreservedMetadata {
    /// Whether an object holds content or holds other objects.
    ObjectKind,
    /// The bytes of a file, exactly, with no transformation of any kind.
    ContentBytes,
    /// The directory entry name, byte for byte as the author wrote it.
    EntryName,
    /// Which directory an object hangs in.
    ParentDirectory,
    /// Whether a file is executable.
    ExecutableBit,
    /// The text a symbolic link points at, as text.
    SymbolicLinkTarget,
}

impl PreservedMetadata {
    /// Every preserved field. Exhaustive; `tests/names.rs` holds it against the enumeration.
    pub const EVERY: [Self; 6] = [
        Self::ObjectKind,
        Self::ContentBytes,
        Self::EntryName,
        Self::ParentDirectory,
        Self::ExecutableBit,
        Self::SymbolicLinkTarget,
    ];

    /// Why this field survives every host.
    #[must_use]
    pub const fn why(self) -> &'static str {
        match self {
            Self::ObjectKind => {
                "a file and a directory are the same two things everywhere, and confusing them \
                 loses the whole subtree below"
            }
            Self::ContentBytes => {
                "the bytes are the work; a transformation applied on the way through is the one \
                 loss no amount of metadata makes up for"
            }
            Self::EntryName => {
                "the name is the author's, and a name mesh cannot write on the target volume is \
                 refused before materialization rather than repaired into something that fits"
            }
            Self::ParentDirectory => {
                "structure is meaning; an object that arrives without its directory has arrived \
                 somewhere else"
            }
            Self::ExecutableBit => {
                "it is the one permission bit that changes what a file IS rather than who may \
                 touch it, and Windows can model it in the adapter even though NTFS has no such \
                 bit"
            }
            Self::SymbolicLinkTarget => {
                "a link's target is content, not a permission, and it round-trips as text on \
                 every host mesh supports"
            }
        }
    }
}

impl fmt::Display for PreservedMetadata {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::ObjectKind => "object kind",
            Self::ContentBytes => "content bytes",
            Self::EntryName => "entry name",
            Self::ParentDirectory => "parent directory",
            Self::ExecutableBit => "executable bit",
            Self::SymbolicLinkTarget => "symbolic link target",
        })
    }
}

/// A metadata field mesh deliberately does not carry.
///
/// Deliberately: each of these was considered and refused for the reason [`DroppedMetadata::why`]
/// gives. A field that is merely unimplemented does not belong here, because a reader would take
/// its presence as a decision.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum DroppedMetadata {
    /// The owning user.
    Owner,
    /// The owning group.
    Group,
    /// Read and write permission bits, for owner, group and other.
    ReadWritePermissionBits,
    /// The setuid, setgid and sticky bits.
    SetuidSetgidSticky,
    /// When the content was last modified.
    ModificationTime,
    /// When the object was last read.
    AccessTime,
    /// When the object was created.
    CreationTime,
    /// The inode or file-index number.
    InodeNumber,
    /// Whether two entries are the same inode.
    HardLinkIdentity,
    /// Named extended attributes.
    ExtendedAttributes,
    /// POSIX and NFSv4 access control lists.
    AccessControlLists,
    /// The HFS resource fork and the NTFS alternate data streams.
    AlternateStreams,
    /// Finder flags, colour labels and the macOS quarantine bit.
    PlatformFileFlags,
    /// The DOS attribute bits: hidden, system, archive, read-only.
    WindowsFileAttributes,
    /// Which byte ranges of a file are holes rather than zeroes.
    SparseRegions,
}

impl DroppedMetadata {
    /// Every dropped field. Exhaustive; `tests/names.rs` holds it against the enumeration.
    pub const EVERY: [Self; 15] = [
        Self::Owner,
        Self::Group,
        Self::ReadWritePermissionBits,
        Self::SetuidSetgidSticky,
        Self::ModificationTime,
        Self::AccessTime,
        Self::CreationTime,
        Self::InodeNumber,
        Self::HardLinkIdentity,
        Self::ExtendedAttributes,
        Self::AccessControlLists,
        Self::AlternateStreams,
        Self::PlatformFileFlags,
        Self::WindowsFileAttributes,
        Self::SparseRegions,
    ];

    /// Why mesh does not carry this field.
    #[must_use]
    pub const fn why(self) -> &'static str {
        match self {
            Self::Owner | Self::Group => {
                "a numeric identifier names a different principal on a different host, so \
                 restoring it faithfully restores a plausible wrong answer where an absent one \
                 would have been obvious"
            }
            Self::ReadWritePermissionBits => {
                "NTFS has no such bits and every host applies its own umask, so a preserved mode \
                 would depend on which machine last wrote the file"
            }
            Self::SetuidSetgidSticky => {
                "carrying a privilege-escalation bit across a trust boundary is a capability \
                 transfer, and mesh's answer to a capability transfer is never to do it silently"
            }
            Self::ModificationTime | Self::AccessTime | Self::CreationTime => {
                "order in mesh is lamport, then event identifier, then content hash, and never a \
                 wall clock; a preserved timestamp is a clock in the data model that some tool \
                 would eventually sort by"
            }
            Self::InodeNumber => {
                "it names a location in one filesystem and means nothing in another; object \
                 identity is mesh's own and does not move when a file does"
            }
            Self::HardLinkIdentity => {
                "two names for one inode is a graph, not a tree, and Windows and most \
                 synchronization surfaces cannot represent it; each name materializes as its own \
                 object"
            }
            Self::ExtendedAttributes | Self::AccessControlLists => {
                "neither has a portable schema, and a partially-restored access control list is \
                 more dangerous than none at all"
            }
            Self::AlternateStreams => {
                "resource forks and alternate data streams exist on one host each and would \
                 vanish on the other, so carrying them would promise a round trip that cannot \
                 happen"
            }
            Self::PlatformFileFlags | Self::WindowsFileAttributes => {
                "presentation and policy belonging to one desktop, with no meaning on the other \
                 hosts a workspace is shared with"
            }
            Self::SparseRegions => {
                "a hole and a run of zero bytes are the same content; sparseness is a storage \
                 optimization the target filesystem re-decides for itself"
            }
        }
    }
}

impl fmt::Display for DroppedMetadata {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Owner => "owner",
            Self::Group => "group",
            Self::ReadWritePermissionBits => "read and write permission bits",
            Self::SetuidSetgidSticky => "setuid, setgid and sticky bits",
            Self::ModificationTime => "modification time",
            Self::AccessTime => "access time",
            Self::CreationTime => "creation time",
            Self::InodeNumber => "inode number",
            Self::HardLinkIdentity => "hard link identity",
            Self::ExtendedAttributes => "extended attributes",
            Self::AccessControlLists => "access control lists",
            Self::AlternateStreams => "resource forks and alternate data streams",
            Self::PlatformFileFlags => "platform file flags",
            Self::WindowsFileAttributes => "Windows file attributes",
            Self::SparseRegions => "sparse regions",
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_preserved_field_is_listed_and_reasoned() {
        assert_eq!(PreservedMetadata::EVERY.len(), 6);
        for field in PreservedMetadata::EVERY {
            assert!(!field.why().is_empty(), "{field} has no reason");
            assert!(!field.to_string().is_empty());
        }
    }

    #[test]
    fn every_dropped_field_is_listed_and_reasoned() {
        assert_eq!(DroppedMetadata::EVERY.len(), 15);
        for field in DroppedMetadata::EVERY {
            assert!(!field.why().is_empty(), "{field} has no reason");
            assert!(!field.to_string().is_empty());
        }
    }

    #[test]
    fn no_field_appears_twice() {
        let mut preserved = PreservedMetadata::EVERY;
        preserved.sort_unstable();
        preserved
            .windows(2)
            .for_each(|pair| assert_ne!(pair[0], pair[1]));
        let mut dropped = DroppedMetadata::EVERY;
        dropped.sort_unstable();
        dropped
            .windows(2)
            .for_each(|pair| assert_ne!(pair[0], pair[1]));
    }

    #[test]
    fn no_timestamp_is_preserved() {
        let names: Vec<String> = PreservedMetadata::EVERY
            .iter()
            .map(ToString::to_string)
            .collect();
        for word in ["time", "clock", "date"] {
            assert!(
                !names.iter().any(|name| name.contains(word)),
                "a preserved field mentions {word}"
            );
        }
    }
}
