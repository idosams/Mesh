# Retained recovery for attached-file integration

Status: native regular-file addition/replacement/removal groups, restart inspection and retained-file
restoration into existing or absent regular-file paths implemented; desktop source connects
single-file and complete regular-file group confirmation and recovery inspection. Directory changes
and packaged graphical proof are unfinished.
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
The native host must obtain explicit confirmation of this proposal before calling apply. No agent, MCP or CLI tool exposes apply. Desktop commands prepare a fresh native proposal and
require a complete native confirmation before invoking apply. Approval of main alone does not authorize
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

Still required: packaged native confirmation and desktop recovery proof, crash-process campaigns,
retention that does not discard live descriptors' work, grouped directory changes and dependency
handling, external-volume UX, Linux
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

The native proposal exposes frozen before/after bytes and exact metadata facts for native
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

## Desktop connection

The native host now uses `file-recovery` within the exact external project metadata store as its
configured default. Allocation is descriptor-relative, owner-private and symlink-refusing. Read-only
inspection never creates a missing directory. Same-volume exchange remains mandatory; an attached
project on a different volume can still be captured/reviewed, but this default cannot apply files
there. A native external-volume location selector remains unfinished.

Desktop source exposes bounded recovery inspection and exact transaction lookup, plus explicit
single-file apply and restore requests. The renderer supplies project/review/file selectors or an
exact recovery transaction, never recovery locations, receipts or replacement bytes. Native code
retains the prepared transaction across a complete text confirmation, including project location,
relative path, byte digests, permissions and frozen before/after text. Text is quoted with escaped
control characters. Binary, NUL-containing or oversized confirmation content refuses rather than
being omitted; the entire prompt is limited to 48 KiB. Cancelled or refused preparations remain as
recovery records and do not authorize replay.

Applying requires an existing regular file matching the exact accepted base; it is not a grouped
integration. Restoration uses its original displaced inode and a fresh frozen snapshot, including
later editor work. The host refuses detached projects and revalidates its native session generation
under the control lock after confirmation. Detach/reattach or resume during the dialog invalidates
the operation. The underlying transaction rechecks content, identities, policy and approval trust.
Result errors remain uncertain; the UI refreshes recovery without automatic retries or claims of
rollback. Exact resulting transactions remain inspectable beyond the bounded overview. Inspection
views remain fixed while capture runs and display stale observations explicitly after refresh errors.

Native and renderer tests cover managed allocation, source/store replacement refusal, session
invalidation, complete bounded confirmation, selector binding and uncertain replies. These are
source tests; an actual packaged human-confirmation/recovery journey remains required.

## Canonical desktop transfer adaptations

Source `1c332ba96d3a74c33701143ef54003f35895e1e3` is transferred on canonical PR #42. The
localized English/Hebrew view and literal bidirectional path/transaction rendering remain intact.
The native formatter receives project identity and folder separately and quotes both, alongside
complete quoted content and relative file name. Native confirmation text remains English; actual
rendered-dialog completeness is not established by string construction tests.

Recovery observations serialize the full POSIX regular-file mode (`st_mode`), including file type,
not permission bits alone. The older renderer predicate rejected every such native file record.
The canonical parser accepts only regular-file modes from `0100000` through `0107777`; directory,
link, permission-only and malformed values refuse. Permission changes in an observed retained file
do not grant mutation authority: preparation/application still revalidate identities, metadata and
supported modes. A real temporary file's metadata fails the older parser and passes the correction.
The native receipt/observation format itself is unchanged.

## Accepted-review replacement groups

The native `prepare_main_integration` operation now selects all changed paths in one accepted review.
The initial executor supported regular-file replacements; the removal extension is described below.
An unsupported directory change,
conflict, exclusion, unavailable member or group budget overflow refuses the complete group. It never
silently reduces the accepted result to a supported subset. Up to 64 changed files and the configured
aggregate byte budget are admitted. Already-present accepted files are recorded and not rewritten;
unrelated current work is preserved. Directory executors and desktop group confirmation
remain unfinished, so this does not complete general integration.

Preparation first derives a complete plan from trusted accepted history and a fresh bounded source
capture, then stages each replacement under one private `integration-group-<32 lowercase hex>`
directory on the source filesystem. Each member retains its existing single-file preparation receipt.
A create-new `group-prepared.json` (`mesh.attachment-integration-group/v1`) binds project/attachment,
accepted head/bundle/target, exclusion fingerprint, group directory identity, ordered member paths,
transaction identities and proposal digests, plus already-present paths. It explicitly records
`automatic_replay:false` and `filesystem_atomic:false`. No source write occurs during preparation.

The single-use native group exposes every frozen member's complete before/after bytes for a future
native confirmation. Apply holds the verified history lock, validates the group and every member
before the first exchange, then revalidates each member at its own boundary. Before each attempt it
writes `attempt-NNNN.json` (`mesh.attachment-integration-group-attempt/v1`) binding group digest,
index and member transaction. A failed attempt receipt prevents that exchange. Any failed or uncertain
member stops the group; subsequent members remain staged and unattempted. Prior exchanges are never
rolled back automatically. Late writes to displaced inodes remain retained.

