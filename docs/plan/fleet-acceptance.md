# Mesh fleet acceptance audit — 4 October 2026

This is a dated requirement-to-evidence map, not a completion claim. The accepted
[full plan](fleet-orchestration.md) remains the authority for phase exits. The
[migration ledger](fleet-migration.md) retains source provenance and delivery receipts.
Source coverage and hosted tests do not establish packaged or real-provider acceptance.

At this observation canonical main is `147e4695af612aac938a37635ebbcf7f7420fc48`,
including [PR #252](https://github.com/idosams/Mesh/pull/252), merged at
23:38:01 UTC on 3 October after all seven
[exact-head checks](https://github.com/idosams/Mesh/actions/runs/37161611871) passed.
The [preceding main checks](https://github.com/idosams/Mesh/actions/runs/37161549987)
passed on `d88e589`; [current main verification](https://github.com/idosams/Mesh/actions/runs/37162398846)
was still running at this observation.

[PR #254](https://github.com/idosams/Mesh/pull/254) corrects installed Codex discovery.
Its combined source passed the full local gate and all seven hosted checks at
`c2641c64f3918e40b906d742295a24e17b490f10`. Reconciled head
`3f66a4115912c52abfe261e47a405fcdd4551172` has exactly the same source tree and targets
current main; [fresh CI](https://github.com/idosams/Mesh/actions/runs/37162696782) is running.
This documentation increment is stacked on that published branch. Neither this PR nor those
checks establish the unfinished acceptance journeys below.

| Requirement | Verified evidence / current boundary | What still proves completion |
|---|---|---|
| Canonical repository and identity checks | Actual fetch/push remotes are idosams/Mesh; target guards run before edits/delivery; PR252 merge receipt retained | Continue exact identity/base checks through every remaining delivery |
| Preserve prior repositories and work | Complete verified bundles, original dirty Mesh-internal checkout and per-increment evidence retained under Mesh-delivery-preserved | Final provenance audit against migration ledger; no history deletion or settings changes |
| Coherent published and merged increments | Canonical delivery through PR252; PR254 published with fresh reconciliation checks running | Merge accepted increments after fresh checks; verify final combined main |
| Deprecate Mesh-internal through PR | Guidance points forward to Mesh; [legacy #1488](https://github.com/idosams/Mesh-internal/pull/1488) remains OPEN at `99b55d`, with latest baseline Linux/clippy/deny failures | Legitimately resolve its checks and merge deprecation; no bypass/settings change |
| Attach existing dirty project without moving or changing Git | Repeated visible-window attachment journey passed on merged main `97e263f`; original Git preserved. It predates PR251/252 and is not proof on current main | Full manual acceptance including integration/recovery and native approval; exact final revision evidence |
| Manual capture, independent line and parallel saved review | Same packaged journey: visible 1156×764 window, two fixed comparisons, fork, restart/resume/detach, original Git preserved; no provider launched | Full manual baseline through reviewed main/integration/restore, broader concurrent-edit/failure campaign |
| Already-running external harness remains usable | Required by accepted plan; ordinary external edits tested, but that does not prove a live harness session | Run the packaged harness-led journey without Mesh launching the provider, then exact review/main approval |
| Truthful identity and attribution | Native/source model separates project/lane/run/version and retains unknown attribution | Current-revision end-to-end session attribution and mixed manual/agent handoff evidence |
| Coordinator creates child workers through tools | Current native Codex serial/parallel pair passed on merged main `1c0bcd1`; four distinct lanes, one attempt each and exact saved reviews ([record](evidence/fleet-four-worker-2026-10-04.json)) | Repeat supported provider journey in current packaged app without manual folder handoffs |
| Durable scheduling, inherited limits, cancellation | Native regression suites and source coverage; reviewed current full suites passed | Full specified fault campaign and no-duplicate recovery across real process/worker failure |
| Ordinary progress saved automatically and explicit checkpoints | Source/background capture coverage and explicit packaged capture proof | Actual provider/background capture journey with incomplete/gap/unsupported-entry cases; do not call explicit capture automatic proof |
| Live overview and stable parallel review during execution | Packaged manual pins/restart proven at `97e263f`; merged PR251/252 retain remote assignment identity and independently refresh signed observations across lanes | Interactive packaged review while a third real worker writes, current stale-data/reconnect behavior |
| Requests for changes and exact protected-main approval | Native mechanisms/source exist; packaged proof explicitly reports protected_main_approval=false | Eligible signed build with human presence, stale approval refusal, accepted integration and recovery |
| Dependency closure and rejection/revocation | Source mechanisms and tests exist; plan keeps acceptance open | Show downstream private consumption before upstream publication, rejection invalidates downstream publication, full closure reviewed |
| Retention preserves active/reviewed versions | Explicit plan requirement; held native roots alone are not durable retention policy | Retention/GC and storage-exhaustion campaign preserving pinned inputs/reviews |
| Worker restart preserves acknowledged work | Merged native recovery increments retain physical allocation identity, resume original partial initialization and preserve uncertain launch ownership; source tests cover lost receipts and closed mailboxes | Real-provider and second-host partial transfer/initialization recovery, lost acknowledgments and uncertain process reconciliation without duplicate execution |
| Signed remote input inspection in normal workflow | Merged native, desktop and CLI inspection/recovery routes; signed execution-history presentation and independent fleet observations are also merged | Actual eligible-signed packaged interaction with second-host inspection/recovery and truthful dated results; fixtures do not prove this journey |
| Real second machine execution and recovery | Protocol/native fixtures and local proofs are insufficient | Real SSH second host, execute, disconnect/reconnect, lost acknowledgment, exact returned review, no duplicate/lost acknowledged work |
| Second real provider | Claude adapter source is present. Installed Claude Code 2.1.220 reports signed out; no account details or credentials were copied | Successful second-provider conformance and packaged journey with recorded version/cost |
| Four-worker velocity and coordination | [Current native Codex pair](evidence/fleet-four-worker-2026-10-04.json) at `1c0bcd1`: 184.925s serial/64.458s parallel, peaks one/four workers, Codex 0.158.0-alpha.2.1; historical failures retained | Packaged renderer/event lag, resource use, cost, human coordination, time to accepted main, matched external-harness baseline and repeatability |
| Reliability and fault hardening | [Issue #37](https://github.com/idosams/Mesh/issues/37) and [issue #172](https://github.com/idosams/Mesh/issues/172) remain open. Recent full gates passed without leak warnings; this does not identify earlier causes | Resolve/reproduce startup/stop/process-leak concerns and execute specified crash/revocation/cancellation/approval/substitution campaign |
| Exact final packaged and merged delivery | Existing-project window proof is ad-hoc signed at `97e263f`. Fixed checkpoint `c2641c6` has verified embedded revision/seal, but its provider launch is unverified; source equality does not change embedded revision | Final revision-bound package, eligible signing/native approval, full end-to-end acceptance and final combined-main checks |

## Fixed testing checkpoint and evidence boundaries

The separate user checkpoint is
`/Users/idoosams/Development/Mesh-checkpoints/2026-10-04-c2641c6`.
Its guide covers attachment, saved versions, parallel comparisons, an independent line and restart,
plus an optional small Codex fleet. The launcher verifies the executable hash and application seal,
sets a separate application-data home for its child process, and retains the existing provider
configuration in its original location. It does not copy credentials or replace the installed app.
Older checkpoints and their data remain preserved. The earlier launcher redirected only
`CFFIXED_USER_HOME` and could activate an existing Mesh window; its isolation claim was insufficient.

The checkpoint's full source gate passed 3,774 native tests (17 skipped), 185 rendered tests,
646 desktop tests and all 44 daemon checks. Its exact package is ad-hoc signed, not notarized,
and cannot satisfy eligible native signing or protected-main approval. Packaged provider execution
is pending desktop access; an app launch or seal check is not evidence that an agent started.
The separate `97e263f` window journey passed attachment/capture, two pinned comparisons, fork,
restart/resume/detach and original-Git preservation, with provider launch and protected-main approval
both explicitly false. A window reported on-screen by the proof does not establish a manually
unlocked desktop or interactive provider acceptance.

Earlier ledger entries remain historical evidence. Their then-pending checks do not override
newer verified delivery, and their source-level success does not waive unproved phase exits.
The full requirement map remains open wherever the final column lacks matching evidence.

## Next order

Finish PR254's normal checked delivery, then test its packaged fleet path through ordinary desktop
controls after manual unlock: coordinator delegation, independent workers, two saved reviews while
another worker writes, change requests and recovery. Preserve the fixed user checkpoint throughout.
Continue the already-running external-harness journey, eligible signed main/integration/restore,
second-provider login and acceptance, real second-host fault recovery, retention/storage exhaustion,
terminal capacity reconciliation and measured acceptance time. Resolve the legacy deprecation PR's
checks without bypassing them. Final completion requires all plan exits and combined-main/package
proof, not just another green source increment.
