//! The oracle three filesystem backends are graded by.
//!
//! # One sentence this module is built around
//!
//! **A backend is conformant when every rule the published material states is observably true of
//! it — including the rules about the things it says it cannot do.** [`run_conformance`] takes
//! `&dyn WorkspaceAdapter`, so it can never name a concrete backend, and adding a fourth backend
//! adds a call rather than a file.
//!
//! Design `01KZEZGDPMZ5RH7E60WDYDYYEE` is the contract; task `01KZC2M9305RFTMVSHJPG7HJGT` is where
//! it came from; `tests/compatibility/adapter/v0/` is the published half a backend author reads,
//! and every case here cites that rather than citing this file.
//!
//! # An undeclared capability is probed, never skipped
//!
//! The one decision this module turns on. A protocol suite never asks a client about a capability
//! it did not declare; here the operation is called **once**, and the answer is graded:
//! `Err(Unsupported { capability })` passes, `Ok` fails, an unwind fails under a different
//! identifier, and a clean refusal that is not `Unsupported` fails under a third. Never asking
//! cannot tell a backend that has not built `write` from one whose `write` answers `Ok(n)` and
//! drops the bytes — and on a filesystem the second one is data loss.
//!
//! # What this module may name, and what it may not
//!
//! It names the adapter contract's own items plus `NormalizedName`, `NameError`,
//! `PortableMetadata`, `ObjectKind`, `ObjectId`, `HeadId`, `ActorId` and `WorkspaceId`, and
//! nothing else from this crate. `tests/adapter-conformance.rs` reads this file as source text and
//! fails if that stops being true, because a suite that cannot be written without this crate's
//! internals is a suite whose backends cannot be written without them either.
//!
//! # Four things this module does not do, stated rather than discovered
//!
//! 1. **It does not silence a backend that unwinds.** A case body runs under `catch_unwind`, so an
//!    unwind is graded rather than fatal, but the message still reaches stderr. A run that prints
//!    one and reports `fail` is working correctly.
//! 2. **It allocates no buffer for file content.** Reads and writes go through fixed stack arrays
//!    and byte-string literals, so the crate's "no byte of file content" invariant survives the
//!    suite as well as the seam.
//! 3. **It reads no clock, no file and no environment.** The report is a pure function of the
//!    answers the backend gave, which is what makes running it twice a determinism check rather
//!    than a coincidence.
//! 4. **It does not manufacture a workspace or a head — it asks for one.**
//!    [`WorkspaceAdapter::prepare_fixture`] is called once, before anything else, and everything
//!    below is mounted and presented with what it answered. A backend that refuses an identifier
//!    it was never given is therefore graded rather than skipped. A backend that declares
//!    `MountActorView` or `MaterializeReadonlyView` and then will not prepare is graded `fail` on
//!    those families, never `unsupported`: a declaration nothing can check is the silent
//!    divergence this suite exists to find, and `unsupported` is the word an honest partial
//!    backend earns.

use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::Path;

use crate::adapter::{
    AdapterCapability, AdapterError, AdapterFixture, CapabilitySet, CheckpointCandidate,
    EventSequence, FsEvent, FsEventKind, OpenHandle, OpenMode, ViewAccess, ViewId,
    WorkspaceAdapter, WorkspaceView, WORKSPACE_ADAPTER_CONTRACT, WORKSPACE_ADAPTER_CONTRACT_V1,
};
use crate::ids::{ActorId, HeadId, ObjectId, WorkspaceId};
use crate::name::{NameError, NormalizedName, PortableMetadata};
use crate::version::ObjectKind;

// ---------------------------------------------------------------------------------------------
// The three results and the eight families
// ---------------------------------------------------------------------------------------------

/// How one case came out.
///
/// The same three words a protocol client is graded with, reused so a reader of one report can
/// read the other.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum CaseResult {
    /// The backend answered, and the answer matches the published rule.
    Pass,
    /// The backend answered and broke a rule, or unwound, or refused a capability it declared.
    Fail,
    /// The case was not run, and nothing about the backend is claimed by it.
    Unsupported,
}

impl CaseResult {
    /// The published name, which is the name in `tests/compatibility/adapter/v0/vocabulary.json`.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Pass => "pass",
            Self::Fail => "fail",
            Self::Unsupported => "unsupported",
        }
    }

    /// Whether this result makes the whole report non-conformant.
    #[must_use]
    pub const fn affects_verdict(self) -> bool {
        matches!(self, Self::Fail)
    }
}

impl core::fmt::Display for CaseResult {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// Which question a case is asking.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum CaseFamily {
    /// The declaration itself.
    Cap,
    /// One filesystem operation's pre- and post-conditions.
    Op,
    /// A read-only view refuses every mutating operation.
    ReadOnly,
    /// Names the portable-name rules refuse.
    Name,
    /// Enumeration order.
    Order,
    /// Durable-boundary observation.
    Boundary,
    /// Mount lifetime.
    Mount,
    /// The catalogue's own rules.
    Catalogue,
}

impl CaseFamily {
    /// Every family this suite emits, in the order the report renders them.
    ///
    /// `VOC` is deliberately absent: it holds `vocabulary.json` against the Rust enumerations,
    /// which needs to read a file, and nothing under `crates/mesh-materializer/src/` may name a
    /// filesystem. It lives in `tests/adapter-conformance.rs`.
    pub const ALL: [Self; 8] = [
        Self::Cap,
        Self::Op,
        Self::ReadOnly,
        Self::Name,
        Self::Order,
        Self::Boundary,
        Self::Mount,
        Self::Catalogue,
    ];

    /// The published identifier, which is the identifier in `vocabulary.json`'s `families`.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Cap => "CAP",
            Self::Op => "OP",
            Self::ReadOnly => "RO",
            Self::Name => "NAME",
            Self::Order => "ORD",
            Self::Boundary => "BND",
            Self::Mount => "MNT",
            Self::Catalogue => "CAT",
        }
    }
}

impl core::fmt::Display for CaseFamily {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

// ---------------------------------------------------------------------------------------------
// A rule, and the catalogue of them
// ---------------------------------------------------------------------------------------------

/// One rule a backend is held to, independent of any backend.
///
/// The catalogue is the list of these, and it is the same list for every backend — the mechanical
/// half of "adding an adapter requires no change to the suite".
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ConformanceRule {
    id: &'static str,
    family: CaseFamily,
    capability: Option<AdapterCapability>,
    rule: &'static str,
    citation: &'static str,
}

impl ConformanceRule {
    const fn new(
        id: &'static str,
        family: CaseFamily,
        capability: Option<AdapterCapability>,
        rule: &'static str,
        citation: &'static str,
    ) -> Self {
        Self {
            id,
            family,
            capability,
            rule,
            citation,
        }
    }

