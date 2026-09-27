# Existing-project attachment identity

Status: native registration, bounded observation, immutable capture inputs and signed external
history commits and a native background reconciliation controller implemented. Desktop integration
and incremental filesystem event handling remain planned.

## Decision

Attach to an existing project without relocating it, adding project files, editing Git state,
restarting tools or claiming exclusive custody. The native caller supplies an existing external
metadata directory. The first implementation writes only a create-new `attachment.json` receipt
there. Descriptor-relative creation and native ancestor identity checks refuse metadata inside the
project. An identical registration is idempotent; conflicting or damaged receipts are preserved.

The canonical `mesh.project-attachment/v1` receipt records the original canonical root and its
device/inode identity. Reopen checks the entire receipt, including schema and field set, and pins the
named folder again. A missing, renamed or replaced root is unavailable; reopen never creates a new
project or silently binds to a replacement. While attached, observation must recheck the retained
descriptor and namespace. Source renames and filesystem identity reuse need explicit reconciliation
before future reattachment; this initial receipt is not a portable or cryptographic project identity.

The receipt is outside versioned project content. Its location is a native application configuration
decision, not a path the renderer or an agent may choose. The development CLI accepts explicit
absolute folders from the user. The metadata directory is trusted local configuration; this is not
an authentication scheme against another process with the same user's filesystem privileges.

Registration grants no write-back, restore, approval, provider session or exclusive custody. Registration
status explicitly reports `observation: not-started` and no saved version; the separate native history
API lists saved captures. Existing managed-workspace custody
guards remain intact. No filesystem inventory or accepted-main identity is inferred from a receipt.

## Development entry point

```sh
meshctl attach /absolute/existing/project /absolute/external/metadata-directory
meshctl attachment-status /absolute/external/metadata-directory
meshctl attachment-observe /absolute/external/metadata-directory
```

Both directories must already exist. Only the external receipt is created. These native CLI
operations do not need a daemon, launch a watcher, or claim that the desktop already tracks the
project. A failed final identity check may leave a receipt for recovery; it never deletes source
data or tries to repair an unfamiliar directory. Crash-interrupted receipts are refused and retained.

## Next integration

The on-demand observer performs a descriptor-confined inventory with entry, depth, file-size and
total hashing budgets. Default limits are 10,000 directory names, depth 64, 8 MiB per file and 64 MiB
of hashed content. One overflow probe detects budget exhaustion. Root `.gitignore` and `.meshignore`
rules reuse the existing exclusion parser, with each rule read capped at 1 MiB before and after the
scan; unsupported rules refuse observation. This is not full Git ignore semantics, including nested
ignore files. Structural `.git` entries (including case variants) are never traversed, even with negated rules. Links and
special files report incomplete observation instead of being followed or silently included.

Every file reports path, size, digest and native identity with unknown attribution. Files and
directories are checked again after hashing; observed changes, unreadable entries and exhausted
budgets produce explicit incomplete results. An unavailable or over-budget directory stops the
traversal, preserving already observed results. `entries_listed` counts successful directory lists,
not entries consumed by a failed enumeration. Reading uses independent directory cursors so repeated
scans cannot inherit EOF from an earlier scan. The observation operation does not persist captured file bytes or observations.
Even a complete observation is explicitly not an atomic snapshot or a saved version. Registration
status remains separate from this one-shot report and does not imply a continuously running watcher.

`capture_inputs` now uses the same confined traversal to retain exact file bytes, executable bits,
empty directories, source directory identity and the exclusion rules used. It refuses the entire
input if any traversal or recheck is incomplete. Opaque native input types expose immutable byte
slices to the native version writer; subsequent source edits do not replace captured content. Debug
and observation JSON exclude raw content. An exclusion-policy fingerprint lets a writer distinguish
policy changes from missing files before deciding how to update an existing saved tree. Attribution
remains unknown. These inputs are held in memory and are not saved-version acknowledgments.

The storage adapter now accepts multiple prepared file manifests and chunks with one authenticated
ChangeSet. It validates that supplied content is named by the signed operations, deduplicates content,
and acknowledges only after the manifest records and final operation record reach the existing
journal. Reused manifests must appear in the journal-derived view, not merely an index transaction
that may be ahead of a failed journal append. Tests retain an attached project's captured bytes after
later source edits and reconstruct them from an external store as one version; torn operation appends
expose no partial version. This storage primitive does not itself expose a CLI save command.
The capture actor attests to observed content; it does not establish who originally edited the files.
No persisted operation, manifest, signature, or approval format changes in this adapter.

## Native history ownership

`ProjectAttachment::save_capture` accepts opaque admitted input, a capture actor public key and a
native signing callback. It verifies the source identity and registration, serializes only the
external metadata directory, and commits through the existing authenticated batch adapter. The
signer attests to captured content, not original human or agent authorship. No source contents are
reread after capture; ordinary edits during signing remain untouched. The source is never a managed
working root, and attachment history opening skips working-file mutation recovery. Ordinary managed
workspace recovery and custody requirements remain unchanged.

