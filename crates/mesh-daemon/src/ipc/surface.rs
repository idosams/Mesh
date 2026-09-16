//! The versioned method catalogue, the version negotiation, and the dispatch seam.
//!
//! # The catalogue is the whole of the surface
//!
//! Plan §8.3's last bullet — the user interface may not write the database — only means anything
//! if there is a *narrow* place the interface has to go through. That place is [`METHODS`]. A
//! method that is not in this table cannot be called, and the desktop client's own operation
//! catalogue is checked against this one from both sides: `crates/mesh-daemon/tests/ipc.rs`
//! compares this table with `crates/mesh-daemon/ipc-contract.json`, and
//! `apps/desktop/src/ipc/contract.test.ts` compares the same file with the client's table.
//!
//! # The catalogue is deliberately short, and honest about why
//!
//! `mesh-daemon` composes one crate, `mesh-store`, and every method below is answered from
//! something that crate can actually compute over records that are actually on disk. Adding one
//! that returns a plausible-looking shape the daemon cannot compute would make the surface look
//! finished and make every later task's test fixture a lie. `## Failure and recovery` in the task
//! contract says it the other way round — extend the surface when the interface needs data, never
//! reach past it — and that is the direction this table grows in.
//!
//! Where a subject has no answer at all, it is named as one: `workspace.state` carries a `not_yet`
//! list from [`crate::workspace::OpenWorkspace::not_yet`], so a user interface can grey an
//! affordance out with a reason instead of rendering an empty list that reads like an answer.
//!
//! # Versions 2 and 3
//!
//! Version 1 was three methods over one recovery diagnostic. Version 2 added `workspace.open`,
//! `workspace.state` and `events.subscribe`, and the pushed
//! [`crate::ipc::message::DaemonMessage::Event`] the last of those turns on. The addition is
//! strictly additive: `since` keeps a version 1 client from calling any of the three, and a
//! connection that never subscribes never receives an unsolicited line.
//! Version 3 adds the bounded local-folder management transaction and read-only restore preview.
//! Version 4 adds a read-only snapshot of the daemon's performance counters. Neither version
//! exposes arbitrary record writes or restore execution.
//!
//! # Dispatch is a trait, not a match on a concrete daemon
//!
//! [`Operations`] is the seam: the transport in [`crate::ipc::server`] knows nothing about what
//! answers a call, and a test, a simulator or a later real daemon all plug into the same hole.

use core::fmt;

use crate::ipc::events::EventBacklog;
use crate::ipc::json::Json;
use crate::ipc::message::{ClientMessage, DaemonMessage, SUPPORTED_VERSIONS, SURFACE_VERSION};
use crate::recovery::{RecoveryDiagnostic, RecoveryOutcome, Severity};
use crate::user_messages;
use crate::version_state::PrivateVersion;
use crate::workspace::{
    NativeUnsupportedEntry, RestoreVersionIdentity, WorkspaceCondition, WorkspaceEntry,
    WorkspaceFileHistory, WorkspaceVersion,
};

/// One entry in the versioned method catalogue.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Method {
    /// The name a client calls.
    pub name: &'static str,
    /// The first surface version that has this method.
    pub since: u32,
    /// What it answers, for `surface.describe` and for a person reading this file.
    pub summary: &'static str,
}

/// Every method on this surface, in the order `surface.describe` reports them.
pub const METHODS: &[Method] = &[
    Method {
        name: "daemon.status",
        since: 1,
        summary: "whether the background service is serving requests, and which surface it speaks",
    },
    Method {
        name: "startup.report",
        since: 1,
        summary: "what the background service found when it last started",
    },
    Method {
        name: "surface.describe",
        since: 1,
        summary: "this catalogue, so a client can tell whether it is talking to a newer service",
    },
    Method {
        name: "workspace.open",
        since: 2,
        summary: "open the workspace in a folder and read back what is saved there",
    },
    Method {
        name: "workspace.state",
        since: 2,
        summary: "what the open workspace holds, and what this build cannot answer yet",
    },
    Method {
        name: "review.open",
        since: 2,
        summary: "open or reuse an exact review bundle over one saved change",
    },
    Method {
        name: "review.open-current",
        since: 6,
        summary: "compute and open the exact first-publication review for the current private head",
    },
    Method {
        name: "review.approve",
        since: 2,
        summary: "request shared publication; unavailable until HumanHeld authority is durable",
    },
    Method {
        name: "events.subscribe",
        since: 2,
        summary: "hear about what the background service is doing, as it happens",
    },
    Method {
        name: "folder.import.preview",
        since: 3,
        summary: "hash one selected local folder without changing it or creating a managed copy",
    },
    Method {
        name: "folder.import.confirm",
        since: 3,
        summary:
            "verify the accepted summary, create durable private history, and open the managed copy",
    },
    Method {
        name: "folder.import.rollback",
        since: 3,
        summary: "remove an unchanged receipt-owned managed copy while preserving the original",
    },
    Method {
        name: "workspace.restore.preview",
        since: 3,
        summary: "preview an exact append-only earlier-version restore without authorizing it",
    },
    Method {
        name: "workspace.version.fork",
        since: 5,
        summary: "open one durable version of the exact displayed workspace as a new independent native working folder",
    },
    Method {
        name: "performance.counters",
        since: 4,
        summary: "read every live performance counter and its collection conditions",
    },
];

/// The catalogue entry for `name`, when there is one.
#[must_use]
pub fn method(name: &str) -> Option<&'static Method> {
    METHODS.iter().find(|entry| entry.name == name)
}

/// The newest version both ends can speak, or `None` when there is no overlap.
///
/// The daemon chooses, and it chooses the highest common version rather than the client's
/// preferred one: a client that lists a version it can speak has committed to speaking it, and
/// letting the client pick would make the daemon's own deprecation schedule unenforceable.
#[must_use]
pub fn negotiate(client_versions: &[u32]) -> Option<u32> {
    SUPPORTED_VERSIONS
        .iter()
        .copied()
        .filter(|ours| client_versions.contains(ours))
        .max()
}

/// What the daemon can answer. The seam between the transport and whatever is behind it.
///
/// The workspace and feed methods carry default implementations that **refuse**, and that is the
/// design rather than a convenience: an implementation with no workspace behind it — a test, a
/// simulator, the version 1 [`RecoveredDaemon`] — says so in a typed refusal a client can branch
/// on. It does not answer zero, and it does not answer an empty list, because both of those read
/// like a workspace that happens to be empty.
pub trait Operations: Send + Sync {
    /// Whether the daemon is serving requests at all.
    fn serving(&self) -> bool;

    /// What the last start-up found.
    fn startup(&self) -> StartupSummary;

