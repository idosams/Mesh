# Retained recovery for attached-file replacement

Status: native regular-file transaction implemented; desktop confirmation, recovery browser,
restart classification, additions/removals, grouped integration and packaged proof are unfinished.
This is a foundation for Phase 1, not completion of source integration.

## Decision and reason

Attached projects remain writable by ordinary tools. An editor may hold a file descriptor after
Mesh atomically replaces the corresponding pathname. Deleting the displaced inode after checking
its bytes can lose subsequent writes through that descriptor. The existing managed-copy replacement
helper's cleanup therefore cannot serve as the non-exclusive attachment write-back transaction.

A trusted native caller separately prepares and applies one regular-file replacement from the
current, verified accepted main review. Preparation checks its exact base against freshly captured
source bytes and executable state, retains exact parent/file identity, and stages approved bytes
in a private external recovery directory. It does not write the source, advance main or change Git.
The native host must obtain explicit confirmation of this proposal before calling apply. No agent,
MCP, CLI or renderer command exposes apply in this slice. Approval of main alone does not authorize
source write-back. The in-process proposal is not serializable or cloneable.

The recovery root is native-configured, outside project content, owner-private and on the source
filesystem. A host can select a suitable private root for an external volume instead of requiring
all projects to use the metadata store's volume. This slice does not implement that picker or a
cross-filesystem fallback. It must never fall back to an unlink-and-copy sequence.

Each random transaction directory contains `exchange`, staged bytes that become the displaced
source inode after a single descriptor-relative atomic exchange. Stage and directory durability
precede the create-new preparation receipt. Apply revalidates trusted main, exclusion policy,
source/parent identity, bytes, permissions, metadata, stage and receipt. It then exchanges names,
attempts both directory durability barriers, and checks the resulting content and identities.

Neither successful application nor failed verification automatically removes either entry or
exchanges them back. Rollback can itself race with another editor replacement. A detected race or
failed barrier returns `reconciliation-required`; observed success returns `applied-observed`.
Both outcomes retain the displaced inode. An error or lost reply after exchange is uncertain, not
proof that source was untouched. Late writes after return remain in `exchange`; observed success
is explicitly not a final observation. Application is one file, not an atomic project transaction.

macOS copies ACLs and extended attributes through descriptors and applies ownership and flags
explicitly. Broad `COPYFILE_STAT` copying is excluded because it transplants the source creation
timestamp and weakens destination allocation identity. Preparation verifies that metadata copying
preserves the new file identity. Portable executable state comes from approved content. A bounded
metadata digest checks ownership, flags, sorted attribute names/values and ACL text before and
after exchange. Attribute inspection is limited to 256 attributes and 1 MiB total evidence. Special
permission bits refuse. Linux currently refuses extended attributes (including ACLs) and ownership
changes instead of dropping them; broader Linux metadata handling remains required. Source time
metadata remains on the retained inode, but timestamps are not version content or concurrency
proof. Changed file incarnation on platforms without birth-time support can conservatively refuse.

## Durable format and compatibility

`prepared.json` uses canonical `mesh.attachment-file-integration/v1` JSON. It binds project and
attachment identity, external history store identity, approved head/bundle/target, relative path,
source parent/file installation, source digest/mode, proposed file installation/digest/mode,
native metadata digest, exclusion fingerprint and recovery-directory identity. `automatic_replay`
is false. Native metadata hashes are platform-local evidence, not portable content identities.

`observed.json`, when durably written, uses `mesh.attachment-file-integration-result/v1`. It binds
the encoded proposal digest, observation status, retention statement and `observation_final:false`.
It records what was observed at that attempt, never authorizes another exchange and never proves
that late editor writes have ceased. Unknown formats, incomplete receipts, an absent outcome,
changed identities or later writes need explicit reconciliation. No restart path replays this
transaction. Abandoned preparations also remain, with source unchanged.

These are new external recovery records; existing attachment receipts, saved versions, journals,
review signatures and approval formats are unchanged. Older binaries do not process this separately
configured directory. There is no automatic cleanup or migration. A future recovery reader must
validate schema/field sets, rederive approved content from trusted history, inspect current identities
and content through retained authority, and display uncertain states before any explicit recovery.
A receipt by itself is never deletion, overwrite, replay or approval authority.

## Evidence and remaining requirements

Native tests exercise late open-editor writes after successful return, concurrent replacement at the
exchange boundary, post-exchange edits, failed durability barriers, failed preparation receipts,
abandonment, replaced parents, changed stage/source, copied macOS attributes/ACLs, metadata races,
and exact approval/trust/exclusion revalidation. The domain test retains Git index/HEAD, unrelated
live files and the history journal while changing only the explicitly selected approved file.
Restart classification and recovery inspection remain dependent increments.

Still required: a recovery catalog/classifier and human recovery actions, crash-process campaigns,
retention that does not discard live descriptors' work, grouped additions/deletions and dependency
handling, integration with native confirmation and desktop rendering, external-volume UX, Linux
metadata preservation, and revision-bound packaged graphical proof. No general integration or
packaged user-presence claim follows from these native tests.

## Canonical transfer accounting

The native replacement foundation transfers source `38d0a9386e3fef2c3670ff371533e204e1bc76e3`.
The metadata copier and its allocation-identity regression from later source
`633af5ca9d81e6c71532b24332fe4310dac0899d` move into this prerequisite so the canonical branch
does not introduce the known creation-time transplant defect. No recovery or restoration modules
from the intervening source commits are included. Existing receipt formats are retained.