    /// The case identifier, which begins with its family.
    #[must_use]
    pub const fn id(&self) -> &'static str {
        self.id
    }

    /// Which question it asks.
    #[must_use]
    pub const fn family(&self) -> CaseFamily {
        self.family
    }

    /// The capability it grades, when it grades one.
    #[must_use]
    pub const fn capability(&self) -> Option<AdapterCapability> {
        self.capability
    }

    /// The rule in one sentence, written for the backend author who has to satisfy it.
    #[must_use]
    pub const fn rule(&self) -> &'static str {
        self.rule
    }

    /// Where the rule is written down. Always published material, never `crates/**`.
    #[must_use]
    pub const fn citation(&self) -> &'static str {
        self.citation
    }
}

const README: &str = "tests/compatibility/adapter/v0/README.md";
const VOCABULARY: &str = "tests/compatibility/adapter/v0/vocabulary.json";

macro_rules! rule {
    ($name:ident, $id:literal, $family:expr, $capability:expr, $rule:literal, $citation:expr) => {
        const $name: ConformanceRule =
            ConformanceRule::new($id, $family, $capability, $rule, $citation);
    };
}

rule!(
    CONTRACT_STRING,
    "CAP/contract-string",
    CaseFamily::Cap,
    None,
    "describe() returns exactly the contract string this suite grades.",
    README
);
rule!(
    DESCRIPTION_IS_STABLE,
    "CAP/description-is-stable",
    CaseFamily::Cap,
    None,
    "describe() answers the same thing twice; a declaration that moves makes every later grade meaningless.",
    README
);
rule!(
    DECLARED_THEN_REFUSED,
    "CAP/declared-then-refused",
    CaseFamily::Cap,
    None,
    "A capability the adapter declared never answers Unsupported. A backend is graded on the answer, not on the claim.",
    README
);
rule!(
    DECLARED_THEN_PANICKED,
    "CAP/declared-then-panicked",
    CaseFamily::Cap,
    None,
    "A capability the adapter declared answers rather than unwinding.",
    README
);
rule!(
    UNDECLARED_BUT_ANSWERED,
    "CAP/undeclared-but-answered",
    CaseFamily::Cap,
    None,
    "An undeclared capability is called once and never answers Ok. Answering Ok and dropping the work is the silent divergence the probe exists to find.",
    README
);
rule!(
    UNDECLARED_NOT_A_CLEAN_REFUSAL,
    "CAP/undeclared-not-a-clean-refusal",
    CaseFamily::Cap,
    None,
    "An undeclared capability refuses; it does not unwind. A hole and a stub are separate defects and are graded separately.",
    README
);
rule!(
    UNDECLARED_REFUSAL_NAMES_THE_CAPABILITY,
    "CAP/undeclared-refusal-names-the-capability",
    CaseFamily::Cap,
    None,
    "An undeclared capability refuses with exactly Err(Unsupported { capability }), naming the capability that was asked for.",
    README
);
rule!(
    SYMLINK_IS_RESERVED,
    "CAP/symlink-is-reserved",
    CaseFamily::Cap,
    Some(AdapterCapability::Symlink),
    "Symlink is reserved: mesh-workspace-adapter/0 publishes no symlink operation, so nothing here can probe it either way.",
    VOCABULARY
);
rule!(
    CITATION_IS_PUBLISHED,
    "CAT/citation-is-published",
    CaseFamily::Catalogue,
    None,
    "Every case cites published material — tests/compatibility/adapter/v0/** or the plan — never crates/**.",
    README
);
rule!(
    IDENTIFIER_NAMES_ITS_FAMILY,
    "CAT/identifier-names-its-family",
    CaseFamily::Catalogue,
    None,
    "Every case identifier begins with its own family.",
    VOCABULARY
);
rule!(
    REFUSED_NAMES_ARE_UNREPRESENTABLE,
    "NAME/refused-names-are-unrepresentable",
    CaseFamily::Name,
    None,
    "The four refused name classes cannot be built at all, so no view method can be handed one. This is why the NAME family is graded at the path surface instead.",
    README
);