    /// Open the workspace rooted at `path` and describe what is saved there.
    ///
    /// # Errors
    ///
    /// [`Unavailable`] when the folder cannot be reached, when what is saved there is damaged, or
    /// when this implementation has no workspace to open at all.
    fn open_workspace(&self, path: &str) -> Result<WorkspaceSummary, Unavailable> {
        let _ = path;
        Err(Unavailable::no_workspace_support())
    }

    /// Describe the workspace that is already open.
    ///
    /// # Errors
    ///
    /// [`Unavailable`] when nothing is open yet, or when this implementation has no workspace.
    fn workspace_state(&self) -> Result<WorkspaceSummary, Unavailable> {
        Err(Unavailable::no_workspace_support())
    }

    /// Open or reuse one exact review bundle.
    fn open_review(
        &self,
        bundle: &str,
        target: &str,
        opened_by: &str,
    ) -> Result<WorkspaceSummary, Unavailable> {
        let _ = (bundle, target, opened_by);
        Err(Unavailable::no_workspace_support())
    }

    /// Compute and open the exact current first-publication review.
    fn open_current_review(&self, opened_by: &str) -> Result<WorkspaceSummary, Unavailable> {
        let _ = opened_by;
        Err(Unavailable::no_workspace_support())
    }

    /// Verify and durably consume canonical locally signed approval receipt bytes.
    fn approve_review(
        &self,
        bundle: &str,
        target: &str,
        receipt: &str,
    ) -> Result<WorkspaceSummary, Unavailable> {
        let _ = (bundle, target, receipt);
        Err(Unavailable::no_workspace_support())
    }

    /// Hash one local folder for an exact confirmation step.
    fn preview_folder_import(&self, source: &str) -> Result<Json, Unavailable> {
        let _ = source;
        Err(Unavailable::no_workspace_support())
    }

    /// Confirm one preview into an external private store and open its ordinary working folder.
    fn confirm_folder_import(
        &self,
        source: &str,
        destination: &str,
        expected_summary: &str,
    ) -> Result<Json, Unavailable> {
        let _ = (source, destination, expected_summary);
        Err(Unavailable::no_workspace_support())
    }

    /// Confirm an import while keeping the destination outside exact directory identities held
    /// by the native client. Older and non-desktop implementations retain the original method;
    /// they may accept only an empty protection set.
    fn confirm_folder_import_protected(
        &self,
        source: &str,
        destination: &str,
        expected_summary: &str,
        protected_roots: &[String],
    ) -> Result<Json, Unavailable> {
        if protected_roots.is_empty() {
            self.confirm_folder_import(source, destination, expected_summary)
        } else {
            Err(Unavailable::new(
                "import-protected-roots-unsupported",
                "This service cannot verify the protected workspace locations for that import.",
            ))
        }
    }

    /// Roll back one unchanged receipt-owned managed copy.
    fn rollback_folder_import(&self, destination: &str) -> Result<Json, Unavailable> {
        let _ = destination;
        Err(Unavailable::no_workspace_support())
    }

    /// Preview an earlier file version without authorizing a write.
    fn preview_file_restore(&self, object: &str, target: &str) -> Result<Json, Unavailable> {
        let _ = (object, target);
        Err(Unavailable::no_workspace_support())
    }

    /// Open a durable whole-workspace operation point as a new independent native workspace.
    fn fork_workspace_version(
        &self,
        operation: &str,
        destination: &str,
        expected_root: &str,
        expected_digest: &str,
        expected_installation: &str,
    ) -> Result<Json, Unavailable> {
        let _ = (
            operation,
            destination,
            expected_root,
            expected_digest,
            expected_installation,
        );
        Err(Unavailable::no_workspace_support())
    }

    /// Read every live performance counter without starting a benchmark or workload.
    fn performance_counters(&self) -> Result<Json, Unavailable> {
        Err(Unavailable::new(
            "counters-not-served",
            user_messages::COUNTERS_NOT_SERVED,
        ))
    }

    /// The sequence of the newest feed entry, or `0` when there is none.
    fn event_cursor(&self) -> u64 {
        0
    }

    /// Every feed entry after `cursor` that is still held.
    fn events_since(&self, cursor: u64) -> EventBacklog {
        let _ = cursor;
        EventBacklog {
            entries: Vec::new(),
            dropped: 0,
        }
    }
}

/// A typed refusal: this build cannot answer, and here is the machine code and the sentence.
///
/// Deliberately not an `Option` and deliberately not an empty answer. "Nothing is open" and "there
/// is nothing in it" are different facts about a person's work, and a surface that renders both as
/// an empty list has told the person the second when the truth was the first.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Unavailable {
    /// A stable machine code.
    pub code: String,
    /// One sentence a person can read, from [`crate::user_messages`].
    pub message: String,
}

impl Unavailable {
    /// Build one from a code and a sentence.
    #[must_use]
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
        }
    }

    /// No workspace has been opened on this connection's daemon yet.
    #[must_use]
    pub fn no_workspace_open() -> Self {
        Self::new("no-workspace-open", user_messages::NO_WORKSPACE_OPEN)
    }

    /// This implementation of the surface holds no workspace at all.
    #[must_use]
    pub fn no_workspace_support() -> Self {
        Self::new("workspace-not-served", user_messages::WORKSPACE_NOT_SERVED)
    }

    /// This refusal as a `failed` reply.
    #[must_use]
    pub fn as_failure(&self, id: u64) -> DaemonMessage {
        DaemonMessage::Failed {
            id,
            code: self.code.clone(),
            message: self.message.clone(),
        }
    }
}

impl fmt::Display for Unavailable {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{} ({})", self.message, self.code)
    }
}

