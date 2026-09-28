# Existing-project attachment identity

Status: native attachment, bounded capture, signed external history, desktop inspection and persistent
comparison pins, detachment, and macOS event wakeups with periodic reconciliation are implemented
on review branches. Packaged graphical proof, incremental hashing, correlation, approval and
integration remain incomplete.

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

The live worker registry is in memory. Native catalog discovery now restores persistent project
listing from the existing receipts, with workers stopped as described below. Automatic resume
preferences, detached/offline history browsing and packaged lifecycle proof remain unfinished;
UI/history presentation is described below. Dropping the host
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

This panel describes stopped recovery and retained history. Its presentation follows the canonical
English/Hebrew preference, while paths and saved identities remain literal and left-to-right.
It is source-integrated graphical UI, not yet a packaged runtime proof. Automatic resume preferences,
approval/integration, signed parallel review and the full packaged journey remain
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

## Persistent project discovery and stopped desktop recovery

The native catalog now discovers registrations from the existing `project-<receipt digest>` child
directories. It bounds enumeration to 256 root entries and validates each recognized name, retained
real child directory, bounded canonical receipt and full receipt digest. Discovery can report the
registered root even while the source is offline, without recreating it. Invalid, linked, partial or
modified registration evidence is preserved and returns an explicit catalog error; it is not silently
omitted from an apparently complete list. Discovery changes no receipt or history format.

`AttachmentStorage::reopen(id)` admits only an existing catalog child, verifies its receipt identity,
and rechecks the original source's recorded native identity. It never provisions an alternative root.
The desktop lazily restores up to 32 projects when the list is requested, without generating keys or
starting capture workers. Verified history restores the latest saved identity. A source or history
that cannot be verified remains listed with an unavailable recovery state; a missing saved identity
in that state is not presented as proof that no history exists. Read-only discovery does not create
an absent storage root.

All recovered projects start stopped, including ones that were running when the desktop quit.
Explicit resume reuses retained authority or reopens the exact registered identity after an offline
folder returns, creates a new capture session, and reconciles edits made while Mesh was closed.
A replacement folder at the same pathname is refused. This preserves stop intent without introducing
an implicit autorun policy; automatic resume preferences remain unfinished. Pins and their navigation
remain session-only. Missing or corrupt history is not repaired or replaced as a side effect of discovery.

Tests exercise disk-backed rediscovery, no worker on restart, last saved identity recovery, refusal
of capture-before-resume, catch-up after resume, offline listing, replacement refusal, recovery when
the original folder returns, linked stores, malformed receipts and preserved partial provisioning.
The UI distinguishes restored-stopped projects from unavailable history. These are native and source-UI
proofs; packaged restart and durable pin restoration remain required.

## Native durable pin selector snapshots

`AttachmentStorage::load_comparison_pins` and `save_comparison_pins(expected_revision, pins)` now
provide a native persistence seam for comparison navigation. The `mesh.attachment-pins/v1` record
stores at most eight ordered selectors: display key, registered project identity, base and target
operation identities, page cursor and selected path. It stores no file bytes, rendered previews,
credentials, approval state or claimed validity of those versions. Consumers must reverify each
selector through native history before restoring content; unavailable history must remain unavailable.
The desktop pin controller now uses this seam as described below.

Snapshots are bound to the retained catalog's device/inode identity and read through native directory
authority. Parsing is bounded to 128 KiB and validates canonical fields, unique numeric display keys,
hex identities and bounded relative path selectors. The final record is owner-only. A missing initial
record yields revision zero; an empty snapshot after removal of the last pin is persisted with a new
revision, so restart cannot resurrect the earlier list. Repeating an unchanged snapshot at its current
revision does not write. Stale revisions refuse instead of overwriting another update.

A catalog-directory lock serializes cooperating readers/writers. Publication creates and syncs an
owner-only pending file through the retained descriptor, checks catalog identity, atomically renames
it to the fixed snapshot name, syncs the parent, rechecks identity and reads back the exact published
snapshot before acknowledgment. Partial
staging is preserved. A pending initial snapshot is not interpreted as an empty catalog, and a pending
file beside a valid snapshot prevents publication of a different snapshot until reconciliation. Copied,
linked, corrupt or substituted records refuse rather than becoming trusted preferences. This is local
native identity protection, not a portable or signed approval format.

Tests cover restart round-trips, stale and unchanged updates, durable removal, concurrent writers with
one winner, field/count limits, traversal refusal, copied catalog identity, symlink/corruption refusal,
interrupted initial staging and replaced catalog paths. This introduces an additive preferences format;
existing registration and signed history formats are unchanged. Desktop load/save orchestration and selector revalidation are described below. Packaged restart proof remains outstanding.

