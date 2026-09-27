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
start saves. Separate desktop-binary harness commands now create a native in-process capture identity
and invoke these APIs; there is still no packaged graphical save flow or agent attribution. Tests cover ordinary concurrent editing, unchanged Git
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

## Harness-accessible native commands

The desktop executable now supports `--mesh-attachment capture|watch|versions <absolute-metadata-folder>`
before graphical app initialization. These commands use an existing registration and do not open a
window, switch desktop workspaces, configure a harness, or launch an agent. Capture saves once;
versions lists exact immutable operation identities; watch runs the background controller and emits
its redacted JSON status as revisions arrive. On watch stdin, `capture`, `status`, and `stop` are
newline-delimited commands; EOF requests a stop too. Control input is bounded, with one queued command,
and invalid input is refused without echoing it. The capture worker is joined before normal exit or
control/output failure. A graceful watch exit does not itself assert a save: consumers must inspect
`saved_version` and the reported outcome.

The native host uses the existing `SoftwareActorCustody` implementation for a fresh capture-session
key. The key remains in process, is not written to a key file or project, and is dropped with its
native session. Restart creates a new capture actor while preserving the same attachment history;
this is evidence of capture, not original authorship or a human approval credential. Graphical key
and lifecycle integration remain separate work. The desktop adds a direct dependency on the existing
workspace `mesh-types` crate for the signer interface; no new third-party dependency is introduced.

The executable proof at `apps/desktop/scripts/prove-attached-capture.mjs` drives actual built binaries
against an isolated dirty Git project. It verifies duplicate-free direct saves, periodic capture with
no event signal, restart catch-up, explicit and EOF stop, invalid-command refusal, version listing,
and preservation of the Git index/HEAD and original directory identity. See the adjacent
`README-attached-capture.md` for reproduction. This is a development executable proof, not a packaged
GUI or installed-app claim.

Implement incremental filesystem observations, evidence-based file/session correlation, graphical
host lifecycle integration, and the packaged existing-project journey.
Persist captured content and history outside the project. Present registration, catch-up, incomplete
capture and saved versions separately. Then integrate the attachment with desktop onboarding and
parallel review. Registration tests alone do not satisfy the full existing-project acceptance journey.

## Native host provisioning

`AttachmentStorage` pins an existing host-configured storage directory. `provision(source)` admits
an existing project and derives a direct child name from the full canonical registration receipt's
BLAKE3 digest. The identifier is a native lookup hint, not original authorship or approval authority.
A new child is created with owner-only permissions through the retained parent descriptor; existing
children must have the exact receipt. Missing, partial, foreign or linked children are preserved and
refused rather than silently initialized. An interrupted allocation before receipt persistence needs
explicit recovery. Registration and signed-history formats remain unchanged.

The parent must be outside the original project, and its retained identity must still agree with its
pathname before provisioning. Source and store identity checks surround allocation. The resulting
`ProvisionedAttachment` shares both retained descriptors directly with `start_capture`, without
reopening and adopting a replacement store between allocation and background startup. Repeated
provisioning of the same admitted source recovers the same location. Source rename or replacement
is not an automatic history migration. The native host remains responsible for maintaining the
project catalog, admitting its own private root, deduplicating running controllers, restart preferences,
and graphical lifecycle. This API does not yet connect the desktop onboarding or review UI.

Tests cover idempotent provisioning, unchanged source contents, owner-only child creation, refusing
storage inside the source, partial receipts, linked or replaced storage, actual signed capture through
the provisioned handle, and replacement between provisioning and capture startup.

## Desktop session controller

The graphical native host now manages an independent `AttachmentHost` alongside the selected managed
workspace. `attach_existing_project(source)` lazily creates the host's owner-only `attached-projects`
storage root below application data, provisions a registered project and starts native capture with
an in-process software session key. A renderer never supplies metadata destinations or signing keys.
`attached_projects()` returns the session's project handles, original roots and redacted capture
status. `control_attached_project(id, generation, action)` admits only capture, stop or resume for
that exact session generation. These operations run on the blocking worker pool, not the UI thread.