/// What one open workspace holds, in the shape `workspace.open` and `workspace.state` answer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorkspaceSummary {
    /// The folder it lives in.
    pub root: String,
    /// Opaque identity of the physical directory object admitted by this open workspace.
    pub installation: String,
    /// How many whole saved records are on disk.
    pub records: u64,
    /// How many bytes of an unfinished save were set aside. Zero on a clean shutdown.
    pub unfinished_bytes: u64,
    /// How many operations the fold indexed.
    pub operations: u64,
    /// How many distinct people or agents authored them.
    pub actors: u64,
    /// How many file manifests are indexed.
    pub manifests: u64,
    /// How many peers this workspace replicates with.
    pub peers: u64,
    /// How many immutable review records exist over it.
    pub reviews: u64,
    /// A bounded, read-only projection of the current automatic review candidate followed by
    /// durable review identities and their subject operations. `recorded: false` distinguishes
    /// the candidate from an immutable review record. Neither form can authorize publication.
    pub review_items: Vec<Json>,
    /// How many durable review records were omitted from `review_items` by the response bound.
    pub review_items_not_listed: u64,
    /// The digest of the index this process folded from those records.
    pub digest: String,
    /// This replica's own version state, derived from the records on disk.
    pub private_version: PrivateVersion,
    /// The protected shared version derived from a receipt plus durable HumanHeld authority, or
    /// `None` while `not_yet` names that unavailable authority boundary.
    pub shared_version: Option<String>,
    /// File and folder paths derived from saved ChangeSet payloads.
    pub entries: Vec<WorkspaceEntry>,
    /// Nonauthoritative regular-file discoveries absent from durable history. A native client must
    /// inspect and explicitly adopt one before it becomes a workspace entry.
    pub native_untracked_files: Vec<String>,
    /// Native links, special filesystem objects, and structurally unrepresentable re-inclusions
    /// that Mesh will not follow or version. Their explicit presence keeps an incomplete native
    /// scan from looking review-complete.
    pub native_unsupported_entries: Vec<NativeUnsupportedEntry>,
    /// Whether every native directory entry was readable and classifiable during this inventory.
    /// `false` makes the inventory fail closed without guessing which unseen path was involved.
    pub native_inventory_complete: bool,
    /// Retained immutable versions grouped by the currently materialized file that owns them.
    pub file_histories: Vec<WorkspaceFileHistory>,
    /// Whole-workspace operation points that can be opened as independent native folders.
    pub workspace_versions: Vec<WorkspaceVersion>,
    /// Recoverable conditions beside a partial names-and-folders result.
    pub conditions: Vec<WorkspaceCondition>,
    /// What this build cannot answer about the workspace, and why. Each entry is
    /// `(subject, reason)`.
    pub not_yet: Vec<(String, String)>,
    /// The redacted support document composed from this process's already-verified journal and
    /// checkpoint state. Present on `workspace.state`; absent from an initial `workspace.open`
    /// answer, whose caller can immediately request state after installation.
    pub support_bundle: Option<Json>,
}

impl WorkspaceSummary {
    /// This summary as a method answer.
    ///
    /// Key order: `root`, `installation`, `records`, `unfinished_bytes`, `operations`, `actors`,
    /// `manifests`, `peers`, `reviews`, `review_items`, `review_items_not_listed`, `digest`,
    /// `private_version`, `shared_version`, `entries`, `native_untracked_files`,
    /// `native_unsupported_entries`, `native_inventory_complete`,
    /// `file_histories`, `workspace_versions`,
    /// `conditions`, `not_yet`,
    /// and optionally `support_bundle`.
    ///
    /// `private_version` is **additive within surface version 2**: the method catalogue does not
    /// move, `SUPPORTED_VERSIONS` does not move, and a client that reads only the keys it already
    /// knew is unaffected. Removing or renaming a key is the direction that is not additive, and
    /// `crates/mesh-daemon/ipc-contract.json` states that rule where a client author reads it.
    #[must_use]
    pub fn to_json(&self) -> Json {
        let mut fields = vec![
            ("root", Json::text(self.root.clone())),
            ("installation", Json::text(self.installation.clone())),
            ("records", Json::Number(self.records)),
            ("unfinished_bytes", Json::Number(self.unfinished_bytes)),
            ("operations", Json::Number(self.operations)),
            ("actors", Json::Number(self.actors)),
            ("manifests", Json::Number(self.manifests)),
            ("peers", Json::Number(self.peers)),
            ("reviews", Json::Number(self.reviews)),
            ("review_items", Json::Array(self.review_items.clone())),
            (
                "review_items_not_listed",
                Json::Number(self.review_items_not_listed),
            ),
            ("digest", Json::text(self.digest.clone())),
            (
                "private_version",
                private_version_json(&self.private_version),
            ),
            (
                "shared_version",
                self.shared_version
                    .as_ref()
                    .map_or(Json::Null, |head| Json::text(head.clone())),
            ),
            (
                "entries",
                Json::Array(
                    self.entries
                        .iter()
                        .map(|entry| {
                            Json::object([
                                ("path", Json::text(entry.path())),
                                ("type", Json::text(entry.entry_type())),
                            ])
                        })
                        .collect(),
                ),
            ),
            (
                "native_untracked_files",
                Json::Array(
                    self.native_untracked_files
                        .iter()
                        .cloned()
                        .map(Json::text)
                        .collect(),
                ),
            ),
            (
                "native_unsupported_entries",
                Json::Array(
                    self.native_unsupported_entries
                        .iter()
                        .map(|entry| {
                            Json::object([
                                ("path", Json::text(entry.path())),
                                ("kind", Json::text(entry.kind())),
                            ])
                        })
                        .collect(),
                ),
            ),
            (
                "native_inventory_complete",
                Json::Bool(self.native_inventory_complete),
            ),
            (
                "file_histories",
                Json::Array(self.file_histories.iter().map(file_history_json).collect()),
            ),
            (
                "workspace_versions",
                Json::Array(
                    self.workspace_versions
                        .iter()
                        .copied()
                        .map(workspace_version_json)
                        .collect(),
                ),
            ),
            (
                "conditions",
                Json::Array(
                    self.conditions
                        .iter()
                        .map(|condition| {
                            Json::object([
                                ("code", Json::text(condition.code())),
                                ("recoverable", Json::Bool(condition.recoverable())),
                                ("message", Json::text(condition.message())),
                                (
                                    "related",
                                    Json::Array(
                                        condition
                                            .related()
                                            .iter()
                                            .cloned()
                                            .map(Json::text)
                                            .collect(),
                                    ),
                                ),
                            ])
                        })
                        .collect(),
                ),
            ),
            (
                "not_yet",
                Json::Array(
                    self.not_yet
                        .iter()
                        .map(|(subject, reason)| {
                            Json::object([
                                ("subject", Json::text(subject.clone())),
                                ("reason", Json::text(reason.clone())),
                            ])
                        })
                        .collect(),
                ),
            ),
        ];
        if let Some(bundle) = &self.support_bundle {
            fields.push(("support_bundle", bundle.clone()));
        }
        Json::object(fields)
    }
}

fn file_history_json(history: &WorkspaceFileHistory) -> Json {
    Json::object([
        ("path", Json::text(history.path())),
        ("object_id", Json::text(history.object().to_string())),
        (
            "current",
            history.current().map_or(Json::Null, version_identity_json),
        ),
        (
            "retained_versions",
            Json::Array(
                history
                    .retained()
                    .iter()
                    .copied()
                    .map(version_identity_json)
                    .collect(),
            ),
        ),
    ])
}