`group-observed.json` (`mesh.attachment-integration-group-result/v1`) records ordered member outcomes
and a final non-atomic observation. A fresh full-plan check must find all accepted changes present
before reporting `applied-observed`; otherwise the result is `reconciliation-required`. An absent
outcome or lost acknowledgment never authorizes retry. All outcomes state `observation_final:false`.
These new external group records leave existing file receipts, history journals and approval formats
unchanged. Older readers cannot process group directories and must not replay them.

`inspect_main_integration_group` accepts a native-configured recovery root and exact group identity.
It validates the closed group format, attachment/directory identities, member proposal digests and
trusted approval evidence. It rederives full accepted-review path coverage, then reads each member
through the existing native recovery inspector with a shared content budget. Current observations
are explicitly non-atomic, confer no write authority and never replay operations. Already-present
paths are labeled as preparation evidence. Copied, missing, tampered or incomplete groups refuse.

Native tests cover whole-group preflight, retained late editor writes, already-present inode
preservation, refusal of mixed unsupported changes before staging, restart inspection and altered
membership. A deterministic mid-apply test changes the second of three files after the first exchange:
apply stops, retains the completed member and every stage, preserves the new user content and leaves
the final member untouched. Restart inspection does not replay the partial group. Desktop confirmation,
directory support and packaged graphical proof remain required for the full journey.


The original group and capture-reuse source commits report 3,252 and 3,254 Rust tests respectively
(14 skipped), and 666 desktop tests. Those historical results do not validate these canonical
transfers. Local native execution remains queued behind the preserved full run.

### Bounded full-project captures per group

Group staging now shares one complete captured project input while retaining the verified history
lock. The internal staging helper verifies that input's project/root identity and exclusion digest,
then reopens and checks each selected file before preparing its retained replacement. Every staged
file still binds its exact source/parent identity, bytes, mode, metadata and recovery receipt.

Member validation rereads the bounded root ignore files twice and rechecks attachment identity;
changed or unstable policy refuses. It verifies that fingerprint against the stored history policy
and original proposal. Selected source/stage/metadata checks remain at every individual exchange.
Whole-project capture brackets group preparation and application; standalone single-file apply keeps
its existing complete project preflight. The capture fingerprint encoding and all durable receipts
are unchanged.

A regression test integrates both 2-file and 24-file groups alongside an unrelated 1 MiB file,
asserting at most three full captures for preparation and two for application in either case.
A separate test changes ignore rules after the first member and confirms the next member is untouched.
When executed, these assertions check bounded scan count independent of group size, not a wall-clock speedup or
large-project latency target. Historical-state reconstruction and per-file verification still cost
work proportional to the group; broader performance measurement remains required.


Canonical focused, failing-before and full native verification remains pending. No packaged
graphical or wall-clock performance claim is made.

## Retained regular-file removals in accepted groups

The native group executor now admits regular-file removal alongside replacement. The complete
accepted review must still contain only supported changes; additions and directory changes refuse
before staging. An already-absent removed path counts as present only after successfully opening its
confined parent and observing leaf absence twice under the same parent identity. Missing parents,
symlinks, inaccessible entries and failed reads never establish absence. This remains a bounded live
observation, not an atomic project snapshot.

Each removal retains an `integration-<32 lowercase hex>` transaction with an explicit new
`mesh.attachment-file-removal/v1` preparation schema. It uses the existing ordered preparation keys,
with `installed_file`, `installed_digest` and `installed_mode` set to JSON null. All source, parent,
metadata, approval, exclusion and storage identities remain mandatory. Verified history must contain
the exact source file in the accepted base and no entry at that path in the accepted result. Older
readers reject the unknown schema or absent installed facts; existing replacement and restoration
schemas are unchanged. The result schema is `mesh.attachment-file-removal-result/v1`, with the same
proposal digest, status, retention and nonfinal-observation fields as replacement outcomes.

Preparation does not alter source files or create a replacement stage. Native apply rechecks source
identity, content, mode, metadata and the vacant private recovery destination, then performs a
same-volume descriptor-relative no-replace rename into `exchange`. Both source and recovery directory
barriers are attempted. The retained file and the absent original name under the exact source parent
must match before reporting `applied-observed`. A writer can replace the source between verification
and rename; any unexpected displaced inode is retained and reported as requiring reconciliation.
There is no automatic unlink, rollback, retry or cleanup. An editor can keep writing its open handle
into the retained file after success; a recreated source path remains independent user work.

Recovery recognizes explicit removal receipts and separately reports confirmed source absence.
It does not infer absence from a null content observation. Missing outcomes remain uncertain;
changed retained bytes, recreated files, insufficient read budgets and replaced parents do not
become successful deletion evidence. Group restart inspection keeps its complete membership and
approval verification. The single-file preparation API remains replacement-only so its existing
desktop confirmation cannot silently change meaning. Future group confirmation can distinguish
removal from an empty replacement through `removes_path()` and the explicit receipt schema.