rule!(
    LOOKUP_RESOLVES,
    "OP-lookup/resolves-what-was-bound",
    CaseFamily::Op,
    Some(AdapterCapability::Lookup),
    "lookup resolves a name a create bound, to the same object, with the metadata it was created with.",
    README
);
rule!(
    LOOKUP_MISSING,
    "OP-lookup/missing-name-is-not-found",
    CaseFamily::Op,
    Some(AdapterCapability::Lookup),
    "lookup of a name nothing bound is NotFound, never an empty success.",
    VOCABULARY
);
rule!(
    ENUMERATE_IS_SORTED,
    "ORD/enumerate-is-byte-lexicographic",
    CaseFamily::Order,
    Some(AdapterCapability::Enumerate),
    "enumerate returns entries in ascending byte-lexicographic order of the name's UTF-8 bytes — not insertion order, and not locale order.",
    README
);
rule!(
    ENUMERATE_REPEATS,
    "ORD/repeated-calls-agree",
    CaseFamily::Order,
    Some(AdapterCapability::Enumerate),
    "Two enumerations of an unchanged directory are the same sequence.",
    README
);
rule!(
    ENUMERATE_A_FILE,
    "ORD/a-file-is-not-a-directory",
    CaseFamily::Order,
    Some(AdapterCapability::Enumerate),
    "enumerate addressed to a file is NotADirectory.",
    VOCABULARY
);
rule!(
    OPEN_ROUND_TRIPS,
    "OP-open/handle-names-what-was-opened",
    CaseFamily::Op,
    Some(AdapterCapability::Open),
    "open answers a handle naming the view, object and mode it was taken for, and close accepts that handle exactly once.",
    README
);
rule!(
    OPEN_A_DIRECTORY,
    "OP-open/a-directory-is-not-a-file",
    CaseFamily::Op,
    Some(AdapterCapability::Open),
    "open addressed to a directory is IsADirectory.",
    VOCABULARY
);
rule!(
    READ_IS_REPORTED,
    "OP-read/short-read-is-reported",
    CaseFamily::Op,
    Some(AdapterCapability::Read),
    "read answers how many bytes it filled. Reading at or past the end fills fewer and says so; it never answers the buffer length.",
    README
);
rule!(
    WRITE_IS_REPORTED,
    "OP-write/short-write-is-reported",
    CaseFamily::Op,
    Some(AdapterCapability::Write),
    "write answers how many bytes it took, and exactly that many are readable afterwards. Answering from.len() while storing fewer is data loss.",
    README
);
rule!(
    SET_LENGTH_SHRINKS,
    "OP-length/shrink-removes-the-tail",
    CaseFamily::Op,
    Some(AdapterCapability::SetFileLength),
    "set_file_length shortens a file exactly: reading the whole file returns the preserved prefix and no byte of the old tail.",
    README
);
rule!(
    SET_LENGTH_GROWS_WITH_ZEROES,
    "OP-length/grow-zero-fills",
    CaseFamily::Op,
    Some(AdapterCapability::SetFileLength),
    "set_file_length grows a file by preserving its prefix and filling every added byte with zero.",
    README
);
rule!(
    SET_LENGTH_ZERO_IS_EMPTY,
    "OP-length/zero-is-empty",
    CaseFamily::Op,
    Some(AdapterCapability::SetFileLength),
    "set_file_length to zero leaves an empty file.",
    README
);
rule!(
    SET_LENGTH_DIRECTORY_IS_REFUSED,
    "OP-length/a-directory-is-not-a-file",
    CaseFamily::Op,
    Some(AdapterCapability::SetFileLength),
    "set_file_length addressed to a directory is IsADirectory.",
    VOCABULARY
);
rule!(
    SET_LENGTH_MISSING_IS_REFUSED,
    "OP-length/missing-object-is-not-found",
    CaseFamily::Op,
    Some(AdapterCapability::SetFileLength),
    "set_file_length addressed to an object the view does not hold is NotFound.",
    VOCABULARY
);
rule!(
    CREATE_NAME_TAKEN,
    "OP-create/name-already-taken",
    CaseFamily::Op,
    Some(AdapterCapability::CreateFile),
    "create_file over a bound name is AlreadyExists, and the entry that was already there is untouched.",
    README
);
rule!(
    CREATE_IS_VISIBLE,
    "OP-create/entry-is-visible",
    CaseFamily::Op,
    Some(AdapterCapability::CreateFile),
    "A created file is a File carrying the metadata it was created with, and the entry create_file answered is the entry the directory holds.",
    VOCABULARY
);
rule!(
    CREATE_DIRECTORY_IS_EMPTY,
    "OP-createdir/a-new-directory-is-an-empty-directory",
    CaseFamily::Op,
    Some(AdapterCapability::CreateDirectory),
    "create_directory answers a Directory with no entries, and a second create over the same name is AlreadyExists.",
    README
);
rule!(
    RENAME_KEEPS_METADATA,
    "OP-rename/metadata-survives",
    CaseFamily::Op,
    Some(AdapterCapability::Rename),
    "rename changes the name and nothing else: the object identity and the portable metadata survive unchanged.",
    README
);
rule!(
    RENAME_CLEARS_THE_OLD_NAME,
    "OP-rename/old-name-is-gone",
    CaseFamily::Op,
    Some(AdapterCapability::Rename),
    "After a rename the old name is NotFound and the new name resolves.",
    VOCABULARY
);
rule!(
    MOVE_REFUSES_A_CYCLE,
    "OP-move/refuses-a-cycle",
    CaseFamily::Op,
    Some(AdapterCapability::Move),
    "move_entry refuses to place a directory inside its own subtree, with WouldCycle, and changes nothing.",
    README
);
rule!(
    MOVE_KEEPS_METADATA,
    "OP-move/metadata-survives",
    CaseFamily::Op,
    Some(AdapterCapability::Move),
    "move_entry across two directories keeps the object identity and the portable metadata unchanged, and clears the old binding.",
    README
);
rule!(
    UNLINK_ENTRY_IS_GONE,
    "OP-unlink/entry-is-gone",
    CaseFamily::Op,
    Some(AdapterCapability::Unlink),
    "After unlink the name is NotFound and the directory no longer lists it.",
    README
);
rule!(
    RMDIR_NOT_EMPTY,
    "OP-rmdir/directory-not-empty",
    CaseFamily::Op,
    Some(AdapterCapability::RemoveDirectory),
    "remove_directory on a directory that still has entries is DirectoryNotEmpty, never a recursive delete.",
    README
);
rule!(
    RMDIR_EMPTY,
    "OP-rmdir/an-empty-directory-is-removed",
    CaseFamily::Op,
    Some(AdapterCapability::RemoveDirectory),
    "remove_directory on an empty directory removes the binding, and the name is NotFound afterwards.",
    VOCABULARY
);
rule!(
    METADATA_REPORTS,
    "OP-metadata/reports-what-create-set",
    CaseFamily::Op,
    Some(AdapterCapability::ReadMetadata),
    "metadata answers the portable metadata the object was created with.",
    README
);
rule!(
    SET_METADATA_IS_VISIBLE,
    "OP-metadata/set-is-visible",
    CaseFamily::Op,
    Some(AdapterCapability::WriteMetadata),
    "set_metadata is visible afterwards, to metadata and to the directory entry alike.",
    README
);
rule!(
    TWO_VIEWS_COEXIST,
    "MNT/two-actor-views-coexist",
    CaseFamily::Mount,
    Some(AdapterCapability::MountActorView),
    "Two actors mount at once: two distinct view identifiers, both resolving, neither serialised behind the other.",
    README
);
rule!(
    RELEASED_VIEW_IS_UNKNOWN,
    "MNT/released-view-is-unknown",
    CaseFamily::Mount,
    Some(AdapterCapability::MountActorView),
    "A released view identifier is UnknownView from then on, and releasing it twice is an error rather than a silent success.",
    README
);
rule!(
    RELATIVE_NAMES_ARE_REFUSED,
    "NAME/relative-names-are-refused",
    CaseFamily::Name,
    Some(AdapterCapability::MountActorView),
    "A mountpoint holding a path component the portable-name rules refuse is refused with NameRejected, naming the rule it broke.",
    README
);
rule!(
    READONLY_ACCESS,
    "RO/access-is-read-only",
    CaseFamily::ReadOnly,
    Some(AdapterCapability::MaterializeReadonlyView),
    "The view a read-only presentation answers reports ReadOnly access.",
    README
);
rule!(
    READONLY_WRITE_IS_REFUSED,
    "RO/write-is-refused",
    CaseFamily::ReadOnly,
    Some(AdapterCapability::MaterializeReadonlyView),
    "write on a read-only view is ReadOnly. Not Ok, and not NotFound: the refusal says why the view will not take it.",
    README
);
rule!(
    READONLY_EVERY_MUTATION,
    "RO/every-mutating-operation-is-refused",
    CaseFamily::ReadOnly,
    Some(AdapterCapability::MaterializeReadonlyView),
    "Every mutating operation on a read-only view is ReadOnly, including set_metadata.",
    README
);
rule!(
    RELATIVE_TARGET_IS_REFUSED,
    "NAME/relative-target-is-refused",
    CaseFamily::Name,
    Some(AdapterCapability::MaterializeReadonlyView),
    "A read-only target holding a path component the portable-name rules refuse is refused with NameRejected.",
    README
);
rule!(
    ONE_CLOSE_ONE_CANDIDATE,
    "BND/one-close-one-candidate",
    CaseFamily::Boundary,
    Some(AdapterCapability::ObserveDurableBoundary),
    "One release of one handle offers at most one candidate, and a candidate names the view it belongs to and a position inside that view's stream.",
    README
);
rule!(
    REPLAY_IS_IDENTICAL,
    "BND/replay-is-identical",
    CaseFamily::Boundary,
    Some(AdapterCapability::ObserveDurableBoundary),
    "observe_durable_boundary is a pure function of the stream it has been shown: replaying one stream produces the identical answer sequence.",
    README
);

