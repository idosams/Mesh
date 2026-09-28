# Retained recovery for attached-file replacement

Status: native regular-file integration, restart inspection and retained-file restoration implemented;
desktop confirmation/recovery, additions/removals, grouped integration and packaged proof are unfinished.
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
configured directory. There is no automatic cleanup or migration. The native recovery reader
validates schema/field sets, rederives approved content from trusted
history, inspects current identities and content through retained authority, and reports uncertain
states without enabling mutations.
A receipt by itself is never deletion, overwrite, replay or approval authority.

## Evidence and remaining requirements

Native tests exercise late open-editor writes after successful return, concurrent replacement at the
exchange boundary, post-exchange edits, failed durability barriers, failed preparation receipts,
abandonment, replaced parents, changed stage/source, copied macOS attributes/ACLs, metadata races,
and exact approval/trust/exclusion revalidation. The domain test retains Git index/HEAD, unrelated
live files and the history journal while changing only the explicitly selected approved file.
Native restart inspection now classifies retained artifacts without replaying writes.

Still required: native confirmation and desktop recovery presentation, crash-process campaigns,
retention that does not discard live descriptors' work, grouped additions/deletions and dependency
handling, integration with native confirmation and desktop rendering, external-volume UX, Linux
metadata preservation, and revision-bound packaged graphical proof. No general integration or
packaged user-presence claim follows from these native tests.

## Canonical transfer accounting

The native replacement foundation transfers source `38d0a9386e3fef2c3670ff371533e204e1bc76e3`.
The metadata copier and its allocation-identity regression from later source
`633af5ca9d81e6c71532b24332fe4310dac0899d` move into this prerequisite so the canonical branch
does not introduce the known creation-time transplant defect. No recovery or restoration modules
from the intervening source commits were included in that foundation. Existing receipt formats are retained.
Read-only inspection now transfers source `7d06b8bd68d77ac9c358bf9002c829cd02995fa0` on #36;
restoration follows as a separate increment from source
`11df227f749aa8654fe89a0612e8f2b286c29b4a` on published #38, preserving the earlier allocation fix.

## Restart inspection

`ProvisionedAttachment::inspect_integration_recovery` accepts a native-configured recovery root
and optional exact transaction identifier. It returns `mesh.attachment-integration-recovery/v1`.
A bounded directory prefix lists at most 32 entries (or the smaller caller entry limit), with an
explicit `more` flag rather than a fabricated total. Prefix order is filesystem enumeration order,
sorted within the returned prefix; it is not pagination. Direct lookup remains available for every
valid transaction identity. Unknown, incomplete, linked or unavailable directories stay visible as
problems; one bad receipt does not hide neighboring entries. The complete-inventory reader retains
its overflow refusal and independent cursor behavior.

A prepared receipt must have the exact canonical field sequence and schema, admitted attachment
and history-store identity, retained transaction-directory identity, canonical relative path and
file installation tokens, valid file modes and the original history exclusion fingerprint. Its
bundle/target/head must resolve to a head admitted by the trusted approval fold, with an independently
verified human receipt and exact native review context. The saved base and result are rederived
through verified historical content. Advancing main does not invalidate older accepted recovery
evidence; removing trust or changing a claimed content digest yields `unverified-history`.

The reader observes source and retained files through pinned descriptors, without following links
or blocking on FIFOs. Hashing uses a fixed buffer, per-file and aggregate live-content limits;
failed reads conservatively consume their allowance. Identity, mode, length and change-time checks
refuse observed races. Native metadata is inspected under the existing separate 1 MiB bound.
Historical CAS verification still follows the history engine and is separate from the reported
`live_content_budget_remaining`; this is not a measured fleet-performance claim.

Results distinguish `prepared-arrangement`, `applied-arrangement`, `changed-files`, identity
mismatches and incomplete observations. These describe current observations, not proof that an
execution happened or never happened. Missing outcome records cannot imply that apply was never
attempted. Invalid or contradictory outcomes are explicit. An applied arrangement without a valid
success observation requires attention, as do changed or uncertain files. All results explicitly
grant no write, cleanup or replay authority and make no atomic or final-observation claim. Current
exclusions are not checked for application readiness: inspection compares explicitly selected
historical recovery evidence and never authorizes applying it.

