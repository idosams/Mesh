# Workspace adapter contract 1

Contract 1 is an explicit opt-in extension of frozen
`mesh-workspace-adapter/0`. It adds disposition-aware rename and move operations
that return a separate `RenameBindingEvidence` object, and it removes the
unproduced `MetadataSettled` boundary reason from the new closed list.

The Rust entry points are `WorkspaceView::rename_with_evidence`,
`WorkspaceView::move_entry_with_evidence`, and
`WorkspaceAdapter::observe_durable_boundary_v1`. The legacy operations remain
the contract-0 create-or-fail surface; a backend claims `/1` only after it
implements the whole extension.

`RenameDisposition::Fail` preserves `/0` behavior. `Replace` atomically moves a
file over a destination file while holding the backend's mutation guard. It
refuses directory replacement. Returned evidence binds exact `ObjectId` and
`NormalizedName` bytes before and after the operation to `(view, sequence)`.
Moved-object identity and destination-binding replacement are independent.

Missing evidence is `Unsupported` with code
`rename-binding-evidence-unavailable` and a non-empty closed missing-member
list. No path snapshot, Unicode normalization, case folding, or wall clock may
fill a missing member.

The boundary list is exactly `Closed`, `Synced`, and `RenamedIntoPlace`:

- `Closed` is produced when the last modified handle is closed.
- `Synced` is produced by a successful fsync event.
- `RenamedIntoPlace` is produced only by an atomic replacement with complete,
  matching binding evidence.

Run the compatibility checks from the repository root:

```sh
node tests/compatibility/adapter/v1/verify.mjs
node tests/compatibility/adapter/v1/verify.mjs --mutations
```

`run_conformance_v1` checks the inherited contract-0 behavioral subset under
the exact `/1` declaration. The v1-specific Rust matrix is
`crates/mesh-materializer/tests/adapter_v1.rs` plus
`crates/mesh-fuse/tests/adapter_v1.rs`; neither green surface substitutes for
the other.