## Desktop restoration of comparison selectors

The typed native commands load and save a bounded `mesh.desktop-pin-selectors/v1` projection,
with decimal revision and the same six selector fields. This additive wire projection does not
change the on-disk snapshot or signed history formats. The desktop saves no preview bytes.
On first opening the attachment panel it loads the stored selectors, then reads each exact comparison
through native history. A selected changed path can be resolved independently of the displayed page;
the native filtered comparison contains exactly one verified change and a total of one. File-side
previews are then read and checked against their exact version, path and content metadata.

Unavailable projects or history retain the selector as an unavailable card. They never fall back
to current working files. Pin mutations serialize into native revision-checked snapshots, including
an empty snapshot after closing the final pin. An uncertain save can be acknowledged on retry when
the native snapshot equals the desired selectors. A divergent external snapshot refuses overwrite.
The user can explicitly replace the open set with the saved set. Initial load failures never publish
empty defaults. Closing a pin during an outstanding preview cannot recreate it.

Focused native and coordinator tests cover projection bounds, independent selected-path lookup,
restoration outside the current page, unavailable pins, close during save, uncertain acknowledgements,
revision conflicts and corrupt initial records. These are source and native test results; packaged
desktop restart proof, human approval and integration remain outstanding.

## Persistent detachment without deleting work

The native desktop now exposes Detach Mesh and Reattach project. Detachment requests stop and
joins its owned capture worker before acknowledging completion. It writes an owner-only
`mesh.attachment-detached/v1` marker in that project's external store, bound to the exact
registration receipt and store device/inode. The marker is created through the retained directory
descriptor, synchronized and reread. Reattachment removes only that verified marker and syncs the
directory. Corrupt, partial, linked or substituted markers refuse instead of becoming an attached
default. A missing marker preserves compatibility with existing registrations.

Detachment retains receipts, history, review pins and original project contents. Discovery reads the
marker even while the source is offline. The desktop projects projection adds a boolean `detached`
field. Native controls reject capture/resume while detached; reattachment verifies the original source
identity and advances the control generation. Reattachment stays stopped until explicit resume.
This keeps reattachment distinct from accidentally restarting a previously stopped worker.

All native capture entry points refuse detached storage at worker start, and signed save checks the
marker under the history serialization lock before preparing and before committing a capture.
Other already-running processes are not forcibly terminated; their subsequent saves are refused.
The desktop only claims to have joined the worker it owns. A failure while writing the marker can
leave capture stopped without a confirmed detach and is reported as such. Partial evidence is retained.

Tests cover owned-worker termination, generation refusal, restart, unavailable/replaced source
reattachment refusal, continued ordinary edits, historical reads while detached, resumed catch-up,
unchanged Git status, offline discovery, and corrupt/symlink marker preservation. This is native and
source-UI validation; packaged detach/restart proof remains outstanding.


## Packaged executable capture verification

The capture harness accepts an explicit local app bundle and exact embedded revision. It runs the
bundle verifier before and after the journey and checks that the executable SHA-256 is unchanged.
After validating the seal, the verifier queries `--mesh-build-identity` and compares its structured
`mesh.desktop-build-identity/v1` revision and exact-build flag. This side-effect-free native mode
runs before any attachment, MCP or graphical setup and rejects extra arguments. Searching arbitrary
executable strings alone is insufficient: the all-zero revision occurs in unrelated constants and
was accepted by the historical verifier. A regression covers that false-positive case.
The package gate checks the current attachment heading, detach/reattach controls and pinned
comparisons alongside existing import/review markers. Marker presence proves embedded UI resources;
it does not prove a rendered or exercised window.

The fixture is registered using this checkout's development meshctl. Capture, periodic
reconciliation, restart catch-up, explicit/EOF stop, invalid-command refusal, and Git index/HEAD
plus directory-identity preservation run against the packaged desktop executable. Output records
`packaged: true` and `graphical: false` separately. A missing or malformed revision is refused.

The preserved fleet source's bundle evidence is historical and is not canonical Mesh validation.
Fresh canonical bundle results must identify the exact commit, executable hash and observed checks.
Rendered attachment, pin restoration and detach/reattach interactions remain separate packaged
acceptance work. No installed application is replaced, and neither Apple-trusted signing nor human
approval is claimed by this executable proof.


Canonical verification built the clean commit `fda9df38835421d72ca71abff2ac0c568162297d` in an
isolated packaging checkout. The sealed executable SHA-256 is
`8363abbf5e05f526eff51d1bd28900183a6f37c785352dc8d487be983ad74071`. The packaged capture
journey passed with three versions in 6,000 ms on this host (an observation, not a latency promise).
Wrong all-zero revision, extra identity-command arguments and a modified executable copy were
refused. The original bundle remained sealed; the preserved older bundle without the identity mode
was refused before launch. These results prove the executable boundary above, not the graphical
attachment, review-pin or detachment acceptance journeys.


