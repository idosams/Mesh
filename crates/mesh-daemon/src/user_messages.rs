//! Every sentence the daemon sends that a person will read.
//!
//! # Why they are all in one file
//!
//! `tools/program/vocab-lint/surfaces.json` already names this exact path as a user-facing
//! surface — *"error and notification text the desktop client renders verbatim"* — with the
//! forbidden-words rule and an exemption budget of zero. That entry predates this file. Putting
//! every user-readable string here is what makes the lint's coverage complete: a sentence written
//! inline somewhere else in the crate is a sentence the gate never sees.
//!
//! # The vocabulary rule, in force
//!
//! The internal model is a version history; the user model is six words. Nine terms are banned
//! from anything a person reads — the acyclic graph, the leading edge of a person's work, the
//! per-actor counter set, the version-control words for a line of work, for saving, for moving
//! work onto a newer base, for the pre-review area, for a named pointer, and for the ordered
//! record of operations. None of them appears below, and `vocab-lint` is what proves that rather
//! than this paragraph. Run it:
//!
//! ```text
//! node tools/program/vocab-lint/lint.mjs --user-facing
//! ```
//!
//! # What these sentences may say
//!
//! A message here names **what happened to the person's work** and **what they can do next**. It
//! never names how storage is built. "Mesh" is the product; "the background service" is what the
//! desktop window talks to, because a person who has to restart something needs a name for it.

use crate::recovery::RecoveryOutcome;

/// A path was handed to the exclusion report that this workspace cannot name at all.
pub const EXCLUSION_PATH_UNUSABLE: &str =
    "That is not a path inside this workspace, so Mesh has no answer about whether it is saved. \
     Give a path relative to the workspace folder, with no leading slash and no `..`.";

/// Native discovery could not safely apply this workspace's ignore rules.
pub const EXCLUSION_RULES_UNAVAILABLE: &str =
    "Mesh could not apply this workspace's ignore rules, so it did not offer any new native files \
     or folders for saving. Check `.gitignore` and `.meshignore`, then refresh folder changes.";

/// A zero-history workspace contains ordinary native content that Mesh has not admitted.
pub const UNVERSIONED_NATIVE_CONTENT: &str =
    "This folder contains ordinary files or folders that are not saved in Mesh history. Choose \
     Preview this folder to review and preserve them in a new private workspace; the current \
     folder stays unchanged.";

/// No version of the surface is spoken by both ends.
pub const NO_SHARED_VERSION: &str =
    "This app and the Mesh background service are too far apart in age to talk to each other. \
     Update both to the same release and try again.";

/// A second handshake arrived on a connection that already had one.
pub const ALREADY_OPEN: &str =
    "This connection is already open. Nothing was changed. Close it and open a new one if you \
     need to start over.";

/// A request arrived before the handshake.
pub const NOT_OPEN: &str =
    "This connection has not finished opening yet. Nothing was changed. Wait for it to finish and \
     try again.";

/// A request named a surface version other than the negotiated one.
pub const VERSION_NOT_NEGOTIATED: &str =
    "This request used a different version of the Mesh service interface than the one this \
     connection agreed on. Nothing was changed.";

/// A request named a method the catalogue does not have.
pub const UNKNOWN_METHOD: &str =
    "The Mesh background service does not offer this operation. Nothing was changed. Updating \
     both the app and the service usually fixes this.";

/// A request named a method that exists but arrived only in a later surface version.
pub const METHOD_TOO_NEW: &str =
    "This operation arrived in a later version of the Mesh service interface than the one this \
     connection agreed on. Nothing was changed.";

/// A line arrived that is not a message this surface understands.
pub const UNREADABLE_REQUEST: &str =
    "The Mesh background service could not read that request and nothing was changed.";

/// A request asked about a workspace before one was opened.
pub const NO_WORKSPACE_OPEN: &str =
    "No workspace is open yet, so there is nothing to show. Nothing was changed. Choose a folder \
     to open first.";

/// The surface this connection reached holds no workspace at all.
pub const WORKSPACE_NOT_SERVED: &str =
    "This Mesh background service does not hold a workspace, so it has nothing to show. Nothing \
     was changed.";

/// This implementation has no live performance counter registry behind the surface.
pub const COUNTERS_NOT_SERVED: &str =
    "This Mesh background service cannot read live performance counters. Nothing was changed.";

/// A request to open a workspace did not say which folder.
pub const WORKSPACE_PATH_REQUIRED: &str =
    "Mesh needs to know which folder to open and nothing was changed. Choose a folder and try \
     again.";

