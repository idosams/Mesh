# Mesh operation contract

This public compatibility artifact pins the durable operation vocabulary implemented by
`mesh-operations`. It is intentionally narrower than the internal engineering execution plan that
informed the first implementation. Tests read this file so that changing, removing, or reordering
an operation is a visible protocol decision rather than an accidental code edit.

## 4.3 Operations

Initial operation vocabulary:

```text
CreateFile
CreateDirectory
WriteFileVersion
LinkDirectoryEntry
UnlinkDirectoryEntry
RenameEntry
MoveEntry
DeleteObject
RestoreObject
SetPortableMetadata
ResolveNameConflict
ResolveContentConflict
AdvanceActorHead
RecordReadObservation
RecordDerivedNode
CreateReviewBundle
RecordValidation
AdvanceCanonicalHead
InitializeWorkspace
```

Raw `write()` calls are not distributed as permanent user-visible operations. They are accumulated
locally and coalesced into durable file versions.

The canonical encodings, schemas, and vectors live under `protocol/`. Any vocabulary change must
update those artifacts and their compatibility tests in the same pull request.

### Explicit empty workspace root

`InitializeWorkspace { root_id }` uses the additive canonical record
`["mesh.v0.op.initialize-workspace", bytes16(root_id)]`. It declares immutable root identity,
including when the saved tree has no entries. It does not clear state, mint another directory,
change heads or grant publication authority. Replay of the same declaration preserves state;
a different root is rejected. Root inference retains every declaration even if a directory
creation names the same ID; conflicting declared or inferred roots remain ambiguous.

Existing records and their identifiers are unchanged. Readers without this operation reject its
unknown domain; they must upgrade before opening histories containing it. No existing journal is
rewritten. Legacy empty operation sets still cannot identify a root. Native worker initialization
will adopt this record in a separate increment; this protocol addition alone does not enable it.