## Native event wakeups with reconciliation

macOS capture now owns a recursive FSEvents stream on a private serial dispatch queue. The native
adapter is a small platform-gated FFI boundary using the installed SDK contract; it adds no dependency.
Its context is retained/released with Arc through the framework callbacks, panics cannot cross the C
callback, and a separate bounded native helper creates, stops, invalidates and releases the stream.
Registration can overlap the initial scan; every callback batch, including dropped-event or root-change
notifications, only sets one pending rescan flag. No event path, identifier or flag authorizes content,
proves authorship or substitutes for the complete descriptor-confined inventory.

Manual requests remain immediate and coalesced. Event requests wait at least 250 ms after the last
attempt completed. The normal five-second reconciliation deadline remains independent of callbacks,
so native startup failure, missed events, unsupported platforms or explicitly disabled signals still
reconcile. Every attempt retains the original source/store checks and signed-history boundary.
A replaced root may wake capture but is never adopted. Ignored-path events can cause an unchanged
rescan; selective dirty-path hashing and large-project resource measurements remain future work.

Status adds `native_events` and `event_signals` (callback batches, not edit counts). The desktop shows
native signals plus periodic checks or periodic fallback. Capture termination clears active-stream
status while native cleanup may remain pending; native_signal_state reports that separate lifecycle.
Real macOS tests use a five-minute reconciliation
interval and observe nested edits, atomic replacement and root-change refusal within an eight-second
deadline to require the wakeup path rather than the fallback timer. Separate disabled-signal tests
verify periodic capture and restart catch-up. This does not establish a four-worker latency target or
the packaged graphical journey.


Canonical packaged verification at `c80c9fab57a19f10589bdb8bd9cf6cbdebb5f9e4` passed the capture
journey with three saved versions, an active native stream and one callback batch after the edit.
The observed total was 19,749 ms, not an event-latency benchmark. Executable SHA-256:
`7db038cb9bbbb012a70f41efd8f9910af8f57862b3e316e0004ec1e3d980ed77`. Strict seal and native build
identity checks passed before and after the journey. Fixture registration used development meshctl.
The build wrapper's first post-sign identity check ended without a normal exit code; the unchanged
executable subsequently reported the correct revision within the same timeout and completed the
entire proof. The initial failure log is retained and its cause is not established. No timeout or
check was weakened. This is packaged executable evidence, not graphical acceptance.


## Preserve pending review bases before attachment review admission

A pending immutable review must remain readable against its original main version even after
another review advances main. Publication replay now retains the genesis and independently verified
main heads, plus journal-order hints for review requests. Reading an unapproved review tries its
hint, current main and verified historical heads, accepting a base only when recomputing the complete
bundle reproduces the recorded review identity. A delayed request can therefore recover its exact
base even when its journal arrival order gives the wrong hint.

This is presentation recovery, not approval authority. Receipt verification still checks the current
main predecessor independently. Stale approval previews and receipts refuse; malformed, contradictory
or replayed receipts still make current authority unavailable. A verified historical prefix can remain
readable without authorizing another publication. No journal record or approval-receipt format changes.

The native regression advances main past a pending review, reopens the workspace, verifies unchanged
review presentation and context, then refuses the stale receipt without appending a record. It also
reorders the pending request after the accepted approvals and checks a contradictory receipt. Tests
use fixture credentials, not native human presence. This prerequisite is transferred before the
attached-project review-request UI; that UI and its approval journey remain separate increments.

## Durable review requests from attachment history

An attached saved version can now be requested for local review through the existing native
publication-review engine. The operation checks exact attachment/store identity and history binding,
requires the target in that project's saved history, reconstructs its immutable causal closure, and
appends the existing `ReviewRecord` to the existing journal under the history lock. Acknowledgement
reopens durable history and requires the exact bundle/target record. Repeating the request, including
from a different fresh local opener identity, returns the same deterministic bundle without another
record. Concurrent requests serialize. This changes neither registration nor journal formats.

Review submission is available while capture is stopped or detached and never writes the source
folder, switches desktop workspace selection, signs approval or advances main. The native opener
identity records a local request; it does not attribute the observed edits to that person or agent.
The attachment projection explicitly reports unknown change authorship and no approval authority.
It omits the core review's capture-actor author field to avoid presenting an observation signer as
the author of the files.