Native tests cover mixed accepted groups, already-absent paths, late editor writes, changed or
substituted sources, occupied recovery destinations, failed durability barriers, missing parents,
receipt tampering, lost outcomes and stopping later members after a concurrent edit. Restoring into
an absent source path, additions, directory changes, desktop group confirmation/recovery and packaged
graphical proof remain unfinished.

The original removal source reports 3,264 Rust tests (14 skipped) and 666 desktop tests. These
historical results do not validate the canonical transfer. Local native focused, failing-before and
full verification remains queued behind the preserved run. Packaged graphical confirmation and
platform-backed human-presence acceptance remain outstanding.

## Regular-file additions in accepted groups

An accepted review can now add a regular file beneath an existing confined directory. The first
approved main uses an empty saved base, so it can supply additions without inventing a prior version.
Groups still reject directory changes as a whole; missing parents are not created implicitly. The
prospective path and every ancestor must pass the exact captured ignore policy and structural Git
boundary. This closes the gap where an absent ignored path would have no captured file to check.
An already-present file with the accepted bytes and executable state is recorded without rewriting
its inode. A different current file refuses the group before staging.

Preparation creates the complete approved content as `exchange` in a private external transaction.
The initial implementation used fixed creation modes, 0644 or 0755 according to approved executable
state, and bound actual staged ownership/attributes/ACL metadata. The destination-permission
extension below replaces those fixed modes and staging-folder inheritance for new preparations. Stage and directory are flushed before the preparation receipt. Failure or abandonment
leaves the stage for inspection and never writes the attached project.

`mesh.attachment-file-addition/v1` uses the existing ordered preparation keys. `source_file`,
`source_digest`, `source_mode` and `source_executable` are JSON null. `source_parent` is the exact
observed existing parent; installed identity, digest, mode and native metadata bind the staged file.
Verified approval history must have no entry at the path in the saved base and the exact regular
file in the accepted result. `mesh.attachment-file-addition-result/v1` has the existing result fields
with `displaced_file_retained:false`, because no file was displaced. Unknown schemas and null source
facts make older readers refuse rather than reinterpret an addition as a replacement. No existing
receipt, journal, approval or saved-version encoding changes.

Apply revalidates current accepted main, project/exclusion policy, the exact absent destination,
parent identity, stage identity/content/mode/metadata and prepared receipt. One descriptor-relative
no-replace rename moves the stage into the working folder. Both directory barriers are attempted;
post-install inspection must verify the installed file under the exact parent and the absent stage
before reporting `applied-observed`. A collision leaves the user file and approved stage untouched.
A post-install edit, substituted namespace or failed durability barrier needs reconciliation; apply
never unlinks, rolls back or retries automatically. Later group members stop on uncertainty.

Read-only recovery separately observes source and stage absence. It recognizes prepared and applied
arrangements, concurrent collisions, later edits, malformed receipts, lost outcomes and unavailable
content without gaining write authority. The existing restoration action does not turn an addition
receipt into permission to delete the newly added file. Single-file preparation remains replacement-
only so its desktop confirmation does not silently gain new behavior. Group native confirmation can
use `adds_path()` to distinguish creation from replacing an empty file.

Native tests cover mixed add/replace/remove groups, first-main additions, already-present inode
preservation, executable state, capture exclusions, source collisions before and at rename,
parent/root replacement, stage changes, failed preparation receipts and directory barriers,
partial-group stopping, lost outcomes, receipt tampering and restart inspection. Directory changes,
restoration into absent paths and the complete grouped desktop/packaged graphical journey remain
unfinished. Destination-permission inheritance is addressed by the extension below.

The original addition source reports 3,273 Rust tests (14 skipped) and 666 desktop tests.
Those historical results do not validate this canonical transfer. Local focused, failing-before
and full native verification is queued behind the preserved run. Grouped packaged graphical and
platform-backed human-presence acceptance remains outstanding.

## Destination permissions for newly added files