/// The folder could not be reached: it is missing, or Mesh may not read it.
pub const WORKSPACE_UNREACHABLE: &str =
    "Mesh could not reach that folder and has changed nothing. Check that it still exists and \
     that you have permission to open it, then try again.";

/// What is saved in the folder does not read back as whole, correct records.
pub const WORKSPACE_DAMAGED: &str =
    "Mesh could not read what is saved in that folder and has changed nothing. Your other \
     workspaces are unaffected. This one needs attention before it can be used.";

/// There is an unfinished save in the folder and nothing finished before it.
///
/// Deliberately not the sentence an empty folder gets. The two folders differ by nothing a person
/// can see, so the difference has to be in what Mesh says: reporting this one as a normal start
/// would turn an interrupted shutdown into silence.
pub const WORKSPACE_NOTHING_READABLE: &str =
    "Mesh found an unfinished save in that folder and nothing finished before it, so it has not \
     opened the folder and has changed nothing. This folder is not empty and Mesh will not treat \
     it as though it were. Nothing you were told had been saved privately is missing, because \
     nothing there had finished being saved. This folder needs attention before it can be used.";

/// The records read back, and they disagree with each other.
pub const WORKSPACE_CONTRADICTORY: &str =
    "What is saved in that folder does not fit together, so Mesh has not opened it and has \
     changed nothing. Nothing you were told had been saved privately has been removed.";

/// The workspace's content store itself could not be opened.
pub const PAYLOAD_STORE_UNREACHABLE: &str =
    "Mesh could not open the saved content for this workspace, so the folder has not been opened. \
     Nothing was removed or changed. Check that the folder is readable and try again.";

/// The disposable metadata index could not be recreated from the immutable saved records.
pub const WORKSPACE_INDEX_UNAVAILABLE: &str =
    "Mesh could not rebuild its local workspace index, so the folder has not been opened. Your \
     saved work remains unchanged. Check that the folder is writable and try again.";

/// One saved change names content that is not present locally.
pub const OPERATION_PAYLOAD_MISSING: &str =
    "One saved change is missing the content needed to show all file names and folders. Mesh is \
     showing the part it can recover; reconnect this workspace to a replica that still has the \
     missing content.";

/// One saved change's content could not be verified or read.
pub const OPERATION_PAYLOAD_UNREADABLE: &str =
    "One saved change has content that cannot be verified, so Mesh is showing only the file names \
     and folders it can recover. The saved change remains recorded and needs repair.";

/// One saved change's verified bytes do not describe an operation this build understands.
pub const OPERATION_PAYLOAD_INVALID: &str =
    "One saved change contains data this version of Mesh cannot understand, so only the file \
     names and folders that can be recovered are shown. Update Mesh before trying again.";

/// No unique workspace root can be inferred from an otherwise complete saved change set.
pub const WORKSPACE_ROOT_MISSING: &str =
    "The saved changes do not identify the workspace's top folder, so Mesh will not present an \
     empty folder as though it were the answer. The saved changes remain untouched.";

/// More than one workspace root can be inferred from an otherwise complete saved change set.
pub const WORKSPACE_ROOT_AMBIGUOUS: &str =
    "The saved changes identify more than one possible top folder, so Mesh will not guess which \
     file names belong together. The saved changes remain untouched.";

/// The materializer explicitly refused at least one decoded saved change.
pub const MATERIALIZATION_INCOMPLETE: &str =
    "Some saved changes could not be applied to this workspace state. Mesh is showing the part it \
     can recover and has kept every refused change for diagnosis.";

/// A process-lost private save no longer verifies against rebuilt immutable journal/index truth.
pub const CHECKPOINT_RECOVERY_NEEDS_ATTENTION: &str =
    "Mesh could not safely finish one saved change after restart. Its durable records were left \
     unchanged, and managed changes are paused until this workspace is repaired.";

/// The open workspace changed while one checkpoint operation was preparing its durable state.
pub const CHECKPOINT_WORKSPACE_CHANGED: &str =
    "Mesh changed workspaces while preparing this saved change. Nothing was changed; try again \
     in the workspace that is open now.";

/// The directory instance behind the open workspace path was replaced outside Mesh.
pub const WORKSPACE_DIRECTORY_CHANGED: &str =
    "The folder Mesh opened was replaced outside Mesh. Managed changes are paused. Reopen the \
     folder before continuing.";

/// A second subscription arrived on a connection that already has one.
pub const ALREADY_SUBSCRIBED: &str =
    "This connection is already being kept up to date. Nothing was changed.";

/// An event subscription supplied a cursor that cannot describe this daemon's current feed.
pub const EVENT_CURSOR_INVALID: &str =
    "The event-feed position is not valid for this background service. Refresh the workspace state and try again.";