The desktop can request review from a saved-version page, list up to 32 durable requests with an
explicit omitted count, select an exact bundle/target, and inspect that target's saved files.
A direct native lookup resolves any recorded request independently of the overview bound; requesting
a saved target again also reopens its request. Selected reviews do not follow later capture updates.
The overview carries the verified presentation identity, at most 128 changed-path summaries,
omitted-operation/change counts and explicit unavailable/incomplete status. It is not a complete
review or an approval surface merely because it lists paths.

Tests cover historical requests after newer saves, restart, concurrent/idempotent submissions,
detached access, cross-target refusal, requests outside the overview cap, replaced-store refusal,
and renderer routing that cannot substitute newer capture state. The prerequisite review-base fix preserves already-recorded pending reviews across main advances
and restart. A new request is computed against the current accepted main; reopening an existing
bundle retains its original base. Attachment approval/integration and packaged graphical proof
remain unfinished. No broader parallel
publication or dependency-closure guarantee is claimed by this request-queue step.

The review-request increment includes the independent directory-lock prerequisite originally found
in the subsequent approval source commit. Cloned pinned roots retain the same directory object but
must acquire flock through separately opened file descriptions; a duplicated descriptor inherits
lock ownership. The native cloned-root regression checks that a concurrent contender cannot enter
until the first guard is released. Directory identity is still rechecked after the wait.

## Native approval of attached Mesh main

The A08b native increment prepares an exact human-approval preview from a recorded review and
verifies a canonical receipt against that same context, credential, challenge and current main.
Human confirmation and signing occur outside the history lock; receipt application reopens the
verified store under independent custody before checking the main predecessor again. A newer capture
or unsaved editor change cannot substitute content into the receipt. Only external Mesh main advances;
source bytes, open editor handles, Git index and HEAD remain unchanged.

A lost response can be retried only with the identical retained receipt while that result remains
current main. A different ceremony, an older approved result, an empty or reused challenge, a foreign
project, unknown credential, changed store/source identity or malformed receipt refuses. Challenge
reuse is rejected before append, preserving the existing accepted main. Concurrent approvals of the
same predecessor serialize so exactly one can advance. Retained approval records that cannot be
verified mean unavailable authority, never a fresh empty starting state. Review requests after the
first approval therefore require configured native reviewer trust; existing no-trust wrappers refuse
to construct new requests against an invented genesis.

The existing receipt and journal formats remain unchanged. The directory-lock prerequisite from the
same source commit was already transferred in A08a. Tests use fixture P-256 credentials to exercise
receipt verification and durable replay; they do not prove native human presence. Desktop confirmation,
provider-backed signing, graphical acceptance and applying accepted results to source files remain
separate pending work.

## Desktop confirmation and accepted-main inspection

The A08c desktop increment obtains native credential availability and independently verified main
status. It labels main as last checked, distinguishes unverified authority from an empty starting
state, and can reopen the accepted review even when it is outside the bounded request queue.
English/Hebrew views retain exact literal identifiers and paths. Renderer messages carry only known
project, bundle and target selectors; they never supply receipts or human authority.

Approval retains the exact native attachment throughout the ceremony, reconstructs the preview from
durable history, bounds the complete native prompt, requires explicit confirmation and supported
Secure Enclave user presence, then applies the receipt through A08b's current-main checks. Changed
source/store identity or a moved main refuses. Native trust is supplied to subsequent review reads
and requests. Credentials, challenges and signatures stay native. Ineligible builds report approval
unavailable, and incomplete or unavailable reviews cannot initiate approval from the UI.

The coordinator clears cached approval availability while a mutation is in flight, permits only one
ceremony at a time, rechecks main after success or uncertainty, and never describes a lost response
as a rollback. A failed refresh leaves approval disabled until a new native check succeeds.
These source, rendering and native fixture tests do not prove an actual platform ceremony or the
packaged graphical journey. Those acceptance boundaries require separate evidence.


## Canonical watcher reconciliation with the grouped desktop source

The older grouped-desktop source also split monitoring from capture, but canonical PR #41 already
provides that separation with explicit native lifecycle states, bounded permits and independent
cleanup status. Its terminal capture transition sets the stop flag, clears event activity and sets
Stopped/Failed under the same lock, so callbacks and late registration cannot revive that terminal
service. The older background/signal_worker module must not replace this implementation.

Native event acceptance now waits for actual registration and one subsequent completed capture
before editing, then requires the event counter to increase after the nested edit. Initial saved
identity remains unchanged through baseline establishment. Existing eight-second waits, real native
registration requirement and five-minute reconciliation interval are unchanged. This is a stronger
baseline assertion, not a workaround for startup failure. The preserved local FSEvents registration
RPC stall and issue #37 remain unresolved; canonical execution of this test adjustment is pending.