New macOS preparations now configure staged additions using the actual destination parent's group
and inheritable extended ACL. They clear the staging folder's inherited file ACL, preserve each
applicable destination entry's principal, allow/deny rights and ordering, and apply native file
inheritance flags. This uses descriptor-based native calls without creating a permission probe in
the attached project. Native tests compare the result with files created directly by the kernel
across allow/deny, file-only/directory-only, inheritance-control and mixed ACL cases. Separate tests
compare destination group ownership. The behavior follows Apple's
[filesystem permission model](https://developer.apple.com/library/archive/documentation/FileManagement/Conceptual/FileSystemProgrammingGuide/FileSystemDetails/FileSystemDetails.html)
and [native ACL inheritance implementation](https://raw.githubusercontent.com/apple-oss-distributions/xnu/main/bsd/kern/kern_authorization.c).

The initial private file creation requests 0644 or 0755 and lets the kernel apply the process umask.
The resulting mode is retained while destination ownership and ACLs are configured. Production code
never reads or changes the process-global umask. An isolated single-test child verifies restrictive
077 behavior for both executable and non-executable additions. A mask that removes every requested
execute bit refuses an executable addition instead of changing the approved executable state.

New receipts use `mesh.attachment-file-addition/v2`, appending `parent_metadata_digest` and
`parent_mode` to the existing ordered addition keys. Results use the corresponding addition-result
v2 schema. Installed modes may reflect the process umask; trusted approval still verifies exact
content and executable state. Existing v1 receipts remain readable with their original fixed-mode
validation and without invented parent-policy evidence. Replacement, removal, approval and saved
version encodings are unchanged. Older readers refuse the unknown v2 schema.

Preparation binds the parent identity, mode and bounded native metadata digest. Apply rechecks the
policy before rename and after installation. A pre-apply policy change refuses without source writes;
a change at the rename boundary produces reconciliation evidence without undoing the installed file.
Recovery reports `parent_policy_matches` as true, false or unavailable and requires attention for
false/unavailable results while preserving the observed file arrangement. It never replays a write.
Native tests cover policy changes, malformed policy receipts and legacy-v1 read-only recovery.

Linux uses its setgid-parent or effective-process group rule, but the existing native metadata
boundary still refuses extended attributes/default ACLs. The historical source was exercised on macOS;
it does not establish Linux runtime proof or complete portable ACL inheritance. Directory operations,
restoration into absent paths and the grouped desktop/packaged graphical journey remain unfinished.

The original source reports 3,279 Rust tests (14 skipped) and 666 desktop tests, including six
new native/persistence regressions. It also records a lingering-handle diagnostic for an unrelated
CAS test and a passing isolated rerun. These are historical source reports, not validation of the
canonical transfer. Local focused, failing-before and full native execution remains queued behind
the preserved run; packaged graphical and human-presence acceptance remains outstanding.

## Restoring retained work into an absent path

`prepare_retained_restoration` now accepts a genuinely absent leaf under an existing confined parent.
It checks the prospective path against captured exclusions and the structural Git boundary, and
requires explicit native absence evidence. An unreadable path, symlink, directory, missing parent
or substituted parent is never treated as absence. This covers retained files from approved removals
and retained replacements whose installed working path was subsequently removed by an ordinary tool.

The native proposal exposes `adds_path()` so callers distinguish file creation from replacement of
an empty file. It stages a new copy of the exact bounded retained snapshot, including the selected
retained permissions, ownership, extended attributes and ACL supported by the native adapter. Unlike
an ordinary approved addition, restoration preserves the selected file's metadata instead of
synthesizing destination inheritance or masking the saved mode again. Unsupported special bits or
metadata refuse. The origin inode stays named in its existing recovery transaction, including late
writes through already-open editor handles.

After explicit confirmation, the origin snapshot, receipt, approval ancestry, exclusions, staged
content and metadata, exact parent identity/policy and absent leaf are checked again. A no-replace
rename installs the copy. A concurrent destination creation is preserved and refuses the operation;
post-rename uncertainty requires inspection and never causes automatic undo, unlink or retry.
Original retained files are never consumed. No new displaced file is claimed when creating a path.
A restoration that displaced nothing cannot authorize deleting its installed file as an implicit undo.

The new canonical schema is `mesh.attachment-file-restoration-addition/v1`. Its ordered fields are
the existing base keys, the existing restoration snapshot/ancestry keys, then
`parent_metadata_digest` and `parent_mode`. Source file, digest, mode and executable fields are null.
The native and installed metadata digests both bind the staged retained snapshot. The corresponding
`mesh.attachment-file-restoration-addition-result/v1` records `displaced_file_retained:false`.
Existing replacement-restoration and addition/removal schemas remain unchanged and readable.
Older readers refuse the new schema; no durable history, approval or journal migration is needed.

Read-only restart inspection verifies the bounded ancestry back to an accepted integration, reports
prepared/applied/changed/uncertain arrangements, and surfaces parent-policy mismatch separately.
Restored content remains `content_is_approved_main:false` even when its ancestry identifies an
accepted head. Missing outcomes never trigger replay. Malformed null-source facts, snapshot bindings,
parent policy and retention claims are rejected. An absent parent yields incomplete observation.

The existing desktop native confirmation now explicitly says that the destination is absent, shows
the complete bounded proposed text and permissions, explains creation without replacement, and
rejects contradictory source facts. Binary or oversized text remains unsupported by that dialog.
The renderer still supplies only an attachment and recovery reference. Native/source tests cover
both removal and replacement origins, metadata and late-editor retention, concurrent files/symlinks,
changed inputs/trust/policy, missing parents, budgets, malformed receipts, lost outcomes and exact
confirmation wording. Packaged graphical confirmation remains unverified; directory restoration and
the complete grouped desktop workflow remain unfinished.

The original source reports 3,284 Rust tests (14 skipped), 666 desktop tests and 28 isolated
restoration tests, including the four new integration cases and native confirmation case. These
historical results do not validate this canonical transfer. Local focused, failing-before and full
native execution remains queued behind the preserved run; graphical acceptance remains outstanding.


Native desktop commands now support complete regular-file group confirmation, verified group/member
recovery inspection and explicit retained-member restoration. Every changed file must fit the native
preview; project/folder labels remain literal. The host rechecks the project generation after consent.
This is the native host increment only: renderer controls, current native execution and packaged
graphical proof remain pending. Canonical monitoring lifecycle behavior is preserved; the unresolved
local FSEvents registration failure is not resolved by these commands.

The renderer now routes group application through the complete native confirmation and presents
partial member outcomes without describing them as atomic. Verified group/member lookup and explicit
restoration use native identifiers only; failed refreshes preserve observations while disabling
restoration. Already-present members remain preparation evidence. Hebrew labels never translate
paths or transaction identities. These controls do not authorize replay, cleanup or main advancement;
current packaged graphical acceptance remains required.


## Desktop confirmation and recovery for regular-file groups

The desktop now connects native `prepare_main_integration` and its single-use apply handle to one
explicit native confirmation. The renderer supplies only attachment, accepted bundle and target
identities. Native preparation rederives the complete accepted review and rejects unsupported
directory changes, divergent inputs or incomplete observations before offering the dialog. The
existing attachment generation check invalidates confirmation after detach/reattach.

The dialog lists every create, replace and remove operation with the complete frozen before/after
text, digests and permissions. Already-present paths are listed separately and are not rewritten.
The total native prompt is bounded to 48 KiB; binary, incomplete and oversized presentations refuse
without omitting any member. Prepared records remain discoverable even after refusal or cancellation.
The dialog states that application is sequential and non-atomic, can stop after partial progress, and
never automatically rolls back or retries. Existing native retained-inode and no-replace executors
remain the source-write authority.

The result view retains per-member observed, uncertain and not-attempted outcomes. It clears the
old application comparison after an attempt and requires a new comparison for another apply intent.
Refresh, recovery inspection and lost-response handling never dispatch another apply. A verified
outcome remains visible if later inspection fails; stale recovery views disable restoration until
refreshed. All displayed outcomes remain observations, not immutable filesystem state.

Bounded native recovery catalogues now emit `group-reference` records for canonical group names.
Discovery alone does not verify a group. Selecting a reference invokes native complete-membership,
trusted-history, receipt and bounded member inspection. The UI distinguishes historical
already-present evidence from fresh member observations. Exact lookup also accepts group references;
malformed names never become renderer-supplied paths. Group-file lookup/restoration resolves only
beneath a natively verified recovery group. Restorations create independent retained transactions
inside that group and can be inspected by their returned reference without replaying the group.
Group inspection also discovers bounded restoration references after restart, with an explicit
possible-omission flag and an exact file-reference lookup within the selected group. These names
are discovery only; selecting one independently verifies its receipt and approval ancestry.

Native recovery additionally emits `retained_file_is_displaced`, based on exact retained/source inode
identity. A prepared stage or addition collision therefore cannot offer restoration as though it were
displaced user work. The renderer accepts native regular-file mode values including their file-type
bits, addition/removal observations and absent-path restoration results. Previous parser assumptions
accepted only permission bits and replacement results; those assumptions could hide valid native
recovery. Missing displacement evidence remains readable but never enables restoration. These are
additive live-view fields and desktop envelopes; existing durable receipts are unchanged. Older
clients may refuse the new catalogue status rather than interpret it as write authority.

Tests cover complete mixed-action native prompts, text bounds and binary refusal, native catalogue
references and displacement evidence, typed controller identities, partial/lost responses, group
member restoration scope, stale-view refusal and rendered disabled states. The regular-file desktop
source flow is implemented; directory changes, large/binary confirmation and real packaged graphical
approval/application/recovery proof remain unfinished.


This contract is transferred from preserved source into canonical PRs #87 and #88. Controller,
TypeScript and focused rendering checks passed on #88; full local desktop/native and packaged
verification remain outstanding. Source implementation coverage is not current runtime acceptance.

## Native saved group execution evidence

Group inspection reads existing `attempt-NNNN.json` and `group-observed.json`
records before and after independent member observations. The additive
`mesh.attachment-integration-group-execution/v1` projection reports `recorded`,
`no-outcome`, `invalid` or `changed`, ordered member attempts and an outcome only
when the sequence is consistent. It carries `historical:true`,
`observation_final:false`, `automatic_replay:false` and `write_authority:false`.
Durable record formats are unchanged. Each JSON read remains bounded to 64 KiB;
a group contains at most 64 members.

Checks bind the exact proposal digest, canonical shape, index, member order,
contiguous attempt prefix and stop-on-uncertainty ordering. A failed marker write
can leave an attempt record without invoking the file operation. A missing final
record cannot establish that files were unchanged. Changed, unreadable, linked or
contradictory execution records suppress the aggregate outcome but preserve
independent file observations. The double read does not establish an atomic
snapshot or current filesystem truth, and never grants replay or cleanup authority.

Native regressions cover interruption and reopening, later editor work, missing,
corrupt and linked outcomes, contradictory ordering and changing records. These
are transferred tests, pending canonical runtime verification. Desktop parsing
and localized display of this evidence are a following increment; packaged
graphical acceptance remains outstanding.

### Localized saved execution view

The desktop now reopens execution evidence without a previous apply reply, validates
exact proposal and ordered-member identities, and labels historical outcomes
separately from current file observations. Missing outcomes do not imply unchanged
files. Unreliable execution details are hidden while independent member recovery
remains available. English and Hebrew labels preserve literal file identifiers.
Old projections without execution evidence remain readable with an unavailable
message. No refresh or restart initiates a write. A reconciliation-required
aggregate may still contain all observed members when its final check failed.
Controller identity and no-mutation tests pass; rendered and packaged acceptance
remain separate verification requirements.

### Directory creation foundation

Retained directory staging needs ordinary child directories with destination
permissions subject to the existing process umask. The pinned-root primitive now
accepts requested ordinary modes without modifying process-global settings. It
rejects empty, dot, parent, multi-component and special-mode requests, refuses
existing children and revalidates the pinned parent namespace. Existing private
child creation retains its 0700 request. This primitive alone does not expose
directory integration, approval, replay or deletion authority; the tree executor
and end-to-end confirmation are separate increments. Runtime validation is pending.

### Destination permissions for staged directories

New-entry permission inheritance now distinguishes regular files and directories.
On macOS it selects directory-inheritable rules, preserves descendant inheritance
unless limited, clears only-inherit on the new entry and removes unrelated staging
ACLs. On Linux it preserves destination group/setgid behavior; default ACLs and
extended attributes remain unsupported and refuse. Parent metadata is checked
before and after configuration. No probe is created in the user folder and the
process umask is unchanged. Kernel-comparison regressions are present for both
platforms, pending native execution. Directory installation remains a following
increment; no additional approval or write command is exposed here.

### Native complete-subtree installation

A wholly new approved subtree can be staged outside the project, including frozen
files, executable state and empty directories. Every descendant must be absent
from the accepted base. Exact allocated identity, metadata, permissions and
content are bound to the receipt; raw bounded observation detects unexpected
entries even when ordinary capture would exclude them. Failed preparation retains
the stage and leaves the source unchanged.

Apply rechecks trusted main, exclusion policy, receipt, destination parent and the
entire stage, then uses a descriptor-relative no-replace rename on the same
filesystem. Both durability barriers are attempted. Concurrent files, directories
and symlinks are preserved. Later edits, changed parents/policy and uncertain
durability produce reconciliation evidence without undo, cleanup or replay. Open
descriptors remain usable after installation. Groups remain sequential, not atomic
project snapshots.

The new `mesh.attachment-directory-addition/v1` prepared receipt lives in a
`directory-<32 lowercase hex>` transaction. It binds project, attachment, store,
recovery, accepted head/bundle/target, canonical path, exclusions and parent
identity/mode/metadata. The complete ordered tree binds path, kind, installation
identity, mode, metadata, content digest and byte count. Limits are 64 entries
including root, 64 MiB content and 64 KiB prepared JSON, additionally constrained
by caller budgets. Read-only recovery rederives coverage from approved history,
reports source/stage observations and parent agreement, and grants no write,
cleanup or restoration authority. Existing file transaction formats are unchanged.

Groups containing staged directories use `mesh.attachment-integration-group/v2`;
file-only groups remain v1. Recovery reads both, expands directory members for
exact coverage and rejects overlap/missing paths. Older readers reject v2. Existing
execution records still bind the exact proposal digest. Entire already-present
approved trees are accounted for without rewriting unrelated extra user content;
partial or divergent trees refuse. Directory removal and type replacement remain
unsupported here. The current desktop prompt refuses directory-member groups
until complete-tree confirmation is implemented. Standalone directory records
require exact native inspection; the file catalogue does not discover them.

Native regressions cover interruption, races, late writes, staged changes, policy,
trust, budget and recovery boundaries. They are transferred pending canonical
runtime verification; packaged graphical approval remains outstanding.

### Complete-tree native confirmation

The native group prompt now includes staged directories in execution order rather
than omitting them. Every empty directory and frozen file is listed, with exact
file content, digest, executable state and permissions checked against the tree
receipt. Member accounting includes expanded tree entries. Missing or extra content,
mismatched identity/digest/mode, binary text and oversized prompts refuse the whole
group. Project and folder labels stay separately escaped. The native operation
still revalidates after consent; the renderer cannot supply write authority.
Localized directory recovery display and packaged graphical approval remain
pending. Source confirmation regressions are present, awaiting native execution.

### Localized directory recovery presentation

The renderer accepts bounded typed directory observations within verified groups,
displays source/stage entries including empty directories, and distinguishes
changed parent identity and permissions. Directory additions never gain retained
file restoration or cleanup authority. English/Hebrew labels preserve literal
left-to-right paths; incomplete observations remain explicit. Native confirmation
handles complete frozen tree text, while the renderer only presents facts.
Standalone directory preparations still require exact native inspection; normal
desktop creation is discoverable through group catalogue references.

The source directory feature is fully accounted for across #93–#97, but source
transfer is not acceptance. Directory removal/type replacement, larger/binary
confirmation and packaged graphical approval/recovery remain unfinished. Historical
source gate counts are recorded only as provenance in the migration ledger.

### Native retained whole-directory removal

A wholly removed approved subtree is one native group member. Preparation verifies
a raw bounded source tree against the approved base, including empty directories.
Extra/ignored entries, links, missing descendants, changed content or executable
states refuse. Frozen base bytes support confirmation; current source identity,
metadata and parent policy bind the operation. Preparation only writes external
recovery records. An already absent root beneath the exact parent is accounted for
without a new operation.

Apply rechecks main, trust, exclusions, receipt, parent and tree, then moves the
root into an empty recovery exchange name using descriptor-relative no-replace
rename. No recursive unlink occurs. The actual tree remains named; open file and
directory descriptors can continue writing or creating descendants there. Both
durability barriers are attempted. Races, recreated source entries, changed policy
and uncertain durability require reconciliation, never automatic undo or cleanup.

Removal receipt/result/recovery v1 binds the complete original tree against the
approved base and reports displaced-entry retention. Recovery independently
observes source and retained trees; missing outcomes grant no replay authority.
Group v3 supports removal members, while v1/v2 remain readable and older readers
reject v3. Addition type naming remains a compatibility alias, but addition
preparation still refuses removal. Confirmation files describe frozen base content;
proposed files remain empty for removal. Existing journals/signatures are unchanged.

Native confirmation explicitly describes REMOVE DIRECTORY TREE and retained open
handles, retaining complete-text limits. Localized removal recovery follows in a
separate increment; its previous addition-only parser refuses the new schema. Whole
tree restoration and file/directory conversion remain unfinished. Transferred tests
cover late writes, collisions, substitution, recreation, durability, unknown/ignored
children, trust/receipt bindings, group version refusal, mixed-group interruption
and historical recovery. Native runtime and packaged graphical proof are pending.

### Localized retained removal view

The renderer now accepts verified removal-recovery v1, distinguishes the operation
from addition, and labels retained-tree observations separately from the working
tree. It explains that open file and directory handles may still write there.
Directory references remain excluded from retained-file restoration; observed
state never grants replay, cleanup or automatic restoration authority. English
and Hebrew keep file identifiers literal. Whole-tree restoration requires its own
confirmed preserving operation and remains outstanding.

The source's disposable macOS experiment reported file/directory exchange with
open-handle continuity. That historical experiment is not canonical implementation
or Linux/packaged acceptance. Conversion still requires a complete exchange
executor, exact source/stage bindings, race handling, receipts, confirmation and
recovery; a remove-then-add sequence is not an equivalent substitute.

### Native approved entry conversion

An approved regular file can become a complete directory subtree, or an approved
subtree can become a regular file. Preparation verifies complete approved base and
result, refuses unknown/excluded/divergent entries and freezes both sides for
confirmation. Separate addition/removal entry points retain their original scope.
Replacement staging is external and follows destination inheritance and umask.
Each side is bounded to 64 entries with per-file and combined approved-content
budgets of at most 64 MiB or the smaller caller limit.

Apply rechecks current trusted main, exclusions, receipt, parent identity/policy
and complete source/replacement evidence, then performs one descriptor-relative
exchange. It never removes the source first. Both durability barriers are attempted.
The original object remains named in recovery, preserving open file and directory
handles. Boundary edits, replacement and uncertain durability require reconciliation
without automatic undo, cleanup or retry. A lost reply does not prove no exchange.

Conversion receipt v1 adds before_tree to the ordered directory binding fields;
before_tree is verified against the base and tree against the result. Exactly one
root is a file. The result binds the proposal and displaced-entry retention;
recovery reports independent source/retained observations and before/after kinds
without authority. Existing addition/removal formats stay unchanged. Group v4
requires conversion receipts and verifies union coverage without duplicate roots;
readers retain v1/v2/v3 compatibility and old readers reject v4. Journals and
signatures do not migrate.

The current desktop prompt refuses this new schema. Complete original/replacement
confirmation and localized recovery follow separately. Native regressions cover
both orientations, late handles, source/stage changes, durability, approved history,
budgets, version/overlap refusal and historical recovery. They await canonical
local execution. Whole-entry restoration and packaged graphical acceptance remain
required; historical source test counts do not establish completion.

### Complete conversion confirmation

The native group prompt now shows both complete conversion sides: original work
to retain and replacement to install, including empty directories and exact frozen
file content, digests and permissions. It names the direction and explains the
single preserving exchange. Missing, extra, mismatched, binary or oversized content
refuses the prompt rather than omitting a side. Expanded group accounting counts
shared root paths once and preserves execution order. Native authority is still
revalidated after consent. Separate escaped project/folder identity is preserved.
Localized recovery and packaged graphical proof remain pending; transferred native
confirmation regressions do not establish that an actual OS dialog was exercised.

## Conversion recovery presentation in canonical Mesh

The localized desktop parser accepts `mesh.attachment-entry-conversion-recovery/v1`
only for file-to-directory or directory-to-file direction. An observed root may be
a regular file for that schema; directory addition/removal retain their directory
root requirement. Observations remain bounded and validate paths, identities,
permissions and file digests. Neither direction grants write, replay, cleanup or
ordinary retained-file restoration authority.

The view presents working and retained entries, including an original root file,
with localized direction and literal paths. It explains that original objects and
open handles remain in recovery. These changes reconcile the remaining source
conversion documentation without adopting its historical test counts as canonical
proof. Whole-entry restoration and packaged graphical confirmation/recovery still
require separate implementation and acceptance.


## Native whole-entry restoration

`prepare_retained_entry_restoration` adds a native-only, single-use restoration operation for retained
entries from directory removals, file/directory conversions, and earlier whole-entry restorations.
It accepts both regular files and complete directory trees. The destination can be absent, a regular
file, or a directory; a parent must already exist. Existing single-file restoration formats and APIs
are unchanged. This is native library progress: desktop confirmation, discovery and recovery controls
are not yet connected to this new operation.

Preparation first checks bounded raw tree evidence against retained-root identity and current exclusion
policy, before freezing content or creating a staged copy. A refused excluded tree leaves no new
content copy. It then freezes the retained entry and current destination as independently observed
complete trees. A bounded walk retains file bytes and native descriptors, checks each identity/content/mode/
metadata record, then compares another complete observation. These checks detect observed changes;
they do not establish an atomic filesystem-wide snapshot. Each side is limited to 64 entries and the
combined frozen content to 64 MiB or the smaller caller budget, with per-file limits. Symbolic links, unsupported
entries, excluded paths, non-UTF-8 names and special permission bits refuse. Ordinary permissions,
ownership, supported native attributes, flags and ACLs are copied and compared. Files and directories
are freshly allocated; directory metadata is applied after constructing children so restrictive
permissions cannot be silently widened for construction. Unavailable metadata refuses preparation.
Linux runtime behavior remains unverified.

The origin remains in its original recovery transaction. New staging contains a frozen copy rather
than the origin inode. Apply rechecks origin and destination content/identity/metadata, parent identity
and policy, all receipts, historical approval ancestry and current exclusions. A present destination
is exchanged once with the new stage; an absent destination uses a no-replace rename. Both durability
barriers are attempted. Late writes through original recovery handles stay on the original object;
late destination-handle writes stay on the newly displaced object. Boundary changes and uncertain
outcomes retain all available copies without undo, cleanup or replay. Explicit undo chooses the new
transaction's retained destination, creating another transaction and preserving both earlier ones.
Restoration never advances Mesh main, and continued main advancement does not invalidate the trusted
historical origin. An absent-path restoration has no displaced entry to restore again; its older
origin remains available.

The new `entry-restoration-<32 lowercase hex>` namespace uses
`mesh.attachment-entry-restoration/v1`. Ordered fields bind project/attachment/store/recovery,
path, origin transaction/proposal digest/recovery identity, complete frozen origin/current/installed
trees, source parent identity/policy, exclusions and no-replay semantics. Absent current content is
`null`, not an empty tree. Tree entries use the existing seven fields. Origin ancestry is bounded to
eight levels, ends in a trusted directory-removal or conversion receipt, binds the retained root
allocation, and never treats a saved outcome as write authority. Later content and new children under
that original root may be selected explicitly, while a recreated root refuses. Previous receipt
schemas require no migration and older discovery readers do not interpret the new namespace.

`mesh.attachment-entry-restoration-result/v1` binds the exact proposal digest and records observed or
reconciliation-required status, whether destination work was displaced, and that the origin remains.
Missing results are not proof that nothing moved. The native `inspect_retained_entry_restoration`
reader validates ancestry and provides independent current source, stage and origin observations
through `mesh.attachment-entry-restoration-recovery/v1`. It distinguishes prepared/applied arrangements,
changed entries or origin, parent identity/policy changes, incomplete observation, invalid outcomes
and contradictory recorded success. Inspection has a shared live-content budget and grants no write,
cleanup or replay authority. A fresh native preparation and complete confirmation remain mandatory.

Transferred native tests are intended to exercise file/tree restoration over absent/file/tree destinations, preserved permissions
and empty folders, fresh allocations, late original handles, changed inputs, last-instant collisions,
post-exchange replacement and failed durability barriers. Transferred domain tests are intended to exercise trusted historical
ancestry, later main advancement, retained new children, reopened undo, untouched Mesh history,
untrusted/recreated/excluded origin refusal, missing outcomes and independent restart observations.
These checks do not establish graphical confirmation or packaged restoration behavior.

Canonical validation status: this is a transfer of preserved, uncommitted work, not
a previously tested source commit. Native execution and failing-before proof on
this base are pending; the previous combined validation remains running.
