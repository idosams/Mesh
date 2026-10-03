# Mesh fleet acceptance audit — 3 October 2026

This is a dated requirement-to-evidence map, not a completion claim. The accepted
[full plan](fleet-orchestration.md) remains the authority for phase exits. The
[migration ledger](fleet-migration.md) retains source provenance and delivery receipts.
Source coverage and hosted tests do not establish packaged or real-provider acceptance.

At this observation canonical main is `273307176a82806bfdeebba97b7c78751e62c048`,
including [PR #223](https://github.com/idosams/Mesh/pull/223).
[PR #224](https://github.com/idosams/Mesh/pull/224), head
`1149a841902d42d5d924b33f3dc9fafd0746ecff`, is published against main; its
[exact-head CI](https://github.com/idosams/Mesh/actions/runs/37132831828) and
[combined-main CI](https://github.com/idosams/Mesh/actions/runs/37132811999) are running.

| Requirement | Verified evidence / current boundary | What still proves completion |
|---|---|---|
| Canonical repository and identity checks | Actual fetch/push remotes are idosams/Mesh; target guards run before edits/delivery; PR221 merge receipt retained | Continue exact identity/base checks through every remaining delivery |
| Preserve prior repositories and work | Complete verified bundles, original dirty Mesh-internal checkout and per-increment evidence retained under Mesh-delivery-preserved | Final provenance audit against migration ledger; no history deletion or settings changes |
| Coherent published and merged increments | Canonical delivery through PR223; PR224 published separately with original CI preserved | Merge accepted increments after fresh checks; verify final combined main |
| Deprecate Mesh-internal through PR | Guidance points forward to Mesh; [legacy #1488](https://github.com/idosams/Mesh-internal/pull/1488) remains OPEN at `99b55d`, with latest baseline Linux/clippy/deny failures | Legitimately resolve its checks and merge deprecation; no bypass/settings change |
| Attach existing dirty project without moving or changing Git | Packaged existing-project journey recorded in [PR #221](https://github.com/idosams/Mesh/pull/221) passed on `6b82368`; it predates PR223/224 and is not proof on current main | Full manual acceptance including integration/recovery and native approval; exact final revision evidence |
| Manual capture, independent line and parallel saved review | Same packaged journey: visible 1156×764 window, two fixed comparisons, fork, restart/resume/detach, original Git preserved; no provider launched | Full manual baseline through reviewed main/integration/restore, broader concurrent-edit/failure campaign |
| Already-running external harness remains usable | Required by accepted plan; ordinary external edits tested, but that does not prove a live harness session | Run the packaged harness-led journey without Mesh launching the provider, then exact review/main approval |
| Truthful identity and attribution | Native/source model separates project/lane/run/version and retains unknown attribution | Current-revision end-to-end session attribution and mixed manual/agent handoff evidence |
| Coordinator creates child workers through tools | Native Codex four-worker record at historical PR120 reports exact one-attempt lanes and saved reviews | Repeat supported provider journey in current packaged app without manual folder handoffs |
| Durable scheduling, inherited limits, cancellation | Native regression suites and source coverage; reviewed current full suites passed | Full specified fault campaign and no-duplicate recovery across real process/worker failure |
| Ordinary progress saved automatically and explicit checkpoints | Source/background capture coverage and explicit packaged capture proof | Actual provider/background capture journey with incomplete/gap/unsupported-entry cases; do not call explicit capture automatic proof |
| Live overview and stable parallel review during execution | Packaged manual pins/restart proven; source and rendered fleet controls covered | Interactive packaged review while a third real worker writes, current stale-data/reconnect behavior |
| Requests for changes and exact protected-main approval | Native mechanisms/source exist; packaged proof explicitly reports protected_main_approval=false | Eligible signed build with human presence, stale approval refusal, accepted integration and recovery |
| Dependency closure and rejection/revocation | Source mechanisms and tests exist; plan keeps acceptance open | Show downstream private consumption before upstream publication, rejection invalidates downstream publication, full closure reviewed |
| Retention preserves active/reviewed versions | Explicit plan requirement; held native roots alone are not durable retention policy | Retention/GC and storage-exhaustion campaign preserving pinned inputs/reviews |
| Worker restart preserves acknowledged work | Merged PR223 records allocation identity before acknowledgment; published PR224 adds read-only restart inspection; both passed their full local gates | Complete partial transfer/initialization/uncertain launch reconciliation without duplicate execution; graphical actions |
| Signed remote input inspection in normal workflow | Next integration scoped: explicit request, not routine status polling; no implementation yet | Versioned authenticated request/reply, native pre/post checks, closed compatibility/refusal tests, visible truthful results |
| Real second machine execution and recovery | Protocol/native fixtures and local proofs are insufficient | Real SSH second host, execute, disconnect/reconnect, lost acknowledgment, exact returned review, no duplicate/lost acknowledged work |
| Second real provider | Claude adapter source is present; successful authenticated acceptance remains outstanding | Successful second-provider conformance and packaged journey with recorded version/cost |
| Four-worker velocity and coordination | [Historical native Codex pair](fleet-native-measurements.md) at PR120: 168.531s serial/57.748s parallel; four acknowledged overlapping workers; failed first startup retained | Repeat current revision; renderer/event lag, resource use, cost, human coordination, time to accepted main and matched external-harness baseline |
| Reliability and fault hardening | [Issue #37](https://github.com/idosams/Mesh/issues/37) and [issue #172](https://github.com/idosams/Mesh/issues/172) remain open. Recent full gates passed without leak warnings; this does not identify earlier causes | Resolve/reproduce startup/stop/process-leak concerns and execute specified crash/revocation/cancellation/approval/substitution campaign |
| Exact final packaged and merged delivery | Latest existing-project packaged proof is ad-hoc signed at `6b82368`; source equality does not change its embedded revision | Final revision-bound package, eligible signing/native approval, full end-to-end acceptance and final combined-main checks |

## Evidence and delivery hygiene

- R90 full suite: 3,674 native tests, 156.092s, 17 skipped, 632 desktop tests; restricted-run socket/capture failures retained, unchanged host-permission rerun passed.
- R91 full suite: 3,677 native tests, 150.711s, 17 skipped, 180 rendered checks, 632 desktop tests and real daemon demo passed. Neither count completes real-host acceptance.
- The user's fixed checkpoint remains /Users/idoosams/Development/Mesh-checkpoints/2026-10-02-858bcb3. Do not silently replace it.
- Earlier ledger checkpoints and source overviews remain dated historical evidence. Their then-pending checks and limitations must not be interpreted as current delivery state.

## Next order

Finish PR224 delivery and combined-main verification. Then connect explicit signed input inspection to native/desktop recovery, proceed through interrupted initialization and uncertain execution reconciliation, and complete the remaining local/harness/provider/signing/second-host acceptance in this map. None of those requirements is waived by completing a smaller native foundation.