fn version_identity_json(identity: RestoreVersionIdentity) -> Json {
    Json::object([
        ("version_id", Json::text(identity.version().to_string())),
        ("manifest_id", Json::text(identity.manifest().to_string())),
    ])
}

fn workspace_version_json(version: WorkspaceVersion) -> Json {
    Json::object([
        ("operation", Json::text(version.operation().to_string())),
        ("ordinal", Json::Number(version.ordinal())),
        (
            "actor_sequence",
            Json::text(version.actor_sequence().to_string()),
        ),
    ])
}

/// This replica's version state as a method answer.
///
/// Key order: `version`, `state`, `changes_applied`, `concurrent_changes`, `waiting`,
/// `waiting_not_listed`, `apply_order_agrees`, `derivation`, `checked_against_author_claim`; and
/// within a `waiting` entry, `change`, `missing`.
///
/// `checked_against_author_claim` is published as a `false` rather than omitted. The alternative —
/// sending a version identifier with nothing beside it — invites a client to read a value this
/// service derived as a value some peer agreed to, and those are different claims.
fn private_version_json(version: &PrivateVersion) -> Json {
    Json::object([
        (
            "version",
            version
                .version()
                .map_or(Json::Null, |head| Json::text(head.to_owned())),
        ),
        ("state", Json::text(version.state().as_str().to_owned())),
        (
            "changes_applied",
            Json::Number(version.changes_applied() as u64),
        ),
        (
            "concurrent_changes",
            Json::Number(version.concurrent_changes() as u64),
        ),
        (
            "waiting",
            Json::Array(
                version
                    .waiting()
                    .iter()
                    .map(|entry| {
                        Json::object([
                            ("change", Json::text(entry.change.clone())),
                            (
                                "missing",
                                Json::Array(
                                    entry
                                        .missing
                                        .iter()
                                        .map(|id| Json::text(id.clone()))
                                        .collect(),
                                ),
                            ),
                        ])
                    })
                    .collect(),
            ),
        ),
        (
            "waiting_not_listed",
            Json::Number(version.waiting_not_listed() as u64),
        ),
        ("apply_order_agrees", Json::Bool(version.order().agreed())),
        ("derivation", Json::text(version.derivation().to_owned())),
        (
            "checked_against_author_claim",
            Json::Bool(version.checked_against_author_claim()),
        ),
    ])
}

/// The user-visible summary of one start-up, in the product's own words.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StartupSummary {
    /// Whether the workspace can be used after this start-up.
    pub serving: bool,
    /// How serious it is.
    pub severity: Severity,
    /// How long the start-up took, in milliseconds.
    pub elapsed_ms: u64,
    /// One sentence a person can read, from [`crate::user_messages`].
    pub sentence: String,
}

impl StartupSummary {
    /// This summary as the `startup.report` answer.
    ///
    /// Key order: `serving`, `severity`, `elapsed_ms`, `sentence`.
    #[must_use]
    pub fn to_json(&self) -> Json {
        Json::object([
            ("serving", Json::Bool(self.serving)),
            ("severity", Json::text(severity_word(self.severity))),
            ("elapsed_ms", Json::Number(self.elapsed_ms)),
            ("sentence", Json::text(self.sentence.clone())),
        ])
    }
}

impl From<&RecoveryDiagnostic> for StartupSummary {
    fn from(diagnostic: &RecoveryDiagnostic) -> Self {
        Self {
            serving: diagnostic.is_serving(),
            severity: diagnostic.severity(),
            elapsed_ms: u64::try_from(diagnostic.elapsed().as_millis()).unwrap_or(u64::MAX),
            sentence: user_messages::startup_sentence(diagnostic.outcome()),
        }
    }
}

/// The wire word for a severity. Stable, lowercase, and not the `Display` form, so that changing
/// a diagnostic's prose cannot silently change a machine field.
#[must_use]
pub const fn severity_word(severity: Severity) -> &'static str {
    match severity {
        Severity::Routine => "routine",
        Severity::Notable => "notable",
        Severity::Blocking => "blocking",
    }
}

/// The `surface.describe` answer, built from [`METHODS`].
///
/// Key order: `protocol`, `surface_version`, `supported_versions`, `methods`; and within a method,
/// `name`, `since`, `summary`.
#[must_use]
pub fn describe() -> Json {
    Json::object([
        ("protocol", Json::text(crate::ipc::message::PROTOCOL)),
        ("surface_version", Json::Number(u64::from(SURFACE_VERSION))),
        (
            "supported_versions",
            Json::Array(
                SUPPORTED_VERSIONS
                    .iter()
                    .map(|version| Json::Number(u64::from(*version)))
                    .collect(),
            ),
        ),
        (
            "methods",
            Json::Array(
                METHODS
                    .iter()
                    .map(|entry| {
                        Json::object([
                            ("name", Json::text(entry.name)),
                            ("since", Json::Number(u64::from(entry.since))),
                            ("summary", Json::text(entry.summary)),
                        ])
                    })
                    .collect(),
            ),
        ),
    ])
}

/// The state one connection carries: whether it has said hello, on which version, and whether it
/// asked to be told what the daemon is doing.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Conversation {
    negotiated: Option<u32>,
    subscription: Option<u64>,
    cursor: u64,
}

