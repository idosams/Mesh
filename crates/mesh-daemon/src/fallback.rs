//! What Mesh gives up when it watches a folder instead of connecting to it, and how it says so.
//!
//! # One sentence this module is built around
//!
//! **The folder-watching fallback is chosen only when the direct connection is unavailable, and
//! choosing it always produces a sentence.** Plan §7.4: a watcher-based directory adapter is
//! useful for adoption and recovery and *must not be considered the authoritative final
//! mechanism*, because watchers coalesce and omit. Nothing here can present it as one.
//!
//! # Why the announcement is not a `bool` and not an `Option` the caller fills in
//!
//! [`BackendChoice`] has no public constructor. [`choose_backend`] is the only way to obtain one,
//! it takes an [`Availability`], and the fallback arm carries
//! [`crate::user_messages::FALLBACK_IN_USE`] and the whole of [`FallbackRestriction::ALL`]
//! unconditionally. A caller cannot build a silent fallback, and a future edit that tried would
//! have to add a constructor — which is a review-visible act rather than a forgotten argument.
//! That is acceptance criterion 4 of task `01KZC2QR9VVJK6Y60PS8D360JT` made structural.
//!
//! # What is here and what is next door
//!
//! This module is the **product surface**: the restriction list, the sentence each restriction is
//! said in, and the selection rule. The `WorkspaceAdapter` implementation that walks a real
//! directory is [`crate::folder_watch`], one module along in this same crate, and
//! [`crate::folder_watch::DECLARED_RESTRICTIONS`] is [`FallbackRestriction::ALL`] mapped through
//! [`FallbackRestriction::id`] rather than a second hand-kept copy of it.
//!
//! Both of those are new, and what they replaced is worth recording. Until task
//! `01KZC2QR9VVJK6Y60PS8D360JT` was wired, `mesh-daemon` had no dependency edge to
//! `mesh-materializer`, so the backend could only be compiled inside that crate's test target —
//! and this file was **not declared in `lib.rs` at all**, so the compiler never saw it. The
//! restriction list, the announcement and the selection rule below were source text that one test
//! in another crate read with a string matcher. Nothing a person could start reached any of it.
//!
//! # Nothing here reads a clock
//!
//! A restriction is a property of the mechanism, not of when it ran, and the selection rule is a
//! total function of one enumeration. Both are `const`.

use core::fmt;

use crate::ipc::json::Json;
use crate::user_messages;

/// One thing the folder-watching fallback cannot do that a direct connection can.
///
/// The list is closed and every entry is published: the criterion is that *every* restriction
/// relative to a direct connection is declared and surfaced, so a restriction discovered later is
/// a variant here rather than a paragraph somewhere.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum FallbackRestriction {
    /// Changes are found by re-reading the folder, so several edits arrive as one.
    ChangesAreFoundLate,
    /// A file created and removed between two readings is never seen.
    ShortLivedWorkIsMissed,
    /// A rename made outside Mesh is worked out afterwards, never observed.
    RenamesAreWorkedOutAfterwards,
    /// Watching a folder cannot tell Mesh when an application finished writing.
    NoSavePointFromTheFolder,
    /// The fallback reaches only inside the folder it was given.
    ConfinedToItsFolder,
    /// A shortcut to another location is left out of what Mesh presents.
    LinksAreNotPresented,
    /// An earlier version put on disk is read-only in Mesh, not to other applications.
    EarlierVersionIsNotProtected,
}

impl FallbackRestriction {
    /// Every restriction, in the order a person is shown them.
    ///
    /// Ordered most-surprising-first: what Mesh misses comes before where it is confined.
    pub const ALL: [Self; 7] = [
        Self::ChangesAreFoundLate,
        Self::ShortLivedWorkIsMissed,
        Self::RenamesAreWorkedOutAfterwards,
        Self::NoSavePointFromTheFolder,
        Self::ConfinedToItsFolder,
        Self::LinksAreNotPresented,
        Self::EarlierVersionIsNotProtected,
    ];

    /// The stable identifier, which is what the adapter's own declaration is matched against.
    ///
    /// A machine name and not a sentence: the sentence changes when a writer improves it, and a
    /// check that compared sentences would break every time one did.
    #[must_use]
    pub const fn id(self) -> &'static str {
        match self {
            Self::ChangesAreFoundLate => "changes-are-found-late",
            Self::ShortLivedWorkIsMissed => "short-lived-work-is-missed",
            Self::RenamesAreWorkedOutAfterwards => "renames-are-worked-out-afterwards",
            Self::NoSavePointFromTheFolder => "no-save-point-from-the-folder",
            Self::ConfinedToItsFolder => "confined-to-its-folder",
            Self::LinksAreNotPresented => "links-are-not-presented",
            Self::EarlierVersionIsNotProtected => "earlier-version-is-not-protected",
        }
    }

    /// The sentence a person reads for this restriction.
    ///
    /// Every arm resolves to a constant in [`crate::user_messages`], which is the one path
    /// `tools/program/vocab-lint/surfaces.json` scans. A sentence written inline here would ship
    /// to a person without ever meeting the vocabulary gate.
    #[must_use]
    pub const fn headline(self) -> &'static str {
        match self {
            Self::ChangesAreFoundLate => user_messages::FALLBACK_CHANGES_ARE_FOUND_LATE,
            Self::ShortLivedWorkIsMissed => user_messages::FALLBACK_SHORT_LIVED_WORK_IS_MISSED,
            Self::RenamesAreWorkedOutAfterwards => {
                user_messages::FALLBACK_RENAMES_ARE_WORKED_OUT_AFTERWARDS
            }
            Self::NoSavePointFromTheFolder => user_messages::FALLBACK_NO_SAVE_POINT_FROM_THE_FOLDER,
            Self::ConfinedToItsFolder => user_messages::FALLBACK_CONFINED_TO_ITS_FOLDER,
            Self::LinksAreNotPresented => user_messages::FALLBACK_LINKS_ARE_NOT_PRESENTED,
            Self::EarlierVersionIsNotProtected => {
                user_messages::FALLBACK_EARLIER_VERSION_IS_NOT_PROTECTED
            }
        }
    }
}

