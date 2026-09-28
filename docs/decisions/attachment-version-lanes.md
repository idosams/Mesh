# Independent work from attached saved versions

Status: ordinary lane allocation and desktop source implemented. Native managed fleet allocation
is connected; desktop fleet controls and packaged graphical proof remain incomplete.

## Decision

An attached project's exact saved version can create an additional ordinary folder for a human or
an existing harness. It uses the same non-exclusive attachment capture, inspection and comparison
model as the original project. No provider, agent credential or exclusive custody transition is
required. The original project's folder, open editor descriptors, Git state and capture session stay
in place. This is an explicitly requested additional line of work, not a prerequisite for attachment.

`AttachmentStorage::open_version_lane` takes a retained source attachment, exact saved operation and
stable request identity. Native code chooses every destination beneath its admitted private storage
root. Verified historical manifests stream bytes into a create-only folder. A bounded final inventory
and file verification check the result before registration. Saved executable state is restored into
owner-private permissions; arbitrary source ACLs and other metadata are not version content. Git
internals and ignored content absent from the saved version are not copied or recreated.

Ancestry records which project/version initialized a lane. It does not authenticate the author of
later edits, assign an agent/provider, grant approval, inherit a shared main, or establish an OS
sandbox. Each lane has independent private capture/history. The original project's main remains
separate. Desktop source does not offer a lane approval as if it integrated into the original project.
Combined comparison, dependency-aware review and explicit integration back to that main remain work.

## Identity, retry and compatibility

The native directory layout is `work-lanes/lane-<digest>/files`, outside the original project. The
digest binds source project identity and a 32-digit lowercase hexadecimal request. Destinations are
not accepted from the renderer. Canonical path aliases are resolved and checked against retained
directory identity; path spelling alone cannot bind allocation, retry or ancestry lookup.

An owner-private `intent.json` uses canonical `mesh.attachment-lane-intent/v1`, binding source project,
saved operation, request and allocated directory device/inode. It is written before file creation.
`ready.json` uses `mesh.attachment-lane-ready/v1`, binding the intent digest, registered child identity
and complete attachment receipt. The child history remains outside its working folder. Existing
attachment journals, version identities and registration receipts are unchanged. Older catalogue
readers ignore the new `work-lanes` directory and can still read registered child projects; they do
not understand this ancestry or retry operation.

Identical completed requests return the same exact registered lane, preserving edits made after the
first result. Reusing a request with a different version refuses. Parent history locking serializes
concurrent same-project retries. A missing/partial/tampered intent or ready record, missing history,
replaced root or linked destination refuses. Failures retain allocated material, never overwrite it,
remove it, silently allocate another folder or infer completion. A crash between child registration
and the ready record leaves a registered child whose ancestry is shown as unavailable. Allocation
reconciliation and explicit retention management are still required.

The renderer retains an unacknowledged request for explicit retry during the session. It cannot
choose credentials, supply file bytes, redirect paths or attribute changes. Successful allocations
appear in the existing native project catalogue. On restart their capture remains stopped until
explicit resume, just like other attachments. Missing child folders remain visible with unavailable
ancestry. Native folder opening uses the admitted macOS directory identity rather than renderer paths.

## Evidence and remaining work

Native tests create two lanes from one saved point, preserve a dirty Git index/HEAD and an open
source editor, verify binary bytes, empty directories and executable state, preserve later lane edits
across retries, serialize concurrent requests, recover ancestry, and refuse changed inputs, incomplete
records, unsupported budgets and replaced directories. A race regression that failed before the fix
replaces an empty copied directory with a regular file; the final inventory now checks both names
and entry types and refuses while retaining that concurrent work. Desktop host tests exercise independent
background capture, retained source sessions, retry, restart and offline rows. Coordinator tests
exercise exact selectors, lost acknowledgements, unchanged saved views and path-injection refusal.

This does not complete fleet orchestration. Managed provider dispatch, authenticated existing-session
correlation, shared project review/integration, allocation crash reconciliation, empty-project initial
history, four-worker performance measurements and revision-bound graphical journeys remain required.


## Managed fleet entry

`FleetService::create_root_from_attachment` is a separate native entry point for an explicitly
requested managed agent lane. It validates the retained attachment and exact saved operation,
records the source project in the fleet ledger, and allocates an additional folder through
`NativeLaneAllocator`. It does not convert the original project or ordinary attached lanes to
exclusive custody. Existing non-exclusive source capture and open editor descriptors remain valid.

The allocator checks that its pinned directory is outside the original project and protected roots
before writing. The shared bounded materializer streams verified saved content into a private
`source` staging directory. An import without original-folder writeback authority creates a managed
workspace. Before admitting it to a new independent daemon, the native service verifies the exact
historical content, current file tree and initial recovery state using the existing version-fork
predicates. Granting a worker still requires the normal fleet custody/session boundary.

The staging directory is retained; this first bridge does not automatically delete it or reconcile
partial allocations. Durable intent without a workspace binding and completed bindings without a
live native context require recovery. Retrying a completed request in the same host reuses its lane
and preserves later edits; changing project/version/goal/provider under that request refuses.
Descendants inherit source-project correlation, not source authorship or approval. Their managed
versions remain separate histories. Integration of results into the original project's main,
desktop scheduler hosting and live fleet controls remain unfinished.

Native tests allocate from an older attached version, preserve a continuing editor, binary content,
empty directories and executable state, delegate through the normal scoped agent surface, replay
project correlation, preserve edits across retries, and refuse source-overlapping destinations,
unknown versions, incomplete allocations and unreattached contexts after restart. These are native
source tests, not graphical or real-provider proof of this attachment entry point.