Tests reopen history after preparation/application, remove an outcome to model lost acknowledgement,
retain late editor writes after main advances, reject tampered/unknown/untrusted receipts, refuse
same-content replacement identities, bound oversized catalogues and live reads, and preserve linked
or special entries. The missing-outcome test is fault injection, not a killed-process campaign.
The current entry point still requires the admitted source root to be available; offline/renamed
source recovery, automatic catalogue discovery and graphical recovery actions remain unfinished.

## Explicit retained-file restoration

`prepare_retained_restoration` creates a new single-use native proposal to restore a frozen snapshot
of an exact displaced file to its original project-relative path. A prior apply is never exchanged
back or replayed. Preparation verifies its origin receipt and trusted approval ancestry, requires the
origin's `exchange` to retain the original source installation, captures the current project under
its unchanged exclusions, and reads retained bytes within the remaining capture budget. An unused
staged proposal is not a displaced source and cannot be restored through this operation.

The native proposal exposes frozen before/after bytes and exact metadata facts for a future native
confirmation. No source mutation occurs during preparation. Applying after explicit confirmation
revalidates origin ancestry, retained content/identity/metadata, current exclusions, staged receipt
and exact current source. A change to either input while confirmation is pending refuses before
exchange. Changes racing the final exchange still preserve both named inodes and report uncertainty.
The original retained inode remains untouched, so late writes after restoration remain available.
The displaced current working inode is retained in the new transaction. Restoring that transaction's
retained file implements undo as another explicit operation, including any later editor writes.

Restoration copies the selected retained file's native metadata and permissions, while retaining the
current file with its own metadata. The replacement primitive therefore separately binds current
and proposed metadata digests; ordinary approved-file integration continues to require matching
metadata except for the approved executable state. Special permission bits and unsupported metadata
still refuse. No restoration appends approval/history records or advances main; background capture
may later save the restored private work through the ordinary observation path.

New directories use `restoration-<32 lowercase hex digits>`. Their canonical prepared receipt is
`mesh.attachment-file-restoration/v1`: the integration receipt's ordered fields followed by
`installed_metadata_digest`, `origin_transaction`, `origin_proposal_digest`, `origin_file`,
`origin_digest`, `origin_mode`, and `origin_metadata_digest`. The origin fields bind the selected
retained snapshot and parent receipt. `head`, `bundle` and `target` identify approval ancestry, not
approval of restored content. The source fields describe current work to preserve; proposed content
may contain later private edits absent from the accepted version. The result schema is
`mesh.attachment-file-restoration-result/v1`, with the existing proposal-digest/observation/retention
fields. Both receipts retain `automatic_replay:false` semantics. Older readers do not recognize
restoration directory names or receipt schemas and cannot replay or delete them.

The restart inspector validates each restoration's exact field sequence, snapshot binding, parent
receipt digest, original path and approval ancestry. Chains are bounded to 16 records including the
initial integration; preparation reserves a level for the new transaction and refuses an overflow
before allocating it. Missing, tampered or untrusted ancestry remains unverified. Inspection labels
restoration as `restore-retained` and `content_is_approved_main:false`; the original approved head
cannot promote private restored bytes implicitly. Existing integration receipts are unchanged.

Tests preserve two simultaneously open editor streams, restore late retained edits, retain newer
current work, reopen and classify restoration records, undo through a new transaction, preserve
separate native metadata, reject changes during confirmation, refuse abandoned stages and damaged
ancestry, and enforce the ancestry bound without dropping prior work. These native tests do not
prove an actual human confirmation or packaged recovery UI. Offline/renamed roots, restoring a
currently absent path, grouped directory changes, long-chain consolidation, remote recovery and
safe explicit retention management still require further work.