/// The declaration order is the identifier order, so slot `i` holds variant `i`.
///
/// Without it a variant inserted in the middle would silently reorder what a person is shown while
/// every test that iterates `ALL` still passed.
const _: () = {
    let mut index = 0;
    while index < FallbackRestriction::ALL.len() {
        assert!(
            FallbackRestriction::ALL[index] as usize == index,
            "FallbackRestriction::ALL is not in declaration order"
        );
        index += 1;
    }
};

/// Whether a direct file-system connection can be used for this workspace.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Availability {
    /// A direct connection is available and will be used.
    DirectConnection,
    /// No direct connection is available on this device.
    NoDirectConnection,
}

impl Availability {
    /// What this build can actually reach.
    ///
    /// **It answers from what is linked in, never from the platform name.** A direct file-system
    /// connection is a mounted `WorkspaceAdapter` — `mesh-fskit-ffi` on macOS, `mesh-fuse`
    /// elsewhere — and `mesh-daemon` declares an edge to neither, so no build of this service can
    /// open one. Answering [`Availability::DirectConnection`] on macOS because macOS is the
    /// platform FSKit ships on would announce a mechanism that is not in the binary, and the
    /// person would be told they have full fidelity by a process that cannot deliver it.
    ///
    /// This is the one function that has to change when an adapter is linked in, and
    /// [`DIRECT_CONNECTION_CRATES`] names the crates whose absence is being reported, so the
    /// change is one grep away rather than a search of the whole service.
    #[must_use]
    pub const fn probe() -> Self {
        Self::NoDirectConnection
    }
}

/// The crates that would supply a direct file-system connection, and neither is a dependency.
///
/// Published rather than left in a comment because [`Availability::probe`] is a constant answer
/// and a constant answer needs its reason to be checkable: `daemon_declares_no_direct_connection`
/// in `crates/mesh-daemon/tests/folder-watch.rs` reads this crate's own manifest and fails the
/// build the day one of these becomes a dependency while `probe` still says there is none.
pub const DIRECT_CONNECTION_CRATES: [&str; 2] = ["mesh-fskit-ffi", "mesh-fuse"];

/// Which mechanism a workspace is being served through.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum WorkspaceBackend {
    /// The direct file-system connection.
    DirectConnection,
    /// The folder-watching fallback.
    FolderWatch,
}

impl WorkspaceBackend {
    /// The published name.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::DirectConnection => "direct-connection",
            Self::FolderWatch => "folder-watch",
        }
    }

    /// Whether this mechanism may be presented as the authoritative one.
    ///
    /// Plan §7.4 says the watcher fallback may not be, in as many words.
    #[must_use]
    pub const fn is_authoritative(self) -> bool {
        matches!(self, Self::DirectConnection)
    }
}

/// What Mesh decided to serve a workspace through, and what it owes the person as a result.
///
/// No public constructor: [`choose_backend`] is the only way to make one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BackendChoice {
    backend: WorkspaceBackend,
    announcement: Option<&'static str>,
    restrictions: &'static [FallbackRestriction],
}

impl BackendChoice {
    /// The mechanism chosen.
    #[must_use]
    pub const fn backend(self) -> WorkspaceBackend {
        self.backend
    }