Admission and launch are serialized. Concurrent selections of one project return one capture worker;
reselecting a stopped or failed project does not resume it. Resume requires a terminal prior worker
and the same registered project identity, creates a new generation and rejects earlier controls.
The controller bounds its session list to 32 projects. Stop is a request, not a claim of termination;
status reports the native worker's actual phase. No operation switches the selected managed workspace,
reconfigures a harness, writes project files, or grants approval authority.

The session registry is currently in memory. Saved registrations/history survive, and explicit
reattachment recovers them; persistent project listing, restart preferences, detached/offline browsing,
packaged lifecycle proof remain unfinished; UI/history presentation is described below. Dropping the host
requests its workers to stop through their existing controller lifecycle; graceful process shutdown
is not yet a verified desktop guarantee. Tests cover eight concurrent requests sharing one worker,
stop/reselect/resume, stale controls, source replacement refusal and linked storage refusal. These are
native controller tests, not evidence of user-visible controls or a packaged attachment journey.

## Initial graphical attachment panel

The existing folder-entry page now leads with an attached-project panel, followed by the separate
working-copy import flow. The React panel emits typed intents to a shell coordinator; only that
coordinator calls the native attach/status/control commands. The native picker and a typed absolute
source path are supported. The user sees original location, capture phase and outcome, latest saved
identity, age of the last complete capture and unknown authorship. Capture, stop and explicit resume
use the native project handle and session generation. No UI action grants main approval.

The coordinator serializes requests and refreshes native status every two seconds while the panel
is mounted. It validates bounded projections, rejects stale control generations and unexpected
operations, and stops scheduling when unmounted. Failed refresh retains the last displayed saved
identity, labels status as potentially stale and disables control intents until refresh succeeds.
Picker cancellation does not attach a project. Native capture continues independently of UI polling.

This panel explains session-only listing and retained history. Its presentation follows the canonical
English/Hebrew preference, while paths and saved identities remain literal and left-to-right.
It is source-integrated graphical UI, not yet a packaged runtime proof. Persistent catalog,
restart preferences, approval/integration, signed parallel review and the full packaged journey remain
required before the attachment product loop is complete.

## Saved-version list in the attachment panel

`ProvisionedAttachment` now retains clonable native directory authority for both capture and history
reads. `saved_versions()` uses that exact retained store, rather than reopening a renderer-supplied
path. The desktop keeps this history handle after launching capture. Resume now uses the same handle,
so a replaced project cannot cause resume to allocate another registration before refusing.

The read-only `attached_project_versions(id, before)` command verifies the saved journal and returns
up to 50 immutable operation identities, newest first. The optional cursor must be an actual version
of that project's verified history. A page reports the exact project and input cursor plus its next
cursor. Adding newer captures does not shift an older page anchored to an existing identity. Storage
replacement and invalid cursors refuse; neither falls back to live source content. Journal verification
currently reconstructs the full history before slicing a bounded response, so this is not a large-history
performance claim.

The attachment panel exposes latest and older version pages. The coordinator validates identities,
project binding and cursor continuity. Labels follow the English/Hebrew preference and exact
version IDs remain left-to-right. A normal status refresh never replaces the explicit history
page. The UI lists saved identities and now exposes the saved-file inspection described below;
the comparison path is described below; approval and parallel review remain pending. Native tests save more
than one page, continue saving, verify
cursor stability and refuse replaced history storage. UI tests cover malformed pages and preserving
a loaded page while the live saved-version status advances. Packaged runtime verification is pending.

## Exact saved-file inspection

`ProvisionedAttachment::inspect_entries(operation, after)` and `inspect_text(operation, path)` retain
the same native directory authority used for capture. Each operation validates the canonical saved
identity, receipt, history binding and signed journal membership before reconstructing that causal
version. It rechecks source and storage identity after the read. Neither reads current source bytes
or creates a working copy. Exact relative paths are looked up in the materialized saved version, not
resolved as arbitrary filesystem paths.