The new canonical `mesh.attachment-history/v1` binding contains the existing attachment receipt,
the external store's device/inode and the capture exclusion fingerprint. Its digest determines the
workspace identity carried by signed history operations. The native reader checks both the binding
and the journal's workspace identity. Copied, changed or unknown bindings cannot silently inherit
history. Initial creation requires a dedicated external directory containing only `attachment.json`;
no other files are adopted or deleted. Missing journals after binding creation are refused, including
an initialization interrupted between the binding and journal writes. Those files remain available
for explicit recovery. Existing registration receipts need no migration. Operation, manifest,
signature, review and approval formats are unchanged.

Successive captures preserve logical file objects and parent versions at unchanged paths. New names,
content, executable modes, empty directories, removals and file/directory replacements are represented
in one operation set. Missing names mean removal only when the complete capture has the same exclusion
policy; changed policy refuses until a future explicit reconciliation flow. Renames currently appear
as removal plus addition, not an inferred filesystem identity or attribution match. An unchanged
capture returns the existing history point without invoking the signer or adding a version. Concurrent
saves serialize in the external store; their order is commit order, not a claimed atomic ordering of
source edits. The native background controller serializes its own capture attempts and reconciles
from complete bounded input; it does not claim event-time ordering.

`saved_versions` and `saved_file` reopen the same journal/CAS history without a live-file fallback.
They do not return a mutable workspace or any main-version/restore authority. Current entry points
require the registered source identity to remain available; offline history browsing and source
rename reconciliation remain unfinished. The initial completely empty project cannot yet mint a
root history point; removing all content from a previously saved project does save an empty version.

This is a native library path. CLI attachment registration/observation remain separate and do not
start saves. There is no automatic key generation, CLI background command, packaged desktop save
flow, or agent attribution in this milestone. Tests cover ordinary concurrent editing, unchanged Git
index/HEAD, old bytes after restart, file history, no-op and concurrent saves, policy changes,
missing journals, copied bindings and source replacement while signing.

## Native background controller

`AttachmentCaptureService::start` explicitly starts one native worker for an already registered
project. The host supplies a `CheckpointSigner` and observation/scheduling limits. That shared native
signing interface now lives with checkpoint storage; its former fleet-service path remains a
compatible re-export. Everyday capture has no provider, agent credential or fleet requirement.
No keys or service preferences are created or persisted by this controller.

The worker retains the admitted project and external store descriptors throughout its lifetime.
Both history recovery and saves use that exact store authority. It performs an initial reconciliation
and bounded full rescans every five seconds by default, measured from the end of the prior attempt.
Native policy may select 250 ms to five minutes. `request_capture` coalesces explicit checkpoint or
missed-event signals into at most one pending rescan. Attempts never overlap within one controller;
no event queue or event stream is treated as proof of complete source content. A restarted controller
loads the existing saved point and captures edits made while stopped. This is polling reconciliation,
not yet incremental filesystem observation, and it is not a battery or large-project performance claim.

The redacted status keeps current activity separate from the last completed result. It reports the
last acknowledged version, attempt and new-version counts, attempt duration, and monotonic ages
within this service instance. `mesh.attachment-capture/v1` projects those values for a future native
UI/event route, with unknown attribution and no atomic-snapshot claim. Paths, file contents and raw
signer/IO errors are omitted. Incomplete scans and failed saves retain the prior saved identity while
reporting the current failure; later scheduled attempts retry. Worker failure is terminal and never
silently launches a replacement. Directory replacement is unavailable, never implicit reattachment.

`request_stop` wakes an idle worker and stops future scans. A stop observed while the signer is still
running prevents that result from entering a commit. An already-started durable write may finish;
there is no rollback or process-interruption claim. `stop_and_join` waits for worker termination and
returns final status. Slow external signing, filesystem I/O or store-lock waits can delay it, so the
host must not treat a stop request as confirmed termination. Dropping the controller requests stop
without blocking. The original project and its existing tool sessions remain untouched throughout.

Native integration tests exercise periodic saves without signals, restart catch-up, incomplete
capture and signer failure recovery, unchanged captures, signal coalescing, stopping during signing,
and source/store replacement. They do not establish packaged desktop lifecycle behavior or sustained
performance on representative projects.

Implement incremental filesystem observations, evidence-based file/session correlation, native host
key/lifecycle integration, and the packaged existing-project journey.
Persist captured content and history outside the project. Present registration, catch-up, incomplete
capture and saved versions separately. Then integrate the attachment with desktop onboarding and
parallel review. Registration tests alone do not satisfy the full existing-project acceptance journey.
