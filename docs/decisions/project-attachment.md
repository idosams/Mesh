# Existing-project attachment identity

Status: native registration, bounded observation and immutable capture inputs implemented;
durable version commits remain planned.

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

Registration grants no write-back, restore, approval, provider session or exclusive custody. Status
explicitly reports `observation: not-started` and no saved version. Existing managed-workspace custody
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
scans cannot inherit EOF from an earlier scan. No captured file bytes or observations are persisted.
Even a complete observation is explicitly not an atomic snapshot or a saved version. Registration
status remains separate from this one-shot report and does not imply a continuously running watcher.

`capture_inputs` now uses the same confined traversal to retain exact file bytes, executable bits,
empty directories, source directory identity and the exclusion rules used. It refuses the entire
input if any traversal or recheck is incomplete. Opaque native input types expose immutable byte
slices to a future version writer; subsequent source edits do not replace captured content. Debug
and observation JSON exclude raw content. An exclusion-policy fingerprint lets a writer distinguish
policy changes from missing files before deciding how to update an existing saved tree. Attribution
remains unknown. These inputs are held in memory and are not saved-version acknowledgments.

The durable writer must use the existing operation, journal and content-store contracts with an
external attachment store. It must consume captured bytes, not reopen the original files after
admission. Existing managed-workspace initialization and mutation recovery can act on their working
folder; they must not be applied to the user's original project as an attachment shortcut. Preserve
the original as read-only input to capture, and bind version history separately to native attachment
identity. Do not introduce a second approval or main-version protocol for attached projects.

Implement incremental observation plus reconciliation
after gaps, immutable capture under concurrent writes, and evidence-based file/session correlation.
Persist captured content and history outside the project. Present registration, catch-up, incomplete
capture and saved versions separately. Then integrate the attachment with desktop onboarding and
parallel review. Registration tests alone do not satisfy the full existing-project acceptance journey.