/// The bounded event feed wrapped before a returning client resumed it.
pub const EVENT_BACKLOG_LOST: &str =
    "Some background-service events are no longer available. Refresh the workspace state before continuing.";

/// A publication call omitted one of its exact identifiers or receipt bytes.
pub const PUBLICATION_PARAMETER_REQUIRED: &str =
    "Mesh needs the exact review, saved change, and approval named by this operation. Nothing was changed.";

/// A publication parameter was not a 32-byte identifier.
pub const PUBLICATION_IDENTIFIER_INVALID: &str =
    "One publication identifier could not be read. Nothing was changed.";

/// The requested operation has not been saved in this workspace.
pub const PUBLICATION_TARGET_ABSENT: &str =
    "That saved change is not present in this workspace, so no review or approval was recorded.";

/// A review bundle already names another target.
pub const PUBLICATION_REVIEW_CONFLICT: &str =
    "That review already belongs to a different saved change, so nothing was changed.";

/// A caller supplied a review name that is not the bundle computed from durable workspace truth.
pub const PUBLICATION_REVIEW_BUNDLE_MISMATCH: &str =
    "The review identifier does not match the exact saved workspace state, so nothing was changed.";

/// Durable workspace truth was not complete or unambiguous enough to compute a review.
pub const PUBLICATION_REVIEW_NOT_COMPUTABLE: &str =
    "Mesh cannot compute an exact review for this saved version yet, so nothing was changed.";

/// The requested review does not exist yet.
pub const PUBLICATION_REVIEW_ABSENT: &str =
    "That review has not been opened, so the approval was not recorded.";

/// The ordinary native folder no longer equals the exact saved point being reviewed.
pub const PUBLICATION_NATIVE_WORK_PENDING: &str =
    "The native folder no longer matches the saved version being reviewed. Inspect and save or resolve its newer work before recording or approving; nothing was changed.";

/// Publication trust was not configured when the service started.
pub const PUBLICATION_TRUST_ABSENT: &str =
    "No trusted human reviewer was configured when Mesh started, so the approval was not recorded.";

/// No OS-mediated human signing authority is available to the daemon.
pub const PUBLICATION_HUMAN_AUTHORITY_UNAVAILABLE: &str =
    "Mesh can show this review, but it cannot approve to the shared version yet because no verified human-held signing authority is available. Nothing was changed.";

/// The receipt bytes were not hexadecimal canonical bytes.
pub const PUBLICATION_RECEIPT_INVALID: &str =
    "The approval receipt could not be read in its one accepted form, so it was not recorded.";

/// The signer is not in the configured trust set.
pub const PUBLICATION_REVIEWER_UNTRUSTED: &str =
    "The approval was signed by a reviewer this service does not trust, so it was not recorded.";

/// Receipt verification or exact-context binding failed.
pub const PUBLICATION_APPROVAL_INVALID: &str =
    "The approval did not verify for this exact review, saved change, and current shared version, so it was not recorded.";

/// The target is already the protected shared version.
pub const PUBLICATION_ALREADY_SHARED: &str =
    "That saved change is already the shared version, so this approval was not recorded again.";

/// A durable publication write failed.
pub const PUBLICATION_SAVE_FAILED: &str =
    "Mesh could not save this review decision, so it has not reported a new shared version.";

/// Required recovery preservation failed before a review could be opened.
pub const PUBLICATION_RECOVERY_PRESERVATION_FAILED: &str =
    "Mesh could not preserve the current private work before opening review, so no review was \
     recorded. The workspace needs attention before trying again.";

// ---------------------------------------------------------------------------------------------
// The folder-watching fallback — plan §7.4, task 01KZC2QR9VVJK6Y60PS8D360JT
//
// Seven sentences, one per entry in `crate::fallback::FallbackRestriction`, plus the two
// announcements — the fallback is in use, or the direct connection is. They are here rather than
// next to the enumeration for the reason the header gives: this is the only path `vocab-lint`
// scans, and a sentence written anywhere else in this crate is a sentence the gate never sees.
// `crate::fallback::FallbackRestriction::headline` is what binds each one to the restriction it
// describes, and a daemon test holds the two lists against each other in both directions.
// ---------------------------------------------------------------------------------------------

/// Mesh is watching a folder because the direct file-system connection is not available.
///
/// Sent whenever the fallback is chosen, and it is never optional: acceptance criterion 4 of task
/// `01KZC2QR9VVJK6Y60PS8D360JT` is that the fallback is never selected silently, and
/// [`crate::fallback::choose_backend`] has no way to build a fallback choice without it.
pub const FALLBACK_IN_USE: &str =
    "Mesh is watching this folder instead of connecting to it directly, because the direct \
     connection is not available on this device. Your work is still saved privately and nothing \
     has been changed. Mesh sees less this way, and the list below says exactly what it misses.";