    /// The sentence that must reach the person, when one is owed.
    ///
    /// `Some` for the fallback, always. `None` for the direct connection, because there is nothing
    /// to warn about and a warning nobody needs is how people learn to ignore warnings.
    #[must_use]
    pub const fn announcement(self) -> Option<&'static str> {
        self.announcement
    }

    /// The restrictions in force under this choice.
    #[must_use]
    pub const fn restrictions(self) -> &'static [FallbackRestriction] {
        self.restrictions
    }

    /// Whether the fallback was chosen without saying so. Always false, and checkable.
    ///
    /// The criterion is a negative — *the adapter is never selected silently over an available
    /// direct connection* — and a negative that no expression evaluates is a negative nobody can
    /// test. This is the expression.
    #[must_use]
    pub const fn is_silent_fallback(self) -> bool {
        matches!(self.backend, WorkspaceBackend::FolderWatch) && self.announcement.is_none()
    }

    /// This choice as one line a person reads, and a client can parse.
    ///
    /// Key order: `backend`, `authoritative`, `announcement`, `restrictions`; and within a
    /// restriction, `id`, `headline`. The announcement is `null` for the direct connection and the
    /// restriction list is empty there, which is the same fact said twice on purpose: a client
    /// that renders only one of the two fields still cannot show a fallback as unrestricted.
    #[must_use]
    pub fn to_json(self) -> Json {
        Json::object([
            ("backend", Json::text(self.backend.as_str())),
            ("authoritative", Json::Bool(self.backend.is_authoritative())),
            (
                "announcement",
                self.announcement.map_or(Json::Null, Json::text),
            ),
            (
                "restrictions",
                Json::Array(
                    self.restrictions
                        .iter()
                        .map(|restriction| {
                            Json::object([
                                ("id", Json::text(restriction.id())),
                                ("headline", Json::text(restriction.headline())),
                            ])
                        })
                        .collect(),
                ),
            ),
        ])
    }
}

impl fmt::Display for BackendChoice {
    /// What `meshd` writes to standard error when it has chosen.
    ///
    /// The announcement first and then one line per restriction, numbered, because a person
    /// reading a terminal reads the first line and skims the rest — so the first line has to be
    /// the fact that Mesh is watching rather than connecting, not a heading.
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.announcement {
            None => write!(formatter, "{}", user_messages::DIRECT_CONNECTION_IN_USE),
            Some(announcement) => {
                write!(formatter, "{announcement}")?;
                for (index, restriction) in self.restrictions.iter().enumerate() {
                    write!(formatter, "\n  {}. {}", index + 1, restriction.headline())?;
                }
                Ok(())
            }
        }
    }
}

/// Choose how to serve a workspace.
///
/// The whole rule: the folder-watching fallback is used when, and only when, there is no direct
/// connection, and choosing it carries the announcement and the full restriction list. There is no
/// argument that would let a caller prefer the fallback while a direct connection is available,
/// which is what acceptance criterion 4 asks for.
#[must_use]
pub const fn choose_backend(availability: Availability) -> BackendChoice {
    match availability {
        Availability::DirectConnection => BackendChoice {
            backend: WorkspaceBackend::DirectConnection,
            announcement: None,
            restrictions: &[],
        },
        Availability::NoDirectConnection => BackendChoice {
            backend: WorkspaceBackend::FolderWatch,
            announcement: Some(user_messages::FALLBACK_IN_USE),
            restrictions: &FallbackRestriction::ALL,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_direct_connection_is_used_when_one_is_available() {
        let choice = choose_backend(Availability::DirectConnection);
        assert_eq!(choice.backend(), WorkspaceBackend::DirectConnection);
        assert!(choice.backend().is_authoritative());
        assert_eq!(choice.announcement(), None);
        assert!(choice.restrictions().is_empty());
        assert!(!choice.is_silent_fallback());
    }

    #[test]
    fn the_fallback_is_never_chosen_without_saying_so() {
        let choice = choose_backend(Availability::NoDirectConnection);
        assert_eq!(choice.backend(), WorkspaceBackend::FolderWatch);
        assert!(!choice.backend().is_authoritative());
        assert_eq!(choice.announcement(), Some(user_messages::FALLBACK_IN_USE));
        assert_eq!(choice.restrictions().len(), FallbackRestriction::ALL.len());
        assert!(!choice.is_silent_fallback());
    }

    /// Both arms, exhaustively, rather than the two spelled above: a third `Availability` would
    /// otherwise reach a person through whichever arm somebody wrote for it.
    #[test]
    fn no_choice_of_any_availability_is_a_silent_fallback() {
        for availability in [
            Availability::DirectConnection,
            Availability::NoDirectConnection,
        ] {
            let choice = choose_backend(availability);
            assert!(!choice.is_silent_fallback(), "{availability:?}");
            if !choice.backend().is_authoritative() {
                assert!(choice.announcement().is_some(), "{availability:?}");
                assert!(!choice.restrictions().is_empty(), "{availability:?}");
            }
        }
    }

    #[test]
    fn every_restriction_has_its_own_identifier_and_its_own_sentence() {
        let mut ids: Vec<&'static str> = FallbackRestriction::ALL.iter().map(|r| r.id()).collect();
        let mut sentences: Vec<&'static str> = FallbackRestriction::ALL
            .iter()
            .map(|r| r.headline())
            .collect();
        assert_eq!(ids.len(), FallbackRestriction::ALL.len());
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(
            ids.len(),
            FallbackRestriction::ALL.len(),
            "two restrictions share an identifier"
        );
        sentences.sort_unstable();
        sentences.dedup();
        assert_eq!(
            sentences.len(),
            FallbackRestriction::ALL.len(),
            "two restrictions share a sentence"
        );
        for restriction in FallbackRestriction::ALL {
            assert!(
                !restriction.headline().is_empty(),
                "{} has no sentence",
                restriction.id()
            );
            assert!(
                restriction
                    .id()
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b == b'-'),
                "{} is not a machine name",
                restriction.id()
            );
        }
    }
}
