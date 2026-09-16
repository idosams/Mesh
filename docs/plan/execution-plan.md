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
```

Raw `write()` calls are not distributed as permanent user-visible operations. They are accumulated
locally and coalesced into durable file versions.

The canonical encodings, schemas, and vectors live under `protocol/`. Any vocabulary change must
update those artifacts and their compatibility tests in the same pull request.