/// Restriction: changes are found by re-reading the folder, not by being told about them.
pub const FALLBACK_CHANGES_ARE_FOUND_LATE: &str =
    "Mesh finds changes by re-reading this folder rather than being told about them as they \
     happen, so several edits to one file between two readings arrive as a single change.";

/// Restriction: a file created and removed between two readings is never seen.
pub const FALLBACK_SHORT_LIVED_WORK_IS_MISSED: &str =
    "A file that is created and removed again between two readings of this folder is never seen \
     at all. Mesh will not claim it was there and will not claim it was not.";

/// Restriction: a rename made outside Mesh is worked out afterwards, not observed.
pub const FALLBACK_RENAMES_ARE_WORKED_OUT_AFTERWARDS: &str =
    "When a file is renamed outside Mesh, Mesh works out afterwards that it is the same file \
     rather than watching it happen. That is recorded as something Mesh worked out, never as \
     something it saw, so nothing here is presented as more certain than it is.";

/// Restriction: no durable boundary can be read from the folder alone.
pub const FALLBACK_NO_SAVE_POINT_FROM_THE_FOLDER: &str =
    "Mesh cannot tell from this folder alone when an application finished writing a file, so it \
     will not mark work as saved privately on the strength of watching the folder.";

/// Restriction: the fallback reaches only inside the folder it was given.
pub const FALLBACK_CONFINED_TO_ITS_FOLDER: &str =
    "This way of working reaches only inside the folder Mesh was given. Anything outside it is \
     refused rather than followed.";

/// Restriction: a shortcut to another location is left out of what Mesh presents.
pub const FALLBACK_LINKS_ARE_NOT_PRESENTED: &str =
    "A shortcut that points at another location is left out of what Mesh shows for this folder, \
     and Mesh neither changes it nor claims it is not there. If your work depends on one, this \
     folder needs attention before Mesh can be trusted with it.";

/// Mesh is connected to this folder directly, so nothing on the fallback's list applies.
///
/// The other half of the announcement, and it is deliberately short. A person who reads the
/// fallback's seven restrictions and then starts Mesh on a device where the direct connection is
/// available needs one sentence telling them those restrictions are not in force — not silence,
/// which reads the same as a message that failed to arrive.
pub const DIRECT_CONNECTION_IN_USE: &str =
    "Mesh is connected to this folder directly. Nothing on the folder-watching list applies here.";

/// Restriction: an earlier version presented on disk is read-only in Mesh, not to other apps.
pub const FALLBACK_EARLIER_VERSION_IS_NOT_PROTECTED: &str =
    "When Mesh puts an earlier version on disk for you to read, Mesh itself will not write to it, \
     but other applications on this device still can. Treat it as a copy to read, not as a \
     protected one.";

/// One sentence describing what the last start-up found.
///
/// Written from the outcome rather than from `RecoveryDiagnostic`'s `Display`, which carries the
/// index digest and the millisecond count — true, useful in a log, and not a sentence for a
/// person. The two are deliberately different renderings of the same fact.
#[must_use]
pub fn startup_sentence(outcome: &RecoveryOutcome) -> String {
    match outcome {
        RecoveryOutcome::Rebuilt { records, .. } => {
            format!("Mesh started and your workspace is up to date. {records} saved changes were read back.")
        }
        RecoveryOutcome::RebuiltAfterAnInterruptedSave { records, .. } => format!(
            "Mesh started after an unexpected shutdown. Your workspace is up to date and \
             {records} saved changes were read back. One save had not finished when the shutdown \
             happened and was set aside — nothing you were told had been saved privately is \
             affected."
        ),
        RecoveryOutcome::NothingDurableToRecover { .. } => WORKSPACE_NOTHING_READABLE.to_owned(),
        RecoveryOutcome::Unrecoverable { .. } => {
            "Mesh could not open your workspace and has changed nothing. Your saved work is still \
             on this device. Quit Mesh and start it again; if this message comes back, the \
             workspace needs attention before it can be used."
                .to_owned()
        }
    }
}

// # Where this module's tests are
//
// In `crates/mesh-daemon/tests/ipc.rs`, not here, and that is not a style choice. `vocab-lint`
// scans THIS FILE for string literals with an exemption budget of zero — including the literals
// inside a `#[cfg(test)]` module, which it cannot tell apart from shipped copy. A unit test whose
// job is to assert that nine banned words are absent has to name all nine, and naming them here
// makes this surface fail its own gate. The assertions are identical; only the file differs.