Entry pages contain at most 200 files/folders with file size, content digest and executable metadata.
Cursors must identify an entry in that exact version. Text previews are bounded to 256 KiB; invalid
UTF-8 or NUL-containing content is reported as binary, and larger files return metadata plus an explicit
unavailable-preview state. Text is returned as data and rendered by React as plain text, never HTML.
The historical preview currently verifies manifests across the selected version before reading a
file, so bounded response size does not imply constant-time reads on large projects.

The desktop `inspect_attached_version` command accepts an existing project handle, saved operation,
and either entry paging or a file selection. The shell validates project/version/cursor binding and
requires text metadata to match the selected native file entry. The panel displays the exact selected
version, file list and saved preview. Background status and version-list refresh do not replace it;
mismatched saved-version or unlisted file intents are refused. An absent inspection is rejected
before dereferencing a selection or invoking native code. Native tests cover paging, text remaining unchanged
after external edits, binary preview after live deletion, large-file limits, absent identities, invalid
paths/cursors and replaced storage. Coordinator tests bind responses to content metadata and preserve
selected text while the latest capture advances. Comparison, parallel pinned review and packaged proof
remain outstanding.

## Comparison of saved attachment versions

`compare_versions(base, target, after)` verifies both exact identities in the same admitted history
under one native store lock. It materializes each saved version, verifies its manifests, and compares
reachable paths by kind, content digest/size and executable metadata. Changes are added, removed,
modified, mode-changed or type-changed. A content-and-mode change is modified with both exact side
metadata included. This path comparison reports renames as removal/addition and does not infer author,
rename intent, acceptance or merge safety. Empty directories participate in the comparison.

The response includes both identities, total changed-path count and at most 200 changes. Cursors must
name a changed path in that exact comparison. New captures leave existing comparison pages unchanged.
Canonical alpha.5 already skips retained unlinked objects when building its visible path map; these
objects are valid causal history after removal/replacement. Complete materialization, unique reachable
paths, every visible entry's object binding and manifest verification remain required. The migrated comparison
regression covers this behavior; no replacement of the canonical path-resolution implementation
or its instrumentation is required.

The desktop exposes a typed `compare_attached_versions` command. Users select a saved base and then a
saved target; the panel keeps the open comparison pair separate from the next selected base. Selecting
a changed path reads exact saved file previews for each present file side, checked against that side's
digest, size and executable metadata. Missing sides, folders, binary content and oversized previews
have explicit states. These are bounded whole-file before/after previews, not line-level diff hunks.
The view does not approve or apply content. Capture refresh, version-list paging and next-base selection
do not replace an open comparison. Tests exercise incorrect tuple/metadata rejection and both previews
remaining pinned while the latest capture and next selected base change. Parallel pinned reviews,
main-version approval/integration and packaged user-journey proof remain required.

## Independent pinned comparison views

The attachment panel can pin up to eight comparison views across attached projects, including
multiple views of the same pair with different file selections. Every pin captures the verified
project handle, base/target identities, current page and selected saved previews. Pins use distinct
monotonic session identifiers and appear in a responsive side-by-side grid. Subsequent comparison
selection, next-base selection and capture/status refresh do not replace their contents.

Pin paging and file selection are routed through the pin's own project and exact saved pair. Updating
one pin replaces only that pin's display state; siblings and the active comparison keep independent
pages and selections. A stale, closed or cross-project pin control cannot issue a native read. Closing
a pin is local UI state and remains available during a read; a late response cannot recreate the pin.
The eight-pin limit bounds retained preview data, and closed identifiers are not reused in the session.
No pin itself carries approval or source-write authority: native reads still verify exact history.

Tests cover independent file selections, independent paging, capture and next-comparison updates,
closing during two outstanding before/after reads, limits, non-reused pin identifiers and attempts to
redirect another project's pin. Pins survive leaving/returning to the mounted panel in the same shell
session, but are not yet persisted across desktop restart. These are pinned inspection/comparison
views; human approval, dependency-aware review bundles and integration are still required for the full
parallel review journey. Packaged rendering/runtime proof remains outstanding.