impl Conversation {
    /// A conversation that has not yet negotiated.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            negotiated: None,
            subscription: None,
            cursor: 0,
        }
    }

    /// The version in force, once `hello` has been answered.
    #[must_use]
    pub const fn negotiated(&self) -> Option<u32> {
        self.negotiated
    }

    /// The identifier of the `events.subscribe` call this connection made, when it made one.
    #[must_use]
    pub const fn subscription(&self) -> Option<u64> {
        self.subscription
    }

    /// How far this connection has been told about the feed.
    #[must_use]
    pub const fn cursor(&self) -> u64 {
        self.cursor
    }

    /// Take every feed entry this connection has not been sent yet, as lines to push.
    ///
    /// Empty for a connection that never subscribed, which is what keeps version 2's addition
    /// invisible to a client that did not ask for it. The cursor advances only over entries that
    /// are handed back, so a write that fails does not silently skip them — the caller drops the
    /// connection in that case and the next one starts from its own cursor. A ring wrap between
    /// subscription admission and a later drain closes the subscription with an explicit failure
    /// instead of emitting a suffix whose missing prefix the client cannot detect.
    pub fn drain_events(&mut self, operations: &dyn Operations) -> Vec<DaemonMessage> {
        let Some(id) = self.subscription else {
            return Vec::new();
        };
        let backlog = operations.events_since(self.cursor);
        if backlog.dropped != 0 {
            self.subscription = None;
            return vec![DaemonMessage::Failed {
                id,
                code: "event-backlog-lost".to_owned(),
                message: user_messages::EVENT_BACKLOG_LOST.to_owned(),
            }];
        }
        let mut lines = Vec::with_capacity(backlog.entries.len());
        for entry in backlog.entries {
            self.cursor = entry.sequence;
            lines.push(DaemonMessage::Event {
                id,
                sequence: entry.sequence,
                kind: entry.kind.word().to_owned(),
                value: entry.kind.to_json(),
            });
        }
        lines
    }

    /// Answer one client message, advancing the conversation.
    ///
    /// `already_known` says whether this daemon process has seen the session name before; the
    /// caller owns the registry because the transport owns the lifetime of the process.
    pub fn answer(
        &mut self,
        request: &ClientMessage,
        operations: &dyn Operations,
        already_known: bool,
    ) -> DaemonMessage {
        match request {
            ClientMessage::Hello {
                id,
                versions,
                session,
            } => {
                if self.negotiated.is_some() {
                    return refused(*id, Refusal::AlreadyOpen);
                }
                match negotiate(versions) {
                    None => refused(*id, Refusal::NoSharedVersion),
                    Some(version) => {
                        self.negotiated = Some(version);
                        DaemonMessage::Welcome {
                            id: *id,
                            version,
                            session: session.clone(),
                            resumed: already_known,
                            surface_version: SURFACE_VERSION,
                        }
                    }
                }
            }
            ClientMessage::Call {
                id,
                method: name,
                version,
                params,
            } => {
                let Some(negotiated) = self.negotiated else {
                    return refused(*id, Refusal::NotOpen);
                };
                if *version != negotiated {
                    return DaemonMessage::Failed {
                        id: *id,
                        code: "version-not-negotiated".to_owned(),
                        message: user_messages::VERSION_NOT_NEGOTIATED.to_owned(),
                    };
                }
                let Some(entry) = method(name) else {
                    return DaemonMessage::Failed {
                        id: *id,
                        code: "unknown-method".to_owned(),
                        message: user_messages::UNKNOWN_METHOD.to_owned(),
                    };
                };
                if entry.since > negotiated {
                    return DaemonMessage::Failed {
                        id: *id,
                        code: "method-newer-than-surface".to_owned(),
                        message: user_messages::METHOD_TOO_NEW.to_owned(),
                    };
                }
                self.dispatch(entry, *id, params, operations)
            }
        }
    }

    /// Compute one catalogue method's reply, which may be an answer or a typed refusal.
    ///
    /// Takes `&mut self` because `events.subscribe` is the one method that changes what the
    /// connection is, rather than only what it says.
    fn dispatch(
        &mut self,
        entry: &Method,
        id: u64,
        params: &Json,
        operations: &dyn Operations,
    ) -> DaemonMessage {
        match entry.name {
            "workspace.open" => {
                let Some(path) = params.get("path").and_then(Json::as_text) else {
                    return DaemonMessage::Failed {
                        id,
                        code: "workspace-path-required".to_owned(),
                        message: user_messages::WORKSPACE_PATH_REQUIRED.to_owned(),
                    };
                };
                match operations.open_workspace(path) {
                    Ok(summary) => DaemonMessage::Result {
                        id,
                        value: summary.to_json(),
                    },
                    Err(unavailable) => unavailable.as_failure(id),
                }
            }
            "workspace.state" => match operations.workspace_state() {
                Ok(summary) => DaemonMessage::Result {
                    id,
                    value: summary.to_json(),
                },
                Err(unavailable) => unavailable.as_failure(id),
            },
            "review.open" => {
                let Some(bundle) = params.get("bundle").and_then(Json::as_text) else {
                    return required(id, "review-bundle-required");
                };
                let Some(target) = params.get("target").and_then(Json::as_text) else {
                    return required(id, "review-target-required");
                };
                let Some(opened_by) = params.get("opened_by").and_then(Json::as_text) else {
                    return required(id, "reviewer-required");
                };
                match operations.open_review(bundle, target, opened_by) {
                    Ok(summary) => DaemonMessage::Result {
                        id,
                        value: summary.to_json(),
                    },
                    Err(unavailable) => unavailable.as_failure(id),
                }
            }
            "review.open-current" => {
                let Some(opened_by) = params.get("opened_by").and_then(Json::as_text) else {
                    return required(id, "reviewer-required");
                };
                match operations.open_current_review(opened_by) {
                    Ok(summary) => DaemonMessage::Result {
                        id,
                        value: summary.to_json(),
                    },
                    Err(unavailable) => unavailable.as_failure(id),
                }
            }
            "review.approve" => {
                let Some(bundle) = params.get("bundle").and_then(Json::as_text) else {
                    return required(id, "review-bundle-required");
                };
                let Some(target) = params.get("target").and_then(Json::as_text) else {
                    return required(id, "review-target-required");
                };
                let Some(receipt) = params.get("receipt").and_then(Json::as_text) else {
                    return required(id, "approval-receipt-required");
                };
                match operations.approve_review(bundle, target, receipt) {
                    Ok(summary) => DaemonMessage::Result {
                        id,
                        value: summary.to_json(),
                    },
                    Err(unavailable) => unavailable.as_failure(id),
                }
            }
            "folder.import.preview" => {
                let Some(source) = params.get("source").and_then(Json::as_text) else {
                    return required(id, "import-source-required");
                };
                match operations.preview_folder_import(source) {
                    Ok(value) => DaemonMessage::Result { id, value },
                    Err(unavailable) => unavailable.as_failure(id),
                }
            }
            "folder.import.confirm" => {
                let Some(source) = params.get("source").and_then(Json::as_text) else {
                    return required(id, "import-source-required");
                };
                let Some(destination) = params.get("destination").and_then(Json::as_text) else {
                    return required(id, "import-destination-required");
                };
                let Some(expected) = params.get("summary").and_then(Json::as_text) else {
                    return required(id, "import-summary-required");
                };
                let protected_roots = match params.get("protected_roots") {
                    None => Vec::new(),
                    Some(Json::Array(values)) if values.len() <= 32 => {
                        let mut roots = Vec::with_capacity(values.len());
                        for value in values {
                            let Some(root) = value.as_text() else {
                                return invalid_protected_roots(id);
                            };
                            roots.push(root.to_owned());
                        }
                        roots
                    }
                    Some(_) => return invalid_protected_roots(id),
                };
                match operations.confirm_folder_import_protected(
                    source,
                    destination,
                    expected,
                    &protected_roots,
                ) {
                    Ok(value) => DaemonMessage::Result { id, value },
                    Err(unavailable) => unavailable.as_failure(id),
                }
            }
            "folder.import.rollback" => {
                let Some(destination) = params.get("destination").and_then(Json::as_text) else {
                    return required(id, "import-destination-required");
                };
                match operations.rollback_folder_import(destination) {
                    Ok(value) => DaemonMessage::Result { id, value },
                    Err(unavailable) => unavailable.as_failure(id),
                }
            }
            "workspace.restore.preview" => {
                let Some(object) = params.get("object").and_then(Json::as_text) else {
                    return required(id, "restore-object-required");
                };
                let Some(target) = params.get("target").and_then(Json::as_text) else {
                    return required(id, "restore-target-required");
                };
                match operations.preview_file_restore(object, target) {
                    Ok(value) => DaemonMessage::Result { id, value },
                    Err(unavailable) => unavailable.as_failure(id),
                }
            }
            "workspace.version.fork" => {
                let Some(operation) = params.get("operation").and_then(Json::as_text) else {
                    return required(id, "workspace-version-required");
                };
                let Some(destination) = params.get("destination").and_then(Json::as_text) else {
                    return required(id, "workspace-version-destination-required");
                };
                let Some(expected_root) = params.get("expected_root").and_then(Json::as_text)
                else {
                    return required(id, "workspace-version-source-root-required");
                };
                let Some(expected_digest) = params.get("expected_digest").and_then(Json::as_text)
                else {
                    return required(id, "workspace-version-source-digest-required");
                };
                let Some(expected_installation) =
                    params.get("expected_installation").and_then(Json::as_text)
                else {
                    return required(id, "workspace-version-source-installation-required");
                };
                match operations.fork_workspace_version(
                    operation,
                    destination,
                    expected_root,
                    expected_digest,
                    expected_installation,
                ) {
                    Ok(value) => DaemonMessage::Result { id, value },
                    Err(unavailable) => unavailable.as_failure(id),
                }
            }
            "performance.counters" => match operations.performance_counters() {
                Ok(value) => DaemonMessage::Result { id, value },
                Err(unavailable) => unavailable.as_failure(id),
            },
            "events.subscribe" => {
                if self.subscription.is_some() {
                    return DaemonMessage::Failed {
                        id,
                        code: "already-subscribed".to_owned(),
                        message: user_messages::ALREADY_SUBSCRIBED.to_owned(),
                    };
                }
                let cursor = match params.get("after_sequence") {
                    None => 0,
                    Some(value) => {
                        let Some(cursor) = value.as_u64() else {
                            return DaemonMessage::Failed {
                                id,
                                code: "event-cursor-invalid".to_owned(),
                                message: user_messages::EVENT_CURSOR_INVALID.to_owned(),
                            };
                        };
                        cursor
                    }
                };
                let latest = operations.event_cursor();
                if cursor > latest {
                    return DaemonMessage::Failed {
                        id,
                        code: "event-cursor-invalid".to_owned(),
                        message: user_messages::EVENT_CURSOR_INVALID.to_owned(),
                    };
                }
                // A returning connection resumes after the last entry it actually received. If
                // the bounded ring wrapped meanwhile, refuse explicitly instead of presenting a
                // suffix as a complete replay and leaving the client silently stale.
                if operations.events_since(cursor).dropped != 0 {
                    return DaemonMessage::Failed {
                        id,
                        code: "event-backlog-lost".to_owned(),
                        message: user_messages::EVENT_BACKLOG_LOST.to_owned(),
                    };
                }
                self.subscription = Some(id);
                self.cursor = cursor;
                DaemonMessage::Result {
                    id,
                    value: Json::object([
                        ("subscribed", Json::Bool(true)),
                        ("latest_sequence", Json::Number(latest)),
                    ]),
                }
            }
            _ => DaemonMessage::Result {
                id,
                value: answer(entry, operations),
            },
        }
    }
}