const GLOBAL_RULES: [ConformanceRule; 5] = [
    CONTRACT_STRING,
    DESCRIPTION_IS_STABLE,
    REFUSED_NAMES_ARE_UNREPRESENTABLE,
    CITATION_IS_PUBLISHED,
    IDENTIFIER_NAMES_ITS_FAMILY,
];

/// The five gradings every capability gets, in the order they are emitted.
///
/// Five rather than three because a declared capability can go wrong in two ways the undeclared
/// probe has no words for, and because `Ok`, an unwind and the wrong refusal are three separate
/// defects rather than one.
const PROBE_RULES: [ConformanceRule; 5] = [
    DECLARED_THEN_REFUSED,
    DECLARED_THEN_PANICKED,
    UNDECLARED_BUT_ANSWERED,
    UNDECLARED_NOT_A_CLEAN_REFUSAL,
    UNDECLARED_REFUSAL_NAMES_THE_CAPABILITY,
];

/// The cases one capability owns.
///
/// An exhaustive `match`: a nineteenth capability is a compile error here rather than a silent
/// hole in the catalogue.
const fn rules_for(capability: AdapterCapability) -> &'static [ConformanceRule] {
    match capability {
        AdapterCapability::Lookup => &[LOOKUP_RESOLVES, LOOKUP_MISSING],
        AdapterCapability::Enumerate => &[ENUMERATE_IS_SORTED, ENUMERATE_REPEATS, ENUMERATE_A_FILE],
        AdapterCapability::Open => &[OPEN_ROUND_TRIPS, OPEN_A_DIRECTORY],
        AdapterCapability::Read => &[READ_IS_REPORTED],
        AdapterCapability::Write => &[WRITE_IS_REPORTED],
        AdapterCapability::SetFileLength => &[
            SET_LENGTH_SHRINKS,
            SET_LENGTH_GROWS_WITH_ZEROES,
            SET_LENGTH_ZERO_IS_EMPTY,
            SET_LENGTH_DIRECTORY_IS_REFUSED,
            SET_LENGTH_MISSING_IS_REFUSED,
        ],
        AdapterCapability::CreateFile => &[CREATE_NAME_TAKEN, CREATE_IS_VISIBLE],
        AdapterCapability::CreateDirectory => &[CREATE_DIRECTORY_IS_EMPTY],
        AdapterCapability::Rename => &[RENAME_KEEPS_METADATA, RENAME_CLEARS_THE_OLD_NAME],
        AdapterCapability::Move => &[MOVE_REFUSES_A_CYCLE, MOVE_KEEPS_METADATA],
        AdapterCapability::Unlink => &[UNLINK_ENTRY_IS_GONE],
        AdapterCapability::RemoveDirectory => &[RMDIR_NOT_EMPTY, RMDIR_EMPTY],
        AdapterCapability::ReadMetadata => &[METADATA_REPORTS],
        AdapterCapability::WriteMetadata => &[SET_METADATA_IS_VISIBLE],
        AdapterCapability::MountActorView => &[
            TWO_VIEWS_COEXIST,
            RELEASED_VIEW_IS_UNKNOWN,
            RELATIVE_NAMES_ARE_REFUSED,
        ],
        AdapterCapability::MaterializeReadonlyView => &[
            READONLY_ACCESS,
            READONLY_WRITE_IS_REFUSED,
            READONLY_EVERY_MUTATION,
            RELATIVE_TARGET_IS_REFUSED,
        ],
        AdapterCapability::ObserveDurableBoundary => {
            &[ONE_CLOSE_ONE_CANDIDATE, REPLAY_IS_IDENTICAL]
        }
        AdapterCapability::Symlink => &[SYMLINK_IS_RESERVED],
    }
}

/// Every rule in the catalogue, in the order a report renders them.
///
/// Generated by iterating [`AdapterCapability::ALL`] and matching exhaustively over it, so a
/// capability added to the vocabulary with no case does not compile. Published so a backend author
/// — and `tests/adapter-conformance.rs` — can read the whole catalogue without running it.
#[must_use]
pub fn conformance_catalogue() -> Vec<&'static ConformanceRule> {
    let mut rules: Vec<&'static ConformanceRule> = GLOBAL_RULES.iter().collect();
    for capability in AdapterCapability::ALL {
        rules.extend(PROBE_RULES.iter());
        rules.extend(rules_for(capability).iter());
    }
    rules
}

// ---------------------------------------------------------------------------------------------
// A graded case, and the report
// ---------------------------------------------------------------------------------------------

/// One rule, graded against one backend.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConformanceCase {
    rule: &'static ConformanceRule,
    subject: Option<AdapterCapability>,
    result: CaseResult,
    detail: String,
}

impl ConformanceCase {
    /// The rule this case graded.
    #[must_use]
    pub const fn rule(&self) -> &'static ConformanceRule {
        self.rule
    }

    /// The case identifier, which is the rule's.
    #[must_use]
    pub const fn id(&self) -> &'static str {
        self.rule.id()
    }

    /// Which capability this grading was about, when it was about one.
    ///
    /// A `CAP` probe case is emitted once per capability, so the identifier alone does not say
    /// which one; this does.
    #[must_use]
    pub const fn subject(&self) -> Option<AdapterCapability> {
        self.subject
    }

    /// How it came out.
    #[must_use]
    pub const fn result(&self) -> CaseResult {
        self.result
    }

    /// What the backend actually did, for a case that did not pass.
    #[must_use]
    pub fn detail(&self) -> &str {
        &self.detail
    }
}

impl core::fmt::Display for ConformanceCase {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(formatter, "{:<11} {}", self.result.as_str(), self.rule.id())?;
        if let Some(capability) = self.subject {
            write!(formatter, " ({capability})")?;
        }
        if self.result != CaseResult::Pass && !self.detail.is_empty() {
            write!(
                formatter,
                "\n    rule:  {}\n    cited: {}\n    found: {}",
                self.rule.rule(),
                self.rule.citation(),
                self.detail
            )?;
        }
        Ok(())
    }
}

