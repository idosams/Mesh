# Mesh fleet acceptance audit — 5 October 2026

This is a dated requirement-to-evidence map, not a completion claim. The accepted
[full plan](fleet-orchestration.md) remains the authority for phase exits. The
[migration ledger](fleet-migration.md) retains source provenance and delivery receipts.
Source coverage and hosted tests do not establish packaged or real-provider acceptance.

At this observation canonical main is `cd9924dc427b9d0f3d949f4168634c9331192630`,
including [registered external-harness proof #336](https://github.com/idosams/Mesh/pull/336),
[consumed-input decisions #338](https://github.com/idosams/Mesh/pull/338),
[complete review snapshots #340](https://github.com/idosams/Mesh/pull/340),
[owner-bound reviews #344](https://github.com/idosams/Mesh/pull/344), and
[publication record format #346](https://github.com/idosams/Mesh/pull/346).
All seven [#336 checks](https://github.com/idosams/Mesh/actions/runs/37272505757) passed before
merge. The preceding main `ebee3f9` passed
[combined verification](https://github.com/idosams/Mesh/actions/runs/37272341929); verification
of `cd9924d` is [tracked separately](https://github.com/idosams/Mesh/actions/runs/37273947099).
The receipt-inspection increment [#347](https://github.com/idosams/Mesh/pull/347) passed all seven
checks on `c01258b` and is published at reconciled head `f08d629`; its
[fresh combined checks](https://github.com/idosams/Mesh/actions/runs/37274039958) and merge remain
pending at this observation. Neither the format nor receipt inspection enables native publication.

| Requirement | Verified evidence / current boundary | What still proves completion |
|---|---|---|
| Canonical repository and identity checks | Actual fetch/push remotes are idosams/Mesh; target guards run before edits/delivery; PR252 merge receipt retained | Continue exact identity/base checks through every remaining delivery |
| Preserve prior repositories and work | Complete verified bundles, original dirty Mesh-internal checkout and per-increment evidence retained under Mesh-delivery-preserved | Final provenance audit against migration ledger; no history deletion or settings changes |
| Coherent published and merged increments | Canonical delivery through #336 and #346; #347 is published with fresh reconciliation checks pending | Merge accepted increments after fresh checks; verify final combined main |
| Deprecate Mesh-internal through PR | [Legacy #1488](https://github.com/idosams/Mesh-internal/pull/1488) merged at `a4eb8619` with the docs quality gate passed and no required checks bypassed; unrelated legacy Rust failures remain recorded | Deprecation notice delivered; preserve history and keep development in Mesh |
| Attach existing dirty project without moving or changing Git | Repeated visible-window attachment journey passed on merged main `97e263f`; original Git preserved. It predates PR251/252 and is not proof on current main | Full manual acceptance including integration/recovery and native approval; exact final revision evidence |
| Manual capture, independent line and parallel saved review | Same packaged journey: visible 1156×764 window, two fixed comparisons, fork, restart/resume/detach, original Git preserved; no provider launched | Full manual baseline through reviewed main/integration/restore, broader concurrent-edit/failure campaign |
| Already-running external harness remains usable | [Packaged native external-Codex proof](evidence/external-harness-2026-10-04.json) at `5052009`: provider edits before attachment, two captures while the same provider runs, then further provider edits after capture stops; original root inode and Git index/HEAD preserved. Registration uses development meshctl; noninteractive provider and no graphical review | The newer registered consumed-lane proof is merged in #336 and recorded below; complete the interactive packaged harness-led journey, exact review/main approval, integration and restore |
| Truthful identity and attribution | Native/source model separates project/lane/run/version and retains unknown attribution | Current-revision end-to-end session attribution and mixed manual/agent handoff evidence |
| Coordinator creates child workers through tools | Current native Codex serial/parallel pair passed on merged main `1c0bcd1`; four distinct lanes, one attempt each and exact saved reviews ([record](evidence/fleet-four-worker-2026-10-04.json)) | Repeat supported provider journey in current packaged app without manual folder handoffs |
| Durable scheduling, inherited limits, cancellation | Native regression suites and source coverage; reviewed current full suites passed | Full specified fault campaign and no-duplicate recovery across real process/worker failure |
| Ordinary progress saved automatically and explicit checkpoints | Merged automatic scheduling and progress-panel work (#274–#278); #336 proves background capture during a real external Codex process | Actual provider/background capture journey with incomplete/gap/unsupported-entry cases; do not call explicit capture automatic proof |
| Live overview and stable parallel review during execution | Packaged manual pins/restart proven at `97e263f`; merged PR251/252 retain remote assignment identity and independently refresh signed observations across lanes | Interactive packaged review while a third real worker writes, current stale-data/reconnect behavior |
| Requests for changes and exact protected-main approval | Native mechanisms/source exist; packaged proof explicitly reports protected_main_approval=false | Eligible signed build with human presence, stale approval refusal, accepted integration and recovery |
| Dependency closure and rejection/revocation | Merged consumed decisions, complete snapshots, owner-bound reviews and publication framing (#338/#340/#344/#346); exact receipt inspection is pending #347. Native publication admission still refuses | Show downstream private consumption before upstream publication, rejection invalidates downstream publication, full closure reviewed |
| Retention preserves active/reviewed versions | Conservative retention, disk-full campaign #260 and native cleanup #261 are merged; their historical native checks do not prove final packaged storage-pressure acceptance | Retention/GC and storage-exhaustion campaign preserving pinned inputs/reviews |
| Worker restart preserves acknowledged work | Merged native recovery increments retain physical allocation identity, resume original partial initialization and preserve uncertain launch ownership; source tests cover lost receipts and closed mailboxes | Real-provider and second-host partial transfer/initialization recovery, lost acknowledgments and uncertain process reconciliation without duplicate execution |
| Signed remote input inspection in normal workflow | Merged native, desktop and CLI inspection/recovery routes; signed execution-history presentation and independent fleet observations are also merged | Actual eligible-signed packaged interaction with second-host inspection/recovery and truthful dated results; fixtures do not prove this journey |
| Real second machine execution and recovery | Protocol/native fixtures and local proofs are insufficient | Real SSH second host, execute, disconnect/reconnect, lost acknowledgment, exact returned review, no duplicate/lost acknowledged work |
| Second real provider | Claude adapter source is present. Installed Claude Code 2.1.220 reports signed out; no account details or credentials were copied | Successful second-provider conformance and packaged journey with recorded version/cost |
| Four-worker velocity and coordination | [Current native Codex pair](evidence/fleet-four-worker-2026-10-04.json) at `1c0bcd1`: 184.925s serial/64.458s parallel, peaks one/four workers, Codex 0.158.0-alpha.2.1; historical failures retained | Packaged renderer/event lag, resource use, cost, human coordination, time to accepted main, matched external-harness baseline and repeatability |
| Reliability and fault hardening | [Issue #37](https://github.com/idosams/Mesh/issues/37) and [issue #172](https://github.com/idosams/Mesh/issues/172) remain open. Recent full gates passed without leak warnings; this does not identify earlier causes | Resolve/reproduce startup/stop/process-leak concerns and execute specified crash/revocation/cancellation/approval/substitution campaign |
| Exact final packaged and merged delivery | Existing-project window proof is ad-hoc signed at `97e263f`. Fixed checkpoint `5052009` has verified embedded revision/seal and a native bridge journey; graphical provider launch remains unverified; source equality does not change embedded revision | Final revision-bound package, eligible signing/native approval, full end-to-end acceptance and final combined-main checks |

## Fixed testing checkpoint and evidence boundaries

The fixed user checkpoint is
`/Users/idoosams/Development/Mesh-checkpoints/2026-10-04-5052009`.
Use **Open Isolated Mesh Checkpoint.command** and the included **Sample Project**. Save two versions,
start a small two-lane Codex fleet, pin saved progress from both lanes while newer work continues,
and reopen after the panel choices are saved. Up to four saved-progress panels are supported.
The launcher verifies the executable hash and application seal, sets a separate application-data
home, and references the existing provider configuration without copying credentials. The installed
app, prior checkpoints, and this fixed checkpoint remain unchanged during development.

The embedded revision is `5052009ee4fe57a468bca4adec43de6f93c4c6a0`; executable SHA-256 is
`93f8f470db4aa07fd2a034c02599310be9566db55f7098e7c63c13d6e56f3c09` (22,742,368 bytes).
Checksum and strict signature verification passed again on 5 October. The app is ad-hoc signed,
not notarized, and protected-main approval is unavailable. Its source gate passed 3,823 native,
190 rendered, 663 desktop and 44 daemon tests. Its native package bridge journey covered two child
lanes, pinned review, feedback, retry and revoked-session refusal; it did not open the graphical
interface or launch a provider. The Mac remained locked at the latest graphical test attempt.

The checkpoint's unchanged guide describes #274–#278 as then-unmerged development work. Those PRs
have since merged; that old sentence is historical, not current delivery status. The checkpoint does
not include the subsequent native dependency/review/publication increments. Report build `5052009`,
the action, expected result, observed result and visible error when reporting a test failure.

The earlier `97e263f` window journey remains historical proof of attachment/capture, two comparisons,
fork and restart/resume/detach with original Git preserved. It launched no provider and approved no
protected main. A seal check or reported on-screen window does not establish an interactive journey.

Earlier ledger entries remain historical evidence. Their then-pending checks do not override
newer verified delivery, and their source-level success does not waive unproved phase exits.
The full requirement map remains open wherever the final column lacks matching evidence.

## Next order

Finish #347 delivery on combined main. Implement and exercise trusted native publication replay,
exact canonical ancestry for a second review/approval, the durable owner-journal commit and recovery,
and enforcement on every affected route before exposing native main advancement. The full
[publication contract](../decisions/native-dependency-publication.md) and
[issue #345](https://github.com/idosams/Mesh/issues/345) remain open.

After manual desktop unlock, test the packaged fleet path through ordinary controls: coordinator
delegation, independent workers, two fixed reviews while another worker writes, change requests,
recovery and reopened selections. Preserve the fixed user checkpoint. Continue eligible signed
main/integration/restore, interactive harness acceptance, second-provider authentication and testing,
real second-host fault recovery, retention/storage exhaustion, terminal capacity reconciliation,
and measured acceptance time. Final completion requires all plan exits, provenance reconciliation,
merged increments and exact combined-main/package proof.

## External harness native proof — 4 October 2026

The [retained result](evidence/external-harness-2026-10-04.json) records an external Codex CLI
0.158.0-alpha.2.1 session in a disposable existing Git project. The harness was launched directly,
without the Mesh fleet host, bridge or credentials, before attachment. It used workspace-write
sandboxing and the installed account without copying credentials. User configuration was ignored
for this controlled run; this is not proof of compatibility with every existing harness setup.

The fixture staged `work.txt`, then changed its working bytes before starting Codex. The provider
executed one command: write a first value, wait 20 seconds, write a second value, wait 20 seconds,
and write a third value. After the first write, the test confirmed the provider process was alive,
registered the unchanged project root with development meshctl and started the checkpoint package's
attachment watcher. It observed two distinct saved identities with unknown attribution. After a
joined capture stop, the same provider process was still alive; its third edit completed, while
Mesh's saved-version list stayed fixed. Git index and HEAD bytes and the source root inode matched
their pre-attachment values. The fixture remained dirty, with the final external edit intact.

The exact package revision, executable digest and seal were checked before and after execution.
The run passed in 56.561 seconds; that duration includes provider startup and intentional waits,
and is not an event-latency or velocity result. The private evidence directory retains the prompt,
provider protocol output, capture status records, complete fixture, runner script and result; all
37 recorded files passed a subsequent byte-hash manifest verification. Private paths, process IDs
and raw provider logs are not published. This proof establishes neither graphical interaction nor
exact reviewed main approval, and does not close the full harness-led acceptance journey.

## Registered consumed-lane proof — 5 October 2026

[PR #336](https://github.com/idosams/Mesh/pull/336) supplies the reproducible
[registered-harness runner](registered-external-harness-proof.md). The
[refreshed recorded result](evidence/registered-external-harness-2026-10-05-refresh.json) passed on
exact native executable `170518353d8901561ddd7dce5e6572d1f0ffacb0` in 28.148 seconds after fresh
fixture preparation. History advanced from four to six versions while the same external Codex process
continued before watching and after joined stop. Its later edit remained uncaptured; Git HEAD/index,
root inode, original input and executable bytes were preserved. Wrong-digest refusal was separately
verified before mutation. This is a synthetic registered consumed lane using a controlled
noninteractive provider, not graphical provisioning, exact saved-file preview, protected-main
approval, integration/restore, another provider/host, or a velocity baseline. The original result and
refreshed result retain their distinct executable identities; neither is proof on every later main.