fn required(id: u64, code: &str) -> DaemonMessage {
    DaemonMessage::Failed {
        id,
        code: code.to_owned(),
        message: user_messages::PUBLICATION_PARAMETER_REQUIRED.to_owned(),
    }
}

fn invalid_protected_roots(id: u64) -> DaemonMessage {
    DaemonMessage::Failed {
        id,
        code: "import-protected-roots-invalid".to_owned(),
        message: "The protected workspace location list is invalid.".to_owned(),
    }
}

/// Compute the answer of a method that cannot fail and does not change the connection.
fn answer(entry: &Method, operations: &dyn Operations) -> Json {
    match entry.name {
        "daemon.status" => Json::object([
            ("serving", Json::Bool(operations.serving())),
            ("surface_version", Json::Number(u64::from(SURFACE_VERSION))),
        ]),
        "startup.report" => operations.startup().to_json(),
        "surface.describe" => describe(),
        // Unreachable while `METHODS` and this match agree, and
        // `catalogue_and_dispatch_agree` in `tests/ipc.rs` is what keeps them agreeing.
        _ => Json::empty_object(),
    }
}

/// Why a conversation was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Refusal {
    NoSharedVersion,
    AlreadyOpen,
    NotOpen,
}

impl Refusal {
    const fn code(self) -> &'static str {
        match self {
            Self::NoSharedVersion => "unsupported-version",
            Self::AlreadyOpen => "already-open",
            Self::NotOpen => "not-open",
        }
    }

    const fn message(self) -> &'static str {
        match self {
            Self::NoSharedVersion => user_messages::NO_SHARED_VERSION,
            Self::AlreadyOpen => user_messages::ALREADY_OPEN,
            Self::NotOpen => user_messages::NOT_OPEN,
        }
    }
}

fn refused(id: u64, why: Refusal) -> DaemonMessage {
    DaemonMessage::Refused {
        id,
        code: why.code().to_owned(),
        message: why.message().to_owned(),
        supported: SUPPORTED_VERSIONS.to_vec(),
    }
}

/// An [`Operations`] that answers from one recovery diagnostic.
///
/// This is what the real daemon uses: [`crate::recover_on_start`] produces the diagnostic, and
/// everything the interface can ask about start-up is read from it. It is a struct rather than an
/// `impl` on `RecoveryDiagnostic` so that a later task can replace it without touching the
/// transport.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecoveredDaemon {
    diagnostic: RecoveryDiagnostic,
}

impl RecoveredDaemon {
    /// Wrap the diagnostic the daemon's start-up produced.
    #[must_use]
    pub const fn new(diagnostic: RecoveryDiagnostic) -> Self {
        Self { diagnostic }
    }

    /// The diagnostic behind this surface.
    #[must_use]
    pub const fn diagnostic(&self) -> &RecoveryDiagnostic {
        &self.diagnostic
    }
}

impl Operations for RecoveredDaemon {
    fn serving(&self) -> bool {
        self.diagnostic.is_serving()
    }

    fn startup(&self) -> StartupSummary {
        StartupSummary::from(&self.diagnostic)
    }
}