/// What one backend was graded at.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConformanceReport {
    adapter: &'static str,
    contract: &'static str,
    declared: CapabilitySet,
    cases: Vec<ConformanceCase>,
}

impl ConformanceReport {
    /// The backend's own name, as it described itself.
    #[must_use]
    pub const fn adapter(&self) -> &'static str {
        self.adapter
    }

    /// The contract string the backend claimed.
    #[must_use]
    pub const fn contract(&self) -> &'static str {
        self.contract
    }

    /// What the backend declared.
    #[must_use]
    pub const fn declared(&self) -> CapabilitySet {
        self.declared
    }

    /// Every case, in catalogue order.
    #[must_use]
    pub fn cases(&self) -> &[ConformanceCase] {
        &self.cases
    }

    /// Only the cases that failed.
    pub fn failures(&self) -> impl Iterator<Item = &ConformanceCase> {
        self.cases
            .iter()
            .filter(|case| case.result == CaseResult::Fail)
    }

    /// The identifiers of the failing cases, deduplicated, in catalogue order.
    ///
    /// The shape a mutation check reads: the design names *one* case per planted defect, and "some
    /// case failed" is a claim a suite that failed everything would also satisfy.
    #[must_use]
    pub fn failing_ids(&self) -> Vec<&'static str> {
        let mut ids: Vec<&'static str> = Vec::new();
        for case in self.failures() {
            if !ids.contains(&case.id()) {
                ids.push(case.id());
            }
        }
        ids
    }

    /// How many cases came out each way, as `(pass, fail, unsupported)`.
    #[must_use]
    pub fn tally(&self) -> (usize, usize, usize) {
        let count = |wanted: CaseResult| self.cases.iter().filter(|c| c.result == wanted).count();
        (
            count(CaseResult::Pass),
            count(CaseResult::Fail),
            count(CaseResult::Unsupported),
        )
    }

    /// No case failed.
    #[must_use]
    pub fn is_conformant(&self) -> bool {
        self.failures().next().is_none()
    }
}

impl core::fmt::Display for ConformanceReport {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let (passed, failed, unsupported) = self.tally();
        writeln!(
            formatter,
            "{} graded against {}",
            self.adapter, self.contract
        )?;
        writeln!(formatter, "declared: {}", self.declared)?;
        writeln!(
            formatter,
            "{} cases: {passed} pass, {failed} fail, {unsupported} unsupported — {}",
            self.cases.len(),
            if self.is_conformant() {
                "CONFORMANT"
            } else {
                "NOT CONFORMANT"
            }
        )?;
        for case in &self.cases {
            writeln!(formatter, "{case}")?;
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------------------------
// The fixtures — constants, so two runs of one backend are two identical runs
// ---------------------------------------------------------------------------------------------

/// The workspace and head used when [`WorkspaceAdapter::prepare_fixture`] would not name a pair.
///
/// Not a fallback that hides the refusal: nothing a real backend holds is named by these, so the
/// mount and the presentation fail, and the cases that needed them say the backend did not prepare.
/// They exist so that the *undeclared* probe still has arguments to call with — an undeclared
/// capability owes `Unsupported` whatever it is handed, and that grading must not depend on a
/// workspace nobody prepared.
const UNPREPARED_WORKSPACE: WorkspaceId = WorkspaceId::from_bytes([0x5a; 16]);
const UNPREPARED_HEAD: HeadId = HeadId::from_bytes([0x0e; 32]);

const FIXTURE_ACTOR_ONE: ActorId = ActorId::from_bytes([0x01; 32]);
const FIXTURE_ACTOR_TWO: ActorId = ActorId::from_bytes([0x02; 32]);
const FIXTURE_ACTOR_RELEASED: ActorId = ActorId::from_bytes([0x03; 32]);
const FIXTURE_ACTOR_RELATIVE: ActorId = ActorId::from_bytes([0x04; 32]);
const FIXTURE_OBJECT: ObjectId = ObjectId::from_bytes([0x11; 16]);
// These are portable presentation requests, not host-global locations. A real backend may own a
// root it is allowed to write and resolve each request below that root; asking it to create
// `/mesh` would grade host privilege rather than adapter semantics. Keep the refused fixtures
// relative too, so the NAME cases differ only by the forbidden component they plant.
const MOUNTPOINT_ONE: &str = "mesh-conformance/one";
const MOUNTPOINT_TWO: &str = "mesh-conformance/two";
const MOUNTPOINT_RELEASED: &str = "mesh-conformance/three";
const MOUNTPOINT_RELATIVE: &str = "mesh-conformance/one/../two";
const READONLY_TARGET: &str = "mesh-conformance/shared";
const READONLY_RELATIVE_TARGET: &str = "mesh-conformance/shared/../elsewhere";

const NO_VIEW: &str = "no read-write view could be obtained, so this case could not be run";
const EXECUTABLE: PortableMetadata = PortableMetadata::new(true);

/// A directory entry name from a literal this file controls.
///
/// Every caller passes a literal the portable-name rules accept, so the refusal arm is
/// unreachable; it is spelled out rather than silently unwrapped so a future edit that breaks the
/// invariant says which literal broke it.
fn entry_name(text: &str) -> NormalizedName {
    match NormalizedName::new(text) {
        Ok(name) => name,
        Err(error) => unreachable!("the suite's own fixture name {text:?} is refused: {error}"),
    }
}

fn synthetic_handle(view: ViewId, object: ObjectId) -> OpenHandle {
    OpenHandle::new(0, view, object, OpenMode::ReadWrite)
}

/// One editor save, as a filesystem shows it: open, write, flush, close.
///
/// Four events with positions 1 to 4, minted by this suite rather than by the backend, so the
/// stream a backend is replayed against is the same stream both times.
fn boundary_stream(view: ViewId) -> [FsEvent; 4] {
    let event = |position: u64, kind: FsEventKind| {
        FsEvent::new(view, EventSequence::new(position), kind, FIXTURE_OBJECT)
    };
    [
        event(1, FsEventKind::Opened),
        event(2, FsEventKind::Written),
        event(3, FsEventKind::Flushed),
        event(4, FsEventKind::Closed),
    ]
}

/// A `Result` in the shape the case bodies speak: `Ok` is a pass, `Err` is the difference.
type Graded = Result<(), String>;

fn expect_error<T: core::fmt::Debug>(
    what: &str,
    answer: Result<T, AdapterError>,
    wanted: &AdapterError,
) -> Graded {
    match answer {
        Err(ref actual) if actual == wanted => Ok(()),
        Err(actual) => Err(format!(
            "{what} answered {} — {actual}; the rule requires {wanted}",
            actual.name()
        )),
        Ok(value) => Err(format!(
            "{what} answered Ok({value:?}); the rule requires {wanted}"
        )),
    }
}

fn expect_ok<T>(what: &str, answer: Result<T, AdapterError>) -> Result<T, String> {
    answer.map_err(|error| format!("{what} answered {} — {error}", error.name()))
}

// ---------------------------------------------------------------------------------------------
// The probe
// ---------------------------------------------------------------------------------------------

/// What one call to an operation answered.
enum Probe {
    /// The backend did the thing.
    Answered,
    /// The backend refused with `Unsupported`, naming this capability.
    RefusedThisCapability,
    /// The backend refused with `Unsupported`, naming a different capability.
    RefusedAnotherCapability(String),
    /// The backend refused with something that is not `Unsupported`.
    RefusedOtherwise(String),
    /// The backend unwound.
    Panicked,
    /// There is no published operation to ask with, or no view to ask through.
    NotProbeable(&'static str),
}

fn guarded<T>(
    capability: AdapterCapability,
    body: impl FnOnce() -> Result<T, AdapterError>,
) -> Probe {
    match catch_unwind(AssertUnwindSafe(body)) {
        Err(_) => Probe::Panicked,
        Ok(Ok(_)) => Probe::Answered,
        Ok(Err(AdapterError::Unsupported { capability: named })) if named == capability => {
            Probe::RefusedThisCapability
        }
        Ok(Err(AdapterError::Unsupported { capability: named })) => {
            Probe::RefusedAnotherCapability(format!("{named}"))
        }
        Ok(Err(other)) => Probe::RefusedOtherwise(format!("{} — {other}", other.name())),
    }
}

// ---------------------------------------------------------------------------------------------
// The run
// ---------------------------------------------------------------------------------------------

/// Grade one backend against every rule in the catalogue.
///
/// The entry point, and the whole of the integration a backend needs:
///
/// ```
/// # use mesh_materializer::{run_conformance, WorkspaceAdapter};
/// # fn check(backend: &dyn WorkspaceAdapter) {
/// let report = run_conformance(backend);
/// assert!(report.is_conformant(), "{report}");
/// # }
/// ```
///
/// It takes `&dyn WorkspaceAdapter` and therefore cannot name a concrete backend, which is the
/// mechanical reason adding a fourth backend changes no file here.
#[must_use]
pub fn run_conformance(adapter: &dyn WorkspaceAdapter) -> ConformanceReport {
    run_conformance_for(adapter, WORKSPACE_ADAPTER_CONTRACT)
}

/// Grade a migrated backend against the contract-0 behavioral subset under the exact contract-1
/// declaration. Contract-1-only evidence and replacement cases live in the adapter-v1 suite.
#[must_use]
pub fn run_conformance_v1(adapter: &dyn WorkspaceAdapter) -> ConformanceReport {
    run_conformance_for(adapter, WORKSPACE_ADAPTER_CONTRACT_V1)
}

fn run_conformance_for(
    adapter: &dyn WorkspaceAdapter,
    expected_contract: &'static str,
) -> ConformanceReport {
    let description = adapter.describe();
    let (fixture, unprepared) = prepare(adapter);
    let mut run = Run {
        adapter,
        declared: description.capabilities(),
        refused: Vec::new(),
        cases: Vec::new(),
        fixture,
        unprepared,
        actor_view: None,
        second_view: None,
        released_view: None,
        readonly_view: None,
    };

    run.grade(
        &CONTRACT_STRING,
        None,
        if description.contract() == expected_contract {
            Ok(())
        } else {
            Err(format!(
                "describe() claims {:?}; this suite grades {expected_contract:?}",
                description.contract()
            ))
        },
    );
    let again = adapter.describe();
    run.grade(
        &DESCRIPTION_IS_STABLE,
        None,
        if again == description {
            Ok(())
        } else {
            Err(format!(
                "the first describe() said [{description}], the second said [{again}]"
            ))
        },
    );
    run.grade(
        &REFUSED_NAMES_ARE_UNREPRESENTABLE,
        None,
        refused_names_are_unrepresentable(),
    );
    run.grade(&CITATION_IS_PUBLISHED, None, citations_are_published());
    run.grade(
        &IDENTIFIER_NAMES_ITS_FAMILY,
        None,
        identifiers_name_their_family(),
    );

    run.acquire_views();

    // Every capability is asked its one question before any case is run. `Run::refused` records
    // what came back, so a case never leans on a capability already known to refuse itself.
    let probes: Vec<Probe> = AdapterCapability::ALL
        .into_iter()
        .map(|capability| run.probe(capability))
        .collect();
    for (capability, probe) in AdapterCapability::ALL.into_iter().zip(probes.iter()) {
        if run.declares(capability) && matches!(probe, Probe::RefusedThisCapability) {
            run.refused.push(capability);
        }
    }
    for (capability, probe) in AdapterCapability::ALL.into_iter().zip(probes) {
        run.grade_capability(capability, probe);
    }

    ConformanceReport {
        adapter: description.adapter(),
        contract: description.contract(),
        declared: description.capabilities(),
        cases: run.cases,
    }
}

/// Ask the backend for the workspace and head everything below is graded against.
///
/// Once, before anything else, and under `catch_unwind` for the same reason every case body is: a
/// backend that unwinds here is graded rather than fatal. The second half of the pair is empty when
/// the backend prepared, and is the sentence a case prints when it did not.
fn prepare(adapter: &dyn WorkspaceAdapter) -> (AdapterFixture, String) {
    let unprepared = AdapterFixture::new(UNPREPARED_WORKSPACE, UNPREPARED_HEAD);
    match catch_unwind(AssertUnwindSafe(|| adapter.prepare_fixture())) {
        Ok(Ok(fixture)) => (fixture, String::new()),
        Ok(Err(refusal)) => (
            unprepared,
            format!(
                "prepare_fixture() answered {} — {refusal}, so nothing this backend actually holds was mounted or presented",
                refusal.name()
            ),
        ),
        Err(_) => (
            unprepared,
            "prepare_fixture() unwound instead of naming a workspace and a head".to_owned(),
        ),
    }
}

fn refused_names_are_unrepresentable() -> Graded {
    for (text, expected) in [
        ("", NameError::Empty),
        (".", NameError::Relative),
        ("..", NameError::Relative),
        ("a/b", NameError::Separator),
        ("a\\b", NameError::Separator),
        ("a\0b", NameError::Nul),
    ] {
        match NormalizedName::new(text) {
            Err(actual) if actual == expected => {}
            other => {
                return Err(format!(
                    "{text:?} produced {other:?}, so a refused name is representable after all and the NAME family is gradable at the view surface"
                ))
            }
        }
    }
    Ok(())
}

fn citations_are_published() -> Graded {
    for rule in conformance_catalogue() {
        let cited = rule.citation();
        if !(cited.starts_with("tests/compatibility/adapter/v0/")
            || cited.starts_with("docs/plan/execution-plan.md"))
        {
            return Err(format!(
                "{} cites {cited}, which a backend author cannot read without opening this repository's internals",
                rule.id()
            ));
        }
    }
    Ok(())
}

fn identifiers_name_their_family() -> Graded {
    for rule in conformance_catalogue() {
        let family = rule.family().as_str();
        if !(rule.id().starts_with(&format!("{family}/"))
            || rule.id().starts_with(&format!("{family}-")))
        {
            return Err(format!(
                "{} is in family {family} and does not begin with it",
                rule.id()
            ));
        }
    }
    Ok(())
}

/// The case bodies, in their own file: this module is the catalogue, the grading table and the
/// capability probe, and that is already as much as one file should carry.
mod cases;

struct Run<'a> {
    adapter: &'a dyn WorkspaceAdapter,
    declared: CapabilitySet,
    /// Capabilities the backend declared and then answered `Unsupported` for.
    ///
    /// Every capability is probed before any case runs, so a capability that refuses itself is
    /// known to be unusable *before* a case that only needed it to observe something else tries to
    /// use it. Without that, one declared-then-refused capability would fail every case that
    /// happened to lean on it, and "the case that names the rule caught it" would stop being a
    /// statement anybody could check.
    refused: Vec<AdapterCapability>,
    cases: Vec<ConformanceCase>,
    /// What [`WorkspaceAdapter::prepare_fixture`] named, or [`UNPREPARED_WORKSPACE`] and
    /// [`UNPREPARED_HEAD`] when it would not.
    fixture: AdapterFixture,
    /// Why the backend did not prepare, when it did not. Empty when it did.
    unprepared: String,
    actor_view: Option<ViewId>,
    second_view: Option<ViewId>,
    released_view: Option<ViewId>,
    readonly_view: Option<ViewId>,
}

impl<'a> Run<'a> {
    fn emit(
        &mut self,
        rule: &'static ConformanceRule,
        subject: Option<AdapterCapability>,
        result: CaseResult,
        detail: String,
    ) {
        self.cases.push(ConformanceCase {
            rule,
            subject,
            result,
            detail,
        });
    }

    fn grade(
        &mut self,
        rule: &'static ConformanceRule,
        subject: Option<AdapterCapability>,
        outcome: Graded,
    ) {
        match outcome {
            Ok(()) => self.emit(rule, subject, CaseResult::Pass, String::new()),
            Err(detail) => self.emit(rule, subject, CaseResult::Fail, detail),
        }
    }

    /// Run one case body under `catch_unwind`, so a backend that unwinds is graded, not fatal.
    fn checked(
        &mut self,
        rule: &'static ConformanceRule,
        subject: Option<AdapterCapability>,
        body: impl FnOnce() -> Graded,
    ) {
        match catch_unwind(AssertUnwindSafe(body)) {
            Ok(outcome) => self.grade(rule, subject, outcome),
            Err(_) => self.emit(
                rule,
                subject,
                CaseResult::Fail,
                "the backend unwound instead of answering".to_owned(),
            ),
        }
    }

    fn skip(
        &mut self,
        rules: &'static [ConformanceRule],
        subject: Option<AdapterCapability>,
        why: &str,
    ) {
        for rule in rules {
            self.emit(rule, subject, CaseResult::Unsupported, why.to_owned());
        }
    }

    /// What the backend claimed.
    fn declares(&self, capability: AdapterCapability) -> bool {
        self.declared.contains(capability)
    }

    /// What the backend claimed *and* did not then refuse. Cases lean on this, never on the claim.
    fn can(&self, capability: AdapterCapability) -> bool {
        self.declares(capability) && !self.refused.contains(&capability)
    }

    fn resolve(&self, view: Option<ViewId>) -> Option<&'a dyn WorkspaceView> {
        let adapter: &'a dyn WorkspaceAdapter = self.adapter;
        view.and_then(|id| adapter.view(id).ok())
    }

    /// The sentence a case adds when the view it needed does not exist.
    ///
    /// Empty when the backend prepared, so a failure caused by something else is not decorated with
    /// a preparation that went fine.
    fn preparation_note(&self) -> String {
        if self.unprepared.is_empty() {
            String::new()
        } else {
            format!(" {}", self.unprepared)
        }
    }

    /// Mount the working views and present the read-only one, silently: the grading of those two
    /// capabilities happens later, in catalogue order, from what is recorded here.
    fn acquire_views(&mut self) {
        let adapter = self.adapter;
        let (workspace, head) = (self.fixture.workspace(), self.fixture.head());
        if self.declares(AdapterCapability::MountActorView) {
            let mount = |actor, at: &str| {
                catch_unwind(AssertUnwindSafe(|| {
                    adapter.mount_actor_view(workspace, actor, Path::new(at))
                }))
                .ok()
                .and_then(Result::ok)
                .map(|view| view.id())
            };
            self.actor_view = mount(FIXTURE_ACTOR_ONE, MOUNTPOINT_ONE);
            self.second_view = mount(FIXTURE_ACTOR_TWO, MOUNTPOINT_TWO);
            self.released_view = mount(FIXTURE_ACTOR_RELEASED, MOUNTPOINT_RELEASED);
        }
        if self.declares(AdapterCapability::MaterializeReadonlyView) {
            self.readonly_view = catch_unwind(AssertUnwindSafe(|| {
                adapter.materialize_readonly_view(head, Path::new(READONLY_TARGET))
            }))
            .ok()
            .and_then(Result::ok)
            .map(|view| view.id());
        }
    }

    /// Ask one capability exactly one question.
    fn probe(&self, capability: AdapterCapability) -> Probe {
        let adapter = self.adapter;
        let probe_name = entry_name("zzz-probe");
        let other_name = entry_name("zzz-probe-other");
        let absent = entry_name("zzz-probe-absent");

        let view = match capability {
            AdapterCapability::MountActorView
            | AdapterCapability::MaterializeReadonlyView
            | AdapterCapability::ObserveDurableBoundary
            | AdapterCapability::Symlink => None,
            _ => match self.resolve(self.actor_view) {
                Some(view) => Some(view),
                None => return Probe::NotProbeable(NO_VIEW),
            },
        };

        let (workspace, head) = (self.fixture.workspace(), self.fixture.head());
        match capability {
            AdapterCapability::MountActorView => guarded(capability, || {
                adapter.mount_actor_view(workspace, FIXTURE_ACTOR_ONE, Path::new(MOUNTPOINT_ONE))
            }),
            AdapterCapability::MaterializeReadonlyView => guarded(capability, || {
                adapter.materialize_readonly_view(head, Path::new(READONLY_TARGET))
            }),
            AdapterCapability::ObserveDurableBoundary => {
                let event = FsEvent::new(
                    self.actor_view.unwrap_or(ViewId::new(0)),
                    EventSequence::new(1),
                    FsEventKind::Opened,
                    FIXTURE_OBJECT,
                );
                guarded(capability, || adapter.observe_durable_boundary(&event))
            }
            AdapterCapability::Symlink => Probe::NotProbeable(
                "mesh-workspace-adapter/0 publishes no symlink operation, so neither declaring nor omitting Symlink can be graded here",
            ),
            other => {
                let view = match view {
                    Some(view) => view,
                    None => return Probe::NotProbeable(NO_VIEW),
                };
                match other {
                    AdapterCapability::Lookup => {
                        guarded(capability, || view.lookup(view.root(), &probe_name))
                    }
                    AdapterCapability::Enumerate => {
                        guarded(capability, || view.enumerate(view.root()))
                    }
                    AdapterCapability::Open => {
                        guarded(capability, || view.open(view.root(), OpenMode::Read))
                    }
                    AdapterCapability::Read => {
                        let handle = synthetic_handle(view.id(), view.root());
                        guarded(capability, || {
                            let mut buffer = [0u8; 4];
                            view.read(&handle, 0, &mut buffer)
                        })
                    }
                    AdapterCapability::Write => {
                        let handle = synthetic_handle(view.id(), view.root());
                        guarded(capability, || view.write(&handle, 0, b"probe"))
                    }
                    AdapterCapability::SetFileLength => {
                        guarded(capability, || view.set_file_length(view.root(), 0))
                    }
                    AdapterCapability::CreateFile => guarded(capability, || {
                        view.create_file(view.root(), &probe_name, PortableMetadata::default())
                    }),
                    AdapterCapability::CreateDirectory => {
                        guarded(capability, || view.create_directory(view.root(), &other_name))
                    }
                    AdapterCapability::Rename => guarded(capability, || {
                        view.rename(view.root(), &absent, &other_name)
                    }),
                    AdapterCapability::Move => guarded(capability, || {
                        view.move_entry(view.root(), &absent, view.root(), &other_name)
                    }),
                    AdapterCapability::Unlink => {
                        guarded(capability, || view.unlink(view.root(), &absent))
                    }
                    AdapterCapability::RemoveDirectory => {
                        guarded(capability, || view.remove_directory(view.root(), &absent))
                    }
                    AdapterCapability::ReadMetadata => {
                        guarded(capability, || view.metadata(view.root()))
                    }
                    _ => guarded(capability, || {
                        view.set_metadata(view.root(), PortableMetadata::default())
                    }),
                }
            }
        }
    }

    /// The whole of the capability contract, in one table.
    fn grade_capability(&mut self, capability: AdapterCapability, probe: Probe) {
        let declared = self.declares(capability);
        let rules = rules_for(capability);
        let subject = Some(capability);
        let declared_probe_skipped = "the capability is not declared";
        let undeclared_probe_skipped =
            "the capability is declared, so the undeclared probe does not apply";

        // Five outcomes, in `PROBE_RULES` order, and one decision about the capability's own cases.
        let (outcomes, run_cases, why): ([Option<Graded>; 5], bool, String) = match (declared, probe)
        {
            (_, Probe::NotProbeable(why)) => ([None, None, None, None, None], false, why.to_owned()),
            (true, Probe::RefusedThisCapability) => (
                [
                    Some(Err(format!(
                        "{capability} is declared and answered Unsupported"
                    ))),
                    Some(Ok(())),
                    None,
                    None,
                    None,
                ],
                false,
                "the capability is declared and answered Unsupported".to_owned(),
            ),
            (true, Probe::Panicked) => (
                [
                    Some(Ok(())),
                    Some(Err(format!(
                        "{capability} is declared and unwound instead of answering"
                    ))),
                    None,
                    None,
                    None,
                ],
                false,
                "the capability is declared and unwound".to_owned(),
            ),
            (true, _) => (
                [Some(Ok(())), Some(Ok(())), None, None, None],
                true,
                undeclared_probe_skipped.to_owned(),
            ),
            (false, Probe::RefusedThisCapability) => (
                [None, None, Some(Ok(())), Some(Ok(())), Some(Ok(()))],
                false,
                "not declared, and refused cleanly with Unsupported".to_owned(),
            ),
            (false, Probe::Answered) => (
                [
                    None,
                    None,
                    Some(Err(format!(
                        "{capability} was not declared and answered Ok. A backend that answers Ok and drops the work is indistinguishable from one that did it, which on a filesystem is data loss"
                    ))),
                    Some(Ok(())),
                    None,
                ],
                false,
                "not declared".to_owned(),
            ),
            (false, Probe::Panicked) => (
                [
                    None,
                    None,
                    Some(Ok(())),
                    Some(Err(format!(
                        "{capability} was not declared and unwound. A hole and a stub are separate defects: this is the hole"
                    ))),
                    None,
                ],
                false,
                "not declared".to_owned(),
            ),
            (false, Probe::RefusedAnotherCapability(named)) => (
                [
                    None,
                    None,
                    Some(Ok(())),
                    Some(Ok(())),
                    Some(Err(format!(
                        "{capability} was asked for and the refusal named {named}. A caller cannot act on a refusal that names the wrong thing"
                    ))),
                ],
                false,
                "not declared".to_owned(),
            ),
            (false, Probe::RefusedOtherwise(what)) => (
                [
                    None,
                    None,
                    Some(Ok(())),
                    Some(Ok(())),
                    Some(Err(format!(
                        "{capability} was not declared and refused with {what}. The only answer an undeclared capability may give is Unsupported"
                    ))),
                ],
                false,
                "not declared".to_owned(),
            ),
        };

        for (index, outcome) in outcomes.into_iter().enumerate() {
            let rule = &PROBE_RULES[index];
            match outcome {
                Some(graded) => self.grade(rule, subject, graded),
                None => {
                    let skipped = if declared && index >= 2 {
                        undeclared_probe_skipped
                    } else if !declared && index < 2 {
                        declared_probe_skipped
                    } else {
                        why.as_str()
                    };
                    self.emit(rule, subject, CaseResult::Unsupported, skipped.to_owned());
                }
            }
        }

        if run_cases {
            self.run_cases(capability);
        } else {
            self.skip(rules, subject, &why);
        }
    }
}