impl fmt::Display for Method {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{} (since v{})", self.name, self.since)
    }
}

/// A diagnostic for a start-up that found nothing to recover — the shape a caller with no store
/// yet can still serve a surface from.
#[must_use]
pub fn nothing_to_recover() -> RecoveryDiagnostic {
    RecoveryDiagnostic::new(
        RecoveryOutcome::Rebuilt {
            records: 0,
            rows: 0,
            digest: mesh_store::Digest16::from_bytes([0u8; 16]),
        },
        core::time::Duration::from_millis(0),
        crate::recovery::RECOVERY_BUDGET,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ipc::events::{EventFeed, EventKind};

    struct FeedOperations(EventFeed);

    impl Operations for FeedOperations {
        fn serving(&self) -> bool {
            true
        }

        fn startup(&self) -> StartupSummary {
            StartupSummary::from(&nothing_to_recover())
        }

        fn events_since(&self, cursor: u64) -> EventBacklog {
            self.0.since(cursor)
        }

        fn event_cursor(&self) -> u64 {
            self.0.latest()
        }
    }

    #[test]
    fn negotiation_picks_the_highest_shared_version() {
        assert_eq!(negotiate(&[1]), Some(1), "an old client is still served");
        assert_eq!(negotiate(&[1, 2]), Some(2));
        assert_eq!(negotiate(&[1, 2, 3]), Some(3));
        assert_eq!(negotiate(&[3, 4]), Some(4));
        assert_eq!(negotiate(&[7, 8]), Some(7), "never a version we lack");
        assert_eq!(negotiate(&[5]), Some(5));
        assert_eq!(negotiate(&[]), None);
    }

    #[test]
    fn a_version_one_client_cannot_reach_a_version_two_method() {
        let mut conversation = Conversation::new();
        let operations = RecoveredDaemon::new(nothing_to_recover());
        conversation.answer(
            &ClientMessage::Hello {
                id: 0,
                versions: vec![1],
                session: "old".to_owned(),
            },
            &operations,
            false,
        );
        for entry in METHODS.iter().filter(|entry| entry.since > 1) {
            let reply = conversation.answer(
                &ClientMessage::Call {
                    id: 9,
                    method: entry.name.to_owned(),
                    version: 1,
                    params: Json::empty_object(),
                },
                &operations,
                false,
            );
            assert!(
                matches!(reply, DaemonMessage::Failed { ref code, .. } if code == "method-newer-than-surface"),
                "{} was reachable at version 1: {reply:?}",
                entry.name
            );
        }
    }

    #[test]
    fn a_surface_with_no_workspace_refuses_the_workspace_methods_by_name() {
        let mut conversation = Conversation::new();
        let operations = RecoveredDaemon::new(nothing_to_recover());
        conversation.answer(
            &ClientMessage::Hello {
                id: 0,
                versions: vec![1, 2],
                session: "new".to_owned(),
            },
            &operations,
            false,
        );
        for method_name in ["workspace.open", "workspace.state"] {
            let reply = conversation.answer(
                &ClientMessage::Call {
                    id: 9,
                    method: method_name.to_owned(),
                    version: 2,
                    params: Json::object([("path", Json::text("/tmp"))]),
                },
                &operations,
                false,
            );
            assert!(
                matches!(reply, DaemonMessage::Failed { ref code, .. } if code == "workspace-not-served"),
                "{method_name} answered something other than an honest refusal: {reply:?}"
            );
        }
    }

    #[test]
    fn opening_a_workspace_without_a_folder_is_refused_before_anything_is_read() {
        let mut conversation = Conversation::new();
        let operations = RecoveredDaemon::new(nothing_to_recover());
        conversation.answer(
            &ClientMessage::Hello {
                id: 0,
                versions: vec![2],
                session: "new".to_owned(),
            },
            &operations,
            false,
        );
        let reply = conversation.answer(
            &ClientMessage::Call {
                id: 4,
                method: "workspace.open".to_owned(),
                version: 2,
                params: Json::empty_object(),
            },
            &operations,
            false,
        );
        assert!(
            matches!(reply, DaemonMessage::Failed { ref code, .. } if code == "workspace-path-required"),
            "{reply:?}"
        );
    }

    #[test]
    fn a_connection_that_never_subscribed_is_pushed_nothing() {
        let mut conversation = Conversation::new();
        let operations = RecoveredDaemon::new(nothing_to_recover());
        assert!(conversation.drain_events(&operations).is_empty());
        assert_eq!(conversation.subscription(), None);
        assert_eq!(conversation.cursor(), 0);
    }

    #[test]
    fn a_second_subscription_on_one_connection_is_refused() {
        let mut conversation = Conversation::new();
        let operations = RecoveredDaemon::new(nothing_to_recover());
        conversation.answer(
            &ClientMessage::Hello {
                id: 0,
                versions: vec![2],
                session: "new".to_owned(),
            },
            &operations,
            false,
        );
        let subscribe = ClientMessage::Call {
            id: 5,
            method: "events.subscribe".to_owned(),
            version: 2,
            params: Json::empty_object(),
        };
        assert!(matches!(
            conversation.answer(&subscribe, &operations, false),
            DaemonMessage::Result { .. }
        ));
        assert_eq!(conversation.subscription(), Some(5));
        assert!(
            matches!(conversation.answer(&subscribe, &operations, false), DaemonMessage::Failed { ref code, .. } if code == "already-subscribed"),
        );
    }

    #[test]
    fn a_returning_subscription_resumes_after_its_exact_cursor() {
        let operations = FeedOperations(EventFeed::new());
        operations.0.publish(EventKind::Serving);
        operations
            .0
            .publish(EventKind::WorkspaceOpened { records: 3 });
        let mut conversation = Conversation::new();
        conversation.answer(
            &ClientMessage::Hello {
                id: 0,
                versions: vec![2],
                session: "returning".to_owned(),
            },
            &operations,
            true,
        );
        let reply = conversation.answer(
            &ClientMessage::Call {
                id: 5,
                method: "events.subscribe".to_owned(),
                version: 2,
                params: Json::object([("after_sequence", Json::Number(1))]),
            },
            &operations,
            true,
        );
        assert!(matches!(reply, DaemonMessage::Result { .. }));
        assert_eq!(conversation.cursor(), 1);
        let replay = conversation.drain_events(&operations);
        assert_eq!(
            replay.len(),
            1,
            "the already delivered first event was replayed"
        );
        assert!(matches!(
            replay[0],
            DaemonMessage::Event { sequence: 2, .. }
        ));
    }

    #[test]
    fn an_admitted_subscription_fails_closed_if_the_feed_wraps_before_drain() {
        let operations = FeedOperations(EventFeed::new());
        let mut conversation = Conversation::new();
        conversation.answer(
            &ClientMessage::Hello {
                id: 0,
                versions: vec![2],
                session: "slow-subscriber".to_owned(),
            },
            &operations,
            false,
        );
        let reply = conversation.answer(
            &ClientMessage::Call {
                id: 7,
                method: "events.subscribe".to_owned(),
                version: 2,
                params: Json::empty_object(),
            },
            &operations,
            false,
        );
        assert!(matches!(reply, DaemonMessage::Result { .. }));

        for _ in 0..(crate::ipc::events::FEED_CAPACITY + 1) {
            operations.0.publish(EventKind::Serving);
        }
        let pushed = conversation.drain_events(&operations);
        assert_eq!(pushed.len(), 1, "a partial suffix must not be emitted");
        assert!(matches!(
            pushed[0],
            DaemonMessage::Failed { ref code, .. } if code == "event-backlog-lost"
        ));
        assert_eq!(conversation.subscription(), None);
        assert_eq!(
            conversation.cursor(),
            0,
            "a missing prefix cannot advance the cursor"
        );
    }

    #[test]
    fn an_impossible_or_lost_event_cursor_fails_closed() {
        let operations = FeedOperations(EventFeed::new());
        for _ in 0..(crate::ipc::events::FEED_CAPACITY + 2) {
            operations.0.publish(EventKind::Serving);
        }
        for (cursor, expected) in [
            (1, "event-backlog-lost"),
            (u64::MAX, "event-cursor-invalid"),
        ] {
            let mut conversation = Conversation::new();
            conversation.answer(
                &ClientMessage::Hello {
                    id: 0,
                    versions: vec![2],
                    session: "returning".to_owned(),
                },
                &operations,
                true,
            );
            let reply = conversation.answer(
                &ClientMessage::Call {
                    id: 5,
                    method: "events.subscribe".to_owned(),
                    version: 2,
                    params: Json::object([("after_sequence", Json::Number(cursor))]),
                },
                &operations,
                true,
            );
            assert!(matches!(reply, DaemonMessage::Failed { ref code, .. } if code == expected));
            assert_eq!(conversation.subscription(), None);
        }
    }

    #[test]
    fn a_call_before_hello_is_refused() {
        let mut conversation = Conversation::new();
        let operations = RecoveredDaemon::new(nothing_to_recover());
        let reply = conversation.answer(
            &ClientMessage::Call {
                id: 1,
                method: "daemon.status".to_owned(),
                version: 1,
                params: Json::empty_object(),
            },
            &operations,
            false,
        );
        assert!(matches!(reply, DaemonMessage::Refused { .. }));
    }

    #[test]
    fn a_second_hello_is_refused_and_does_not_change_the_version() {
        let mut conversation = Conversation::new();
        let operations = RecoveredDaemon::new(nothing_to_recover());
        let hello = ClientMessage::Hello {
            id: 1,
            versions: vec![1],
            session: "w16".to_owned(),
        };
        assert!(matches!(
            conversation.answer(&hello, &operations, false),
            DaemonMessage::Welcome { .. }
        ));
        assert!(matches!(
            conversation.answer(&hello, &operations, false),
            DaemonMessage::Refused { .. }
        ));
        assert_eq!(conversation.negotiated(), Some(1));
    }

    #[test]
    fn every_catalogue_method_answers_or_refuses_by_name() {
        let mut conversation = Conversation::new();
        let operations = RecoveredDaemon::new(nothing_to_recover());
        conversation.answer(
            &ClientMessage::Hello {
                id: 0,
                versions: SUPPORTED_VERSIONS.to_vec(),
                session: "w16".to_owned(),
            },
            &operations,
            false,
        );
        for entry in METHODS {
            let reply = conversation.answer(
                &ClientMessage::Call {
                    id: 9,
                    method: entry.name.to_owned(),
                    version: SURFACE_VERSION,
                    params: Json::object([("path", Json::text("/tmp"))]),
                },
                &operations,
                false,
            );
            match reply {
                DaemonMessage::Result { value, .. } => {
                    assert!(value.is_object(), "{} answered a non-object", entry.name);
                    assert_ne!(
                        value,
                        Json::empty_object(),
                        "{} fell through the dispatch match",
                        entry.name
                    );
                }
                // A refusal is a correct answer for a surface with no workspace behind it, and it
                // is what the trait's default implementations give. What is NOT allowed is silence
                // or an empty object, and both of the other arms catch that.
                DaemonMessage::Failed { ref code, .. } => assert!(
                    !code.is_empty(),
                    "{} refused without a code to branch on",
                    entry.name
                ),
                other => panic!("{} answered {other:?}", entry.name),
            }
        }
    }

    #[test]
    fn a_method_off_the_catalogue_fails_rather_than_panicking() {
        let mut conversation = Conversation::new();
        let operations = RecoveredDaemon::new(nothing_to_recover());
        conversation.answer(
            &ClientMessage::Hello {
                id: 0,
                versions: vec![1],
                session: "w16".to_owned(),
            },
            &operations,
            false,
        );
        let reply = conversation.answer(
            &ClientMessage::Call {
                id: 1,
                method: "store.write".to_owned(),
                version: 1,
                params: Json::empty_object(),
            },
            &operations,
            false,
        );
        assert!(
            matches!(reply, DaemonMessage::Failed { ref code, .. } if code == "unknown-method"),
            "{reply:?}"
        );
    }

    #[test]
    fn a_call_on_an_unnegotiated_version_fails() {
        let mut conversation = Conversation::new();
        let operations = RecoveredDaemon::new(nothing_to_recover());
        conversation.answer(
            &ClientMessage::Hello {
                id: 0,
                versions: vec![1],
                session: "w16".to_owned(),
            },
            &operations,
            false,
        );
        let reply = conversation.answer(
            &ClientMessage::Call {
                id: 1,
                method: "daemon.status".to_owned(),
                version: 2,
                params: Json::empty_object(),
            },
            &operations,
            false,
        );
        assert!(
            matches!(reply, DaemonMessage::Failed { ref code, .. } if code == "version-not-negotiated"),
            "{reply:?}"
        );
    }

    #[test]
    fn a_returning_session_is_told_it_was_recognised() {
        let mut conversation = Conversation::new();
        let operations = RecoveredDaemon::new(nothing_to_recover());
        let reply = conversation.answer(
            &ClientMessage::Hello {
                id: 0,
                versions: vec![1],
                session: "w16".to_owned(),
            },
            &operations,
            true,
        );
        assert!(matches!(
            reply,
            DaemonMessage::Welcome { resumed: true, .. }
        ));
    }
}
