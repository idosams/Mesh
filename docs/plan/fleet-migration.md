# Fleet consolidation and delivery ledger

Canonical repository: **idosams/Mesh**. Mesh-internal is deprecated for development and remains
historical evidence. The full [fleet objective](fleet-orchestration.md) is unchanged.

## Source provenance audit (2026-10-04)

Verified canonical main: `401f8ce504d885126c1d230aa3a849984ae49cda`, including merged
[PR #263](https://github.com/idosams/Mesh/pull/263). Its exact head passed all seven hosted
checks before merge. Main CI and PR264/265 delivery remain separate pending observations.

The original range `8541601d1e14f225d25e20adfbad38adce009291` through
`6ec9c8ccf9e273de25d1a84c323c0cd4f3f4b821` contains exactly 87 commits. The source table has
exactly the same 87 unique identities, with no omissions or extras. Live canonical PR descriptions
account for every source identity through 88 merged PRs. Explicit future/dependency references were
excluded from the delivery mapping: PR30 for `e320c5c`, PR44 for `c728c41`, PR44/45 for `e987565`,
PR48 for `c7922b3`, and PR103's dirty-work base reference for `6ec9c8c`. Every mapped final PR head
and merge commit is an ancestor of the main revision above. Canonical alpha.5 baseline `9db6136`
also remains an ancestor. The 31 stale “Pending transfer” rows and earlier unmerged labels are
corrected below; historical narrative and initial implementation refs remain preserved.

The retained legacy all-refs bundle verifies and still names the exact original fleet head.
A separate private recovery-manifest audit read 661 retained files: 659 match their recorded hashes.
One changed checkpoint note retains its entire original 546,165-byte prefix; that prefix was copied
separately and verified against the original hash. The original 1,673-byte coordinator-receive PR
draft was recovered from this task's recorded file-creation content, parsed as data without executing
historical commands, and matches its original hash. Thus all 661 expected byte sequences are
available: 659 unchanged in place and two separately recovered original note versions. Current
notes, recovered originals, expected hashes and audit output are preserved without rewriting the
old manifest or snapshots. This is a bounded recovery-manifest result, not a claim that every prior
scratch artifact was inventoried. The recorded Git bundles, patches and archives in it match.

The uncommitted whole-entry implementation remains separately accounted for by PR103, followed by
new canonical desktop work in PR104–107. Those deliveries are merged; their original dirty-source
byte equivalence is not established by the 87-commit audit. Repository reachability and PR accounting
do not independently prove semantic equivalence of every intermediate source tree or any packaged
acceptance journey. The full plan and its remaining acceptance requirements are unchanged.

## Historical delivery checkpoint (2026-10-03)

Canonical main is `b009f2761f3eb1d397b0902ffb2dd3fc43565b0a`, including
[PR #224](https://github.com/idosams/Mesh/pull/224), merged at 15:28:15 UTC.
Its head `1149a841902d42d5d924b33f3dc9fafd0746ecff` passed all seven
[hosted checks](https://github.com/idosams/Mesh/actions/runs/37132831828) before normal merge.
The preceding PR223 combined-main run passed; the new
[combined-main run](https://github.com/idosams/Mesh/actions/runs/37133352356) is running.
The [acceptance map](fleet-acceptance.md) records evidence and remaining requirements
across the complete plan. This documentation increment adds no product behavior and replaces
no preserved implementation commit; it preserves the older dated observations below.

The original work, superseded heads and verification logs remain preserved. Ancestry-only
reconciliations retained identical source trees and received fresh hosted checks. The earlier
checkpoints below are historical observations, including then-pending runs and failure causes.
Mesh-internal #1488 remains open: its latest observed failures concern legacy Linux system
libraries and dependency policy. No check bypass or repository settings change was performed.

## Historical delivery checkpoint (2026-09-28)

Canonical main is verified through [PR #116](https://github.com/idosams/Mesh/pull/116) at
`9c4f7ee97de427aa5d57320ae61d54b4e8f8e401`, merged at 18:36:32 UTC. PRs #71–#116
are now confirmed merged in dependency order. Every exact published head passed all seven hosted
PR checks before its merge. Each merge rechecked canonical repository/base/head identity, clean
worktree state and GitHub rules; non-force fast-forwards preserved the published commits. No
self-approval, required-check bypass or repository settings change occurred.

The owner explicitly authorized merging without human review for this effort. The historical
increment notes below retain their original observations and source mappings; this checkpoint
supersedes their older unmerged/pending-review statements. That authorization does not replace
protected Mesh main's native human-presence approval or complete any product acceptance journey.

### Validation and remaining boundaries

- [PR #116 CI](https://github.com/idosams/Mesh/actions/runs/36463671508) passed all seven checks;
  Linux took 6m32s and macOS 7m35s. Its complete local gate is still running on the same revision:
  repository/docs/license/storage/format/lint checks passed and native compilation completed.
  All native tests, desktop checks and the daemon demonstration have not yet completed in that run.
- The [combined-main run](https://github.com/idosams/Mesh/actions/runs/36466394886) passed all seven
  checks. Intermediate main-push runs were superseded by subsequent main pushes under
  existing CI concurrency settings. Their cancellation is not a passing result; successful exact-head
  PR checks were independently verified before every merge.
- The earlier full #70 local run failed a native filesystem-event test. The preserved full #102
  run passed both previously failing filesystem-event cases but failed the synthetic Codex launcher
  test at its unchanged one-second deadline: 1,836 passed, one failed and 1,506 were not run.
  The unchanged focused launcher then passed in 0.348s. Failed logs remain preserved; this neither
  establishes a cause nor closes [issue #37](https://github.com/idosams/Mesh/issues/37).
- Source transfer, desktop recovery/restoration and provider/remote foundations are merged. The
  preserved whole-entry work is included through #103–#107, with its per-file provenance below.
  Original dirty checkouts, source branches, private backups and superseded heads remain preserved.
- Actual second-provider/four-worker evidence, manual and harness-led packaged journeys, native
  signed confirmation, full private-dependency/fault acceptance and final combined-main validation
  remain required. Remote export, authenticated transport, execution, results and real second-machine
  reconnect/recovery are unfinished. See the unchanged [full plan](fleet-orchestration.md),
  [remote sequence](fleet-remote-delivery.md) and [current status](../project-status.md).
- [Mesh-internal #1488](https://github.com/idosams/Mesh-internal/pull/1488) remains open. Its CI
  previously reported a payment/spending-limit restriction and its checks remain failed. No billing,
  archival, deletion or settings change is authorized or performed. Canonical repository guidance
  already identifies Mesh-internal as deprecated; its own deprecation PR is not merged delivery.

PRs #14 and #16 were closed as superseded, not merged: their complete changed files are identical
to canonical main at `5ea158f9d53b4d247d457f50a1808bb56489f02d`. Their original branches and history
remain preserved, with incorporated corrections traced through the mappings below.

### Newly confirmed merge receipts

All rows below were independently read back from GitHub after merge, and every listed head is
an ancestor of the verified combined main above. Times are UTC on 2026-09-28.

| PR | Preserved published head | Merged at |
| --- | --- | --- |
| [#71](https://github.com/idosams/Mesh/pull/71) | `985adffffd29b3202c6cff22e6964c7454820d2c` | 18:19:35 |
| [#72](https://github.com/idosams/Mesh/pull/72) | `08bd13bc0bc734aa9d3e6ce607fa8fd950111d56` | 18:20:32 |
| [#73](https://github.com/idosams/Mesh/pull/73) | `64f9278fdb0bb56c7a69a5fbf60d9e43f635c477` | 18:20:53 |
| [#74](https://github.com/idosams/Mesh/pull/74) | `cd804ad3708b1056ae29e84019575dfe4e6b42bb` | 18:21:13 |
| [#75](https://github.com/idosams/Mesh/pull/75) | `a4c8f89a1efcba0aa3bf64ad420a8901820b41c7` | 18:21:34 |
| [#76](https://github.com/idosams/Mesh/pull/76) | `a87005e8db37bb917f43468a91c02d1dd9662d91` | 18:21:54 |
| [#77](https://github.com/idosams/Mesh/pull/77) | `0e3c36b30354c11bb61c5aaa7fe80f17bbbf907d` | 18:22:14 |
| [#78](https://github.com/idosams/Mesh/pull/78) | `35b20458158fce6f5d281906ef1a05845483080b` | 18:22:34 |
| [#79](https://github.com/idosams/Mesh/pull/79) | `19e85e29f400f3974c4d72583e93d214f384570f` | 18:22:55 |
| [#80](https://github.com/idosams/Mesh/pull/80) | `4e3daa214e7ccb63dac64622f34e571d53491ec5` | 18:23:16 |
| [#81](https://github.com/idosams/Mesh/pull/81) | `c80d0fc70bfa573985d45ee1de875e90a005daf8` | 18:23:36 |
| [#82](https://github.com/idosams/Mesh/pull/82) | `5f124dc2ac496e9c8e3e66353018e4498b41b8a0` | 18:23:58 |
| [#83](https://github.com/idosams/Mesh/pull/83) | `04408cf8c89bef5d307fa13747282f347dbd43dc` | 18:24:19 |
| [#84](https://github.com/idosams/Mesh/pull/84) | `dea7d1c6bd0b7c468fe3e4cccbd43fd9a46751b4` | 18:24:41 |
| [#85](https://github.com/idosams/Mesh/pull/85) | `ec305a6ada14d5d3e33d64d9fdde33d8c06203b8` | 18:25:03 |
| [#86](https://github.com/idosams/Mesh/pull/86) | `4e55abd227327f3b5bd7387d14e9a831b0d4a513` | 18:25:28 |
| [#87](https://github.com/idosams/Mesh/pull/87) | `54ca0c3d991d90e4a0b88ac0dd29c2a3e5265f42` | 18:25:49 |
| [#88](https://github.com/idosams/Mesh/pull/88) | `6150790f6c8144c61bc1411f9e96206e5cde7630` | 18:26:24 |
| [#89](https://github.com/idosams/Mesh/pull/89) | `cd3405e3dd6f4604837e8c37ba04b2ef620908b1` | 18:26:46 |
| [#90](https://github.com/idosams/Mesh/pull/90) | `3ab0dc58e1951a7c9cb4b1c63e13cacfe3b33d23` | 18:27:08 |
| [#91](https://github.com/idosams/Mesh/pull/91) | `cb5a7cf4071e3702331d730529c01df29ea0d995` | 18:27:28 |
| [#92](https://github.com/idosams/Mesh/pull/92) | `d2ea4b19edb588948b5b94e3ee363a0cceb0c536` | 18:27:51 |
| [#93](https://github.com/idosams/Mesh/pull/93) | `c4b12ffc29c5ffc106fe89b0d2a4fec8ca6c88c0` | 18:28:14 |
| [#94](https://github.com/idosams/Mesh/pull/94) | `eafef923292ffde5c717b0428ed3276afb045fe8` | 18:28:37 |
| [#95](https://github.com/idosams/Mesh/pull/95) | `0f5a5b4276fc2a9a91193edca32a5226f7a7e95b` | 18:28:58 |
| [#96](https://github.com/idosams/Mesh/pull/96) | `62de5dfaa3b441f90770594de2a91bfe539c7c36` | 18:29:19 |
| [#97](https://github.com/idosams/Mesh/pull/97) | `43df0cdd173036d96aa8eb35d029b98207dec0ab` | 18:29:42 |
| [#98](https://github.com/idosams/Mesh/pull/98) | `1a95c9c59e21a850ae53c1962fabe4362a68a0e7` | 18:30:03 |
| [#99](https://github.com/idosams/Mesh/pull/99) | `c1cdf63721014eb774dc520d9b99df9a017571be` | 18:30:27 |
| [#100](https://github.com/idosams/Mesh/pull/100) | `31b4330f547f83e40511c74576fcc657a2df5ec3` | 18:30:50 |
| [#101](https://github.com/idosams/Mesh/pull/101) | `cdb7e6153208ab1ad7b3b46e7819a640fc4c4861` | 18:31:11 |
| [#102](https://github.com/idosams/Mesh/pull/102) | `b50fe49f70bf81bd1ca60879e3a074be87f6d70f` | 18:31:31 |
| [#103](https://github.com/idosams/Mesh/pull/103) | `30462d0d6c7226f6475cf73cc1084c2fd6c8e11e` | 18:31:52 |
| [#104](https://github.com/idosams/Mesh/pull/104) | `ab46c6fed087af8a7e54418233a60cdef50e6339` | 18:32:13 |
| [#105](https://github.com/idosams/Mesh/pull/105) | `1e17abc162c6d27f69c2589f49fe693a3207be7e` | 18:32:34 |
| [#106](https://github.com/idosams/Mesh/pull/106) | `8df187fb55cbf38c7cccc83a8749c3ed5c54ab8e` | 18:32:56 |
| [#107](https://github.com/idosams/Mesh/pull/107) | `673b62dcbbe015ddd83b32a5bcd172cdbf666c4f` | 18:33:18 |
| [#108](https://github.com/idosams/Mesh/pull/108) | `6e4f5c64569bfc97f9a1384a4124819fd87f2a5d` | 18:33:40 |
| [#109](https://github.com/idosams/Mesh/pull/109) | `f8ef047b01d0d25198f6bb5a1502d4f813132392` | 18:34:01 |
| [#110](https://github.com/idosams/Mesh/pull/110) | `6dcff52b999ce4a7fa032edabeb6410eed5a424f` | 18:34:22 |
| [#111](https://github.com/idosams/Mesh/pull/111) | `dee740c36a0bd8f535317ee7228d0687df547d76` | 18:34:44 |
| [#112](https://github.com/idosams/Mesh/pull/112) | `82a218b89132002c4e00eb0f6ba6966a0f362e86` | 18:35:07 |
| [#113](https://github.com/idosams/Mesh/pull/113) | `5c21f94bb6fc0cf5418a9ef8b15a84eb92d56ad4` | 18:35:29 |
| [#114](https://github.com/idosams/Mesh/pull/114) | `02cad17eb9814ef5a53df9a1bee41a4e1f10dc36` | 18:35:51 |
| [#115](https://github.com/idosams/Mesh/pull/115) | `cfde2e9e46094708655e5cb7e4b1c22c2a1a50de` | 18:36:12 |
| [#116](https://github.com/idosams/Mesh/pull/116) | `9c4f7ee97de427aa5d57320ae61d54b4e8f8e401` | 18:36:32 |

## Verified reconciliation baseline

- Canonical main: `9db6136803e9a50a0701d29d054b1fbf31591fd2`, alpha.5, merged through Mesh PR #1.
- Preserved fleet head: `6ec9c8ccf9e273de25d1a84c323c0cd4f3f4b821`, branch `idosams/fleet-orchestration`.
- Shared ancestor: `8541601d1e14f225d25e20adfbad38adce009291`, alpha.4.
- Canonical main has seven independent later commits; the fleet branch has 87.
- Mesh-internal main is `4d951c8aed0eb26ecda3a189b3f4997f58e72d5e`; do not merge its unrelated main history.
- GitHub inspection found no fleet-branch PR in either repository. Local progress is not merged delivery.

All existing refs were preserved in a verified local Git bundle, with separate staged/unstaged
patches and file archives for the fleet and original dirty checkouts. Those backups are private local
artifacts, not material to upload to this public repository. The uncommitted whole-entry restoration
increment is preserved separately, including its three new native modules and tests. Its final full
run failed the macOS native-root-change event test; targeted restoration tests passed. Do not carry
that increment forward as fully verified. Original logs and the unfinished work remain intact.

## PR sequence and replacement accounting

1. Canonical repository guard, shared agent guidance and this ledger: [Mesh PR #2](https://github.com/idosams/Mesh/pull/2), replacement `c36fafa1aa3ec6994d482a8eedba470bd0718b6b`, open and unmerged. This replaces the guidance
   portion of source commit `c4e5b9450962b5bec818022d6e7936f2d5204b89`, not its runtime implementation.
2. Documentation-only [Mesh-internal PR #1488](https://github.com/idosams/Mesh-internal/pull/1488), replacement `99b55d6907b2d36b4196bd2971e9ba6cfa1bd970`, open and unmerged. No deletion, archival or settings change.
   Hosted run 36334032094 did not start its jobs: GitHub reports failed account payments or a
   spending-limit restriction. Human reviews are absent. This deprecation delivery remains blocked
   on those external requirements; no billing or repository setting changes are authorized.
3. Transfer the batches below in dependency order onto canonical history. Each coherent increment
   gets a pushed branch and PR before the next substantial increment begins. Split a batch further
   when its actual diff is too large for focused review. Adjacent documentation-only source commits
   can accompany the implementation they describe, with all source IDs retained in the PR.
4. Resume unfinished work only after its prerequisites are represented in canonical PRs. The pending
   whole-entry restoration follows I06; desktop restoration controls remain incomplete.

A stacked PR must name its dependency and actual base branch in idosams/Mesh. It does not count as
merged because its parent is open. Preserve alpha.5 localization, import fixes, validation and all
existing checks. Do not replace package scripts or documentation wholesale from the older branch.
For every increment record replacement commit(s), PR URL, exact local validation, hosted CI and merge
status. Source test logs are historical context; rerun the actual canonical checks. Human merge
authorization is separate and mandatory; no self-approval or check bypass.

The owner made tested, merged delivery an explicit goal requirement and subsequently authorized
merging without human review for this effort. This is merge authorization, not a completed code review. Integrate dependencies in order, refresh
checks after base changes, and verify the final combined canonical main revision. Keep source PRs
and replacement mappings traceable when a correction is delivered through another increment.
The explicit owner authorization waives named human review for this effort. Do not infer such a
waiver merely from absent GitHub branch rules; required checks still apply. Until final main and the full acceptance journeys are verified, the goal stays
incomplete even if all migration PRs have been published.

The following CI corrections were originally published independently against canonical main and
remain unmerged. The validation increment stacked on #19 now transfers their final patches into
this fleet stack; it does not merge, close or rewrite their original PRs. [Mesh PR #5](https://github.com/idosams/Mesh/pull/5),
`ba211c7c4ea11305fcbaacd34e689cfdb71481d7`, tests the unchanged recovery deadline calculation with
an exact clock, synchronizes the persistence-refusal fixture, and gives real-worker observations an
explicit scheduling budget. Hosted measurements showed a requested 19.915 ms wait had not resumed
after 155.816 ms. All three corrected recovery cases passed hosted run 36340947488; a separate
managed-edit stability test failed, so overall CI remains unresolved. The macOS gate collects all
failures without retrying or skipping tests. [Mesh PR #14](https://github.com/idosams/Mesh/pull/14),
`d1e6567cb9141646501fa8a1724bfbf997e8e576`, synchronizes a counter test so its writer cannot finish
before its reader starts. That test passed hosted CI, whose later recovery test still failed.
[Mesh PR #16](https://github.com/idosams/Mesh/pull/16),
`b1cd55675b4d503a822ca25bdea366e2bc58f59f`, replaces an unsynchronized editor sleep with a direct
configured minimum-wait assertion. Its full local gate passed, and a temporary old-50-ms mutation
failed the corrected test at 74.709 ms against the configured 250 ms. Production code is unchanged;
hosted run 36342433986 passed six jobs but failed the existing persistence-deadline test on
macOS before reaching the corrected stability test (1,345 passed, one failed, 10 skipped).
The correction in #5 is separate and not included in #16.

The main-first delivery path now consolidates the same three CI corrections in [Mesh PR #5](https://github.com/idosams/Mesh/pull/5)
at `eec4448f45b63547e684f5a0c987ceefe5911d15`. It preserves the original #14 and #16 branches and
replaces their final patches with `c28264c` and `eec4448` respectively. Its full local main-based gate
passed (3,037 Rust tests, 543 desktop tests, 44 daemon checks). The six-file stable patch ID
`2c000b0a68b4d03e57e214331ba2ba4cbe739cf1` matches #20. All seven hosted checks passed in run 36346243491. Human review must pass before
main integration, then feature dependencies must be reconciled and tested in order; do not blindly
merge duplicate corrections. No correction has yet merged.

## Validation prerequisite transfer

The current validation increment combines the final patches from #5 (`ba211c7c4ea11305fcbaacd34e689cfdb71481d7`),
#14 (`d1e6567cb9141646501fa8a1724bfbf997e8e576`) and #16 (`b1cd55675b4d503a822ca25bdea366e2bc58f59f`).
Its actual base is `idosams/attached-version-comparison` (#19). This is a separate prerequisite before
further feature migration, not an approval or merged delivery. The original independent PRs and their
failed-run evidence remain available. These three corrections are additional canonical work and are
not counted as transfers of the 87 preserved fleet commits.

Product timer defaults and calculations remain unchanged; the worker uses the same calculation
extracted for exact-clock tests. Real-worker test observation budgets are now explicit rather than
assuming the host schedules a 20/40 ms wake within 150 ms. The persistence-failure trigger is armed
before the worker starts. Counter readers synchronize with the writer, and managed stability checks
the configured minimum wait directly while retaining separate external-edit coverage. macOS CI
collects every failure without retries or skips, and still fails for any failed test. None of these
changes establishes a product latency, packaged lifecycle or remote-execution claim.

Validation replacement: `82310c2a34fe0484948123b0ff84de4702641a18`,
[Mesh PR #20](https://github.com/idosams/Mesh/pull/20), stacked on #19; not merged.
The full combined local gate passed: 3,128 Rust tests (13 skipped), 558 desktop tests and 44 real
daemon checks. All seven hosted checks passed in run 36345404405 on `8331980e4093018b909328eba6316877405137fd`.
No product latency or packaged proof is implied; human review and merge remain outstanding.

## Move-settling validation correction

[Mesh PR #21](https://github.com/idosams/Mesh/pull/21) passed six hosted jobs but failed the existing
`a_newer_os_change_during_move_settling_stays_working` case on macOS (3,127 passed, one failed,
13 skipped). Its observer could replace the file after the 50 ms settling window; its 15 ms sleep
did not establish ordering. The failed log remains retained. This does not establish a pinning defect.

[Mesh PR #23](https://github.com/idosams/Mesh/pull/23), `1911af5902149ea9ef730a8ae3cea31964d7b666`,
relocates that regression into the native unit suite and uses a test-only thread-local hook after
real durable move persistence and before settling. All original outcome assertions and timer values
remain, with additional hook-execution and durable-operation assertions. Temporarily disabling the
native moved-entry stability predicate makes the regression fail; the restored test passed 20 repeats.
All seven source hosted checks passed in run 36347562369. Production builds contain no hook,
and the total test count is retained. No retry, skip or product
latency claim is added.

This separate validation increment transfers #23 onto #22 before further feature migration. It is
additional canonical verification work, not another transferred commit from the preserved 87. Its
full combined gate and hosted CI must be checked on this base; original PRs and histories remain
preserved, and duplicate corrections must be reconciled before main integration. Replacement
`3e0e81c` passed the full combined gate with exit 0: 3,131 Rust tests (13 skipped), 564 desktop tests
and 44 real daemon checks. [Mesh PR #24](https://github.com/idosams/Mesh/pull/24) publishes this transfer; all seven hosted checks pass in run 36348196200
and no merge is implied.

## Preserved source commits

The source IDs identify preserved original work. The replacement column was refreshed on
2026-10-04 from merged PRs and verified Git ancestry at the main revision above. Initial
implementation refs remain historical provenance; the listed final PR heads are merged ancestors.

| Batch | Preserved source commit | Change | Canonical replacement |
|---|---|---|---|
| F01 | `c4e5b9450962b5bec818022d6e7936f2d5204b89` | feat: establish durable fleet lifecycle and isolated lane allocation | Merged: [#2](https://github.com/idosams/Mesh/pull/2) head `526cf26a260fb1e1edee6c248a4535cd1ea0d911`; [#3](https://github.com/idosams/Mesh/pull/3) head `6da0e5a12de572ea4d1cb7323da4c88ffb4eab29`; initial implementation `c0050814fe8b0c96b3a33065d269faea6a0a07ff` |
| F02 | `e1b1aeed6461af349fc943299c31376b5c358739` | feat: connect scoped agent delegation to native workspaces and MCP | Merged: [#4](https://github.com/idosams/Mesh/pull/4) head `95010fa920510a183f28e1f0a204ba8e30f67018`; initial implementation `905db96883ca8989eedbddcd8650332e1d24da3c` |
| F03 | `766852b17e8e06fbd01d5cf8b2504947fb19a280` | feat: capture private agent files under exact native custody | Merged: [#6](https://github.com/idosams/Mesh/pull/6) head `9d74a6ac47abe748e2b11b29931915aa2ec8c906`; initial implementation `451191520d67a35d2f66e4b5d286eb3622da3653` |
| F03 | `4be9ae2f8a0de2515e10ee7fdd9be486ff8ddaa1` | feat: capture agent workspace additions with explicit partial results | Merged: [#6](https://github.com/idosams/Mesh/pull/6) head `9d74a6ac47abe748e2b11b29931915aa2ec8c906`; initial implementation `451191520d67a35d2f66e4b5d286eb3622da3653` |
| F03 | `b0da3c728cfb84dcd1d98dbe2e214a5977016c79` | feat: expose durable signed checkpoints through scoped MCP sessions | Merged: [#6](https://github.com/idosams/Mesh/pull/6) head `9d74a6ac47abe748e2b11b29931915aa2ec8c906`; initial implementation `451191520d67a35d2f66e4b5d286eb3622da3653` |
| F04 | `4741d895b3eb34a5f9c8d6e7a8c482c527cbc8a6` | feat: submit completed agent checkpoints as immutable reviews | Merged: [#7](https://github.com/idosams/Mesh/pull/7) head `d62c9aa486c54e5513859560e9d76a23a423d908`; initial implementation `592dfbf3d7363053073a106d4918e1169af8e9af` |
| F05 | `0d6127f1371bec3cbf150bcf2540097fbf980aad` | feat: launch scoped Codex workers with durable ownership claims | Merged: [#8](https://github.com/idosams/Mesh/pull/8) head `28aa5e29e116d8db6e4f8bd96f1c6704c28ffad1`; initial implementation `5f39c955f4e3c982c762a39b125a442f616b486c` |
| F05 | `ca9b48a59f14934e9b05dacef1ecbaa9c93453f3` | feat: schedule delegated Codex workers within native fleet limits | Merged: [#8](https://github.com/idosams/Mesh/pull/8) head `28aa5e29e116d8db6e4f8bd96f1c6704c28ffad1`; initial implementation `5f39c955f4e3c982c762a39b125a442f616b486c` |
| A01 | `c6e62e0be68307fa315928d0d90bb6eaf8852b94` | docs: prioritize non-disruptive existing-project attachment | Merged: [#9](https://github.com/idosams/Mesh/pull/9) head `62bb029b78ae5cec030727a6f94cac8f763bbaab`; initial implementation `7ce3b0c484ef482ca5a39385d11e38f8206a64d0` |
| A01 | `09f4d863796aa52c7fc60530dfd1a73a68e94178` | feat: register existing projects without moving or taking custody | Merged: [#9](https://github.com/idosams/Mesh/pull/9) head `62bb029b78ae5cec030727a6f94cac8f763bbaab`; initial implementation `7ce3b0c484ef482ca5a39385d11e38f8206a64d0` |
| A01 | `920a71f36fa21e4646129a75d5bfd086630c9c25` | feat: observe attached projects through bounded native inventories | Merged: [#9](https://github.com/idosams/Mesh/pull/9) head `62bb029b78ae5cec030727a6f94cac8f763bbaab`; initial implementation `7ce3b0c484ef482ca5a39385d11e38f8206a64d0` |
| A01 | `ddf6086860b38c104a4e199e4b770800fa34b715` | feat: capture immutable inputs from attached projects without taking custody | Merged: [#9](https://github.com/idosams/Mesh/pull/9) head `62bb029b78ae5cec030727a6f94cac8f763bbaab`; initial implementation `7ce3b0c484ef482ca5a39385d11e38f8206a64d0` |
| A02 | `41bc6bf58615da3ff73922be2bb968b92925bd42` | feat: commit captured file batches as one durable signed version | Merged: [#10](https://github.com/idosams/Mesh/pull/10) head `0488b38807145a87abb9b70662e4bf8b7fc4fee9`; initial implementation `f6bda8b1b915367bd7f9b6e0f981ecf4a1a64521` |
| A02 | `f5860ae7d0647f6539616890037b3b4ec1080ffc` | feat: save attached-project versions in external native history | Merged: [#10](https://github.com/idosams/Mesh/pull/10) head `0488b38807145a87abb9b70662e4bf8b7fc4fee9`; initial implementation `f6bda8b1b915367bd7f9b6e0f981ecf4a1a64521` |
| A03 | `9e3e367c9be2f72fecd2fcd6304fbbac5ac31035` | feat: reconcile attached projects with a native background capture controller | Merged: [#11](https://github.com/idosams/Mesh/pull/11) head `68b2f8597b0b4384bb5634152cd9bcd0f635dbb7`; initial implementation `eb344f5460a332d7daa545e1d051b17368227367` |
| A03 | `8e595348462ebcdc772e71f455f5b1f457aae109` | feat: expose native attached-project capture commands for harnesses | Merged: [#12](https://github.com/idosams/Mesh/pull/12) head `fa2c05a210690e57de10d1df49d92592a6212be3`; initial implementation `4259c8b8c8815d8145e457167e7ec7990d081ff2` |
| A03 | `f8ec9022b9dbe6a44b6a8fa8894758e6962b1a62` | feat: provision native external attachment history storage | Merged: [#13](https://github.com/idosams/Mesh/pull/13) head `de7310f268e841f5b40e42b1384d6fd42ff23b9e`; initial implementation `5f4fe06673e476223ad53edad515b7f4d411521b` |
| A04 | `76f453c67687525a88dd63171530e63a722eaead` | feat: add native desktop attachment session controls | Merged: [#15](https://github.com/idosams/Mesh/pull/15) head `bc9a3b43bedc597fd52afac55f6b8af1f8df90d2`; initial implementation `e2cf7a29922e772d4f4de41ad9429e1879c3a65e` |
| A04 | `367e86a1b933d167083a0cb396e62b0ac3695710` | feat: connect existing-project attachment controls to desktop UI | Merged: [#15](https://github.com/idosams/Mesh/pull/15) head `bc9a3b43bedc597fd52afac55f6b8af1f8df90d2`; initial implementation `e2cf7a29922e772d4f4de41ad9429e1879c3a65e` |
| A05 | `29caceb1bfcb353abe1a6d8927adc8a05fa7c3ca` | feat: browse exact attached-project version history pages | Merged: [#17](https://github.com/idosams/Mesh/pull/17) head `416d88edc65c76e4cc2497546477062bd4a0107c`; initial implementation `c7a7b557a326d4754597649e9317e65e5fc02236` |
| A05 | `d52640a58edc6fb2b9ab07dd59ddbcb087c31499` | feat: inspect immutable attached-project files in desktop | Merged: [#18](https://github.com/idosams/Mesh/pull/18) head `6e5b4367e49b4fa04431c85651007bffd842fd5a`; initial implementation `493927c7461f5beeff1ae686c0e8d0b4cce43d5f` |
| A05 | `923187d270568917c7805435b3e3020ab353d51c` | feat: compare exact attached-project versions with pinned previews | Merged: [#19](https://github.com/idosams/Mesh/pull/19) head `acec455647dfe53012b0e47a465b7013db3d2d13`; initial implementation `8c4884efdbe7bd34d3f05079193324df1a5e534c` |
| A05 | `9a68afb42be45259902fa5061970d297223c7eae` | feat: pin independent attached-project comparisons side by side | Merged: [#21](https://github.com/idosams/Mesh/pull/21) head `3eb9cb22f4250eeb30d6a9183a4a7d45f91d4940`; initial implementation `548fc7b1c488dc9f123a5aee7195e9eaa8f74831` |
| A06 | `894127b78ba9d1c4104b013fd3c3e31dce15a504` | feat: restore registered attachment projects stopped after restart | Merged: [#22](https://github.com/idosams/Mesh/pull/22) head `63b865fe49c0d51123115ff0e874b6e2f7cee965`; initial implementation `397b98b0d3c22814c4f3b569d575e1b02de6ab66` |
| A06 | `d2d9e8c5ca43e592dd5eaeaa889fe21e067c3cec` | feat: persist native comparison pin selectors with revision checks | Merged: [#25](https://github.com/idosams/Mesh/pull/25) head `ca85765b7c3d84192277f3c5f693f1f87d5cd43b`; initial implementation `c8bb6351ea8c11d30f52c07723a5fd8e0f5827ab` |
| A06 | `db065842401b6743d4b44f6f71517306be09d33e` | Restore attached project comparison pins through native history | Merged: [#26](https://github.com/idosams/Mesh/pull/26) head `87d640ca27e934213b5acab93db3b48d07c8e3e8`; initial implementation `4cbd1ca0e87f35b31aa39743a2460cadc789acbb`, `1ed78329fd929dda28e6d4d00d101d1b10dfad31` |
| A06 | `ba65636e1a097a5491172226859c6d61994d8539` | Persist project detachment while retaining history and ordinary workflow | Merged: [#27](https://github.com/idosams/Mesh/pull/27) head `0afd976335255581bef6d42ba5a488894cce26f3`; initial implementation `a7bda0f5105d77fd048ef704cadec8cfe21ada54`, `74a1c0d6f90bfe0dbff17b9b8fac01a593011ed0` |
| A07 | `e3a2dc84ca3fcc33997cb802a0d7ccb319f921c8` | Verify packaged attachment capture against exact sealed bundles | Merged: [#28](https://github.com/idosams/Mesh/pull/28) head `c55b3e912406b22bae12ee03970c23494287eb8d`; initial implementation `7ddffdee0c8e2f57f4c1c0b490c785c7f4630ccb`, `fda9df38835421d72ca71abff2ac0c568162297d`, `fda9df38835421d72ca71abff2ac0c568162297d`, `716de71ecfe96b6a078447f2c960ccc9e1795d7e` |
| A07 | `f99295541624f312172947ec75458dd6ae01bcb3` | Wake attached project capture from native macOS filesystem events | Merged: [#29](https://github.com/idosams/Mesh/pull/29) head `55513452e5bf3cc5cb5e5ee638522c5be8b64ef0`; initial implementation `c80c9fab57a19f10589bdb8bd9cf6cbdebb5f9e4`, `052e0d24254ce7066b433997c2c50c524e4640d4` |
| A08 | `e320c5c928ad01af204570566f546daf8a76a045` | Record exact review requests from attached project history | Merged: [#31](https://github.com/idosams/Mesh/pull/31) head `89f7eea5e46549553aa9e6b5b1e82f65f962e101`; initial implementation `129966e4d669ee969fa80312baabc814e3f02442`, `60b9234ee123d242980f472a2558766f0b62659f`, `2f0e92c49a86f659995a0e526766eec020de4de7` |
| A08 | `0823850023496505f3c45074ea972c2b36c1dbc0` | Preserve pending review bases as shared main advances | Merged: [#30](https://github.com/idosams/Mesh/pull/30) head `49d0a58cffec847f65f8f7a8bd9076682c459d6c`; initial implementation `9b17bf50e10f4c972dd3dae371bb691c26913c6b`, `a054186c4ee0828239dfa247de420507077c3b10` |
| A08 | `60b9234ee123d242980f472a2558766f0b62659f` | Add exact human approval for attached project main | Merged: [#31](https://github.com/idosams/Mesh/pull/31) head `89f7eea5e46549553aa9e6b5b1e82f65f962e101`; [#32](https://github.com/idosams/Mesh/pull/32) head `8c0ab648e6166ef4df1411581aea83abbc11cb99`; initial implementation `893dca74c13f919036397dc7cf2af6baff88c667`, `f58b2107fd3da5667c816d11a701de559d560fe9` |
| A08 | `16eda49617c2e1f146950c2e08b3b0169a63884e` | Wire attachment main review and approval into desktop | Merged: [#33](https://github.com/idosams/Mesh/pull/33) head `cb748dc42abb29e596982a84709bbd5ab5772033`; initial implementation `68ab496a804204111b3087ee7c7fc9a1640adb55` |
| A09a | `c15c354d30178972b90bf919c051328f8ec2637b` | Preview accepted main against ongoing source work | Merged: [#35](https://github.com/idosams/Mesh/pull/35) head `0407e8eeba96ee9b1c1ac1c19d56a7d5dc6baa91`; initial implementation `3119b8b6233d17309d283d30d6aa855c2a14b8ce` |
| A09b | `38d0a9386e3fef2c3670ff371533e204e1bc76e3` | Retain displaced attachment files during native integration | Merged: [#36](https://github.com/idosams/Mesh/pull/36) head `2bb64c51da4e52c4e020d66626e8501c77397981`; initial implementation `c7843205da566684a9541010e03e1a49148e442c` |
| A09c | `7d06b8bd68d77ac9c358bf9002c829cd02995fa0` | Inspect retained integration recovery without replaying writes | Merged: [#38](https://github.com/idosams/Mesh/pull/38) head `05fd3e742d2fcb69ab1d58f7cac9f275181a345e`; initial implementation `7ccdc4be19e66840bc005b6ba9302e7bc706807a` |
| A09d | `11df227f749aa8654fe89a0612e8f2b286c29b4a` | Restore retained work through a new preserving transaction | Merged: [#42](https://github.com/idosams/Mesh/pull/42) head `8dc01603d7e07bc7a969d845b608addd5d13df47`; initial implementation `c6dfcf3ba53a932c4f12e41fdea45c1ca16e5f55` |
| A09b prerequisite | `633af5ca9d81e6c71532b24332fe4310dac0899d` | Preserve file allocation identity while copying native metadata | Merged: [#36](https://github.com/idosams/Mesh/pull/36) head `2bb64c51da4e52c4e020d66626e8501c77397981`; [#42](https://github.com/idosams/Mesh/pull/42) head `8dc01603d7e07bc7a969d845b608addd5d13df47`; initial implementation `c7843205da566684a9541010e03e1a49148e442c` |
| A09 | `1c332ba96d3a74c33701143ef54003f35895e1e3` | Connect attached-file recovery to native desktop confirmation | Merged: [#43](https://github.com/idosams/Mesh/pull/43) head `f1a435410dbef81851b622f724667548fdb0ef06` |
| L01 | `2680f5b1c99e5a4b8c96f24b7678c7d2afe32518` | Open attached saved versions as independent work lanes | Merged: [#44](https://github.com/idosams/Mesh/pull/44) head `c5f9ead824c6062698cc089185a411e405211c66` |
| L01 | `c728c41c9831e636a6bdfbb9b0212739ad6ba974` | Connect attached saved versions to managed fleet lanes | Merged: [#45](https://github.com/idosams/Mesh/pull/45) head `ecdf556f2420fca4f484e09054d54e0cf0d99ece` |
| L01 | `e987565983cc7f57be33fe211f9f4bb4d290fad2` | Expose scoped fleet MCP through the packaged desktop app | Merged: [#46](https://github.com/idosams/Mesh/pull/46) head `ad133b56991f40f97d61471cf3596e9cee458e7d`; initial implementation `1434d269759e0ca62d9bc14b62ec6202580133a5` |
| L02 | `877188898d0151d958bb06230b2e3797611a5ff2` | Persist native fleet discovery without adopting uncertain workers | Merged: [#47](https://github.com/idosams/Mesh/pull/47) head `30cbc1d73521f589e634a465ee01ff54a25cf711`; initial implementation `046ba075efc9290f9402ca2f3e4269610d1053fb` |
| L02 | `60a1c5bc5c573a4b87e57d9f8b0b9c7a1c28c3e9` | Connect desktop-owned fleet scheduling and activity | Merged: [#48](https://github.com/idosams/Mesh/pull/48) head `eaad28111ce07e9d61cff42c1247da8cc2df7f26`; initial implementation `f1578135da3da1aca14d77612117caf8ac031620` |
| L02 | `c7922b3d74cae8715066e2868533ba714d67ddb0` | Connect fleet provisioning and live lane controls to project view | Merged: [#49](https://github.com/idosams/Mesh/pull/49) head `0e25ae670df2f5fa28da3c87bf55c22d4eff5feb`; initial implementation `4a6126868cbe26b59c751488e2aa56f4dbe5072f` |
| R01 | `7d617e60828c6d6cae0e71a52f60fd4095eaed9a` | Read pinned fleet reviews independently of live work | Merged: [#50](https://github.com/idosams/Mesh/pull/50) head `be377cdf7491ef88f28a05afe5aa92a442208381`; initial implementation `c2c625a1ff7fe9bfc0acd42f0655e637890f24d7` |
| R01 | `2e8992f8bbb6c990e399a4a308969c7f88a95d37` | Connect exact fleet results to independent parallel review panels | Merged: [#51](https://github.com/idosams/Mesh/pull/51) head `f5ca25074c929b7f4f0de89c5565241c2ffd2e36`; initial implementation `35f486661ed4aa82ea59327da7cec75e3a2da8f5` |
| R01 | `b1a166da5cfb714c6677b02b0343e816ea47fcf4` | Bind fleet comparisons to verified starting versions | Merged: [#52](https://github.com/idosams/Mesh/pull/52) head `367c0493b19334f436fffce3627f00ae105d8fff`; initial implementation `90be90858d8a8c1271160edc8bdccd3b160c26a5` |
| R01 | `5a553e78177cc045f10af15a2da86bdb32c95bf0` | Show starting-version comparisons in pinned fleet reviews | Merged: [#53](https://github.com/idosams/Mesh/pull/53) head `51d2783e5611641580764b6177da12e3370cd9d2`; initial implementation `4fa921244a3c02322114c60e7e09c54764621a06` |
| R01 | `5b9f758c2db294b636bc2bfd9b4eb867a2fcb2ff` | Persist exact fleet review selectors through native storage | Merged: [#54](https://github.com/idosams/Mesh/pull/54) head `fdb951a07871a93ebc729e2eca47bc3cce2f26ac` |
| R01 | `20b01d6023616e02e2c9e5cabdc82dc39bb82c3b` | Restore exact fleet review selections and independent view state | Merged: [#55](https://github.com/idosams/Mesh/pull/55) head `b9d3f48c1b4adc2fce79099c7941a592a5eda17f` |
| R02 | `b6b388caf30c7ffb1cc4d23f09f7175e0a220089` | Persist verified lane starting versions for history recovery | Merged: [#56](https://github.com/idosams/Mesh/pull/56) head `f9c3f3ee60ef48f4b76c1a8ca858103114352b88` |
| R02 | `ce73d00a2a762eb514d47fbb429e79673157e3e3` | Reopen saved fleet history without adopting execution | Merged: [#57](https://github.com/idosams/Mesh/pull/57) head `835ff42fb1c292a79e67170e20985a2422b9533f` |
| R02 | `e961781d941c938de408b38866c4b43cee36f7fe` | feat(desktop): preview exact saved artifacts in parallel fleet reviews | Merged: [#58](https://github.com/idosams/Mesh/pull/58) head `55ccace07a4e50b73b926b1dbb2f96e1307f83ff` |
| R03 | `76d24877696474f2383429ca0b80c8eeab3ba3f4` | feat(fleet): record exact review change requests for originating lanes | Merged: [#59](https://github.com/idosams/Mesh/pull/59) head `5ea158f9d53b4d247d457f50a1808bb56489f02d`; initial implementation `27ce54b6912dd2da9aafae628cadfec52537e30e` |
| R03 | `367ec923ba6e1bcd3d15d62429ddec7ca49bda06` | feat(fleet): link proposed saved results to review change requests | Merged: [#60](https://github.com/idosams/Mesh/pull/60) head `b65a236c2e6b7ff71b9cf2c0c75759b0c6d3ef30`; initial implementation `dc315053ac01b86cc5125155644a4f726a2be2b6` |
| R03 | `f14d5344ebb5826a63aa9f2878902676e6f0140a` | feat(fleet): confirm reversible review request decisions | Merged: [#61](https://github.com/idosams/Mesh/pull/61) head `0060423e1a2525c32924b72e203affa6328cdb44`; initial implementation `3fbc74fe2c5ba8e86d0686807687394bf0b97ae1` |
| C01 | `302f6cbb0a795b9743ef6c72447d803cdc7860ab` | feat(fleet): verify original project input correspondence | Merged: [#62](https://github.com/idosams/Mesh/pull/62) head `4209afcddcc457d8f730b21b97180030b84b45f9`; initial implementation `d68ea27f6876f3a206d23ec564e706d7098dd45a` |
| C01 | `ee7b74fc47a831a3b7eb3827647f7aa841621fa7` | feat(fleet): trace delegated results to original project inputs | Merged: [#63](https://github.com/idosams/Mesh/pull/63) head `81e72fc267bcfe0bb5ccad37886218f1ffe2df26`; initial implementation `a9bc1c78f486cdfc55a0c9e5985feddf4ca8aaf7` |
| C01 | `0f79d16c0456d83e707e6652e8bd4e3774683eea` | feat(fleet): stage exact project candidates outside capture history | Merged: [#64](https://github.com/idosams/Mesh/pull/64) head `068691d3d2d3deb55a14ba3bd841f39bcbb9646c`; initial implementation `7d2afb61254ef46c691f760ff66a6ed0d52f7ad4` |
| C02 | `27b5efc316a84a896189cb85952fa468c5e5b57c` | feat(fleet): review candidates against fixed project main | Merged: [#65](https://github.com/idosams/Mesh/pull/65) head `b26a97e607772cca2ee8aea146274587f2e412ef`; initial implementation `de5e2f03e68a4d056d99551c1c9e52316c46201d` |
| C02 | `2263d5db328d3aab957218642c61e3e3b94d6047` | feat(desktop): expose fixed fleet project candidate reviews | Merged: [#66](https://github.com/idosams/Mesh/pull/66) head `4d76f4ce987a79c43af49f79bd7e19e87e646a6a` |
| C02 | `c53e4fdc3494eb0037fa43326d2a56e4c7c3a77b` | feat(desktop): pin project comparisons with durable preparation inputs | Merged: [#67](https://github.com/idosams/Mesh/pull/67) head `3dcef8c73b964bb03073773ec1f44afea61f618f` |
| C03 | `3aef42a9373ff0ebb84fd482819d39536b022ab2` | feat(history): prepare operations against exact saved ancestry | Merged: [#68](https://github.com/idosams/Mesh/pull/68) head `97c8c5d5fe3bbd1a06a2ce9f322cfacdb6e4e74e` |
| C03 | `d3b34517d6f9f390f2834a141717254861bc5358` | Keep attachment captures independent of candidate branches | Merged: [#69](https://github.com/idosams/Mesh/pull/69) head `ec3d4052ae10f9d24051676973a5f1c206b9913c` |
| C03 | `0d6114701a778f1277cbd69ff2bc3280feb2a716` | Compile fleet candidates with original project object identity | Merged: [#70](https://github.com/idosams/Mesh/pull/70) head `46d473879e07b24f51bef5e9a2bb816c1448b02e`; initial implementation `46d473879e07b24f51bef5e9a2bb816c1448b02e` |
| C03 | `a8d3bd00105941a9ceb23bd9215b87d367645d5a` | Persist signed fleet candidate imports with exact retry recovery | Merged: [#71](https://github.com/idosams/Mesh/pull/71) head `985adffffd29b3202c6cff22e6964c7454820d2c` |
| C04 | `3a3b0da0139b16fb527de4bf8a9ece0d9772f523` | Connect candidate imports to desktop fixed project reviews | Merged: [#73](https://github.com/idosams/Mesh/pull/73) head `64f9278fdb0bb56c7a69a5fbf60d9e43f635c477` |
| C04 | `15111733b9da3565d5a1ddd8aba6457b0b2f85b9` | Open exact project reviews directly from fleet panels | Merged: [#74](https://github.com/idosams/Mesh/pull/74) head `cd804ad3708b1056ae29e84019575dfe4e6b42bb` |
| C04 | `0ccc58c78b0278b4a8e84cc9de7fdaa073563ccc` | Persist exact pending fleet review operation inputs | Merged: [#75](https://github.com/idosams/Mesh/pull/75) head `a4c8f89a1efcba0aa3bf64ad420a8901820b41c7` |
| C04 | `7497890ae6ae63a64974e958d720c4e446caf9ef` | Recover pending fleet review submissions through the desktop | Merged: [#76](https://github.com/idosams/Mesh/pull/76) head `a87005e8db37bb917f43468a91c02d1dd9662d91` |
| D01 | `62e8c885ab9c72fa257b1f3cea2483b650e3a4a8` | Bind explicit file deletion resolution to agent custody | Merged: [#77](https://github.com/idosams/Mesh/pull/77) head `0e3c36b30354c11bb61c5aaa7fe80f17bbbf907d` |
| D01 | `278beb6d5549a2380953f0cbfb2e1351566637df` | Add durable explicit agent file deletion recovery | Merged: [#78](https://github.com/idosams/Mesh/pull/78) head `35b20458158fce6f5d281906ef1a05845483080b`; [#79](https://github.com/idosams/Mesh/pull/79) head `19e85e29f400f3974c4d72583e93d214f384570f` |
| D01 | `6c40c1e1c6562a931d533219032d4c2f0e04f3f2` | Record exact packaged fleet deletion verification | Merged: [#80](https://github.com/idosams/Mesh/pull/80) head `4e3daa214e7ccb63dac64622f34e571d53491ec5` |
| D01 | `c7ac2e33de18a20b2517e23b667b0b3847e0ed19` | Make deletion-only lane results inspectable without approval authority | Merged: [#80](https://github.com/idosams/Mesh/pull/80) head `4e3daa214e7ccb63dac64622f34e571d53491ec5` |
| D01 | `c1835ae651f4ca57edd399be0697b3c73a17520d` | Record packaged empty-result inspection evidence | Merged: [#80](https://github.com/idosams/Mesh/pull/80) head `4e3daa214e7ccb63dac64622f34e571d53491ec5` |
| I01 | `496debf33e758b05985c493a7ecb30a230fdcd42` | Add native accepted-review replacement groups and retained recovery | Merged: [#81](https://github.com/idosams/Mesh/pull/81) head `c80d0fc70bfa573985d45ee1de875e90a005daf8` |
| I01 | `0a1af9129359a9bfbfe1ef40131d2c38d6ffd481` | Reuse bounded project captures across integration groups | Merged: [#82](https://github.com/idosams/Mesh/pull/82) head `5f124dc2ac496e9c8e3e66353018e4498b41b8a0` |
| I02 | `319a4cd7a0d093689e6aa2d4f168830376a01337` | Retain approved file removals in native integration groups | Merged: [#83](https://github.com/idosams/Mesh/pull/83) head `04408cf8c89bef5d307fa13747282f347dbd43dc` |
| I02 | `b8e9d0dcf1cafd65a06a390a1222579941b3236b` | Add approved regular files through retained integration groups | Merged: [#84](https://github.com/idosams/Mesh/pull/84) head `dea7d1c6bd0b7c468fe3e4cccbd43fd9a46751b4` |
| I02 | `425f2e06e2ab9ce2c7f01ed8013416527d8c7d3f` | Inherit destination permissions for approved file additions | Merged: [#85](https://github.com/idosams/Mesh/pull/85) head `ec305a6ada14d5d3e33d64d9fdde33d8c06203b8` |
| I03 | `59e0b5b13995e516ed112b09cd295311807523e3` | Restore retained files into absent attached paths | Merged: [#86](https://github.com/idosams/Mesh/pull/86) head `4e55abd227327f3b5bd7387d14e9a831b0d4a513` |
| I03 | `fc5b37d9bea6edf4311dea4a2914145da4e600ab` | Connect complete regular-file groups to desktop confirmation and recovery | Merged: [#87](https://github.com/idosams/Mesh/pull/87) head `54ca0c3d991d90e4a0b88ac0dd29c2a3e5265f42`; [#88](https://github.com/idosams/Mesh/pull/88) head `6150790f6c8144c61bc1411f9e96206e5cde7630`; [#89](https://github.com/idosams/Mesh/pull/89) head `cd3405e3dd6f4604837e8c37ba04b2ef620908b1` |
| I03 | `9bfc1f43466ec5a45082a854e51421dab2ae32d2` | Reopen durable group execution evidence in recovery views | Merged: [#91](https://github.com/idosams/Mesh/pull/91) head `cb5a7cf4071e3702331d730529c01df29ea0d995`; [#92](https://github.com/idosams/Mesh/pull/92) head `d2ea4b19edb588948b5b94e3ee363a0cceb0c536` |
| I04 | `c711c6531220f61bfe5e9994f12df19c4450bd2c` | Integrate approved directory subtrees in review groups | Merged: [#93](https://github.com/idosams/Mesh/pull/93) head `c4b12ffc29c5ffc106fe89b0d2a4fec8ca6c88c0`; [#94](https://github.com/idosams/Mesh/pull/94) head `eafef923292ffde5c717b0428ed3276afb045fe8`; [#95](https://github.com/idosams/Mesh/pull/95) head `0f5a5b4276fc2a9a91193edca32a5226f7a7e95b`; [#96](https://github.com/idosams/Mesh/pull/96) head `62de5dfaa3b441f90770594de2a91bfe539c7c36`; [#97](https://github.com/idosams/Mesh/pull/97) head `43df0cdd173036d96aa8eb35d029b98207dec0ab` |
| I05 | `a02b33d8c7454a440d3ca94ae22f34e762913cc6` | Retain approved directory removals in review groups | Merged: [#98](https://github.com/idosams/Mesh/pull/98) head `1a95c9c59e21a850ae53c1962fabe4362a68a0e7`; [#99](https://github.com/idosams/Mesh/pull/99) head `c1cdf63721014eb774dc520d9b99df9a017571be` |
| I06 | `6ec9c8ccf9e273de25d1a84c323c0cd4f3f4b821` | Retain approved file and directory conversions in review groups | Merged: [#100](https://github.com/idosams/Mesh/pull/100) head `31b4330f547f83e40511c74576fcc657a2df5ec3`; [#101](https://github.com/idosams/Mesh/pull/101) head `cdb7e6153208ab1ad7b3b46e7819a640fc4c4861`; [#102](https://github.com/idosams/Mesh/pull/102) head `b50fe49f70bf81bd1ca60879e3a074be87f6d70f` |


## A08 delivery ordering

Transfer `0823850023496505f3c45074ea972c2b36c1dbc0` before `e320c5c928ad01af204570566f546daf8a76a045`.
The publication/history fix uses existing review and approval APIs and does not depend on the new
attachment review wrapper. Applying it first prevents the new review UI from inheriting the known
loss of pending-review presentation when main advances. The separate request, approval and graphical
increments keep their remaining source mappings and acceptance requirements.

The two-file independent-directory-lock prerequisite from `60b9234ee123d242980f472a2558766f0b62659f`
also moves into A08a. Cloned pinned roots share an open file description; duplicating that handle
shares flock ownership and cannot serialize concurrent callers. Each outer custody acquisition
now opens an independent description relative to the retained directory. Preserve this mapping
when transferring the remaining approval implementation rather than applying the fix twice.
An isolated host check demonstrated duplicate-handle lock inheritance and refusal of a separately
opened contender. The native regression failed against the parent locking implementation and passed with the fix;
all three attached-review regressions passed alongside it. Correction `1a7fede14c7b713d86dee7cf135d1d1d062d5a8f`
restores independent lock ownership. The corrected combined gate passed at `7fa2bdd41b631327278669032c6ee38593ca69ab`: 3,146 native
tests, 580 desktop tests and 44 real-daemon checks; 13 platform/provider tests skipped.

The original A08a gate at `129966e4d669ee969fa80312baabc814e3f02442` stopped with two existing
desktop capture waits exceeding ten seconds (1,690 native tests passed, two failed, 1,453 not run
after fail-fast, 13 skipped). Compilation in the independent correction build overlapped that run;
causation is not established. Both unchanged failing tests passed on a focused recheck (4.067s
and 6.371s). Preserve the failure log; neither the recheck nor the original gate validates the
corrected full tree. The corrected combined gate at `7fa2bdd41b631327278669032c6ee38593ca69ab` exited zero,
including both previously failing capture tests. No assertions or wait bounds were changed.

## Remaining full objective

Migration does not close any product phase. Native graphical approval and recovery, worker recovery
and wakeup, private dependency closure and combined review, a second real provider, measured four-worker
performance, remote execution on a second machine, and the final requirements audit remain explicit
work. Consult the full plan rather than treating this preserved commit inventory as the completion scope.

## A08b validation evidence

The native approval transfer at `893dca74c13f919036397dc7cf2af6baff88c667` passed the full canonical
gate: repository/docs/license/storage, formatting, clippy, 3,150 native tests, 580 desktop tests and
44 real-daemon checks. Thirteen platform/provider tests were skipped. The initial 13-test focused
run flagged one passing replaced-store case for a lingering process handle; the restored four-test
approval run and full gate had no such flag. Preserve the original focused log. Disabling the
challenge-reuse guard caused the test to fail journal preservation before append; the implementation
was restored byte-for-byte. A different approval ceremony also refuses retry without changing history.
These fixture-credential tests do not prove native human presence or packaged graphical acceptance.

## A08c validation and remaining acceptance

The desktop confirmation increment passed its full canonical gate at
`68ab496a804204111b3087ee7c7fc9a1640adb55`: 3,151 native tests, 587 desktop tests and 44 real-daemon
checks; 13 platform/provider tests were skipped. The runner flagged the passing detachment/restart
case for a lingering handle. Its unchanged focused recheck passed in 2.081s with no such flag;
the full-run caveat remains retained.
Five focused native approval tests, 28 coordinator tests and 107 UI tests passed. The new rendered
approval-state test fails against the unchanged parent and passes with the localized controls.

A read-only host check found zero valid code-signing identities. Actual Secure Enclave presence
acceptance therefore needs an eligible signed-build environment; the requested choice remains
pending. Unavailable-build behavior can be verified locally but cannot replace successful real
approval acceptance. The existing rendered-app verifier also still asserts IPC surface 7 while
the daemon exposes surface 8 and retains protocol 7 compatibility. Its pinned source assertion
must be reconciled and an actual packaged run completed before current graphical-proof claims.
These remain part of the full objective; working-folder integration and the rest of the fleet plan
are not removed from scope.

## Packaged verifier prerequisite after A08c

The rendered-app verifier still requested protocol 7 and required surface 7 after F02 introduced
surface 8. The daemon retains protocol-7 compatibility but truthfully advertises surface 8, so this
verifier could not establish a current packaged journey. A separate canonical correction on #33
requests protocol 8 while retaining the exact negotiated-version and advertised-surface assertions.
Its repository contract test now compares the proof's declared version with the daemon's declared
surface rather than pinning the stale literal. The regression fails against the original proof
(`7 != 8`). This is a validation correction, not a replacement of any preserved product commit.
Implementation `254ec5bafa5589874051da584413d44c07460dc8` passed the full canonical gate:
3,151 native tests, 587 desktop tests and 44 real-daemon checks; 13 platform/provider tests skipped.
There were no lingering-handle flags. All five focused local-app tests also passed.
A fresh sealed ad-hoc bundle at that exact revision passed the isolated rendered journey
(`mesh-rendered-app-proof/v6`, renderer v5), including import, restart, version selection, Files
open/reveal, review, private export and agent handoff. Both negotiated IPC and advertised surface
were 8. The 2400 by 1586 captured window was inspected; post-journey native identity and seal
verification passed. Executable SHA-256:
`c020f3f47e6ee019ed4d45cb5e5c6d82cf3ac6fce96674031319327a825aab3b`.
The prior sealed bundle was preserved and its executable hash compared before rebuilding.
This managed-workspace journey does not verify the new attached-project controls or actual Secure
Enclave presence. The bundle truthfully reports approval unavailable without validated Apple identity.
[Mesh PR #34](https://github.com/idosams/Mesh/pull/34) publishes this correction, stacked on #33.
All seven hosted checks passed at `ab5907c182d910661929c97fdded64f320b7ed9f` in run
36357973889; ready for required human review, unmerged.

The prerequisite [Mesh PR #33](https://github.com/idosams/Mesh/pull/33) passed all seven hosted
checks at `f27287d4e99e6c63e2a602a83a2c1283703c601c` in run 36356728345 and is ready for required
human review. It remains unmerged.

## A09a read-only working-folder comparison

Transfer source `c15c354d30178972b90bf919c051328f8ec2637b` onto canonical #34, preserving
English/Hebrew localization and the uncertain-approval refresh regression added in #33. Native
comparison binds the exact accepted review and original base, observes bounded current inputs, and
reports divergence without write authority. Removed ancestors remain absent from historical review
presentation even though retained objects still exist for recovery. Review paths are normalized to
confined relative names. Source integration and recovery remain separate dependent increments.
[Mesh PR #35](https://github.com/idosams/Mesh/pull/35) publishes replacement
`3119b8b6233d17309d283d30d6aa855c2a14b8ce`. All seven hosted checks passed in run 36358370954.
All seven native attachment approval/comparison tests, 30 coordinator tests and 108 UI tests passed.
The new localized rendering regression failed against the unchanged parent, and disabling the native
directory safeguard caused the expected conflict regression to fail. Both implementations were
restored byte-for-byte. The first full local gate stopped after an existing version-page test's
capture-stop wait exceeded ten seconds: 1,796 native tests passed, one failed, 1,356 were not run,
and 13 were skipped. Desktop and real-daemon gates were not reached. The unchanged focused test
passed in 6.179s, but the cause remains unproven and the original failure log is retained. A full
unchanged recheck exited zero at `3119b8b6233d17309d283d30d6aa855c2a14b8ce`: 3,153 native tests,
590 desktop tests and 44 real-daemon checks; 13 platform/provider skips. One passing sequence test
was flagged for a lingering process handle. Neither this passing recheck nor green hosted CI erases
the first capture-stop timeout; its cause remains unresolved and is tracked in
[Mesh issue #37](https://github.com/idosams/Mesh/issues/37). Resolving this repeated failure remains
part of the combined-main completion audit.
No packaged attached-project acceptance or merge is claimed.

## A09b retained replacement and metadata prerequisite

The regular-file transaction source `38d0a9386e3fef2c3670ff371533e204e1bc76e3` is transferring
on canonical #35. Its later allocation-identity correction
`633af5ca9d81e6c71532b24332fe4310dac0899d` accompanies this foundation rather than introducing
a known broad-metadata-copy defect first. This is native-only preparation/application, not a
desktop confirmation or recovery browser. The unchanged parent full recheck has completed; its failure and passing evidence are retained above.
Nine native retained-replacement/metadata tests and both approved-file integration tests passed.
The original broad metadata copy failed the allocation-identity regression by transplanting its
source creation timestamp; the corrected implementation was restored byte-for-byte. All nine restored
replacement tests and all nine attachment approval/comparison/integration tests passed.
Full canonical validation and publication remain pending.

## A09c recovery inspection

The read-only restart inspection source `7d06b8bd68d77ac9c358bf9002c829cd02995fa0` transfers
on published #36, preserving its allocation-identity correction and canonical history. Receipt
verification, bounded live observations and explicit uncertainty grant no write, replay or cleanup
authority. Complete directory inventories retain overflow refusal; a separate diagnostic prefix
reports an honest overflow flag. Original source checkouts remain preserved. The parent initial and
unchanged full gates failed during native startup; focused recovery validation began only after
that verification ended. The directory-prefix regression and all three recovery integration tests passed. Temporarily
removing trusted-history verification made the refusal regression fail with prepared-arrangement
instead of unverified-history; source was restored byte-for-byte and all twelve attachment
approval/integration/recovery tests passed in 7.10s. Documentation checks passed. Full validation
and publication remain pending.

## A09b initial local full-gate failure

At `c7843205da566684a9541010e03e1a49148e442c`, the initial full gate failed the existing
`native_root_change_wakes_capture_without_adopting_the_replacement` case after 8.069s. Its status
was still Starting, revision zero, with no native events or capture attempts; the test had not yet
performed the root replacement. The run passed 1,295 native tests, failed one, left 1,868 unrun
after fail-fast and skipped 13. Desktop and daemon gates were not reached. There was no concurrent
native build. Hosted CI passed the same revision; this does not establish the local failure cause.
The original log is retained. The unchanged focused recheck passed in 1.575s, but the complete
unchanged recheck also failed: both native-event cases remained Starting at revision zero before
any capture attempt (8.054s and 8.056s). It passed 1,292 tests, failed two, left 1,870 unrun and
skipped 13; desktop and daemon gates were not reached. Neither focused nor hosted passes resolve
[issue #37](https://github.com/idosams/Mesh/issues/37) or establish a passing combined gate.

## Native capture lifecycle correction and checkpoint dependency

[PR #38](https://github.com/idosams/Mesh/pull/38) published recovery inspection at
`7ccdc4be19e66840bc005b6ba9302e7bc706807a`, replacing source
`7d06b8bd68d77ac9c358bf9002c829cd02995fa0`. Its local full gate passed 1,298 native tests,
failed both native startup waits, left 1,868 unrun and skipped 13. Hosted run 36360628264
passed six checks, including macOS, but Linux failed immediate checkpoint database restart.
The focused recovery assertions passed; the full failures remain evidence, not a passing delivery.

Two separate corrections follow those failures:

- [Issue #39](https://github.com/idosams/Mesh/issues/39): checkpoint workers must release database
  ownership before daemon destruction returns. [PR #40](https://github.com/idosams/Mesh/pull/40)
  publishes `4816f5bb22efb179cd5ba6fc2b04b8bfe870cb1e` on the canonical validation floor (#23).
  All seven hosted checks passed in run 36361925547. Its local full gate is still running.
  The focused ownership regression passes, fails with the original non-draining behavior, and
  passes after restoration; all 26 checkpoint-save integration tests passed. This feature stack
  carries the same correction as `8285ffadc7eac74a81eca2ce64d631e92ed4b4f3`, with only the
  project-status append context adapted. This is a dependency transfer, not a second independent
  fix or a merged change; reconcile identical patches when updating the stack after main delivery.
- [Issue #37](https://github.com/idosams/Mesh/issues/37): an eight-process diagnostic reproduced
  15 startup failures in 16 unchanged native-event executions. A failing worker was sampled in
  the operating system's event-registration RPC. The current correction separates bounded optional
  monitoring from capture, retains generation-specific cleanup status and preserves periodic
  capture when monitoring is pending or unavailable. It is new canonical work, not a replacement
  for an untransferred source commit. Native, full, executable and hosted verification remain
  pending. The source and evidence contract is in the
  [native signal lifecycle decision](../decisions/attachment-native-signal-lifecycle.md).

All listed PRs remain unmerged and require the applicable human review. Retained restoration source
`11df227f749aa8654fe89a0612e8f2b286c29b4a` remains preserved in its separate unpublished worktree;
it has not been validated or delivered by this correction. The later metadata fix `633af5ca` is
already included in #36 and must not be applied twice. The full fleet, second-provider, four-worker,
remote-executor, packaged acceptance and combined-main verification objectives remain required.

## A09d restoration transfer after lifecycle corrections

The preserved restoration implementation from `11df227f749aa8654fe89a0612e8f2b286c29b4a`
now transfers onto [PR #41](https://github.com/idosams/Mesh/pull/41), exact base
`702100952a0d124044f18420fd752f3677fa8a8b`. The earlier staged transfer based on #38 remains
untouched in its original worktree and a binary patch backup. This transfer reuses its code and
resolved documentation, retaining the current canonical migration ledger. No unrelated histories
are merged. The metadata prerequisite from `633af5ca` is already included in #36.

Restoration freezes exact retained content and metadata, revalidates both inputs and their ancestry,
and exchanges a new staged copy while preserving the current inode. The original retained inode
remains available to its existing editor. Undo creates another preserving transaction. New receipts
bind the parent proposal digest and frozen snapshot; bounded ancestry includes the original
integration plus at most fifteen restoration transactions. Recovered private content is explicitly
not approved main. This increment adds no desktop, CLI or agent apply command.

Parent #41 passed all seven hosted checks in run 36363155064: 2,986 Linux native tests (6 skipped),
3,173 macOS native tests (13 skipped), a separate four-test macOS step, and 593 desktop tests.
Both platforms passed the four new lifecycle regressions; macOS passed actual filesystem callbacks,
root replacement refusal and detach/restart. The local UI status regression failed against the
previous view, passed after byte-exact restoration, and all 109 UI rendering tests passed. The
preserved local full checks and native failing-before/executable proof remain pending; neither #40
nor #41 is merged. These parent results do not validate the new restoration source.

This restoration increment still requires focused native/refusal tests, failing-before evidence,
full canonical validation, hosted checks and named human review. Native confirmation, offline or
renamed roots, external-volume UX, grouped changes, crash campaigns, Linux metadata support and
packaged graphical acceptance remain required by the full plan.

## A09e desktop native confirmation and recovery

Source `1c332ba96d3a74c33701143ef54003f35895e1e3` transfers on #42 at
`c6dfcf3ba53a932c4f12e41fdea45c1ca16e5f55`. The canonical English/Hebrew attachment UI and
literal identities are preserved, along with #41's monitoring lifecycle. Native recovery allocation,
exact inspection, complete-content confirmation and generation-bound apply/restore remain separate
from agent authority. This adds three native desktop commands, not agent IPC write capabilities.
The parent passed all seven hosted checks: 2,989 Linux and 3,177 macOS native tests, the separate
four-test macOS step, desktop/docs and all 44 daemon checks. Neither parent nor this increment is merged.

A transfer defect was found before publication: the source renderer accepted permission-only modes,
while actual native observations contain complete regular-file modes. The real-file metadata
regression failed against that source parser; the corrected parser and all 36 coordinator tests
passed. The new recovery rendering test failed against the previous view, the adapted component
was restored byte-for-byte, and all 111 UI rendering tests passed. UI type checking and production
build also passed. An initial localization failure for a missing attention label is retained;
the corrected bilingual test includes uncertain, unavailable and stale recovery states. Native
project/folder prompt fields are separately quoted; no human-dialog acceptance is claimed.

Local native tests, native failing-before proof, the canonical full gate and hosted validation
remain pending. Preserved full-gate session 4896 still owns the shared native build target; no
parallel native build was started. Actual packaged confirmation/recovery, eligible approval,
non-disruptive manual and agent journeys, remaining fleet phases and combined-main testing remain
required. Original checkouts, staged restoration and historical failed verification are preserved.

## L01a independent attached-version lanes

Preserved source `2680f5b1c99e5a4b8c96f24b7678c7d2afe32518` transfers on #43 at
`80e5eda24f8d56e5207a6ed2a01eb47fbd1ac9ec`. Native allocation, exact stable retries, bounded
content/type verification, independent capture and identity-bound folder opening are retained.
The transfer preserves the canonical English/Hebrew UI, literal identifiers, #41's independent
monitoring state and #43's native-mode recovery parser. Dynamic success copy becomes a localized
message; the allocated project remains independently validated and appears in the refreshed list.
This opens ordinary lines for users and existing harnesses. Managed fleet linkage remains L01b
(`c728c41c9831e636a6bdfbb9b0212739ad6ba974`), followed by scoped agent dispatch
(`e987565983cc7f57be33fe211f9f4bb4d290fad2`). Neither prerequisite nor this increment is merged.

All seven hosted checks passed for #43: 2,990 Linux and 3,181 macOS native tests, four separate
macOS tests, 599 desktop tests and all 44 daemon checks. Local UI type/build, 111 rendering tests,
36 coordinator tests and docs passed; its local native/full and packaged confirmation proof remain
pending. #40 completed its local full gate: 3,038 Rust tests (10 skips), 543 desktop tests and all
44 daemon checks passed. It is ready for human review, with all seven hosted checks green.

For L01a, 38 coordinator tests passed, including lost-reply retries without duplicate allocation,
source-version identity refusals and retained pinned views. New bilingual render tests cover
independent ancestry, unavailable history, pending retries, preserved capture status and source-only
main controls. The new lane rendering regression fails on the previous component; the adapted
component is restored byte-for-byte before rerunning. Native/full/hosted validation and packaged
acceptance remain pending until recorded against this increment. Source checkouts and prior failed
verification are preserved. The full fleet objective, human review and combined-main testing remain
required.

## L01b attached saved versions enter managed fleet lanes

Preserved source `c728c41c9831e636a6bdfbb9b0212739ad6ba974` transfers on #44 at
`857ec2836071e7d29d8d11d234437fd04b93da8c`. It adds a native managed-lane entry point,
exact staged-content admission and durable source-project correlation inherited by delegated
children. The original project keeps non-exclusive capture and ordinary editor access. Managed
workers receive authority through the existing scoped custody/session mechanism. The shared
materializer retains L01a's entry-type race refusal; #40's shutdown drain remains intact.

Canonical adaptation retains accumulated project status, acceptance criteria and provenance.
Additional regression assertions verify that changing a completed request's goal, provider or
source project refuses without changing fleet state, allocating another folder or replacing lane
edits. Neither this bridge nor its parent is merged. Desktop fleet hosting, scoped command-line
entry and real-provider acceptance follow as separate increments, including preserved source
`e987565983cc7f57be33fe211f9f4bb4d290fad2`.

Parent #44 passed three native lane integration tests, the entry-type race regression and the
independent desktop capture/restart test. A names-only inventory mutation failed the race test;
byte-exact restoration passed. The shared-target stale-method compilation failure is retained;
rebuilding unchanged current source resolved it. Parent UI type/build, 113 rendering tests,
38 coordinator tests and docs passed. Its full local gate and hosted Linux/macOS checks are running.
For this bridge, native regressions, the full gate and hosted validation remain pending. All
original histories, dirty work and verification records remain preserved. Required human review,
packaged journeys, every remaining fleet phase, merges and combined-main validation are outstanding.

## L01c desktop executable scoped fleet bridge

Preserved source `e987565983cc7f57be33fe211f9f4bb4d290fad2` transfers on #45 at
`630176fda577b58f5cae57dac8c218e6ad3055ab`. The desktop executable gains a dedicated fleet MCP
mode before graphical startup, requiring complete native session credentials and an absolute local
endpoint. The fixed native provider adapter can use that executable; the existing standalone bridge
and selected-workspace read-only mode remain available. Fleet context carries the exact embedded
build identity and refuses reserved-field collisions. No approval authority is added.

Canonical adaptation preserves the earlier build-identity mode and provider verification evidence.
An additional native regression covers both reserved fleet-context build fields and non-object
context refusal. The packaged procedure now requires before/after bundle verification and retained
executable identity; its opt-in test alone proves runtime behavior/revision, not the resource seal.
No packaged or real-provider result is claimed before running that procedure on this revision.

Parent #44 passed all seven hosted checks: 2,994 Linux and 3,186 macOS native tests, four separate
macOS tests and 44 daemon checks. Its full local gate remains running. Parent #45 passed its Linux
2,998-test native gate, including attached-root delegation/replay and changed task/provider/project
refusals; macOS was still running at transfer. Local native/full verification for this increment
waits for the shared target. Named human review, merges, packaged graphical/native approval,
real-provider/four-worker/remote acceptance and final combined-main validation remain outstanding.

## L02a durable native fleet discovery

Preserved source `877188898d0151d958bb06230b2e3797611a5ff2` transfers on #46 at
`1434d269759e0ca62d9bc14b62ec6202580133a5`. The macOS catalogue retains an exclusive directory
lease, identity-bound allocation receipts and guarded ledger access. Reopening never initializes
missing or empty history. Restored facts remain `restored-unattached`; no process ownership,
workspace custody, worker launch or original-project publication is inferred. Native desktop
provisioning retains the source capture generation and reports `started: false`.

The canonical transfer preserves the nonblocking capture lifecycle, shutdown drain, recovery and
build-identity corrections. An additional storage regression covers a committed event whose native
authority check refuses acknowledgment: reopening and exact retry must retain one durable event,
while changed input still refuses. This complements the transferred precommit rollback regression.
It is pending execution on the canonical branch and is not yet proof of crash or packaged recovery.

Parent #45 passed all seven hosted checks: 2,998 Linux and 3,190 macOS native tests, four separate
macOS tests and all 44 daemon checks. Parent #46 is published with hosted checks running; its local
native and packaged execution remain pending. The preserved #44 full gate still owns the shared
native build target. This discovery increment needs native/refusal, full-gate and hosted validation,
named human review and packaged acceptance before delivery. The separate scheduling and live-view
increments remain `60a1c5bc5c573a4b87e57d9f8b0b9c7a1c28c3e9` and
`c7922b3d74cae8715066e2868533ba714d67ddb0`. All later fleet phases, merged delivery and combined-main
verification remain required. Original work and histories are preserved.

## L02b application-owned scheduling and activity

Preserved source `60a1c5bc5c573a4b87e57d9f8b0b9c7a1c28c3e9` transfers on #47 at
`046ba075efc9290f9402ca2f3e4269610d1053fb`. Native desktop start/stop accepts a current catalogue
objective, chooses the admitted provider and its own fixed bridge, registers only the exact service
instance and retains one scheduling loop. Restored services cannot execute. Faults stop further
dispatch while the same owned handles remain observable/cancellable; uncertain slots and custody
are retained. Activity timestamps remain observation facts, not authorship or latency measurements.

The transfer preserves canonical status and all previous corrections, including guarded storage
and the postcommit uncertain-acknowledgment regression. Native fixtures cover duplicate start,
stop-before-start, exact-instance routing, observer-only ticks after slots free, launch failure,
continued polling after a latched fault and owner-loss cancellation. These fixtures do not prove
real-provider desktop scheduling or whole process-tree termination. Local native/failing-before/full
execution remains queued behind the preserved #44 full gate. Hosted checks and exact-revision
packaged scheduling evidence must be recorded before claiming acceptance.

The graphical control increment remains preserved source
`c7922b3d74cae8715066e2868533ba714d67ddb0`, followed by the remaining review/integration/recovery
phases. Original source work, logs and histories are preserved. Every PR remains unmerged until
required review/checks pass, and final combined-main plus full fleet acceptance remain required.

## L02c live fleet controls in the existing-project view

Preserved source `c7922b3d74cae8715066e2868533ba714d67ddb0` transfers on #48 at
`f1578135da3da1aca14d77612117caf8ac031620`. Users choose the exact saved input and limits,
provision an independent fleet, then explicitly start or stop its agents. Polling reads bounded
native facts and never replays commands. Unconfirmed provisioning retains the same request, input,
goal and limits for explicit retry within the renderer session. Saved/restored state never grants
execution authority or claims live observation. Worker activity joins the exact objective/lane/run.

The canonical adaptation keeps the English/Hebrew interface and original-project controls. Paths,
versions, identifiers and provider activity remain literal and direction-isolated; user goals retain
their own direction. Three added rendered regressions cover translated live/stale/restored states,
raw identities/activity, timestamp uncertainty and frozen provisioning input. They fail against the
preserved unlocalized component, which was restored byte-for-byte after the check. Provisioning
success feedback is a static translatable message; the receipt still validates the exact objective
and the catalogue shows its identity.

All 500 desktop coordinator/host tests and 120 interface tests pass locally, including type/build
checks. The sandboxed desktop run failed when creating required Unix sockets; its log is retained,
and the permitted rerun passed. Repository (seven tests), docs (106 documents), vocabulary, formatting
and diff checks also pass. The full native
gate is queued behind the preserved #44 run; hosted checks and exact-revision packaged graphical
acceptance must still be recorded. Parents #46, #47 and #48 have all seven hosted checks passing;
none is merged. Pending-intent recovery across renderer reload, result review, worker/context
recovery and original-main integration remain separate increments. Continue with the preserved
review phases after publishing this increment, retaining the complete second-provider, four-worker,
remote-executor, human-review, merge and final combined-main acceptance obligations.

## R01a independent exact saved-review readers

Preserved source `7d617e60828c6d6cae0e71a52f60fd4095eaed9a` transfers on #49 at
`4a6126868cbe26b59c751488e2aa56f4dbe5072f`. Native reads bind the objective's lane, checkpoint,
completed saved version and recorded bundle to the retained lane context. They reconstruct immutable
review facts/artifacts independently of desktop selection, live files and later private saves. Reads
hold the per-lane workspace guard but release the fleet lock during reconstruction, then revalidate
the exact retained context. Revocation/cancellation does not remove read authority or grant approval.

The transfer includes bounded fifty-row pagination and native desktop list/inspection commands. It
preserves existing approval and physical-root guards without falling back to automatic candidates.
Additional canonical refusal assertions cover substitution with another recorded bundle, a missing
checkpoint borrowing a known bundle, and a syntactically valid object absent from the review. These
complement the transferred newer-save/navigation/cancellation, changed-root and fifty-three-result
fixtures. Local native/failing-before/full execution is queued behind the preserved #44 gate;
source coverage is not a passing runtime or packaged claim. Hosted results must be recorded.

Parent #49 is published before this increment starts. Parallel review panels, comparison to verified
starting versions, saved selectors, offline history/artifacts, correspondence and all remaining
integration/recovery work retain their order in the source ledger. Every increment remains subject
to validation and required human review before merge, followed by combined-main and complete fleet
acceptance. Original source commits, dirty work and verification remain preserved.

## R01b independent parallel fleet review panels

Preserved source `2e8992f8bbb6c990e399a4a308969c7f88a95d37` transfers on published #50 at
`c2c625a1ff7fe9bfc0acd42f0655e637890f24d7`. Up to eight exact saved-result panels remain pinned
while live fleet facts refresh. Each holds an independent file selection and layout. Bounded lists
and pins validate the exact objective/lane/checkpoint/version/bundle; stale or incomplete content is
explicit, late responses cannot reopen closed panels, and retries keep the same selection. Reads
can proceed while the fleet poll is busy, without routing mutation or approval from these panels.

The canonical transfer preserves English/Hebrew presentation and prior controls. Native identities,
user goals and file bytes remain literal; presentation-only model labels are translated without
changing the stored review model. Per-instance folder heading IDs prevent cross-panel accessibility
collisions. Two added Hebrew regressions and the transferred distinct-heading regression fail with
the unlocalized source panel and previous navigator, then pass after byte-exact restoration. An
initial test expectation was corrected to the existing canonical translation for “changes and”;
production wording was preserved and the failed log retained.

Local type checking/build and 127 interface tests pass; all 507 desktop coordinator/host tests pass
with required local socket permission. Repository documentation, vocabulary, formatting and diff
checks accompany delivery. Native full validation remains queued behind the preserved #44 gate;
no new native runtime or packaged graphical proof is claimed. Parents #49 and #50 are published,
unmerged, with hosted validation still running at this point.

This view uses the recorded review base; comparison against the lane's verified starting input is
the next distinct native/presentation increment. Selector persistence, restored history/artifacts,
correspondence and integration/recovery retain their source-ledger order. Required review, actual
provider/package/remote acceptance, eventual merges and final combined-main testing remain open.

## R01c verified lane starting-version comparison

Preserved source `b1a166da5cfb714c6677b02b0343e816ea47fcf4` transfers on published #51 at
`35f486661ed4aa82ea59327da7cec75e3a2da8f5`. Allocation retains the verified single local import
operation separately from the requested source operation. Native comparison binds both that input
and the exact recorded result, pages up to 200 changed object identities, and reads selected text
only from immutable history. Text is bounded to 256 KiB per side; unsafe text/binary/large contents
remain labeled metadata. Object identity, path, content digest, size and executable state preserve
change distinctions. This read neither creates a publication bundle nor grants approval authority.

The canonical transfer preserves the independent saved-review reader and its extra refusal coverage.
Those assertions now also exercise the comparison API with a substituted recorded bundle, missing
checkpoint and absent-but-valid object. Transferred journeys cover attached-source/local-import
correlation, later edits/navigation/cancellation, changed roots, 205 changed entries across pages,
metadata-only differences and bounded binary/unsafe/large-file presentation. Restart reconstruction
of the starting-version binding remains the distinct R02 source increment; it must not guess a base.

Parents #49 and #50 passed all seven hosted checks at their published revisions. The #50 saved-result
and 53-result pagination journeys passed on hosted macOS in 0.400 and 0.489 seconds. #51 is published
with hosted validation running. This increment still needs native/failing-before/full and hosted
validation; the preserved #44 full gate continues to own the shared native target. Source coverage
alone does not establish runtime or packaged acceptance. The next increment connects this exact
starting-input comparison to the parallel panels. All later selector/history/artifact, correspondence,
integration/recovery, provider/package/remote, required-review, merge and combined-main obligations
remain intact. Original work and verification are preserved.

## R01d starting-version comparisons in pinned panels

Preserved source `5a553e78177cc045f10af15a2da86bdb32c95bf0` transfers on published #52 at
`90be90858d8a8c1271160edc8bdccd3b160c26a5`. Each pinned result can independently request its
verified starting-version comparison, page changed objects and select saved before/after content.
The coordinator binds the original source, local starting operation, target, cursor and selected
object. Independent review/input reads cannot overwrite each other, retries preserve the last exact
request, and closing a panel ignores late content. The recorded review remains a separately labeled
section; saved comparisons never claim to inspect current working files or approve main.

The canonical adaptation preserves English/Hebrew controls, content-state warnings and literal
paths/versions/file bytes. The shared text renderer has an explicit saved context while its existing
working-copy behavior remains the default. Two added Hebrew regressions and the transferred saved
context regression fail with the unlocalized source panel/previous renderer, then pass after byte-exact
restoration. Local type/build and all 132 interface tests pass, along with all 514 desktop tests.
Initial duplicate-translation and existing-wording assertion failures are retained in separate logs;
the duplicate was removed and the canonical wording kept. Docs, vocabulary, formatting and diff checks
also pass. Local native/full validation stays queued behind the preserved #44 gate.

Parent #51 passed all seven hosted checks: 3,001 Linux and 3,210 macOS tests, four separate macOS
renderer tests and all 44 daemon checks. #52 is published with native validation running at this
point. This panel increment still needs hosted checks and exact-revision packaged graphical proof.
Selector persistence, restored history/artifacts, correspondence and all integration/recovery phases
remain in dependency order. Required human review, provider/four-worker/remote acceptance, eventual
merges and final combined-main testing remain part of completion. Original work is preserved.

## R01e durable native fleet review selectors

Preserved source `5b9f758c2db294b636bc2bfd9b4eb867a2fcb2ff` transfers on published #53 at
`4fa921244a3c02322114c60e7e09c54764621a06`. The external attachment catalogue stores up to eight
exact fleet selectors and view choices, separately from existing attachment comparisons. Closed,
bounded schemas carry no file content or verification/approval authority. Revision-checked updates
publish a private, identity-bound snapshot through create-only staging, atomic rename and directory
sync. Foreign/corrupt/linked snapshots and interrupted staging are preserved rather than replaced.

The canonical transfer retains the nonblocking capture state import and all earlier corrections.
The added exhaustion regression requires a maximum revision to permit only an exact no-op: changed
or stale snapshots must refuse before staging, retain the acknowledged bytes and survive reopen.
Transferred coverage includes independent namespaces, malformed/oversized selectors, concurrent
writers, copied/corrupt/hard-linked/symbolic-linked records and retained interrupted staging. The
native desktop test proves empty reads create no storage, selector saves create no fleet history,
and reopen never grants execution context.

Parent #52 has all seven hosted checks passing; #53 is published with hosted validation running.
This native store still needs native/failing-before/full and hosted validation. The preserved #44
local full gate remains active on the shared build target. Renderer save/restore follows as a separate
published increment; visible pins currently remain renderer-session-only. Original work and tests are
preserved. Restored history/artifacts, correspondence, integration/recovery, full provider/package/
remote acceptance, required review, eventual merges and final combined-main testing remain required.

## R01f restore exact fleet review selections

Preserved source `20b01d6023616e02e2c9e5cabdc82dc39bb82c3b` transfers on published #54 at
`7333c588b2cfbcd49f000c21771b555f0e744163`. The renderer saves at most eight exact fleet review
selectors and independent page/object/mode/layout choices through the native selector store. Loading
rechecks immutable native history; unavailable history retains its selection without adopting or
starting workers. Serial writes retain closes during pending saves. Unconfirmed acknowledgements and
conflicts expose explicit retry or replacement from the saved set; failed initial loading cannot
replace stored selections. Reload ignores earlier content replies even when panel keys are reused.

The canonical adaptation retains English/Hebrew presentation, literal identities and user goals,
saved-context comparisons and independent accessible panel headings. A missing restored goal receives
a translated fallback without translating a user's identically worded goal. The added Hebrew
regression fails against the unlocalized preserved source, then passes after byte-exact restoration.
Local type/build and all 135 interface tests pass, along with all 523 desktop tests, including existing
attachment persistence and nine fleet persistence regressions. Docs, vocabulary, formatting and diff
checks pass. Full local native validation remains queued behind the preserved #44 gate; no packaged
proof is claimed. Parent #53 has all seven hosted checks passing; #54 has six passing with macOS pending.

This increment still needs its own hosted validation. Durable starting-version bindings and restored
history follow before artifacts/correspondence/integration and the remaining recovery phases. Required
human review, second-provider/four-worker/remote and packaged acceptance, eventual merges and testing
of the final combined canonical main remain required. Original source work and verification are preserved.

## R02a durable lane starting-version binding

Preserved source `b6b388caf30c7ffb1cc4d23f09f7175e0a220089` transfers on published #55 at
`450943a6031ea1d0f14adafd6518d648bd74a6b8`. New allocations persist their verified local initial
import alongside the original source version using the additive `bind-workspace-v2` command. Native
comparison reads require this recorded proof. Existing legacy commands retain their exact encoding
and explicitly lack starting-version evidence; no oldest-version inference or history rewriting is
performed. Older binaries refuse the new command kind, so rollback must retain a compatible binary
for fleets with v2 bindings. Ordinary project history and selector schemas remain unchanged.

Source regressions cover closed canonical wire encoding, new binding replay, replacement refusal and
native allocation-to-import identity. The canonical added regression reopens a legacy binding, checks
that its starting version remains absent, refuses inferred backfill, and checks unchanged state after
another reopen. The transferred provider fixture waits for a completed launch record instead of merely
an existing file, avoiding the shell redirection/write race without weakening its launch assertions.

Local formatting, documentation, diff and 52 desktop host/UI structural checks pass. These checks are
not native runtime evidence. Native/failing-before/full validation remains queued behind the preserved
#44 run on the shared build target; this increment requires its own hosted validation. Parents #53 and
#54 have all seven checks passing, while #55 validation is running. All original work is preserved.
History-only context reopening follows separately, before artifacts/correspondence/integration/recovery.
Required human review, complete provider/four-worker/remote and packaged acceptance, eventual merges
and final combined-main testing remain part of completion.

## R02b reopen retained fleet history without execution adoption

Preserved source `ce73d00a2a762eb514d47fbb429e79673157e3e3` transfers on published #56 at
`e05637926fb11b7d7da48fd4a253e298f6b75d28`. A separate saved-history interface discovers retained
fleets and reconstructs exact lane reviews, starting comparisons and artifact bytes without inserting
live execution contexts. Native allocation/root/store identities and retained ancestry are checked
before and after reads. Existing journals open read-only, indexes remain transient and payload access
refuses mutation or quarantine. Missing history stays missing; pending working-file recovery is skipped.
The desktop can list/pin retained results while start/stop remain unavailable for restored owners.

The canonical adaptation preserves localized controls, literal identities and all prior native fixes.
Added English/Hebrew render checks reject the old current-owner-only review restriction, then pass
after byte-exact restoration. The added native refusal assertions exercise wrong checkpoint, version
and bundle tuples against the reopened-history path for review, comparison and artifact reads, while
requiring unchanged retained bytes. Source tests cover offline originals, preserved unsaved work,
missing durable indexes, missing/linked journals, unavailable CAS directories, corrupt chunks without
quarantine, replaced roots, ancestor substitution, denied credentials/grants and pending mutation
preservation. Current-context readers retain the same exact immutable selection contract.

Local type/build, 137 interface tests and 524 desktop tests pass. Docs, vocabulary, Rust formatting and
diff checks pass. Native/failing-before/full execution remains queued behind the preserved #44 gate;
this increment requires its own hosted checks. Parent #55 passed all seven checks: Linux 3,006/macOS
3,217, four separate macOS renderer tests, 44 daemon checks and all 658 desktop/interface tests. Parent
#56 checks are still running. Full graphical restart acceptance, worker recovery, artifact presentation,
correspondence, original-main integration and remaining recovery increments are separate obligations.
Required named human review, second-provider/four-worker/remote acceptance, eventual merges and final
combined-main testing remain required. Original work and running verification are preserved.

## R02c exact saved artifact previews in parallel reviews

Preserved source `e961781d941c938de408b38866c4b43cee36f7fe` transfers on published #57 at
`af46a567f55cf297bd8423d04e00ca2e57f69088`. Pinned fleet reviews render bounded native image,
PDF-page and Office previews from verified historical bytes. Replies bind objective/lane/checkpoint/
version/bundle, object, side and artifact digest. Each panel keeps transient request/content state;
only selectors persist. Shared native JSON encoding and extracted renderer validation preserve the
existing workspace-review path. Unsupported artifacts stay metadata, partial errors stay explicit,
unequal PDF sides use verified page-count evidence and closed panels ignore late replies.

The canonical adaptation preserves localized panel controls and adds localized fixed artifact failures
in visual/content views while leaving literal paths, versions, image data and comparison models intact.
The exact-preview regression fails with the old fleet component; the Hebrew failure regression fails
with the old shared viewer. Both pass after byte-exact restoration. Local type/build, 139 interface
and 529 desktop tests pass, including existing workspace artifact checks and new fleet selection,
page-bound, malformed-response, concurrency and retry checks. Docs, vocabulary, formatting and diff
checks pass. Native encoding/failing-before/full execution remains queued behind the preserved #44
gate; this increment needs hosted validation and exact packaged graphical proof.

Parent #56 has all seven checks passing: 3,009 Linux and 3,220 macOS tests, four separate macOS renderer
tests, 44 daemon checks and 658 desktop/interface tests. The added legacy-backfill refusal passed on
both platforms. #57 native validation remains running. Correspondence, candidates, original-main
integration and remaining recovery phases follow in ledger order. Second-provider/four-worker/remote
and packaged acceptance, named human review, eventual merges and final combined-main tests remain
required. No original work or running verification was discarded and no PR has been merged.

## R03a exact review change requests for the originating lane

Preserved source `76d24877696474f2383429ca0b80c8eeab3ba3f4` transfers on published #58 at
`18fcb17ef850428afd032ec4099dc1ad78ea13fd`. The additive native event records bounded feedback
against one completed checkpoint, saved version and review bundle. Identical retries recover the same
receipt; changed content or selection under the same request identity refuses. Feedback changes neither
saved work nor scheduling/custody/approval state. Authenticated agent context exposes only the originating
lane's requests; child lanes cannot read their parent's feedback or invoke native reviewer recording.
Restored history can read durable requests, while desktop recording still requires current ownership.

Messages are nonblank UTF-8 text up to 8 KiB, with hidden controls/direction characters refused except
line feed and tab. Limits are 32 requests per lane and 256 per objective. Older binaries reject the new
event; existing event encodings/database envelope remain unchanged. Recorded feedback is not evidence
of delivery, work completion or human signing identity. Desktop drafts and uncertain retry identities
remain session-only; explicit retry preserves the original message, and late replies cannot reopen a
closed panel. Provider wakeup/resume and request resolution remain subsequent work.

The canonical adaptation preserves English/Hebrew controls, literal feedback and all earlier native
regressions. Added Hebrew rendering coverage detects the unlocalized source and coordinator regressions
detect the previous missing feedback route; byte-exact restoration passes. Local type/build, 141
interface and 534 desktop tests pass, along with docs/vocabulary/format/diff checks. Added native UTF-8
boundary coverage preserves exactly 8,192 bytes through restart/retry and refuses the next byte; extra
refusals cover oversized multibyte text, carriage return and C1 controls. Native/failing-before/full
execution remains queued behind the preserved #44 gate and this increment needs its own hosted checks.
Parent #58 has six passing checks with macOS pending. Original work and verification are preserved.

Candidates, decisions, original-main integration, remaining recovery phases, complete provider/four-
worker/remote and packaged acceptance, named human review, eventual merges and final combined-main
verification remain required. No migration PR has been merged.

## R03b proposed saved results for exact change requests

Preserved source `367ec923ba6e1bcd3d15d62429ddec7ca49bda06` transfers on published #59 at
`27ce54b6912dd2da9aafae628cadfec52537e30e`. A scoped MCP action proposes a complete reviewed
checkpoint from the authenticated originating lane/session. Native code derives version and bundle
from the checkpoint and verifies retained review and custody. Proposing unchanged, unreviewed,
incomplete, wrong-session or cancelled work refuses. Exact retries recover the same proposal; eight
append-ordered proposals per request remain bounded. Requests are not resolved and original saved
work, run state and approval/main authority are unchanged.

A v2 desktop activity projection binds recorded requests and proposed results in one observed runtime
state, keeping the v1 receipt contract. Users explicitly pin a proposed result beside the original;
existing pin bounds, duplicate checks and immutable selectors apply. Missing proposed history affects
that new panel only. Canonical English/Hebrew labels, literal feedback/version identities and prior
native corrections are retained. The additive persisted event is rejected by older binaries; rollback
requires a compatible binary, and replay never reconstructs response identities from agent-supplied
version or bundle fields.

Local type/build, 143 interface and 536 desktop tests pass, along with docs/vocabulary/format/diff
checks. The Hebrew presentation regression detects the unlocalized source and the separate-panel
regression detects the previous coordinator; both pass after byte-exact restoration. Transferred
native runtime/service/catalogue/MCP journeys cover origin checks, duplicate/exact retry, restart,
proposal bounds, extra argument refusal, immutable original review and absence of approval authority.
Native/failing-before/full local execution remains queued behind the preserved #44 gate, which has
advanced from discovery into running tests. This increment needs its own hosted validation.

Parent #58 passed all seven checks: 3,012 Linux and 3,226 macOS tests, four separate macOS renderer
tests, 44 daemon checks and all 668 desktop/interface tests. #59 native validation is running. Request
decisions, candidates, original-main integration, remaining recovery phases, second-provider/four-worker/
remote and packaged acceptance, required human review, eventual merges and final combined-main tests
remain in scope. Original work and running verification are preserved; nothing has been merged.

## R03c reversible decisions on exact review requests

Preserved source `f14d5344ebb5826a63aa9f2878902676e6f0140a` transfers onto published #60 at
`dc315053ac01b86cc5125155644a4f726a2be2b6`. A native confirmation marks feedback addressed by
an exact recorded proposal, or reopens it. Per-request revisions reject stale choices, including
address/reopen cycles. Confirmation runs without the fleet lock; original and proposed retained
identities are verified again before append. Exact operation receipts recover lost acknowledgments
without another confirmation, while reporting the latest state separately from the earlier receipt.
Agents can observe decisions but cannot record them; no worker starts and main is unchanged.

Decision history is bounded to 64 revisions per request. Addressed requests reject new proposals
until reopened; exact prior proposal retries remain recoverable. The additive persisted event is
rejected by older binaries; rollback needs a compatible binary. Activity projection v3 requires one
validated current decision per request; original feedback receipts remain v1. Restored history shows
decisions but does not grant current-host decision authority. UI retries retain exact operation
identity, explicit reload permits a new choice only after state validation, and closed panels ignore
late replies. A work decision is separate from approval and integration.

Canonical adaptation retains localized controls, literal user feedback and all previous corrections.
Local type/build, 145 interface and 541 desktop tests pass (686 total). The Hebrew decision regression
fails against the unlocalized source and coordinator decision tests fail against the previous route;
byte-exact restoration returns them to passing. Added native wrong checkpoint/version/bundle cases
require refusal before confirmation and unchanged state. Transferred native coverage checks stale
concurrent choices, cancellation, operation reuse, workspace replacement during confirmation, replay,
limits and agent denial. Native/failing-before/full local execution remains pending behind preserved
#44 verification; hosted checks for this exact increment and graphical confirmation remain required.

Parent #59 now has all seven hosted checks passing. Parent #60 has five passing checks while Linux
and macOS continue. Candidate/integration/recovery migration, second-provider/four-worker/remote and
packaged acceptance, named human review, eventual merges and final combined-main testing remain in
scope. Original histories, dirty work and running verification remain preserved. Nothing has merged.


R03c validation correction: hosted macOS run 36372737993 stopped during compilation because the added
integration regression accessed private selection fields. The regression now uses the original public
checkpoint/review receipts, as the existing fixture does. Selection encapsulation and every refusal
assertion remain unchanged; no production API is widened. The failed log is retained. Formatting and
whitespace checks pass; hosted/native execution must validate the corrected revision.

## C01a original-project input correspondence

Preserved source `302f6cbb0a795b9743ef6c72447d803cdc7860ab` transfers onto corrected published #61 at
`ec99692f61bba6f9f8cc72297e79fa0302a3bd3f`. Native review preparation verifies the complete
immutable input inventory across the original project and a directly attached root lane. Paths, entry
types, content digests, byte lengths and executable bits must agree; independent object and manifest
identities are not equated. Changes map local objects back to original objects, preserving moves,
deletions and additions. Stable pagination returns at most 200 changed objects with an exact cursor.

The projection reads original input and verified main under one retained attachment-history lock.
Observed main is not reserved. Continued manual edits and later captures do not substitute a newer
input. Foreign projects, missing roots, unbound starting versions and unsupported delegated ancestry
refuse. History-only reads do not adopt workers. This creates no candidates, versions, reviews or
approval authority, changes no persisted format, and exposes no desktop or agent action yet.

Transferred native tests cover full inventory/metadata equality, distinct identities, changed objects,
pagination, foreign projects, newer captures, offline originals and history-only restart. Canonical
regressions additionally require a same-path replacement to remain a deletion plus a new object and
reject duplicate paths or object identities in each of the three inventories. Local repository,
documentation (106 documents), license/self-test, storage/self-test, formatting and whitespace checks
pass. Native tests, native failing-before proof and full local verification remain pending behind the
preserved #44 run; this exact revision requires hosted checks. These checks do not prove packaged
behavior, write-back or end-to-end integration.

Next: delegated ancestry, staged project candidates, fixed-main review and approval/integration, then
remaining recovery and acceptance phases. Named human review, eventual merges and final combined-main
tests remain mandatory. Original histories, dirty work and running verification are preserved.

The first local C01a commit `3002f2df104e5079c0e87d99995afb89de7761d4` and its worktree remain
preserved. Delivery uses a fresh worktree on corrected #61 so the original private-field test compile
failure is not inherited. Source implementation and correspondence regressions are unchanged; the
corrected parent test and its failure record are retained. No history was rewritten.

## C01b delegated result ancestry

Preserved source `ee7b74fc47a831a3b7eb3827647f7aa841621fa7` transfers onto published #62 at
`d68ea27f6876f3a206d23ec564e706d7098dd45a`. Native mapping follows every recorded input from
the original attached project through the selected lane, bounded by the fleet depth and 33 retained
histories. Missing/foreign/unbound histories, cycles, depth mismatch and changed bindings refuse.
Every immutable import must match its upstream inventory. Earlier additions, moves and deletions
remain in the final source-relative comparison; later parent work does not replace the exact version
used by delegation. Retained histories and current lineage are revalidated after the read.

Projection `mesh.fleet-project-mapping/v2` adds explicit lane input/output ancestry and changes page
keys to stable `source:`/`result:` correspondence identities. No persisted schema changes. The scope
is recorded input ancestry, not an inferred private dependency graph or approval. The native reader
still has no desktop/agent action and creates no candidate, source write or worker adoption.

Canonical same-path replacement and ambiguous-inventory regressions are retained; replacement
assertions use the v2 correspondence identity/order while preserving deletion-plus-addition semantics.
An additional native regression verifies that reverted ancestor edits and removed private additions
leave no invented net changes, while a remaining executable-mode change maps to the original object.
Transferred tests cover three generations, ancestor deletion/recreation, lineage refusals, exact
parent-version choice, ancestor replacement and history-only restart. The native journey preserves
the current missing-entry refusal rather than treating an incomplete checkpoint as a deletion.

Local repository (7 tests), docs (106 documents), license/self-test, storage/self-test, formatting and
whitespace checks pass. Native/failing-before/full local execution remains pending behind preserved
#44 verification; hosted checks must validate this exact revision. Parent #61 has six passing checks
with macOS pending after its compilation correction, and #62 has five passing checks with both native
jobs pending. No local-native, graphical, integration or merged-delivery claim is made.

Candidate staging, fixed-main review, approval and grouped integration, remaining recovery and
second-provider/four-worker/remote/packaged acceptance, named human review, eventual merges and final
combined-main testing remain required. Original histories, dirty work and active verification remain
preserved.

## C01c exact project candidate staging

Preserved source `0f79d16c0456d83e707e6652e8bd4e3774683eea` transfers onto published #63 at
`a9bc1c78f486cdfc55a0c9e5985feddf4ca8aaf7`. Native staging copies a selected saved result into
private external project metadata, separate from ordinary files and capture history. Intent binds the
request, project, exact selector, complete content manifest, recorded ancestry/agent attribution and
expected main. Retained directory identities, copied bytes and metadata, lineage and main are checked
before a durable ready receipt; copying and revalidation occur outside the short admission lock.

Completed exact retries recover the original receipt, even after main changes, without asserting
current approval eligibility. Different input or modified retained content refuses. Partial candidates
are preserved and never repaired by retry. History-only inspection creates nothing and requires the
original project and source/lane histories. No automatic discovery, cleanup or reconciliation is added.
Limits remain 128 retained allocations including partial ones, 10,000 entries, 64 MiB total and 8 MiB
per file. Existing capture/fleet formats are unchanged; new candidate formats are versioned v1 and
older code has no candidate reader. No source-history import, project review, approval or write-back.

Source native tests cover exact historical bytes despite later edits, stale main, conflicting retry,
restart inspection, unchanged capture history/main, interrupted verification, concurrent edits,
manifest corruption, replaced file directories and capacity. Added canonical regression assertions
verify that changed provenance cannot overwrite intent/ready receipts and executable-mode substitution
refuses while preserving both modified evidence and original ready receipt. Prior mapping and decision
corrections remain in the base.

Local repository (7 tests), documentation (106 documents), license/self-test, storage/self-test,
formatting and whitespace checks pass. Native/failing-before/full local execution remains queued
behind preserved #44 verification; hosted tests must validate this exact revision. #61 and #62 have
six passing checks with macOS pending, while #63 has five passing checks and native jobs running.
No native local, packaged, approval or merged-delivery claim follows.

Fixed-main project review, UI, private dependency validation, human approval and grouped integration,
remaining recovery/acceptance phases, required human reviews, eventual merges and final combined-main
testing remain part of the goal. Original work and active verification remain preserved.

## C02a whole-project candidate review against fixed main

Preserved source `27b5efc316a84a896189cb85952fa468c5e5b57c` transfers onto published #64 at
`7d2afb61254ef46c691f760ff66a6ed0d52f7ad4`. The native review compares the complete staged
project snapshot against its recorded, verified main head (or empty genesis). The base does not follow
later main advancement or newer private captures. Current main and base freshness are separate facts.
An explicit new staging request against a new main produces a different review identity.

The v1 candidate-review projection and context bind candidate receipt digest, project, fixed base,
target version, complete content digest and scope. Pages share one review identity; paths are ordered,
200 per page, with selected text limited to 256 KiB per side. Oversized, binary and unsafe text remains
metadata-only. Exact candidate content and retained history are checked before and after inspection.
Traversal, invalid cursors, simultaneous page/file selection, untrusted main and substituted content
refuse. A derived content-review identity is not a signable original-project review bundle and grants
no approval authority. Existing persisted schemas are unchanged.

Transferred native tests cover whole-project genesis, fixed verified base after main advancement,
explicit new preparation, trust refusal, restart, candidate corruption, stable pagination, saved bytes
versus live edits, unsafe text and refusal to approve the content-review digest. Added canonical
boundary coverage includes exactly 256 KiB of valid text and invalid UTF-8, retaining the over-limit
and hidden-direction refusal cases and verifying all 209 changes remain paged. Prior candidate
provenance/mode refusal assertions and lineage/decision corrections remain intact.

Local repository (7 tests), docs (106 documents), license/self-test, storage/self-test, formatting and
whitespace checks pass. Native/failing-before/full local execution remains pending behind the
preserved #44 full run. Hosted tests must validate this exact revision; no packaged or graphical
claim is made. #62 and #63 have six passing checks with macOS pending; #64 has five passing checks
with native jobs running. All migration PRs remain unmerged.

Desktop review controls and durable preparation pins are next. Exact human approval, original-main
integration and remaining recovery/provider/four-worker/remote/packaged acceptance, required reviews,
eventual merges and final combined-main verification remain in scope. Original work and running
verification are preserved.

## C02b native desktop candidate routes

Preserved source `2263d5db328d3aab957218642c61e3e3b94d6047` transfers onto published #65 at
`de5e2f03e68a4d056d99551c1c9e52316c46201d`. Three asynchronous desktop commands delegate
mapping, exact preparation and fixed-main review to the native attachment host on blocking workers.
The caller supplies explicit project/objective/selection identities; native readers verify them and
load native reviewer trust. No arbitrary path or selected-workspace fallback grants access.

Preparation first recovers an exact completed receipt through history-only inspection. New staging
requires current fleet ownership; restored fleets cannot create candidates or acquire worker custody.
Conflicting and partial receipts remain refused, not repaired by fallback. Review stays read-only and
project capture continues independently. These are command/host foundations; visible renderer panels
and durable preparation pins follow in the next increment. No persisted format changes or approval.

Transferred macOS host coverage exercises missing candidate and stale-main refusal, exact retry,
foreign project, invalid page/file selection, unchanged history, restart receipt recovery and fixed
comparison without adoption, and refusal to stage a new request after restart. Added canonical cases
substitute checkpoint/version/bundle across all three routes, require refusal and unchanged project
versions, then recover the original exact receipt. IDs are read from public fixture receipts; no
private selection fields or production accessors are exposed.

Local repository (7 tests), docs (106 documents), license/self-test, storage/self-test, formatting and
whitespace checks pass. Native host/failing-before/full local execution remains queued behind
preserved #44 verification; hosted macOS must validate this exact revision. #63 passed all seven
checks (3,028 Linux and 3,244 macOS tests, four separate renderer tests); #64 has six passing checks
with macOS pending and #65 has five passing checks with native jobs running. No packaged/graphical
or merged-delivery claim follows.

Visible desktop candidate review, durable pins, exact human approval/integration and remaining
recovery/provider/four-worker/remote/packaged acceptance, required human reviews, eventual merges and
final combined-main testing remain part of the goal. Original work and active verification remain
preserved.

## C02c project comparison panels and durable preparation inputs

Preserved source `c53e4fdc3494eb0037fa43326d2a56e4c7c3a77b` transfers onto published #66 at
`9a9bf400bbd929c76d23cba910979162f10266b5`. Each pinned result can prepare and inspect a fixed
whole-project comparison, with separate pagination, selected text, exact identity and stale-main
observations. The coordinator persists project/request/expected-main before preparation. Retries
reuse those inputs; reopening only reads saved candidates and never stages or adopts workers.
Late responses cannot recreate closed or replaced pins. Native replies remain strictly bound to the
project, result, ancestry, candidate receipt and fixed comparison; inspection grants no approval.

Fleet pins use v2 with optional candidate selectors. v1 navigation loads without inventing a request,
and migrates only on changed save. Old binaries refuse v2. Existing count/byte bounds and revision
checks remain; the canonical exhausted-revision refusal regression is preserved. Desktop envelopes
bind each response to its exact command inputs.

Canonical localization and nullable lane goals are preserved. Hebrew controls and notices translate,
while paths, identifiers and saved text remain raw with explicit direction. Added rendering coverage
checks stale-main notices, disabled retries during persistence failure, and retained verified content.

Local UI type/build and 147 render tests pass; all 550 desktop coordinator/script tests pass. Repository
7 tests, docs 106 documents, license/storage self-tests, vocabulary, formatting and whitespace pass.
Regressions fail with the source English-only panel and with the parent coordinator, then all 34
focused tests pass after exact restoration. Native/full local execution remains queued behind the
preserved #44 run; hosted CI must validate this exact head. #64 has all seven checks passing; #65 and
#66 have six passing with macOS pending. All migration PRs remain unmerged.

Next increments retain exact human approval and original-main integration, followed by remaining
recovery/provider/four-worker/remote/packaged acceptance. Required human review, eventual ordered
merges and final testing of combined canonical main remain completion requirements. Existing work
and running verification remain preserved.

## C03a exact saved-history operation preparation

C02c is published as #67 at `b9b0bd37ee0002555ba5582736beb418c15c137f`, replacing
`c53e4fdc3494eb0037fa43326d2a56e4c7c3a77b`. Its local 697 desktop tests pass; hosted CI is
running. #65 and #66 now both have all seven hosted checks passing, including native macOS.
All remain unmerged and required human review remains outstanding.

Preserved source `3aef42a9373ff0ebb84fd482819d39536b022ab2` transfers onto published #67.
The native read-only planner derives one exact saved predecessor's causal closure, base and clock,
while respecting the current policy epoch. An actor advanced outside that ancestry refuses rather
than forking its sequence or absorbing unrelated work. Operations validate against a temporary
historical materialization. The opaque plan reserves no identity/sequence, writes no journal, grants
no approval and cannot apply working files; a future writer must rederive under custody before append.
Ordinary managed authoring retains its all-tip behavior.

Transferred signed-journal coverage proves independent saved trees, explicit removal, later-branch
exclusion, actor refusal, policy invalidation and changed-root refusal, with ordinary files/main
unchanged. Added canonical refusal coverage applies a valid prefix followed by an invalid operation,
rejects 100,001 operations, and verifies unchanged durable bytes, saved paths and exact repeatable
plan across reopen. This is the authoring foundation; capture-line separation and provenance-bound
candidate compilation/import remain subsequent increments.

Local repository (7 tests), docs (106 documents), license/storage self-tests, formatting and whitespace
checks pass. Native focused/failing-before/full local execution is pending behind preserved #44 full
verification; hosted CI must validate the new exact head. No packaged or integrated delivery claim
follows. The complete fleet plan, human review, ordered merges and combined-main tests remain required.

## C03b independent ordinary capture history

C03a is published as #68 at `a32d18bec2484f5647bc065f5dd7c6602d5d3602`, replacing
`3aef42a9373ff0ebb84fd482819d39536b022ab2`. Both #67 and #68 now have all seven hosted CI
checks passing. The preserved #44 full local run completed successfully: 3,186 native tests
(13 skipped), 603 desktop tests and 44 daemon-demo checks. That result covers #44 only; it is
not evidence for this later head or combined main. No migration PR has merged.

Preserved source `d3b34517d6f9f390f2834a141717254861bc5358` transfers onto published #68.
Attachment capture now records its own exact predecessor and pending signed operation, derives
file identities from that saved ancestry and lists only that observation line. Signed candidate
branches cannot change unchanged-save results or become parents of later ordinary captures.
An exact pending operation is recorded before append and resolved from journal truth after restart;
read-only recovery does not rewrite it. Unexpected predecessors or partial journals refuse.

The bounded private capture-line v1 record is bound to the original history configuration. Migration
wraps the original identity seed in attachment-history v2 on save, retaining existing version IDs.
Read-only legacy access does not migrate; branched legacy history cannot guess a capture tip. A v2
history missing its position refuses, and old writers refuse v2 rather than resume all-tip capture.
Recognized interrupted metadata transitions recover; conflicting or malformed evidence is retained.

Transferred native tests cover signed branch separation, continuing human edits, exact pending-append
recovery, unchanged no-op saves, legacy migration, missing/corrupt records, links and unexpected
transitions. Added canonical coverage refuses nonprivate, oversized, history-rebound and extra-field
records without changing their bytes/modes, history binding, journal or working file; restoring the
original private record recovers the original no-op capture. Prior historical-authoring regressions
are preserved. Local repository/docs/license/storage/format checks pass. Five native capture-line tests, two
historical-authoring tests and all 31 attachment integration tests pass. The added refusal regression fails when the private-permissions
guard is removed, then passes after exact source restoration. Full local verification is next.

Candidate compilation/import and signable original-project review remain subsequent increments.
Human review, ordered merges, latest packaged acceptance and combined-main testing remain required.

## C03c identity-preserving candidate compilation

This increment transfers preserved source `0d6114701a778f1277cbd69ff2bc3280feb2a716` onto
#69 at `ec3d4052ae10f9d24051676973a5f1c206b9913c`. It prepares original-project operations from
complete verified lane ancestry, preserving original identities for moved or edited objects and
distinguishing new objects at reused paths. Bounded content and correspondence refusals remain.
Preparation cannot sign, append, approve, advance protected main, or change ordinary project files.

Transferred tests independently materialize the proposed signed historical tree, compare original
identities/content/modes, exclude newer user work and reject malformed correspondence, content and
no-op imports. Canonical compiler and delegated-history integration regressions each executed and passed.
Repository/docs/license/storage/format checks pass. Replacing retained original identities with
lane-local identities makes the compiler regression fail; restoring the exact source passes. Full
and hosted validation remain pending. Durable signed import
receipts, desktop signable review, the remaining source increments and complete acceptance remain
required. The separate native registration failure is not resolved by this compiler transfer.

## C03d signed candidate import and exact retry recovery

Preserved source `a8d3bd00105941a9ceb23bd9215b87d367645d5a` transfers onto published
[PR #70](https://github.com/idosams/Mesh/pull/70), replacement
`46d473879e07b24f51bef5e9a2bb816c1448b02e`. The original source, dirty work and canonical
validation corrections remain preserved. A separate clean worktree keeps #70's active full test
run unchanged. Canonical native testing for this importer must wait for that run to finish.

A private, bounded signed receipt records the exact candidate provenance, actor, historical plan
and authenticated operation before append. Read-only inspection derives pending/imported state
from journal truth. Explicit retry reuses the retained signature; changed or ambiguous receipts
refuse without overwriting recovery evidence. Ordinary source files and protected main remain
unchanged, and import does not grant approval or create a review.

Compatibility adds the v1 import receipt and statement under existing external candidate storage.
Old candidates have no import receipt; existing capture history remains independent of imported
branches. Partial writes and conflicting evidence remain for reconciliation. Transferred tests
cover a durable-intent fault, retry without resigning, tampered and aliased records, exact delegated
imports and main advancement. Canonical repository/docs/license/storage/format checks pass. Added refusal coverage for public
permissions and oversized receipts checks unchanged bytes, modes, journal, working file and signing
count, then recovers the original pending receipt. Canonical native focused/full/hosted checks remain
pending behind #70's active full run; this is not a packaged acceptance claim. Remaining desktop import/review, deletion, grouped integration and
restoration increments, complete provider/remote/packaged journeys and final-main validation remain.

## C04a desktop project import and fixed review

Transfers source `3a3b0da0139b16fb527de4bf8a9ece0d9772f523` onto the published canonical
PR #72 at `08bd13bc0bc734aa9d3e6ce607fa8fd950111d56`, following signed import PR #71.
Pinned comparisons gain explicit saving as a project version and creation of a fixed project review.
Refresh/restoration inspect retained outcomes without signing, appending or restarting workers.
Public-only actor recovery reuses retained signatures; private bounded import bindings refuse missing
or substituted receipts. Imported reviews remain bound to their original base after main advances.
None of these actions approves or integrates a result or writes ordinary project files.

Canonical conflict resolution preserves the newer localized controls, nullable goals and historical
validation failures. Added Hebrew import labels and a renderer regression keep file paths literal,
isolate their direction, show stale/incomplete reviews and disable review creation while selections
are unconfirmed. The earlier receipt permission/bounds regression is retained.

Repository (7), documentation (5 plus 106-document link check), license/storage self-tests and Rust
format checks pass. All 13 focused coordinator tests pass. Desktop validation is running: its
launcher was sampled at `_dyld_start` before application code. A direct TypeScript invocation passed;
the focused renderer command remains running. These are not completed full-desktop results.
Native focused/mutation/full verification is
queued behind the preserved #70 full run, which remains live in test discovery. No concurrent native
build or packaged acceptance claim is made. #71 has all seven hosted checks passing, including
3,042 Linux and 3,260 macOS native tests and four renderer tests; it is still awaiting additional local
verification. Remaining direct review, pending-input recovery, deletion, integration and restoration
source increments and the full provider/remote/packaged/final-main acceptance scope remain required.

## C04b direct navigation to exact imported reviews

Transfers source `15111733b9da3565d5a1ddd8aba6457b0b2f85b9` onto canonical PR #73
at `64f9278fdb0bb56c7a69a5fbf60d9e43f635c477`. A pinned imported result opens its exact
original-project review. Native response identity is verified before selection and focus; a pending
status refresh defers navigation without creating another review, approving work or changing files.
Keyboard focus lands on the selected review, and newer localized labels and literal identifiers are
preserved. The Hebrew regression also checks that navigation is disabled while selection is unconfirmed.

All 40 attachment coordinator tests pass. Disabling the new navigation handler makes both exact
navigation regressions fail (38 pass, 2 fail); restoring the handler returns all 40 to passing.
Repository (7), docs (5 plus link checking), TypeScript and whitespace validation pass. The initial
TypeScript attempt lacked this worktree's dependencies; after installing the locked dependencies,
the actual check passed. Full local renderer/native verification remains queued behind preserved
runs; hosted and graphical keyboard-focus evidence are still required. This increment grants no
approval authority and does not complete the remaining recovery/integration/restoration or full
provider, remote, packaged and final-main acceptance obligations.

## C04c durable exact pending review inputs

Transfers source `0ccc58c78b0278b4a8e84cc9de7fdaa073563ccc` onto canonical PR #74
at `cd804ad3708b1056ae29e84019575dfe4e6b42bb`. A separate native outbox retains at most eight
exact change-request or decision inputs, independently of open panels. Compare-and-swap revisions
reject stale writers; retained operation tokens cannot silently change their inputs. Private bounded
identity-bound records preserve incomplete staging and refuse aliases or catalog substitution.
Loading does not submit operations, adopt workers or grant approval. The new optional v1 format
leaves existing pin and journal formats unchanged; desktop save-before-dispatch wiring follows.

Canonical adaptation preserves earlier validation limitations and adds a regression for eight-entry
capacity, nonprivate/oversized records, unchanged refusal evidence and restoration of acknowledged
inputs. Focused native/mutation/full execution is queued behind preserved local verification;
no native or graphical completion is claimed. This increment replaces only the listed source commit,
not the remaining desktop recovery, deletion, integration, restoration or full acceptance obligations.

## C04d desktop pending review recovery

Transfers source `7497890ae6ae63a64974e958d720c4e446caf9ef` onto canonical PR #75
at `a4c8f89a1efcba0aa3bf64ad420a8901820b41c7`. Typed native outbox commands validate exact
history selectors. Desktop requests persist pending inputs before dispatch; lost save acknowledgments
prevent submission until their exact inputs are verified. Restart only loads; explicit retry survives
closed panels. Verified receipts remove retry inputs, and decision reconciliation reads current
activity before discarding a retry without submitting another decision.

Canonical adaptation preserves Hebrew controls and validation history. The new pending-operations
view translates labels while retaining literal direction-isolated identifiers and user-authored text.
Added a Hebrew regression for disabled recovery while busy and unchanged request content.
All 45 focused coordinator/persistence/fleet tests pass. Skipping durable retention causes the
save-before-dispatch regression to fail; restoring the implementation returns all 45 to passing.
Repository (7), docs (5 plus links), license/storage self-tests, Rust formatting, TypeScript and
whitespace checks pass. Focused renderer validation is running; native focused/full checks remain
queued behind preserved local verification. Hosted and graphical restart/approval evidence remains
required. Source deletion/integration/restoration and complete provider/remote/packaged/final-main
acceptance continue after this published increment.

## D01a exact agent assignment for explicit missing-file resolution

Transfers source `62e8c885ab9c72fa257b1f3cea2483b650e3a4a8` onto canonical PR #76
at `a87005e8db37bb917f43468a91c02d1dd9662d91`. The native agent entry point binds an already-absent
tracked file to the admitted root, fold, installation, active custody generation, explicit path and
last saved version. It reuses existing authenticated deletion adoption and suspends mutation
authority inside the signer. Ordinary checkpoints still refuse ambiguous missing entries; this
operation does not delete an OS file, infer renames, approve main or release agent custody.

The transferred positive/refusal regressions cover complete capture after explicit resolution,
retained retired-entry history, unchanged source/main, stale assignment and version rejection before
signing, failed signing, callback authority isolation and concurrent file recreation after signing.
No journal-format or existing tool-contract change is introduced. Native focused/failing-before/full
validation is queued behind the preserved local full run; hosted and packaged proof remain required.
Fleet idempotency/tool exposure and later deletion/integration/restoration increments follow in order.

D01a canonical checks: repository 7, docs 5 plus 106-document links, license/storage self-tests,
Rust formatting and whitespace checks pass. These checks do not establish native runtime behavior.

## D01b deletion intent, preparation and outcome records

Transfers the state/replay portion of source `278beb6d5549a2380953f0cbfb2e1351566637df`
onto canonical PR #77 at `0e3c36b30354c11bb61c5aaa7fe80f17bbbf907d`: exactly
`fleet/file_deletions.rs`, the additions in `fleet/mod.rs`, `fleet/wire.rs` and `fleet/tests.rs`.
The remaining authenticated changeset, native workspace/live, service, service tests, fleet-agent
and MCP changes from the same source commit are intentionally deferred to the dependent increment.
This records partial replacement, not completion of the whole source commit.

The journal binds explicit intent before one exact prepared operation and allows only its matching
outcome. Replay has no filesystem effects. New intent/preparation refuse after cancellation while
an already-prepared outcome remains reconcilable. Three transferred regressions cover restart and
idempotency, cancellation, malformed paths/stale runs/replaced inputs and closed additive encoding.
Existing histories remain readable; older binaries refuse new command kinds. Native execution and
harness exposure are not included in this slice. Local native/failing-before/full validation remains
queued behind preserved verification; hosted runtime evidence is required before delivery.

## D01c native deletion retry and authenticated harness access

Completes the remaining implementation portion of source
`278beb6d5549a2380953f0cbfb2e1351566637df`, following records-only PR #78 at
`35b20458158fce6f5d281906ef1a05845483080b`. Transfers authenticated changeset signer inspection,
workspace authenticated-operation lookup, live prepared deletion callbacks, fleet service/recovery
and service tests, fleet-agent and managed-entry tests, MCP tools/tests and the associated source
documentation. Together with D01b this accounts for all paths changed by that source commit;
canonical documentation retains current validation limitations and prior delivery history.

The prepared operation is durably recorded before append. A retry binds exact request/origin/path/
version and either preserves the prepared identity on the same input fold or verifies an already
appended authenticated operation without signing again. Later file recreation is not erased.
Recovery reports an unsettled observation rather than claiming a clean folder. Authenticated harness
inspection is bounded; explicit resolution never removes present OS entries or advances main.
A new complete checkpoint remains necessary before review. Directory deletion, empty-result review
and host reconciliation after cancellation follow as separate preserved source increments.

Transferred tests cover interruption before append, lost completion after append, signer refusal,
callback authority isolation, exact repeat/conflicting inputs, wrong operation/actor and later edits,
plus scoped MCP routing. Local focused/failing-before/full native execution is queued behind the
preserved full run; no packaged or real-agent completion is claimed from these source tests.

## D01d inspection-only deletion results and historical proof accounting

Transfers implementation source `c7ac2e33de18a20b2517e23b667b0b3847e0ed19` onto PR #79 at
`19e85e29f400f3974c4d72583e93d214f384570f`. All six code/test paths are transferred; source
contract text is adapted to preserve canonical validation history. Exact native state equality permits
an independently derived no-change inspection identity, while approval/publication still require
regular bundles. Pinned input comparison retains deletions; project import produces the actual
source-project review. Transferred tests cover immutable later reads, retained history, changed bases,
source preservation, MCP inspection and refusal of approval context/preview for inspection identities.

Two associated source documentation commits are accounted for as historical evidence, not current
canonical acceptance: `6c40c1e1c6562a931d533219032d4c2f0e04f3f2` records the packaged deletion
journey at old source `278beb6d5549a2380953f0cbfb2e1351566637df`, with 3,248 Rust tests (14 skipped)
and 666 desktop tests; `c1835ae651f4ca57edd399be0697b3c73a17520d` records the packaged empty-result
journey at old source `c7ac2e33de18a20b2517e23b667b0b3847e0ed19`, with 3,249 Rust tests (14 skipped)
and 666 desktop tests. Both source records report `source_exact=true`, `graphical=false` and ad-hoc
signing. They do not establish graphical or platform-backed approval; the latter records unavailable
Computer Use permissions. Those historical reports were not rerun here and must not be presented as
verification of any canonical replacement commit. Current packaged and graphical acceptance remains.

Canonical native focused/failing-before/full execution remains queued behind preserved local runs.
This source accounting does not complete directory deletion, cancellation recovery, grouped integration,
restoration, provider/remote acceptance or final combined-main verification.

## I01a native accepted-review replacement groups

Transfers source `496debf33e758b05985c493a7ecb30a230fdcd42` onto canonical PR #80
at `4e3daa214e7ccb63dac64622f34e571d53491ec5`. The native group stages all supported replacements
for an exact accepted review, preserving already-present files and unrelated user work. Every member
is validated before the first exchange and again at its individual boundary. Attempt/outcome records
retain partial results without rollback or replay, and read-only inspection rederives full accepted
membership and verified per-file evidence. Up to 64 changed files and the aggregate byte budget apply.
Unsupported additions/removals/directories refuse the whole proposal; subsequent entry executors and
desktop confirmation/recovery remain required for the complete integration objective.

Canonical conflict resolution retains localized recovery adaptations, full-mode validation correction
and current proof limitations. Existing single-file receipt/journal/approval formats remain unchanged;
new external group records are versioned, identity-bound and non-atomic. Older readers must not replay
them. Transferred tests cover complete preflight, partial interruption after the first exchange,
concurrent editor changes, preserved stages, already-present inode preservation and restart inspection.
Local native/failing-before/full validation is queued behind preserved runs. The source's historical
3,252 Rust/666 desktop result is not validation of this canonical replacement.


## I01b bounded project capture reuse

Transfers source `0a1af9129359a9bfbfe1ef40131d2c38d6ffd481` onto PR #81 at
`c80d0fc70bfa573985d45ee1de875e90a005daf8`. Group staging shares one complete captured input
under the verified history lock. Each member still reopens its exact file and verifies identity,
bytes, metadata and exclusion policy. Standalone replacement retains complete fresh preflight.
No persisted record format or authority boundary changes.

Transferred regressions compare capture counts for 2- and 24-member groups and stop later exchanges
when ignore rules change after the first member. Local native focused/failing-before/full execution
is queued behind the preserved full run. Source-reported 3,254 Rust/666 desktop results are historical
only. Scan-count bounds are not measured latency or packaged acceptance.


## I02a retained regular-file removals

Transfers source `319a4cd7a0d093689e6aa2d4f168830376a01337` onto PR #82 at
`5f124dc2ac496e9c8e3e66353018e4498b41b8a0`. Accepted groups can remove a regular file by
moving its inode into private same-filesystem recovery without overwriting a destination. Open
editors retain their inode; later writes and recreated source paths are preserved. Every supported
member is preflighted, and partial outcomes remain explicit without automatic rollback or replay.
Absence requires an exact confined parent; missing parents and symlink failures are not absence.

New versioned removal receipts have null installed-file fields and bind accepted history, original
identity, metadata and recovery. Existing replacement/restoration formats and full-mode validation
remain intact; older readers must refuse unknown removal receipts. Standalone replacement does not
gain removal authority. Additions, directories, restoration into absence and desktop group flows
remain separate increments.

Transferred tests cover late editor writes, changed sources, occupied recovery, rename races,
substituted inodes, failed directory flushes, missing/replaced parents, mixed groups and restart
inspection. Local native focused/failing-before/full verification is queued behind the preserved
run. Source-reported 3,264 Rust/666 desktop results are historical only, not canonical acceptance.


## I02b approved regular-file additions

Transfers source `b8e9d0dcf1cafd65a06a390a1222579941b3236b` onto PR #83 at
`04408cf8c89bef5d307fa13747282f347dbd43dc`. Accepted groups stage new regular files privately
and install them into an exact existing parent using an atomic no-overwrite rename. Concurrent
creation preserves both user work and the stage; uncertainty never triggers rollback. Genesis
approval is supported. Prospective paths obey captured exclusions and the structural Git boundary.

Versioned addition receipts bind staged identity, native metadata and approved history, with null
source-file fields. Older readers refuse unknown addition receipts; existing replacement/removal/
restoration formats and canonical mode validation remain intact. Standalone desktop preparation
remains replacement-only. Directory changes, absent-path restoration, inherited destination metadata
and complete grouped desktop/packaged acceptance remain separate work.

Transferred regressions cover executable files, concurrent creation, changed stages/parents/roots,
failed receipts and durability, exclusions, genesis and mixed groups, malformed receipts and restart
inspection. Repository structural checks do not establish these runtime properties. Native focused,
failing-before and full execution remains queued behind the preserved run. Source-reported
3,273 Rust/666 desktop results are historical only.


## I02c destination permissions for additions

Transfers source `425f2e06e2ab9ce2c7f01ed8013416527d8c7d3f` onto PR #84 at
`dea7d1c6bd0b7c468fe3e4cccbd43fd9a46751b4`. Staging preserves process umask without changing
process-global state; macOS additions inherit the destination group and file ACL while clearing
staging-folder ACL inheritance. Exact parent policy is checked before and after installation.
Changed policy refuses or reports uncertainty while preserving work.

New addition v2 receipts bind parent metadata digest and mode. Read-only recovery still accepts
v1 receipts and exposes v2 policy mismatch without granting replay authority. Existing replacement,
removal and restoration formats remain intact. Linux group handling is implemented, but default
ACLs/extended attributes remain unsupported and must not silently lose metadata.

Transferred tests compare macOS inheritance with kernel-created files, verify restrictive umask in
an isolated process, refuse changed ACLs/modes, and inspect persisted policy after restart. Local
native focused/failing-before/full verification remains queued behind the preserved run. Historical
source reports of 3,279 Rust/666 desktop tests and its CAS lingering-handle diagnostic are retained
as provenance only. Canonical runtime and packaged acceptance remains outstanding.


## I03a retained restoration into an absent file path

Transfers source `59e0b5b13995e516ed112b09cd295311807523e3` onto PR #85 at
`ec305a6ada14d5d3e33d64d9fdde33d8c06203b8`. Restoration stages a frozen copy of retained
content and supported metadata, preserving the original inode and later editor writes. Exact
absence under an existing confined parent and capture exclusions are required; concurrent creation
is never overwritten. Explicit native confirmation distinguishes creation from replacement.

Canonical conflict resolution preserves literal project/folder labels and the root argument in
confirmation and its regression tests. New versioned restoration-addition receipts retain origin
provenance and parent-policy evidence; existing formats remain supported, unknown schemas refuse,
and inspection never authorizes automatic replay or main advancement. Missing parents, directories
and the complete grouped desktop/packaged journey remain outstanding.

Transferred tests cover removal/replacement origins, metadata preservation, later editor writes,
collisions and substituted parents, exclusions, malformed receipts and restart inspection. Native
confirmation tests reject inconsistent absence facts. Local focused/failing-before/full native
execution remains queued behind the preserved run. Source-reported 3,284 Rust/666 desktop and
28 isolated restoration tests are historical only.


## I03b native desktop group confirmation and recovery host

Partially transfers source `fc5b37d9bea6edf4311dea4a2914145da4e600ab` onto PR #86 at
`4e55abd227327f3b5bd7387d14e9a831b0d4a513`: native attachment host, confirmation and commands,
daemon group/recovery inspection, and attachment approval integration tests (six source code paths).
The renderer controls/tests and source documentation remain a following increment; the source
commit is not yet fully accounted for. Four watcher paths are deliberately not applied over the
newer canonical #41 lifecycle implementation. Their independent differences still need reconciliation;
this transfer does not claim issue #37 resolved.

Native commands prepare the entire regular-file group, present every create/replace/remove and
already-present path, then recheck host generation and apply. Incomplete/binary/oversized previews
refuse before consent. Read-only group/member inspection and explicit retained-member restoration
use verified group identities. Bounded restoration references preserve restart discoverability.
Project and folder labels are separate escaped native facts; the transferred confirmation test now
covers misleading newlines and non-Latin folder names. Renderer wiring and graphical proof remain.

Existing group/receipt formats remain supported; recovery results add bounded restoration reference
fields. Native focused/failing-before/full execution is queued behind the preserved run. Structural
checks and historical source claims are not canonical native or packaged acceptance.

## I03c desktop group controls and recovery projection

Transfers the four renderer paths from source `fc5b37d9bea6edf4311dea4a2914145da4e600ab`
onto PR #87 at `54ca0c3d991d90e4a0b88ac0dd29c2a3e5265f42`: attached-project controller/tests,
React attached-project cards and group recovery rendering tests. Native commands were transferred
in I03b. Conflict resolution preserves canonical import/navigation controls, Hebrew UI and literal
identifiers. New group labels and feedback are translated; group/path/transaction values remain
literal and left-to-right. Source watcher paths and associated source-document reconciliation remain
separate; this does not mark that source commit fully accounted for or issue #37 resolved.

The controller binds exact project/main/group identities, preserves partial outcomes, distinguishes
changed, uncertain and unattempted members, and never retries on refresh. Recovery references permit
explicit bounded group/member lookup; stale or failed observations disable restoration. Approved
regular-file groups are still applied only by native confirmation. Prepared already-present paths
are historical evidence rather than a fresh filesystem observation. Directory support and current
packaged/native graphical acceptance remain outstanding.

Local validation: all 45 attachment controller tests pass. Removing the stale-group error guard
makes the exact group-application regression fail; restoring it returns all 45 tests to passing.
TypeScript passes. All three group rendering tests pass, including Hebrew literal identifiers and
disabled stale restoration. The new Hebrew assertion initially used different wording from the
existing translation; it was corrected to the established label without changing the disabled-state
assertion. Full desktop verification is running; native full and packaged acceptance remain pending.

### PR #88 localization correction propagation

The desktop gate in run `36444262284` exposed two stale localization
expectations after group application and absent-path restoration were delivered.
The correction from `e3472d8e7fe1c9e9f43a00d633e26efbfc97313e` (PR #90) is
applied here without rewriting the original PR #88 commit or its active local
verification tree. It updates the expected safety copy and checks the translated
group action while retaining literal-identifier, inert-markup and disabled-action
assertions. The corrected downstream desktop gate passed in run `36445006220`;
this branch must independently pass its required checks before merge. PR #89 and
#90 will reconcile this correction through ordinary history-preserving merges.

## I03d watcher and documentation reconciliation

Completes path accounting for source `fc5b37d9bea6edf4311dea4a2914145da4e600ab` on PR #88
at `a75d3c9ce02f2f486aa91305194e957f37615472`. Six native paths transferred in #87; four
renderer paths in #88. Of four watcher paths, attachment-background tests now use the source's
post-registration capture baseline and event-counter increment, additionally preserving canonical
initial saved identity. The source background.rs, background/signal_worker.rs and signals_macos.rs
implementation is superseded by canonical #41's explicit native lifecycle and signals_worker.rs.
Terminal capture atomically sets stop, phase and inactive events; callbacks and late registration
check that stop flag. Importing the older worker would lose canonical status and regressions.

The source's four documentation paths are reconciled through this ledger, project status, fleet
plan, retained-replacement contract and corrected project-attachment lifecycle text. Its historical
3,288 Rust/673 desktop result (14 Rust skips), including native event wakeups, is provenance only.
It neither validates this canonical replacement nor resolves the later local registration failure.
Source path accounting is complete; runtime acceptance, issue #37 and the full fleet objective are not.

No production watcher behavior or deadline changes. The test adjustment still requires real native
registration and new callback evidence before the five-minute fallback, with the existing bounded
waits. Local native focused/failing-before/full execution is queued behind the preserved run.

## I03e canonical localization regression correction

Hosted PR #88 run 36444262284 failed desktop-and-docs: two localization assertions still expected
replacement-only wording that the grouped application/absent restoration feature intentionally replaced.
The renderer test stage passed 152 cases and failed those two. This correction updates the expected
safety text to complete native group checks, directory/preview limitations and absent-path restoration,
and explicitly requires the translated group application label. Both English/Hebrew translation,
literal path/identity, inert script text and disabled-control assertions remain intact.

This is new canonical correction work on PR #89 at
`50f5e73bb1b666e40e304b5c78c758fddfa761e2`, not another source-commit transfer. The failing
parent revisions and their local running verification remain preserved. Focused localization/group
rendering execution is running on this corrected tree; hosted and full acceptance must be refreshed.
No product behavior, permissions or verification threshold is changed by the test correction.

## I03f native saved group execution evidence

Partially transfers source `9bfc1f43466ec5a45082a854e51421dab2ae32d2` onto
PR #90 at `3ab0dc58e1951a7c9cb4b1c63e13cacfe3b33d23`. This increment contains
the three native paths: group inspection, the execution record reader and native
group regressions. Four renderer paths and the remaining source documentation
accounting are reserved for the following localized UI increment.

Historical execution is explicitly separate from current independent file
observations. Exact bounded records, contiguous attempts, stop ordering and
before/after record comparison determine whether the saved outcome can be shown.
Missing, corrupt, linked or changing records never grant retry authority. Existing
persisted formats remain unchanged; inspection adds a versioned projection.

Native focused, failing-before and full execution are queued behind the preserved
canonical run. Required hosted checks must pass before merge. Neither historical
source results nor structural checks establish current native or packaged proof.

## I03g localized saved execution recovery

Completes source `9bfc1f43466ec5a45082a854e51421dab2ae32d2` accounting on PR #91
at `cb5a7cf4071e3702331d730529c01df29ea0d995`: four renderer paths transferred,
with canonical Hebrew translations and literal left-to-right identifiers retained.
The three native paths are in #91. The source's three documentation changes are
reconciled here and in the fleet plan, project status and retained-file decision.
Its historical 3,290 Rust/676 desktop tests and 14 skips are provenance only, not
verification of this canonical tree.

The controller binds saved outcomes to the proposal and ordered membership,
rejects inconsistent attempt order and authority flags, and reopens without writes.
The view separates historical execution from current file observations, translates
missing/invalid/changing evidence and hides unreliable attempts. Old projections
remain readable with an explicit unavailable state. Local controller tests: 47
pass; removing the proposal-digest guard makes the identity regression fail, and
restoring it returns all 47 to passing. TypeScript/rendered verification is running;
full native and packaged acceptance remain pending.

## I04a confined directory creation foundation

Partially transfers source `c711c6531220f61bfe5e9994f12df19c4450bd2c` onto
PR #92 at `d2ea4b19edb588948b5b94e3ee363a0cceb0c536`: only
`crates/mesh-daemon/src/root_authority.rs`. The remaining retained-tree primitive,
metadata inheritance, directory executor, group integration, desktop confirmation,
renderer and source documentation changes remain following increments.

Pinned directory creation accepts only ordinary single-component names and modes.
The kernel applies the existing process umask; Mesh neither reads nor changes that
process-global setting. Existing private allocation remains mode 0700. New creation
is create-only and validates pinned namespace identity. The transferred regression
compares ordinary kernel-created directory modes, preserves existing child work,
rejects traversal/special modes and refuses a replaced parent. Native focused,
failing-before and full execution remain queued behind the preserved canonical
run; structural checks are not runtime proof. No durable formats or public write
commands change in this foundation.

## I04b directory destination permissions

Partially transfers three more paths from source
`c711c6531220f61bfe5e9994f12df19c4450bd2c` onto PR #93 at
`c4b12ffc29c5ffc106fe89b0d2a4fec8ca6c88c0`: retained replacement addition,
metadata exports and metadata creation. Root authority is in #93. The remaining
tree primitive, daemon executor/group recovery, desktop confirmation/renderer and
source documentation accounting remain separate increments.

New-entry inheritance distinguishes directories from regular files, carries
macOS inheritable ACL rules to descendants with limited-propagation handling,
clears unrelated staging ACLs and retains Linux destination setgid semantics.
Existing file behavior and permission limits remain. Linux default ACLs/xattrs
remain explicitly unsupported and refuse. Source macOS tests compare kernel
creation, descendants, group and deny entries; an additional Linux regression
compares modes/group/setgid, unchanged parent and invalid-mode refusal. Native
focused/failing-before/full execution remains pending behind the preserved run.

## I04c native retained subtree integration

Transfers nine native paths from `c711c6531220f61bfe5e9994f12df19c4450bd2c`
onto PR #94 at `eafef923292ffde5c717b0428ed3276afb045fe8`: retained-tree
module/export, directory writeback/export, provisioning, group integration/tests,
recovery history extraction and attachment approval integration tests. Root
authority and metadata inheritance are in #93/#94. The native desktop confirmation,
four renderer paths and remaining source documentation accounting are still pending.

Complete approved absent subtrees are staged privately and installed with a
same-filesystem no-replace root rename. Exact history, parent policy, exclusions,
stage identity/content and expanded group coverage are rechecked. Races and
uncertain durability preserve work and record reconciliation; no automatic undo,
cleanup or replay exists. Group v2 adds directory members while v1 remains readable.
The existing desktop confirmation rejects directory groups because its complete
file count differs from group membership; complete-tree confirmation follows.

Transferred regressions cover staged tampering, occupied destinations, changed
parents/policy, receipts/trust/budgets, restrictive umask, late descriptor writes,
partial groups and restart inspection. Native focused/failing-before/full execution
remains pending behind the preserved canonical run. Historical source claims
(3,304 Rust/678 desktop, 14 Rust skips) are not evidence on this tree.

## I04d complete-tree native confirmation

Transfers `apps/desktop/src-tauri/attachment_recovery.rs` from source
`c711c6531220f61bfe5e9994f12df19c4450bd2c` onto PR #95 at
`0f5a5b4276fc2a9a91193edca32a5226f7a7e95b`. The three native foundation
increments already contain its directory executor and permission dependencies.
Four renderer paths and remaining source-document accounting still follow.

The prompt includes every directory (including empty ones) and frozen file with
exact content/digest/mode in group order. Missing, mismatched, binary or oversized
content refuses the whole prompt; expanded membership shares the 64-entry bound.
Conflict resolution preserves the canonical separate escaped project/root labels
and all existing confirmation tests. The new tree test also covers misleading
newlines and non-Latin folder identity. Native focused/failing-before/full tests
remain pending behind the preserved run; no actual OS dialog proof is claimed.

## I04e localized directory recovery

Completes source `c711c6531220f61bfe5e9994f12df19c4450bd2c` accounting on
PR #96 at `62de5dfaa3b441f90770594de2a91bfe539c7c36`: four renderer paths
transferred here; native paths were split across #93–#96. The three source
documentation changes are reconciled across these increments and this ledger,
retained-replacement decision, fleet plan and project status. Historical source
3,304 Rust/678 desktop tests and 14 Rust skips do not validate this canonical tree.

Typed directory observations validate identity, bounds and no-authority flags.
The localized view shows source/stage entries, empty folders and changed parent
facts without retained-file restoration. Hebrew and literal identifiers remain;
updated safety-copy assertions accompany the changed presentation. Controller
tests: 48 pass. Removing the write-authority refusal makes the directory regression
fail; restoring it returns all 48 to passing. TypeScript and all 28 rendering/localization tests pass; full desktop, native and
packaged proof remain pending. Directory removal/type
replacement, large/binary confirmation and the full fleet acceptance remain open.

## I05a retained directory removal and native confirmation

Partially transfers source `a02b33d8c7454a440d3ca94ae22f34e762913cc6` onto
PR #97 at `43df0cdd173036d96aa8eb35d029b98207dec0ab`: eight daemon paths
(retained removal module/export, directory writeback/export, group integration/tests,
provisioning and approval tests) and native desktop confirmation. Four renderer
paths and final source-document accounting follow separately. Canonical escaped
project/root labels and newline regressions are retained through the test conflict.

The operation verifies the complete approved base tree and retains the actual root
and descendants through a no-replace move into recovery. Open file and directory
handles preserve later writes. Unknown/ignored/changed entries refuse; uncertain
races and durability retain evidence without replay or cleanup. Group v3 requires
removal receipts while v1/v2 remain readable. Native confirmation distinguishes
removal from creation and lists frozen base content. Local native focused,
failing-before and full execution remain pending behind preserved verification.
Historical 3,311 Rust/679 desktop tests and 14 skips are provenance only.

## I05b localized retained-directory recovery

Completes source `a02b33d8c7454a440d3ca94ae22f34e762913cc6` accounting on
PR #98 at `1a95c9c59e21a850ae53c1962fabe4362a68a0e7`: four renderer paths
transferred here; nine native paths in #98. Its three documentation changes are
reconciled across the ledger, retained-replacement decision, fleet plan and project
status. Historical 3,311 Rust/679 desktop results and 14 skips are provenance only.
The source macOS file/directory exchange experiment establishes only a historical
platform prerequisite, not canonical conversion or packaged acceptance.

Directory removal recovery is typed separately from addition, labels the retained
tree and explains ongoing descriptor writes without offering file restoration.
Hebrew text and literal paths are preserved; changed safety-copy assertions ship
together. All 48 controller tests pass. Mutating removal classification to addition
makes the operation regression fail; restoration returns all 48 to passing.
TypeScript and all 30 rendering/localization tests pass. Full native, packaged confirmation/recovery,
whole-tree restoration and file/directory conversion remain unfinished.

## I06a native retained entry conversion

Partially transfers source `6ec9c8ccf9e273de25d1a84c323c0cd4f3f4b821` onto
PR #99 at `c1cdf63721014eb774dc520d9b99df9a017571be`: seven daemon paths
(retained conversion/export, shared tree staging, directory writeback, group
integration, provisioning and approval tests). Native desktop confirmation, four
renderer paths and remaining source-document accounting follow separately.

Both file-to-directory and directory-to-file replacements bind approved before/after
content and retain original objects through one native exchange. Whole-source and
stage observations, trust, exclusions and parent policy are rechecked. Group v4
accounts for the union of changed paths while v1/v2/v3 remain readable. Existing
desktop directory confirmation rejects the conversion schema until complete
two-sided presentation is added. Local native focused/failing-before/full execution
is pending behind the preserved run. Source historical 3,318 Rust/680 desktop
tests with 14 skips are provenance only, not canonical runtime proof.

## I06b complete two-sided conversion confirmation

Transfers native `apps/desktop/src-tauri/attachment_recovery.rs` from source
`6ec9c8ccf9e273de25d1a84c323c0cd4f3f4b821` onto PR #100 at
`31b4330f547f83e40511c74576fcc657a2df5ec3`. Seven daemon paths are in #100;
four renderer paths and final source-document accounting still follow.

Confirmation lists the original entry to retain and replacement to install, with
every file and empty directory, exact frozen bytes/digests/modes and conversion
direction. Shared-root coverage is counted once in the bounded group. Missing,
binary or oversized sides refuse. Conflict resolution preserves canonical root
arguments and escaped project/folder tests; conversion tests additionally use a
misleading newline/non-Latin folder. Native focused/failing-before/full tests
remain queued behind preserved verification; packaged dialog proof is pending.

## I06c localized conversion recovery

Completes source `6ec9c8ccf9e273de25d1a84c323c0cd4f3f4b821` accounting:
seven daemon paths are in #100, native confirmation in #101, and the remaining
four renderer paths are reconciled here onto #101 at
`cdb7e6153208ab1ad7b3b46e7819a640fc4c4861`. The three source documents are
reconciled into the retained-replacement decision, product status and fleet plan;
the source's historical 3,318 Rust/680 desktop checks with 14 skips are not
canonical runtime evidence.

Recovery validates both conversion directions, permits a complete root-file
observation, and keeps conversion references out of ordinary file restoration.
English/Hebrew presentation preserves literal paths and labels retained original
objects. All 48 controller tests pass; removing direction validation makes its
regression fail, and restoration returns all 48 to passing. TypeScript and all
32 rendering/localization tests pass. Full canonical validation is still required:
the previous main #70 run failed native watcher registration (issue #37), with
1,274 passes, one failure, 14 skips and 1,982 tests unrun. Its logs remain preserved;
this increment does not resolve that failure or establish packaged acceptance.

## I07a preserved native whole-entry restoration

Reconciles the eight native paths from the preserved dirty fleet worktree based on
`6ec9c8ccf9e273de25d1a84c323c0cd4f3f4b821` onto #102 at
`b50fe49f70bf81bd1ca60879e3a074be87f6d70f`. There is no source commit to claim:
five tracked native patches and three untracked modules were inventoried and
backed up without changing the original checkout. The three dirty source documents
are reconciled here rather than copied with historical progress claims.

This increment adds frozen entry copying, native single-use restoration and
restart inspection. Original retained objects and newly displaced destination work
remain separate. Trusted ancestry, exclusions, exact observations, parent policy,
bounds and ordinary metadata are checked; restoration does not advance Mesh main.
The preserved native/domain regressions are transferred, with an additional
admission-refusal/shared-byte-budget regression requiring no private staged copy.
Native execution and failing-before proof are pending behind #102's active full
validation. Desktop confirmation, command wiring, discovery and localized recovery
for this operation still require implementation; this is not finished restoration.

## I07b native desktop whole-entry consent

New canonical implementation builds on #103 at
`30462d0d6c7226f6475cf73cc1084c2fd6c8e11e`; no preserved desktop restoration
implementation existed to transfer. The registered command uses native recovery
resolution and optional verified group membership, frozen complete confirmation,
and the existing host-generation guard before applying single-use restoration.

Prompt regressions cover both original entry types over absent/file/directory
current work, literal multiline/non-Latin identities, empty folders, complete bytes,
missing sides, inconsistent copies, wrong identity, binary/NUL content and size
refusal. These native tests await execution; #102's full verification still owns the
shared build target. Source checks do not establish native-dialog or packaged
acceptance. Renderer commands, discovery, localized recovery and explicit undo
remain the next phase.

## I07c bounded whole-entry recovery discovery

New canonical implementation based on #104 at
`ab46c6fed087af8a7e54418233a60cdef50e6339`. Native recovery pages expose separate
whole-entry references, and exact selection delegates to the native receipt
inspector before the ordinary-file history lock. Group references remain distinct
from ordinary file restoration. No source commit is replaced by this new work.

A native regression covers direct and grouped conversion/restoration, reopening
both exact record types, unchanged Mesh history, bounded listing, and canonical
names without valid receipts. Names remain references, not authority. Native
execution and failing-before proof are pending behind #102's live full validation;
renderer presentation and packaged acceptance still follow.

## I07d whole-entry recovery controller

New canonical implementation based on #105 at
`1e17abc162c6d27f69c2589f49fe693a3207be7e`. Adds typed discovery, three-side
restoration observations, dedicated native result validation and explicit
restoration/undo intents without renderer filesystem authority. English/Hebrew
feedback is included; visible controls follow separately.

All 52 controller tests pass, including direct/grouped selection, missing receipts,
invalid identities, forged authority, unobserved/prepared selection, stale errors,
extra path injection and no automatic retry. Removing original-retention result
validation makes its regression fail; restoring it returns all 52 to passing.
Full desktop validation is pending, as is combined native validation still running
on #102. Source/controller proof does not establish packaged restoration acceptance.

## I07e localized whole-entry recovery controls

New canonical UI based on #106 at
`8df187fb55cbf38c7cccc83a8749c3ed5c54ab8e`. Adds three-side observations,
direct/group reference lists, exact lookup, and distinct native review requests for
restoration and undo. English/Hebrew labels, literal paths and changed localization
assertions ship together. No source commit is replaced by this new work.

TypeScript and all 35 focused renderer/localization tests pass. Removing the
whole-entry button's disabled-state guard makes the new regression fail; restoring
it returns all 35 to passing. The preceding #106 full desktop run passed 162
renderer and 575 source/controller tests. This increment's full desktop gate remains
pending. #102's full native run is still active; actual native dialogs, packaged
restoration and final combined-main acceptance remain unverified.

## I08a shared provider event decoding

New canonical implementation based on #107 at
`673b62dcbbe015ddd83b32a5bcd172cdbf666c4f`; no preserved source commit is replaced.
Codex's pipe reader now delegates to a bounded external-provider decoder. Mesh IPC
remains integer-only, while provider cost and usage numbers may be fractional or
signed. Existing locked serde/serde_json versions are reused as direct daemon
edges, with no package version changes. Duplicate keys, malformed JSON, excessive
nesting/size and changing session identities refuse.

Claude stream-json observation distinguishes an explicit successful result from
`subtype=success,is_error=true` and assistant authentication errors. Errors remain
sticky across later successful results. Shared protocol tests cover both formats;
raw message bodies and numeric values do not enter observations. The existing
Codex observation name remains a compatibility alias. This does not launch Claude,
change durable dispatch/custody, or claim a second successful real provider.

Repository/docs/license/storage checks pass. Focused source tests are running in
an isolated harness to avoid mutating the target used by the active #102 full
native run. Full canonical validation, mutation evidence and live Claude execution
remain pending. The #107 hosted checks all pass, but its local desktop build is
still waiting in native module loading. No source or branch result substitutes
for the full acceptance journeys and merged combined-main checks.

## I08b native Claude launch and provider-aware scheduling

New canonical implementation based on #108 at
`6e4f5c64569bfc97f9a1384a4124819fd87f2a5d`; no preserved source commit is replaced.
Adds an explicitly admitted Claude adapter and a shared native process driver.
Provider selection feeds the existing durable claim/custody/identity checks before
spawn. Existing Codex APIs remain compatible. Native hosts dispatch their own
provider's lanes while sharing objective limits, without adopting uncertain runs.

The Claude command has fixed Mesh MCP/sandbox configuration, private stdin task,
environment credential references, and no permission bypass. Managed CLI policy
remains authoritative; actual installed-provider sandbox and bridge behavior must
be proven by live acceptance. Desktop provider selection remains follow-up work.
Fixture conformance now covers Claude and mixed-provider scheduling. An ignored
live Claude journey requires an actual file edit, signed checkpoint and immutable
review through the real bridge; it is not an authentication-only probe.

Adapter source compilation passes in a byte-matched isolated harness; this does
not validate the service/host integration. Repository/docs/license/storage/format
checks pass. Full native fixture tests, failure-injection evidence, npm validation
and live Claude acceptance remain pending. Existing #102/#107 verification runs
are preserved; no new large target is built while the shared native target is in
use and local disk headroom is limited. Hosted canonical checks remain mandatory
before merge, alongside the unresolved native and packaged acceptance gates.

## I08c durable native provider policy

New canonical implementation based on #109 at
`f8ef047b01d0d25198f6bb5a1502d4f813132392`; no preserved source commit is replaced.
Native allocation can now bind a coordinator provider and a closed set containing
Codex, Claude, or both. The original creation API remains Codex-only. Explicit
policy creation rejects unknown/duplicate providers and excluded coordinators
before allocation. Services expose their immutable admitted provider identities
for native host configuration; reading the list grants no execution authority.

Default Codex allocations retain the exact v1 receipt format. Non-default choices
use `mesh.native-fleet-allocation/v2` with canonical provider order. Same-request
retries cannot change the coordinator or widen/narrow the set. Discovery validates
receipt policy against retained lane facts and still restores observation only,
without adopting workers or granting current-session execution. Unknown/noncanonical
receipts remain unavailable, with their original bytes preserved.

Native regression cases cover default, Claude-only and mixed policies, exact retries,
conflicting requests, v1 compatibility, restart refusal and receipt/ledger mismatch.
Focused policy tests run against byte-matched source in an isolated harness;
full catalogue execution and canonical npm validation remain pending. Repository,
docs, license, storage and formatting checks pass. Desktop input/receipt controls
and configuration of all admitted worker hosts follow in the next increment; this
native persistence change alone does not expose provider choice in the UI.

## I08d desktop native provider composition and receipts

New canonical implementation based on #110 at
`6dcff52b999ce4a7fa032edabeb6410eed5a424f`; no preserved source commit is replaced.
Desktop execution now admits all executables named by the saved policy before
registering an objective or starting any worker. The application owns one loop
containing the provider-specific native hosts, sharing cancellation, latched errors,
observations and the objective's durable limits. Missing/duplicate configurations
refuse without partial start. Native lookup supports the normal Claude installation
symlink; executable paths and credentials never come from renderer input.

Provisioning accepts optional closed provider JSON. Legacy requests retain v1
response compatibility; explicit choices receive `mesh.desktop-attached-fleet/v2`
with the exact canonical policy. Catalogue rows add the verified policy for current
and restored fleets, or null when unavailable, so the UI can show permitted usage
before Start. Invalid or conflicting provider inputs do not provision new storage.
Visible selector/controller integration remains the next increment.

Native tests cover complete provider admission before launch, a Claude coordinator
with both providers configured, idempotent start, exact provisioning receipts,
unknown/duplicate/injected inputs, legacy compatibility and restart without adoption.
Repository/docs/license/storage/format checks pass. Full desktop validation is
running; native integration and combined npm validation remain pending. The corrected
#109 now passes all seven hosted checks; #110 native checks are still pending.
No successful authenticated Claude, multi-machine or packaged acceptance is claimed.

## I08e visible provider choices and exact retry receipts

New canonical implementation based on #111 at
`dee740c36a0bd8f535317ee7228d0687df547d76`; no preserved source commit is replaced.
The fleet form chooses a Codex or Claude coordinator and the permitted provider set.
The coordinator remains included. Pending requests freeze the goal, saved version,
budgets and canonical provider choices; retries reuse the exact request. Explicit
choices require the matching v2 native receipt. Legacy callers retain v1 compatibility.
Catalogue policies are validated against every lane and the coordinator. Old snapshots
without policy remain readable but cannot start agents. Saved choices appear before
Start, in English and Hebrew, with provider identities displayed literally.

All 378 controller tests pass, including 17 fleet tests. Removing receipt-policy
equality fails the regression suite; restoring it passes. Renderer tests cover
pending controls and saved choices, with their execution still pending at publication.
Repository, docs, license, storage and vocabulary checks pass.
Full combined native, actual Claude, remote recovery and packaged acceptance remain
separate pending gates. This increment does not claim merged delivery.

## I09 four-worker native measurement driver

New canonical implementation based on #112 at
`82a218b89132002c4e00eb0f6ba6966a0f362e86`; no preserved source commit is replaced.
The existing real two-worker journey now shares its exact-result and unchanged-source
checks with a four-task serial/parallel driver. It retains provider version and native
timing samples, reconstructs every saved review, and refuses parallel acceptance when
four-worker overlap was not observed. The [measurement contract](fleet-native-measurements.md)
separates these native timings from still-required human/renderer/resource/cost evidence.
A normal pure percentile regression covers empty and small samples, rank boundaries
and ordered inputs. Actual provider execution, whole integration compilation and full
combined-native validation remain pending; this is a published acceptance driver,
not a performance claim or completion of Phase 5.

## R01 durable remote assignment foundation

New canonical implementation based on #113 at
`5c21f94bb6fc0cf5418a9ef8b15a84eb92d56ad4`; no preserved source commit is replaced.
An additive remote claim binds one existing dispatch to its exact input, bundle,
worker identity and lease. Exact-sequence renewal retains the same owner and never
relaunches or frees its slot. Original local claims and legacy histories retain
compatibility. SQLite replay/refusal regressions cover restart, lost acknowledgments,
stale renewal, changed peers/inputs, duplicate identities, local claim conflicts and
terminal/cancelled runs. Closed wire tests reject injected and fractional fields.
The [remote delivery sequence](fleet-remote-delivery.md) identifies the remaining
authentication, transfer, executor, reconnect, UI and second-machine acceptance work.
No remote execution, authentication or verified transfer is claimed by this reducer.

## R02 single-use assignment worker proof

New canonical implementation based on #114 at
`02cad17eb9814ef5a53df9a1bee41a4e1f10dc36`; no preserved source commit is replaced.
The native RemotePeerChallenge binds OS randomness, the configured worker key, exact
pending context and an expiring assignment under its own signing domain. Strict
verification precedes a revision-checked claim; failed/expired or competing replies
cannot grant another launch. Tests use actual Ed25519 signatures and SQLite records
for durable success and nonce/key/domain/bundle/context/time/cancellation refusal.
The source exposes no network or renderer entry point and does not claim mutual
transport authentication, verified input transfer or actual remote execution.

## R03 bounded immutable fleet input transfer

New canonical implementation based on #115 at
`cfde2e9e46094708655e5cb7e4b1c22c2a1a50de`; no preserved source commit is replaced.
A canonical tree manifest binds the saved input and bundle identity, complete-file
hashes, ordered chunk lengths and portable executable metadata. A serial receiver
uses existing resumable CAS receipt, rejects undeclared/out-of-bound parts and verifies
whole-file reconstruction before reporting complete availability. Tests cover restart,
lost acknowledgments, corrupt chunks, incomplete input, changed identities, closed
encoding, path conflicts, bounds and complete-file integrity. The remote delivery plan
records the bounds and remaining native authority, export, transport, allocation and
actual second-machine obligations. No network or remote execution is claimed here.

## R04 native saved attached-input export

New canonical implementation based on #117 at
`43f2c5e9628286c470d695964106210ced1f738d`; no preserved source commit is replaced.
A provisioned attachment now prepares an immutable manifest and read-only pinned export handle
from exact verified saved history. It releases the inspection lock before returning, so later
capture does not depend on transfer pacing. Declared-chunk reads recheck directory identity,
bound allocation, verify exact length/hash and refuse unavailable or replaced content without
quarantine or repair. No current working-file bytes enter the exported version.

Native regressions cover saved content after a later capture, empty directories/files and modes,
real receiver reconstruction, exact retry/reopen, undeclared or foreign identities, corruption,
missing chunks, oversized files, symlinks and replaced storage. Repository/docs/license/storage,
format and whitespace checks pass; native execution on this new revision remains pending behind
the preserved combined-main run. The [remote plan](fleet-remote-delivery.md) retains managed-lane
export, authenticated transport, receiving execution and actual remote acceptance requirements.
This is not a claim that the remote phase or full goal is complete.

## R05 retained managed-review input export

New canonical implementation based on #118 at
`75f2339218be8735402a4bc5d83e92218325fcb9`; no preserved source commit is replaced.
Native service/history readers now prepare a remote input from the exact recorded saved-review
selection and native allocation. The handle retains parent pins and physical allocation ancestry
for each subsequent chunk read. Reopening exports no live context and adopts no worker.

The existing macOS offline-history journey now checks exact saved bytes through both the original
and reopened export handles, with current edits preserved and substituted review identities refused.
A native allocation regression moves a wrapper outside the admitted lane behind a symlink and
proves that matching final directory identities alone cannot retain export authority. New regression
execution and full CI are pending; repository/docs/license/storage/format/whitespace checks pass.
Current dependency authorization and all remaining remote execution/acceptance obligations remain
required, as described in the [remote sequence](fleet-remote-delivery.md).

## R06 receiving-side private input materialization

New canonical implementation based on #119 at
`1a0ce397bbef5af825bddd6818cff315089a36f2`; no preserved source commit is replaced.
The receiver now materializes exact transferred content through native-admitted private storage,
using create-only allocation and bounded descriptor-based chunk reads. Complete tree verification
checks inventory, file hashes, modes, hard links and physical ancestry. Failed or interrupted
allocations remain named and refuse automatic retry or adoption. No provider starts here.

Native regressions cover exact content and empty entries, changed bytes/modes/extra entries,
incomplete and corrupt input, oversized chunks, links, storage substitution, protected directories,
non-private storage and retained allocation escape. Repository/docs/license/storage checks pass;
new native regression execution and complete CI remain pending. The previous combined-main native
run is preserved. All remaining remote execution and acceptance requirements remain in the
[remote sequence](fleet-remote-delivery.md).

## M02 recorded real four-worker comparison

New evidence-only increment based on merged #120 at
`b08674d4f5ec963204984d2e09e6dacda884fd66`; no preserved implementation is replaced.
The [measurement record](fleet-native-measurements.md) records one completed native
Codex serial/parallel pair, exact binary hashes, retained failure and validation scope.
It does not close cold-start, external-harness, GUI, cost/resource, second-provider,
remote-machine or human-acceptance requirements. All seven hosted PR and combined-main
checks passed for #120; all five receiving-materialization tests also passed locally.
The separate older combined local gate is still active, not counted as passing.

## R07 remote execution integration direction

New planning increment based on #121 at
`2a6759f92116b042845283f8b06bd40a05cd11ca`; no implementation commits are replaced.
The [integration contract](fleet-remote-execution-contract.md) ties native receiving storage,
durable launch ownership, independent worker history, authenticated transport and verified local
result import into one required execution path. It records PR boundaries and decisive tests.
This is design, not remote execution support or second-machine evidence.

## R08 pinned native input receipt

New canonical implementation based on #122 at
`d54dc27f38bdef47c7204f4aea4fddb70c380495`; no preserved source commits are replaced.
A native receiver composes the existing CAS protocol with admitted descriptor-relative storage,
exclusive directory ownership, bounded reads, protected-root rechecks and exact-descriptor append
checks. The generic receiver remains source-compatible through its default filesystem parameter.
No wire format, signing domain or dependency changes. This integrates native receiving authority;
it does not implement transport, launch ownership or remote execution.

Four native regressions cover transfer/resume/exclusive ownership and materialization, store
replacement without writes into the replacement, links/oversized partial objects with retained
external bytes, and ancestor-alias movement into a protected project. All eleven input-transfer native
tests passed locally; all seven exact-head CI checks passed and #123 is merged. The
[execution contract](fleet-remote-execution-contract.md) retains crash/space,
authentication, receiving lifecycle, result reconciliation and actual second-machine requirements.

## R09 atomic insertion provenance for receiving ownership

New canonical implementation based on #123 at
`ee3b2d3de91be911502211604499962484141b86`; no preserved source commits are replaced.
The existing transactional fleet store now exposes inserted-versus-replayed outcomes without
changing its schema, receipt encoding or legacy append behavior. This supplies a required building
block for the receiving attempt registry; it is not the registry, a launch permit, or remote execution.
Three added regressions cover identical concurrent requests, restart/later-write replay, and authority
loss after independently observed durable commit. All thirteen fleet-store tests passed locally;
an isolated replay-as-insertion mutation failed the concurrency regression as expected. All seven
exact-head checks passed and #124 is merged. The
[execution contract](fleet-remote-execution-contract.md) retains all receiving lifecycle and acceptance
requirements.

## R10 durable remote admission before allocation

New canonical implementation based on #124 at
`e3dc216585a82d849edf100c9edf100236b3ea30`; no preserved source commits are replaced.
A shared worker ledger reserves immutable coordinator/objective/assignment work before native
materialization. Only the original atomic insertion returns a single-use input reservation; replay
returns facts, including after restart or expiry. Changing allocation or assignment fields cannot
evade uniqueness. Concurrent requests cannot overbook the objective's retained concurrency slots.
The pinned receiver consumes the reservation and checks the complete assignment before materializing.

Six native regressions passed locally: identical and distinct concurrent connections, retained
restart/expiry claims, changed work/allocation refusal, configuration/history validation, closed
canonical records and actual pinned materialization with no regrant after refusal. The current
store and daemon test crate were rebuilt in independent output locations using unchanged dependency
artifacts. This is focused native evidence, not a full local Cargo gate. All seven exact-head hosted
checks passed and #125 is merged.
No existing fleet event schema or dependency changes. The additive admission record refuses unknown
formats. Shared native ledger provisioning, terminal reconciliation/retry, worker history binding,
launch ownership, authenticated transport and actual second-machine execution remain required.

## V01 shared-target verification path correction

New verification increment based on published #125 at
`c43a4576e065019d45a09c2d758609e26e9aceb2`; no preserved source commits are replaced.
The completed full local #116 run exposed the demo building into `CARGO_TARGET_DIR` but looking for
binaries under the checkout's default target. The demo now uses the same environment override for
lookup. Two actual CLI regressions cover absolute and repository-relative targets from another
invocation directory; both fail against the original script and pass after the fix. They run in the
existing demo gate. The real 44-check demo passed using the retained #116 binaries through the
corrected script. This validates the script correction, not the newer Rust implementation; full
exact-head hosted rebuild/checks remain pending. The original full-run log remains retained: 3,384
native tests passed, two native watcher startup tests failed (issue #37), 16 skipped, and desktop
checks passed. This correction does not resolve those native watcher failures.

## R11 native shared worker ledger

New canonical implementation based on #126 at
`ad6f1c53498dfefcd257a0461981dd6bab7a0c55`; no preserved source commits are replaced.
The macOS native worker directory composes retained directory authority, independent exclusive
ownership, a create-only physical receipt and guarded SQLite access through a stable directory
reference. Every objective registry uses this same worker ledger; retained registry connections keep
ownership alive. Reopen never repairs partial or absent history. Changed namespace, files, keys,
permissions, links, unknown entries and physical movement into protected projects refuse.

All seven native test cases passed locally (six scenarios plus the child-process helper), including
abrupt process exit with committed admission recovery and no renewed reservation. The current store
and daemon test crate were rebuilt independently using unchanged dependency artifacts. Structural
checks pass; full exact-head CI remains pending. This is macOS storage/ownership evidence, not
portable worker support, network authentication, provider execution, second-machine or packaged
acceptance. No existing schema or dependencies changed; the additive worker-directory receipt is
closed and refuses unknown versions. Worker workspace provenance, process supervision, terminal
reconciliation, transport and saved-result recovery remain required.

## R12 received worker baseline integration, draft

New canonical implementation based on merged #127 at
`606fb9d901d655109eea674fc8f8e264ba765c99`; no preserved source commits are replaced.
All seven exact-head checks passed for #127. This draft carries coordinator/objective attribution
through the original input reservation, creates an initialization intent before native ingestion,
and records the actual worker installation/initial operation separately from source input/bundle.
Saved content, immutable input, receipts and physical custody are checked; existing state refuses
without repair. It adds no provider execution or automatic restart adoption.

The focused native run currently has fifteen passing tests and one failing empty-tree regression.
Nonempty binary/executable/empty-entry content, persisted mapping and native-history reopen pass;
changed/unreserved input, existing intent/destination, changed receipts and worker escape refuse.
The empty-tree case exposes the lack of an explicit root declaration in the operation model and
must be fixed before merge. Initial failure logs are retained; the executable-mode expectation was
corrected to preserve the native owner execute bit instead of adding execute permissions for others.
Complete exact-head validation remains pending. The [integration contract](fleet-remote-execution-contract.md)
keeps the empty-root requirement and all remaining supervision, transport and acceptance work intact.

## R12 prerequisite: explicit workspace root declaration

New canonical protocol increment based on merged #127 at
`606fb9d901d655109eea674fc8f8e264ba765c99`; no preserved source commits are replaced.
Draft [#128](https://github.com/idosams/Mesh/pull/128), head
`2b3f1470d5d817c00e8c90a49267ff5756eb0de9`, retains the worker initialization integration
and its failing empty-tree regression. That draft is not ready to merge. This separate prerequisite
adds `InitializeWorkspace` so an empty saved tree can declare a real root without placeholder files
or invented source history. Its [wire compatibility](execution-plan.md#explicit-empty-workspace-root)
is additive and refuses on older readers. Same-root replay leaves existing state intact; mismatched
roots refuse. Conflicting declarations cannot be hidden by directory creation. Native reopen and
historical preview cover the empty-root journal. Local and hosted verification are pending.
After this prerequisite merges, #128 will receive related main history by ordinary merge and adopt
the declaration only for empty imports. No old histories or published commits will be rewritten.

### R12 stacked empty-tree correction

The published prerequisite #129 at `26bf82174b12309199382870d1b08c9481c937c8` was merged
normally into #128 at `78cad259a76ce2f14670e4a712286346224d883a`, preserving both histories
and both provenance entries above. #128 is stacked on #129 while its independent checks finish;
it will return to main after the prerequisite merges. Empty ingestion now emits the explicit root
operation, and its retained regression additionally reopens the native saved history. The original
#128 macOS run passed 3,420 tests with one empty-tree failure and 16 skips; Linux stopped at the
same failure after 1,016 passes. Those logs remain retained. Corrected exact-head validation is
pending, not reported as passing. No preserved source commits are replaced or rewritten.

### R12 delivered baseline and root prerequisite

[#129](https://github.com/idosams/Mesh/pull/129) merged at
`22ffefce875cc509bd0993ab836049736415d91b`; [#128](https://github.com/idosams/Mesh/pull/128)
then merged at `d57c512161a7ce5bf75d7c35d384f43c0a79b971`. Both had all seven required
exact-head checks passing. The worker integration's macOS gate passed 3,429 native tests, including
empty initialization/reopen, plus renderer and real daemon demo checks. The earlier draft entries
above preserve failures and the normal merge sequence; they do not describe current merge status.
Root schema/vector publication now belongs to `mesh-operations` under `protocol/operations`, with
exact generated-artifact and recursive inventory tests. The older signed-record compatibility gate
is unchanged. Its earlier artifact-ownership and literal-expectation failures remain preserved.

## R13 durable remote launch ownership

New canonical implementation based on merged #128 at
`d57c512161a7ce5bf75d7c35d384f43c0a79b971`; no preserved source commits are replaced.
A launch intent binds the exact retained admission, workspace initialization receipt digest, worker
initial operation, native installation and native-generated owner. The same guarded worker ledger
commits it before any future provider launch. Only the original atomic insert returns a reservation;
identical replay, expired/restarted claims and lost acknowledgments recover facts without regrant.
Changed mappings and admissions refuse. The reservation owns its registry connection and workspace,
retaining directory authority even after the outer native worker-directory wrapper is dropped.

Verification covers concurrent claims, restart/expiry replay, changed identities, malformed/unknown
records, actual received-workspace binding, wrong providers/coordinators, retained native directory
ownership and independently observed post-commit authority loss. Focused native and lint checks are
running; full exact-head CI remains required. The additive closed `mesh.remote-launch-intent/v1`
record uses the existing fleet-store schema. No existing event or protocol encoding changes.

This increment does not start a provider, issue agent credentials, adopt an old process, release
capacity or authenticate a network connection. The next composition must connect retained ownership
to native session/custody admission and supervised process lifetime, then implement terminal/result
reconciliation and authenticated transport. Full second-machine and packaged acceptance remain open.

### R13 delivered evidence

[#130](https://github.com/idosams/Mesh/pull/130) merged at
`486b0e0bcc2889e46b33739d9d0a55fc21c102e8` after all seven exact-head checks passed.
Linux passed 3,184 tests; macOS passed 3,437 plus four renderer tests and the real daemon demo.
The initial macOS draft failed its new post-commit authority-loss fixture because it passed the
OS `/var` alias to an opener that requires a native-resolved path. The correction resolves only
the fixture parent; the production no-follow guard and final database-entry check remain unchanged.
Earlier failed logs and unfinished local verification remain preserved, not reported as passing.

## R14 received native session

New canonical implementation on merged #130; no preserved source commits are replaced.
The original launch reservation is consumed into a separate execution stream in the same guarded
worker ledger. The existing native service binds the original source input, independent worker
initial operation, immutable goal/provider and exact lane/run. It retains the received allocation
and daemon rather than creating another workspace or claiming shared ancestry.

This execution session is restricted to one lane, one concurrent attempt, zero delegation depth and
zero retries. The coordinator retains global scheduling authority. Existing custody, credentials,
provider admission and durable launch checks apply. The native clock and retained admission/intent
are rechecked at the pre-spawn boundary. The returned process handle retains the service resources,
so dropping a caller's service handle cannot release the worker ledger/workspace prematurely.
Post-launch observation and retained history do not acquire an expiry-based retry permission.

Regression coverage includes exact version/daemon binding, coordinator budget refusal, stale lease
and mismatched attempt/provider refusal, changed input and launch history, native directory ownership
and actual process composition with a deterministic provider fixture. Validation is in progress;
the fixture is not evidence of a real provider, authenticated transport or second-machine operation.
No persisted record schema changes: the separate session stream uses existing fleet commands.
Independent supervisor/broker lifetime, scoped MCP integration, durable remote acknowledgment/results,
terminal reconciliation, authenticated transport and actual remote acceptance remain required.

### R14 delivered evidence

[#131](https://github.com/idosams/Mesh/pull/131) merged at
`227f3ad1e3f6bb65a4a2b40b276075157bdf0c81` with all seven exact-head checks passing.
Linux passed 3,189 tests; macOS passed 3,443 plus four renderer tests and the real daemon demo.
An earlier local binary, built before the final resource-retention changes, passed 12 tests and
timed out waiting for its provider fixture to exit. That failure is preserved; it is not replaced
by the hosted result or represented as current-commit execution. Current local verification remains
in progress and does not widen the hosted evidence into local or packaged acceptance.

## R15 received worker host

New canonical implementation on merged #131; no preserved source commits are replaced.
`ReceivedWorkerHost` owns the received native service, its sole prepared process and a private local
IPC server. The existing host's signer/grant/launch path is shared by normal dispatch and received
prepared attempts. The new host polls owned work only and cannot dispatch another lane or retry.
Native cancellation revokes credentials and requests direct-process stop, while preserving uncertain
capacity. Individual IPC connections do not own the process or server lifetime.

The provider endpoint's limited router accepts the matching objective's scoped fleet calls. General
workspace operations refuse using the existing operation defaults. The native-configured endpoint
requires an existing private directory and preserves conflicting endpoint files. No new transport
or persisted schema is introduced.

Regressions use an actual deterministic process, local socket and signing key to exercise connection
loss/reconnect, exact custody generation, checkpoint/review submission and saved-review reconstruction,
completion revocation, cancellation with retained capacity, and failed signer/endpoint preservation.
Validation is in progress. This is local native composition, not authenticated remote operation or a
real-provider acceptance run. The resident worker must retain/poll this host independently of its
broker. A deployed worker entry point, bounded broker frames, bidirectional authentication, durable
remote result envelopes/reconnect and actual second-machine proof remain required.


### R15 delivered evidence

[#132](https://github.com/idosams/Mesh/pull/132) merged at
`d5bc096fc7684d1849c12b7fe804b35430d28e58` after all seven exact-head checks passed.
Linux passed 3,192 tests; macOS passed 3,446 plus four renderer tests and the real daemon demo.
Combined-main run 36489497880 also passed. Local socket restrictions and a separate local provider
startup delay are preserved as failed local evidence in [#133](https://github.com/idosams/Mesh/issues/133).
Hosted success does not establish packaged acceptance or resolve that local failure.

## R16 coordinator proof before receiving admission

New canonical implementation on merged #132; no preserved source commits are replaced.
A worker-held, single-use challenge binds the complete existing admission body to a native random
nonce and a window of at most 30 seconds, bounded by the assignment lease. Worker configuration
supplies the expected coordinator key. Verification consumes the challenge and invokes the existing
atomic reservation path; a repeated authentic request returns retained facts, not another grant.

The coordinator derives signing bytes only from an already-claimed current remote attempt in its
native runtime. Exact task/provider, objective, keys, input/bundle, limits, assignment and lease must
match. The worker's allocation token is bounded native metadata, never a transmitted filesystem path.
Unclaimed/cancelled attempts, altered fields, unknown fields, wrong signers/domains/nonces, expiry,
capacity exhaustion and changed ledger authority refuse without granting another reservation.

The additive canonical control body is `mesh.remote-admission-challenge/v1`, at most 65,536 bytes;
its signature domain is `mesh.v1.fleet-coordinator-admission-proof`. Existing durable admission bytes
are unchanged. This is a native signing/verification boundary, not a deployed authenticated broker,
key provisioning or a persisted remote certificate. Status/reconnect authorization and result delivery
remain separate work. Validation is in progress; actual second-machine acceptance remains open.


### R16 delivered evidence

[#134](https://github.com/idosams/Mesh/pull/134) merged at
`5184000c449b0d7284942911796a31040aaeb6ec` after all seven exact-head checks passed.
Linux passed 3,200 tests; macOS passed 3,454 plus four renderer tests and the real daemon demo.
The local focused executable passed all 22 admission/launch tests, including the eight new proof
regressions. Its wall time was 562.175 seconds, while the test harness reported 0.85 seconds;
a preserved sample observed a pre-test loader wait tracked in #133. This is not a local full-gate,
packaged or second-machine acceptance result. Existing running verification remains preserved.

## R17 bounded remote stream framing

New canonical implementation on merged #134; no preserved source commits are replaced.
Synchronous native readers/writers carry control bytes, input manifests and chunk parts without
an accumulating queue. Header bounds are checked before body allocation or reading. Chunk metadata
uses fixed digest/offset/final fields and the existing receiver's part/chunk limits. Native schema,
authentication, assignment and CAS checks remain mandatory after framing.

Malformed headers, unknown versions/kinds/flags, truncated frames and I/O failures end the connection;
no later header is scanned for recovery. Partial bodies are never returned. Outbound frames are fully
range-validated before any write, and partial-write/flush failures refuse further use of that writer.
A flush is not a durable acknowledgment and reconnection grants no permission to repeat execution.

Regressions cover exact header bytes, maximum bounds, rejection before body reads, every truncation,
fragmented/interrupted reads, timeout/would-block, partial writes and flush failure. A real local Unix
stream transfers a manifest and part into the existing CAS, disconnects mid-frame, reopens durable
storage, resumes from the confirmed offset and verifies complete content. Validation is in progress.
This framing is not a deployed broker, SSH transport, authenticated state machine or second-machine
proof. Supervisor entry point, admission/transfer routing, key provisioning, bounded diagnostics,
deadlines, result/reconnect protocol and actual remote acceptance remain required.


### R17 delivered evidence

[#135](https://github.com/idosams/Mesh/pull/135) merged at
`90eed19f0cc535362d1d47060a193ffc35c349f0` after all seven exact-head checks passed.
Linux passed 3,209 tests; macOS passed 3,463 plus four renderer tests and the real daemon demo.
All nine focused local framing/CAS tests passed. Prior #134 combined-main run 36491586154 also
passed. Local aggregate verification remains queued behind preserved earlier runs, not a claimed pass.

## R18 supervisor-owned receiving session

New canonical implementation on merged #135; no preserved source commits are replaced.
The native supervisor retains a fixed assignment, guarded registry, original input reservation and
pinned receiving store. Each broker connection exclusively borrows that session and issues a fresh
coordinator proof. Dropping an unverified challenge restores only native ledger ownership. A failed
signature grants nothing and does not destroy retained transfer state. No nonce or authentication
survives connection drop.

Successful reconnect authentication returns retained admission facts while the same supervisor
continues owning its original reservation. A new supervisor opened from a saved receipt has facts
only and cannot receive/materialize another allocation. Manifest and chunk frames flow through
existing native input admission and CAS verification. Unexpected control frames, changed manifests
and invalid sequencing refuse; a receive failure makes that connection unusable. Native lease,
ledger and destination facts are checked before work and before acknowledging it.

A complete transfer consumes the reservation into a native allocation and returns the same registry
for independent workspace initialization and launch-intent composition. The receiving lock is released
only after successful handoff. Once materialization consumes the reservation, failure preserves work
and ends that session; missing content before consumption remains recoverable. No persisted schema,
provider launch, generic command dispatcher or peer-selected path is added.

Regressions cover fresh proof after disconnect, retained partial offsets and native receiving ownership,
exact workspace/launch-intent handoff, abandoned and invalid proofs, receipt-only restart refusal,
unexpected frames, conflicting allocation preservation, replaced storage and lease refusal. Validation
is in progress. Resident-worker deployment, bounded control-schema routing, SSH/key provisioning,
signed results, saved-history recovery and actual second-machine acceptance remain required.


### R18 delivered evidence

[#136](https://github.com/idosams/Mesh/pull/136) merged at
`a915612ad8e32273bf0ed7585c74db6755bd8496` after all seven exact-head checks passed.
Linux passed 3,216 tests; macOS passed 3,470 plus four renderer tests and the real daemon demo.
The exact local executable passed all 15 receiving/authentication tests. Combined-main run 36494757882
also passed. Earlier compile failures and intermediate/local aggregate verification remain preserved.

## R19 bounded receiving broker loop

New canonical implementation on merged #136; no preserved source commits are replaced.
A synchronous broker loop borrows the supervisor's fixed receiving session and sends its fresh proof.
It accepts only canonical authentication, chunk-status and materialization controls, plus the existing
manifest/chunk frames. Commands cannot choose paths, keys, work or another assignment. Authentication
must precede all other operations. Request IDs are unique within the connection and replies carry
admission correlation facts. Unknown/noncanonical controls and framing/native failures end the connection.

The loop bounds incoming frames (131,072), control identities (32,768), and total framed input bytes
(2 GiB plus 16 MiB framing/control allowance). The byte budget is checked on a complete bounded frame
before native dispatch; at most one already-bounded frame is read beyond the remaining budget.
Synchronous replies add no accumulating queue. The caller still supplies transport deadlines,
connection budgets, initial worker identity admission, authenticated streams and bounded diagnostics.

Materialization returns the original native allocation and guarded registry to the supervisor even
when the final reply write/flush fails. A successful write is not durable peer acknowledgment. EOF or
failure before handoff retains the session and requires fresh proof on reconnect. These are input
handoff facts, not provider completion or signed result receipts.

Seven local tests pass, including real Unix-stream reconnect/status/resume, final reply failure with
retained handoff, unauthenticated refusal, duplicate request IDs, frame/byte budgets and closed canonical
command parsing. Full validation is in progress. This is an embedding loop, not a deployed worker,
SSH connector or key provisioner. Initial worker-proof transport, resident service lifecycle, client
reply verification, signed results/reconciliation and actual second-machine acceptance remain open.

## R20 complete hosted validation gate

New canonical workflow correction on merged #137; no preserved source commits are replaced.
The macOS test job invokes the complete `npm test` command instead of manually repeating only its
Rust and demo portions. It still runs the four native renderer cases afterward. All seven existing
job names, dependency checks, platform scopes, assertions and timeouts remain unchanged. Independent
desktop/docs/storage and license jobs continue to provide earlier failure reporting.

The shared Rust command uses `--no-fail-fast`, matching the existing hosted macOS behavior: remaining
Rust cases run after a failure, and any failure still fails the gate. This does not make later gate
stages run after a failed Rust command. No test is ignored, removed or relaxed. Hosted full-gate
results must be verified at the exact revision before merging; configuration alone is not a pass.
Local repository, docs, license and storage checks and their mutation/regression tests passed.
Earlier local processes and failed logs are preserved, not replaced by a hosted success claim.

## R21 broker handoff to native provider

New canonical integration on merged #138; no preserved source commits are replaced.
`ReceivedWorkerHost::start_received` consumes the broker's original native allocation and guarded
registry, verifies and initializes the independent workspace, commits launch intent using the current
native clock, and starts the admitted adapter through the existing scoped host. Native configuration
supplies the adapter, endpoint, signer factory, reviewers and checkpoint policy. Receipt replay cannot
construct a handoff or a launch reservation. Failure preserves work and committed intent for
reconciliation; it never clears a slot or authorizes another launch.

The integration tests connect real Unix streams, coordinator signature verification, native input
receipt/materialization, workspace initialization, one fixture-provider process, scoped IPC reconnect,
signed checkpoint history and saved review. Both a received final reply and deterministic final-reply
write failure follow that path. Another case changes the materialized input and requires preservation
and refusal before launch intent or endpoint creation. Existing host tests share their full saved-review
assertions with these integration cases. Validation is pending.

This is neither a real-provider acceptance run nor deployed SSH/second-machine evidence. The embedding
resident service must retain and poll the host independently of the broker. Initial worker-proof
transport, client reply validation, native key/host provisioning, authenticated result lookup after
reconnect/restart, signed result export/import and final packaged acceptance remain unfinished.

## R22 coordinator immutable input transfer

New canonical implementation on merged #139; no preserved source commits are replaced.
`transfer_remote_input` accepts a native-selected current attempt, saved-history `RemoteInputSource`,
configured keys and already-authenticated streams. It validates the fresh admission challenge against
the native attempt before invoking the coordinator signer. Every exchange rechecks native cancellation,
lane/limit changes, lease expiry and pinned source identities. No live project scan or assignment
mutation occurs. The owner still supplies transport authentication, deadlines and cancellation.

The serial client verifies exact canonical reply bytes, schema, request/kind, assignment correlation,
allocation and pinned admission revision. It sends the manifest, asks for each distinct chunk's
confirmed offset, reads/hash-verifies native source chunks and sends at most 64 KiB per acknowledged
part. Complete chunks are reused. Invalid offsets, changed correlation and unknown fields refuse.
Only the final matching materialization reply yields an input acknowledgment; EOF or a lost final
reply remains an error/uncertainty. Retained-only admission returns facts without sending input.
Neither outcome is provider execution, a signed saved result or protected-main approval.

Seven focused tests cover actual saved-history export and native Unix-stream receiving, a seeded
partial offset followed by real disconnect/resume, completed-chunk reuse, lost final reply with
worker handoff retained, wrong reply correlation, cancellation during signing, wrong immutable source,
retained-only admission and strict canonical reply matching. An initial fixture assumed one chunk per
file; it was corrected to use actual saved manifest boundaries, preserving the failed log. All seven
focused tests passed, including continued live project edits preserved while transferring the earlier
saved version. Full hosted validation is pending. Deployment, initial worker-proof transport, SSH/key provisioning, signed output transfer
and local result import/reconnect recovery remain unfinished.

## R23 authenticated dispatch before worker proof

New canonical bootstrap on merged #140; no preserved source commits are replaced. The coordinator
signs the exact original pending worker challenge and native objective limits using a separate
dispatch domain. Native lane/cancellation/freshness and limits are checked before and after signing;
a wrong signer refuses. The worker verifies its configured coordinator key and worker identity,
provider, objective limits, lease cap, freshness and closed canonical body before deriving a worker
proof reply. Peer-provided paths, executables and key selection are absent from the message.

The new `mesh.remote-dispatch/v1` envelope and `mesh.worker-dispatch-reply/v1` control reply are
bounded to 64 KiB and reject unknown/noncanonical data. The existing v1 worker challenge bytes and
signing domain remain unchanged. The new coordinator domain is `mesh.v1.fleet-coordinator-dispatch`.
A verified dispatch carries authenticated work facts, not a reservation. Repeated worker replies
cannot recreate the original coordinator challenge or create receiving/launch authority. The
subsequent fresh coordinator admission proof, including exact objective limits, remains mandatory.

All 20 focused native proof/transfer tests passed: six new dispatch cases, six unchanged worker-proof
cases and eight transfer cases. The new real-stream journey sends the signed dispatch, checks the
worker reply and coordinator claim, then transfers an actual saved-history input on the same stream
into exactly one native handoff. Refusals cover wrong keys/domain/provider/budgets/lease, malformed or
changed requests, wrong reply nonce, cancellation during signing and expired proofs. The existing
objective-limit validation was extracted without changing its rules. Full hosted checks are pending.

This supplies bootstrap messages and native verification APIs, not a deployed listener or key store.
Resident worker lifecycle, transport admission, key provisioning, signed results/local import and
reconnect/restart recovery still require integration and actual second-machine acceptance.


## R24 fresh identity proof for input reconnect

New canonical implementation on merged #141; no preserved source commits are replaced.
`RemoteInputReconnectChallenge` derives the exact retained assignment from native state and creates
fresh OS-random proof for a still-launching transfer. Its separate public type has no claim method.
It reuses the v1 signed dispatch and worker-proof encoding: those messages prove authenticated work
facts, while native coordinator state determines whether the proof claims initial ownership or only
revalidates existing ownership. No persisted format or signing domain changes.

Reconnect verification consumes its challenge and performs no ledger write, lease renewal, receiving
reservation or provider launch. It requires exact worker key, assignment/owner, current lane and
freshness. Unclaimed work, another run/peer, cross-nonce replies, cancellation, changed leases and
running work refuse. The existing receiving proof and original retained reservation remain mandatory;
a restart that lost that reservation can report retained facts but cannot recreate execution authority.
This closes the initial-worker-proof gap for input reconnect, not result recovery or process adoption.

Three reconnect regressions were added. Local compilation reached linking, which failed with
`errno=28` (disk full); no local native test pass is claimed. Repository, docs, license and storage
checks passed. Hosted full checks are pending. Resident service integration, transport
and key provisioning, signed result import, real second-machine and packaged acceptance remain open.

The v1 dispatch encodes an initial claim and therefore accepts only lease sequence one. Renewed
leases explicitly refuse this bootstrap and require separate reconciliation; this API never resets
or downgrades a lease to reconnect. Renewed-lease transport remains part of the unfinished scope.


## R25 resident ownership of received providers

New canonical work on merged #142; no preserved source commits are replaced.
`ReceivedWorkerSupervisor` retains a bounded collection of `ReceivedWorkerHost` owners independently
of broker connections. Native configuration pins its worker key and capacity (1 through 64); launch
configuration supplies admitted provider/endpoint, execution signers, reviewers and checkpoint policy.
The original broker handoff is still required. No message, receipt or process ID can recreate it.

Before any initialization or process effect, the supervisor reserves an in-memory slot for the exact
coordinator/objective/assignment identity. A failed start retains an unavailable slot. Duplicates,
other worker keys and exhausted capacity refuse. Completed processes also retain slots; no removal,
retry or restart adoption is implemented. Durable worker admission and launch intent remain the
cross-process authority, supplied through the same guarded worker ledger by native provisioning.

Polling visits every retained owner and returns correlated per-owner results. One unavailable owner
does not abort observation of another. Exact admission facts select native snapshot/cancellation;
unknown or changed facts refuse. Cancellation and drop never establish descendant termination.
The embedding resident loop must drive polling independently of blocking transport operations.

Two added regressions compose actual broker handoffs (including a lost final reply), a failed signer
and a real fixture provider, then test independent polling, completion, retained capacity, duplicate
identity, wrong worker and capacity bounds. The broker fixture now compares its actual assignment
identity instead of a hard-coded value. Complete native compilation passed with warnings denied;
test execution and full hosted validation are pending. This is native resident ownership composition,
not a deployed listener. Key/host provisioning, persistent service lifecycle, renewed-lease and signed
result recovery, real second-machine and packaged acceptance remain unfinished.


R25's first Linux run caught a fixture-path error: the resident test waited for provider output in
received immutable input rather than the initialized independent worker workspace. The regression
now reads the actual root from the native snapshot, asserts the roots differ and verifies that input
bytes remain unchanged with no provider markers. The same startup deadline and all launch/capacity
assertions remain; the capacity refusal also checks its exact error code. The failed run is retained.
Corrected validation is pending.


R25 was merged through [PR #143](https://github.com/idosams/Mesh/pull/143) as
`83872041f5e0bf455ec6b8e50f2d1c204a6e2e19`, including corrected tested head
`94abfc6cdd366277ad111fc7d0d4215e84099d83`. All seven PR checks passed: Linux
3,245 passed / 7 skipped; macOS Rust 3,499 passed / 17 skipped, desktop 166 and
580 passed, full `npm test` and all four renderer cases passed. The separate local
corrected run outside the sandbox failed five provider startup deadlines (three tests passed).
Inside the sandbox it failed five endpoint creations. Logs remain preserved; [#133](https://github.com/idosams/Mesh/issues/133)
remains unresolved. Hosted success is not local acceptance.

## R26 resident observation loop

New canonical work on merged #143; no preserved source commits are replaced.
[Issue #144](https://github.com/idosams/Mesh/issues/144) tracks the native loop around
`ReceivedWorkerSupervisor`. A fixed 32-request mailbox transfers original handoffs and exact
snapshot/cancellation requests. Full/disconnected sends return the original request to the caller;
a rejected handoff must be retained, never reconstructed or interpreted as permission to retry.

The resident thread polls before each request and during idle periods. Losing all control senders
or the observation receiver does not stop observation. Reply and observation delivery is nonblocking;
a full observation queue drops the new sample, so observation timestamps remain essential.
The nominal 50 ms idle interval is not a latency guarantee: native launch or storage can block.

A separate native stop flag returns control while leaving the original supervisor owners, occupied
slots and queued mailbox requests retained. An already dequeued request completes before the next
stop boundary. Stop is not cancellation or process-tree termination. No broker message or renderer
command exposes stop or these native control objects.

Two regressions exercise autonomous provider completion after control disconnect, blocked reply and
observation consumers, exactly one provider launch, original ownership after explicit stop, and
return of the same handoff on queue exhaustion/disconnection. Complete native test compilation passed with warnings denied (42.908 seconds). Native execution
and full hosted validation are pending; repository/docs/license/storage checks passed. This increment supplies an embedding loop, not an installed
worker binary or listener. Persistent execution identity, deployment, renewed-lease and signed-result
recovery, real second-machine and packaged acceptance remain required.


R26 merged in [PR #145](https://github.com/idosams/Mesh/pull/145) as
`a59e0b1995aef85b7c8977394aa0b5aeecd1e026`, including tested head
`c56f7a06d1497b51ddd99b5c3af4a3b0ae7598c1`. All seven PR and combined-main jobs passed.
Linux passed 3,247 tests / 7 skipped; macOS Rust passed 3,501 / 17 skipped, full `npm test`,
the 44-check daemon demo and all four renderer cases passed. The separate outside-sandbox native
run failed six unchanged provider-startup deadlines (four passed); [#133](https://github.com/idosams/Mesh/issues/133)
remains open. These failed logs are retained, distinct from hosted success.

## R27 persistent native worker actor identity

New canonical work on merged #145, tracked in [#146](https://github.com/idosams/Mesh/issues/146).
No preserved source commits are replaced. `AppleActorCustody` supplies explicit create-only macOS
execution-key persistence, expected-public-key reopening and per-signature identity loading.
The existing ephemeral software signer and the separate P-256 human-approval credential are unchanged.
No new dependency, ledger record, wire format, rotation, deletion or fallback is introduced.

The native bridge first checks the existing exact Apple-signed Mesh desktop identity, then uses its
explicit app-private group and a worker-only generic-password service. Data Protection, no sync and
after-first-unlock device-only accessibility are required; native reads disallow interaction.
Missing, inaccessible, malformed and substituted identities refuse. Seed material never leaves the
custody API; it does enter process memory for signing. The guarantee is `AppleKeychain` / `OsGated`,
which cannot supply human approval authority. In-flight signatures are not retrospectively revoked
by deleting an item; current assignment/lease checks remain the transport's responsibility.

The build-time and test-time export scans now include the new Rust module. The support matrix and
its regression explicitly allow this one macOS backend and continue to refuse unimplemented OS
backends. Deterministic tests cover duplicate creation without replacement, repeated reopen/sign,
wrong identity, backend loss and substitution. Native Objective-C tests validate stored length,
type, accessibility, synchronization, service, account and access group before any output copy.
The ordinary test executable must fail the read-only application preflight before a provisioning
call; no user keychain item is created by these tests.

Initial local native testing passed 26 cases and failed the old assertion that no persistent
backend exists. The corrected assertion requires exactly macOS AppleKeychain source support;
all other platform expectations and human-authority refusals remain. The failed log is retained.
Corrected local validation passed all 45 focused tests (28 unit, 14 isolation, 3 native approval
bridge), crate clippy with warnings denied, Objective-C syntax with warnings denied and
repository/docs/license/storage checks. Full hosted checks are pending. Actual create/reopen/sign under an
eligible signed app, worker-directory provisioning and resident transport integration remain
unfinished; fixture storage is not OS keychain acceptance.


The unchanged R26 native executable was rechecked outside the sandbox after local native test
startup became prompt and available disk space was observed at 3.8 GiB (previously about 361 MiB).
All ten received-host tests passed in 6.89 seconds. No source, timeout or assertion changed and
prior failed logs remain preserved. This is a passing focused rerun, not proof that storage pressure
caused the earlier delay or that full native reliability is resolved; issue #133 remains open.

R27 merged through [PR #147](https://github.com/idosams/Mesh/pull/147) as
`c66cff63c8003d3dfe9521992037f9bde168001b`, including tested head
`5144ad5803ca92eb55f2e82b14555960a497837a`. All seven PR and combined-main checks passed.
Linux passed 3,250 / 7 skipped; macOS Rust passed 3,505 / 17 skipped, the full `npm test`,
44-check daemon demonstration and all four renderer cases passed. The separate full local gate
remains running and is not counted as a pass.

## R28 guarded worker installation and native setup commands

New canonical work on merged #147, tracked in [#148](https://github.com/idosams/Mesh/issues/148).
No preserved source commits are replaced. `NativeWorkerInstallation` durably binds a native random
custody account, expected public identity, guarded parent and existing private ledger. Its canonical
v1 intent is written before key creation; any failure retains evidence and a repeated provision
refuses before another key call. Reopen requires complete exact state and never initializes missing
history. Existing standalone ledger APIs and their persisted v1 format remain unchanged.

The parent keeps an exclusive native lock and exactly two receipt files plus its ledger child.
Child ledger authority inherits parent checks, so retained registry handles still refuse replaced
parent receipts/namespaces and keep the parent locked after the installation wrapper drops. Public
identity inspection also checks the ledger. Callback completion is followed by complete physical
verification; neither callback success nor a copied receipt grants continued authority by itself.

Native `--worker provision|identity <absolute-private-folder>` modes run before the graphical app.
The first custody preflight refuses an unentitled build before filesystem work. The user explicitly
supplies a private metadata folder outside projects; provisioning requires it to be empty. Output is
public identity only. There is no listener, provider launch, selected-workspace change, human-key use,
repair, deletion, rotation or automatic conversion of existing ledger folders.

Fourteen focused native installation/ledger tests passed, including seven new lifecycle/refusal
cases. The actual native command module compiled against current daemon/custody code and its two
parser/application-identity refusal tests passed. These are not full packaged-app or signed-keychain
acceptance. The first isolated command harness failed to resolve a transitive library; the corrected
search path passed without weakening product assertions, and the failed log remains preserved.
Repository target, documentation, license, storage and formatting checks passed. Full hosted
validation is pending. Resident transport, signing-eligible
provisioning, renewed leases, signed-result recovery and real second-machine acceptance remain open.

Initial R28 hosted validation caught an undeclared direct `mesh_store` reference in the desktop
command and a startup-order regression whose exact expected sequence lacked the new worker mode.
The command now formats public key bytes directly without a dependency change. The startup test
requires the worker mode, its failure exit and early return before the existing attachment/MCP and
AppKit sequence. Failed CI logs remain preserved; full corrected validation is pending.

## R29 resident authenticated worker endpoint (in progress)

New canonical work tracked in [#150](https://github.com/idosams/Mesh/issues/150), based on merged
[#149](https://github.com/idosams/Mesh/pull/149), `f21329b8eb9a02b7727c834730d2bb3f2ba9c426`.
No preserved source commits are replaced. R28's corrected PR passed all seven checks, including
full macOS `npm test`; all seven combined-main checks also passed. Failed initial build/startup-order
checks and successful corrected results remain preserved.

The resident connection owner binds fresh signed dispatch to the same guarded installation ledger,
retains bounded transfer state across disconnect, refuses changed assignments, and holds original
materialized handoffs through full/disconnected mailbox delivery. Native endpoint storage is pinned,
private, exclusively owned and create-only; it never removes stale or substituted socket entries.
Nonblocking native connection opening and stream clones share an absolute bounded deadline.

Explicit signed-application `serve` and `connect` modes compose installation custody, native provider
configuration, authenticated transfers and independent resident provider observation. Configuration
is local, closed, bounded and loaded once; network input cannot choose paths or executables. Separate
protected metadata roots remain outside configured user projects. Bridge EOF half-closes its own
connection and cannot stop the resident supervisor. Neither materialization acknowledgments nor
local process observations authorize protected-main changes.

Focused transfer/reconnect/refusal and endpoint tests have passed. Native CLI compilation passed;
expanded end-to-end endpoint/provider tests, final CLI regressions and full validation are still
running or pending. This increment is published as [draft PR #151](https://github.com/idosams/Mesh/pull/151) and is not yet merged. Successful eligible signed-app
provisioning, actual second-machine transport, renewed leases and signed-result recovery remain
required by the full fleet plan.

The first complete private-endpoint regression compiled but failed with a native `InvalidInput`
(`EINVAL`) during frame reading, before provider launch. The earlier pair-based transfer and
connect-only tests did not establish data exchange on this path. Its failed log is preserved; the
transport error is under investigation and is not waived. The final native command/configuration
suite passed all three cases with warnings denied.

The transport investigation reproduced the Darwin failure independently: changing a Unix socket
timeout after full peer closure fails even while its final reply remains buffered. The stream now
uses nonblocking I/O and readiness polling against its original absolute deadline. Regressions cover
buffered final replies/EOF, idle reads, blocked writes and unchanged clone deadlines. Corrected
end-to-end validation is pending; neither timeout bounds nor assertions were relaxed.

The corrected buffered-reply/deadline tests passed and the complete endpoint test reached provider
startup. It then correctly refused the fixture's non-private provider socket parent. The fixture
now uses the same private physical endpoint folder as the production service; the privacy check
remains unchanged. The failed fixture run is retained, and the final native suite is pending.

The final focused native run passed 20 of 21 cases: installation, endpoint deadlines (including
blocked writes), buffered EOF, authentication and handoff checks passed. The complete endpoint test
now reached native provider startup but failed its existing ten-second acknowledgment deadline.
That local failure is preserved; its cause remains unconfirmed and the deadline is unchanged. The
increment is being published as a draft for full hosted validation, not claimed as merged delivery.

Initial hosted macOS validation stopped before runtime tests on `large_enum_variant`. The connection
outcome now boxes its admission payload; no lint is suppressed. Direct native lint checks use the
repository's declared Rust 1.85 minimum, matching Cargo's setting rather than suggesting newer APIs
in unchanged baseline code. Full corrected hosted validation remains required.

The exact provider fixture also missed ten seconds outside Mesh and then exited normally after
160.717 seconds, with one launch and the expected completion event. A read-only sample showed
`_dyld_start` before program entry. This narrows the local startup investigation without identifying
the underlying OS cause or resolving [#133](https://github.com/idosams/Mesh/issues/133). No running
process was stopped and the test deadline remains unchanged.

## R30 native SSH connection ownership (in progress)

New canonical work tracked in [#152](https://github.com/idosams/Mesh/issues/152), based on merged
[#151](https://github.com/idosams/Mesh/pull/151), `16fdf498610a6ba4bf30ede2e6f1d01f712f0b30`.
No preserved source commits are replaced. R29 passed all seven PR checks: macOS full `npm test`
passed 3,523 Rust tests (17 skipped), the authenticated reconnect/provider regression, desktop
checks and all four renderer cases; Linux passed 3,250 (7 skipped). All seven combined-main checks passed in run `36545003482`.
The earlier local provider-startup failure and its independent reproduction remain unresolved in #133.

The native SSH destination accepts explicit host/account/port and existing private identity and
known-host files. It invokes the installed system OpenSSH with closed options and fixed
`mesh-worker-v1` subsystem. It never constructs a remote shell command, chooses remote paths from
task input, enrolls trust, installs a worker or changes SSH/account configuration. File identity and
metadata are checked around spawn; operator control of these files throughout the connection is
required. This is not filesystem immutability against other same-user programs.

Input/output use nonblocking native pipes and one unchanged absolute deadline. A separate reader
drains stderr into fixed memory, retains no diagnostic contents and exposes only a saturating byte
count. Closing input preserves output; dropping the handle terminates and reaps only its owned local
SSH child. Process spawn/reaping are OS operations, not claimed bounded by the pipe deadline. EOF,
client status, transport loss or deadline expiry confer no remote completion/retry authority.

Validation is in progress. The standalone check initially selected the surrounding fleet test module;
correcting only its harness test-path resolution compiled the actual new source with warnings denied.
Direct lint passed. Focused runtime tests and full canonical checks remain required. This API is not
yet wired to coordinator dispatch or presentation. Mesh mutual proof, lease renewal, signed-result
recovery, a provisioned second machine and eligible packaged/signing acceptance remain required.

Initial R30 full macOS validation caught a conflicting `fcntl` declaration before runtime tests.
The installed Darwin SDK declares the function variadic; the older resident endpoint declaration
now matches that ABI, with an added runtime check that connected descriptors retain close-on-exec.
This also matters on [Apple ARM64's distinct variadic calling convention](https://developer.apple.com/documentation/xcode/writing-arm64-code-for-apple-platforms).
No warning is suppressed. Failed CI logs are preserved. The focused seven-case SSH suite passed
before this endpoint correction; full corrected native and hosted validation remain required.

## R31 authenticated coordinator input delivery (in progress)

New canonical work tracked in [#154](https://github.com/idosams/Mesh/issues/154), based on merged
[PR #153](https://github.com/idosams/Mesh/pull/153), `ad1458ae0be5c97fe3e27931f530c35072852d61`.
All seven corrected R30 PR checks passed, including full macOS `npm test` (3,531 Rust tests passed,
17 skipped), all seven SSH cases, the descriptor regression and four renderer cases. Combined-main
validation is running. No preserved
source commits are replaced. The native `deliver_remote_input_over_ssh` API composes explicit
SSH destination policy, current coordinator context, bidirectional Mesh proofs and immutable input
transfer. Desktop/agent exposure, signed-result recovery and remote acceptance remain unfinished.

The caller explicitly chooses a first claim or input reconnect. Before opening transport, native
code compares the saved input/bundle, checks the applicable current attempt and signs only its
canonical dispatch. The bounded worker reply is verified before a first durable coordinator claim
or the receiving admission signature. Existing transfer logic then checks current state around
every exchange and resumes confirmed immutable chunks. No automatic retry, lease reset, new
assignment, provider-success observation or protected-main approval is introduced. Any return
drops only the owned local SSH connection; errors may follow durable work and retain uncertainty.

Tests compose the actual native resident broker with saved-history export, interrupt a chunk
transfer, refuse a second initial claim and explicitly reconnect to materialize one assignment.
Refusals cover changed saved-input identity, an unclaimed reconnect, and a canonical reply with an
invalid worker signature before admission signing or coordinator ownership. Native compilation/lint
and focused tests are in progress; full canonical checks and real second-machine proof remain required.

The first focused composition run passed both refusal cases and completed one materialization,
then failed its exact-byte inspection because the fixture omitted the native `input-` allocation
prefix. The fixture path is corrected without weakening protocol or byte assertions. Its failed
log is preserved; corrected focused and full validation remain required.

The corrected three-case native composition suite passed in 0.78 seconds. Complete daemon lint
with warnings denied and repository/docs/license/storage/format checks passed. Full hosted checks
remain required; native loopback composition is not actual second-machine SSH acceptance.

## R32 signed retained worker status (in progress)

New canonical work tracked in [#156](https://github.com/idosams/Mesh/issues/156), based on merged
[PR #155](https://github.com/idosams/Mesh/pull/155), `bef131c69224c427ade919c680940b32f7f4efc3`.
No preserved source commits are replaced. R31 passed all seven PR checks, including full macOS
`npm test` with 3,534 Rust tests passed / 17 skipped, all three delivery cases and four renderer
cases. Its combined-main checks are running; R30 combined-main checks passed.

The additive closed v1 worker-status query/reply uses separate signing domains and fresh native
nonces. Native context binds coordinator/worker keys, objective, lane/run, immutable input/bundle,
provider and a goal digest. The worker verifies configured keys/caps before reading the guarded
registry, signs only retained facts, and rechecks history and freshness after signing. The coordinator
consumes its original query, verifies the worker reply, checks current context and exposes correlated
read-only facts with an observation time. Unknown fields, noncanonical data and replay refuse.

Admission is not materialization; launch intent is not provider acknowledgment or completion. Null
admission means unrecorded/unknown and grants no retry. Reading can recover expired/cancelled work
using fresh authentication without resetting a lease, adopting a process, allocating, launching,
changing ownership or freeing a slot. Initial lease fields are explicitly historical. The existing
resident endpoint routes status separately from input transfer, and a native SSH inspection API
closes only its owned connection. Status reads require guarded installation/history authority, not
input-store materialization authority; input transfer retains its separate destination verification.

Five focused native tests passed: durable reopen with no second reservation, key/staleness/replay
refusal, expired-lease read, changed context/history/unknown-field refusal, and actual resident-broker
recovery of a signed launch intent without launching a process or changing coordinator state. An
initial test compile failed because the deliberate history-mutation fixture omitted `mut`; that log
is preserved. Complete native compilation and initial lint passed; full final-base checks remain
required. This is not signed saved-result transfer, current process liveness, lease renewal, UI
readiness or real second-machine acceptance.

## Validation follow-up: managed-save replacement synchronization

New canonical test-only work tracked in [#158](https://github.com/idosams/Mesh/issues/158),
based on R31 merge `bef131c69224c427ade919c680940b32f7f4efc3`. No preserved implementation
commit is replaced. R31 passed all seven PR checks, but its
[combined-main macOS run](https://github.com/idosams/Mesh/actions/runs/36548847897/job/109341756509)
failed: 3,533 native tests passed, one managed-save replacement test failed, 17 skipped. The
remaining six checks passed; combined main must not be reported green.

The test observed a pending checkpoint before starting replacement, leaving a 50 ms scheduling
window in which the original save could correctly finish. It now pauses the real signing callback
while the native workspace authority guard is held, checks that replacement cannot finish, and
releases the save. Exact durable assertions still require the original workspace to settle and
the replacement's identically numbered checkpoint to remain pending. No production code, timeout
threshold, retry or skip changes. Focused native and full hosted validation are pending. This
follow-up does not resolve native event registration issue #37 or process startup issue #133.

R32 merged through [PR #157](https://github.com/idosams/Mesh/pull/157) as
`aa48e79dc093cf9a3bd0a08b9c8969953c2e811e`. All seven hosted PR checks passed: 3,539 native
tests passed / 17 skipped, desktop and daemon demo passed, four renderer cases passed. Its local
full run ended during compilation with no disk space; that failure is retained and is not a test
pass. Combined-main validation is running. The test-only [PR #159](https://github.com/idosams/Mesh/pull/159)
now includes that merged base. Its initial focused native managed-edit suite passed all 49 cases
in 19 seconds; final merged-base hosted validation remains required.


## R33 authenticated remote lease renewal (validation in progress)

New canonical work for [#160](https://github.com/idosams/Mesh/issues/160), based on
[PR #159](https://github.com/idosams/Mesh/pull/159) merge
`0d74eb085258dbe44c34c48a59061f1e5fcffe45`. No preserved source commits are replaced. PR #159
passed all seven hosted checks and the full local gate: 3,539 native tests passed / 17 skipped,
desktop checks and all 44 daemon-demo checks passed. Its combined-main run also passed.

Separate immutable coordinator intent precedes transmission; worker compare-and-advance lease
records precede its signed acknowledgment. Only verified acknowledgment advances the coordinator.
Exact replay reconciles lost replies without another admission, launch or attempt. Native keys,
limits, time, canonical schemas, exact work correlation and current context are checked around
signing. Original launch/session checks read the effective lease but preserve original ownership.
New renewal after expiry refuses. Acknowledgment/expiry does not establish liveness or free a slot.
The resident worker route and bounded native SSH exchange are included.

All 13 selected renewal/deadline tests passed, including 11 new cases. Complete daemon lint with
warnings denied passed before the final two ledger tests; full final-source canonical validation is
running. Two earlier fixture failures are preserved: a wrong cancellation variant, then a reopen
helper that attempted a second claim. The corrected test reopens the original coordinator ledger.
Read-only status v1 still reports historical initial leases. The next coherent increment adds a
versioned effective-lease read; #160 remains open until that recovery surface is delivered. Signed
saved results, actual remote execution/disconnect/reconnect, second-provider and packaged acceptance
remain part of the full fleet objective.


## R34 versioned effective-lease inspection (validation in progress)

Follow-up for [#160](https://github.com/idosams/Mesh/issues/160), stacked on published
[PR #161](https://github.com/idosams/Mesh/pull/161),
`870bd5120ab4766c84744f1ef549b41220e13a51`. No preserved source commits are replaced. R33's full
local gate passed: 3,550 native tests, desktop checks and all 44 daemon-demo checks; 17 native tests
were skipped. Its hosted checks and merge remain pending.

Read-only status v2 adds effective worker lease facts with version-specific signing domains and
preserves v1 wire compatibility. The caller explicitly selects the v2 challenge or native SSH
inspection API. Original admission/launch facts stay distinguishable from effective sequence,
deadline and renewal acceptance time. Unknown admission and expired work confer no retry, launch,
renewal, liveness or protected-main authority. Signing rechecks guarded facts; replies reject
version/domain downgrades and impossible lease/time relationships.

All eight status tests passed, including v1/v2 real resident routing, expired-lease reads, renewed
lease recovery after reopen, null admission, downgrade refusal and mutation during signing. Full
final-source checks and canonical PR delivery remain required. Actual remote execution/results,
provider and packaged acceptance remain part of the full fleet objective.


## R35 received-worker immutable result export (validation in progress)

New canonical implementation for [#163](https://github.com/idosams/Mesh/issues/163), following
[PR #162](https://github.com/idosams/Mesh/pull/162). No preserved source commits are replaced.
The native received session verifies the exact saved checkpoint/review, original attempt and
workspace binding before exporting the recorded immutable tree with allocation pins. It does
not rely on the ordinary lane allocator, which intentionally cannot recreate received work.

The composed received-host regression passed: saved bytes exclude later unsaved working edits,
wrong selection refuses, the ledger revision stays unchanged, execution credentials stay revoked,
content survives dropping the worker owner, and replacing its workspace path refuses reads.
Daemon lint with warnings denied passed. Full canonical checks and PR delivery remain pending.
Signed offers, resumable output transfer, exact coordinator import/recovery and real second-machine
acceptance remain required; this increment introduces no new wire or persisted format.


## R36 signed saved-result offers (validation in progress)

New canonical work for [#163](https://github.com/idosams/Mesh/issues/163), following
[PR #164](https://github.com/idosams/Mesh/pull/164). No preserved source commits are replaced.
R34 merged through [PR #162](https://github.com/idosams/Mesh/pull/162) as
`3a6533dfc739c050ced2e04d05f9e6c961875a7f`; all seven PR checks passed, including 3,553 native
tests / 17 skipped, 44 daemon-demo checks and four renderer cases. Combined-main checks are running.
R35's local full gate and initial seven hosted checks passed; its final-main-base run is pending.

The native original received session signs an exact immutable result identity under the configured
worker key and separate signing domain, rechecks custody around the callback, and retains the
signed offer before returning it. Exact replay requires no second signature; restart inspection
returns retained facts without reconstituting execution. Coordinator verification binds its current
assignment and supplied manifest without changing state. Result streams are additive; existing
admission, launch and session records remain unchanged.

Three focused regressions and full daemon lint passed. An initial misplaced method failed compile;
a later test module path accidentally selected unrelated tests. Both are corrected and their logs
are retained. Full local and hosted validation and PR delivery remain pending. Authenticated result
transport, output integrity/recovery, exact candidate import and actual remote acceptance remain
required.


## R37 authenticated known-result recovery (validation in progress)

New canonical work for [#163](https://github.com/idosams/Mesh/issues/163), following published
[PR #165](https://github.com/idosams/Mesh/pull/165). No preserved source commits are replaced.
R35 merged through [PR #164](https://github.com/idosams/Mesh/pull/164) as
`fc3487c18a12f6332a698951e33ecf12fa255f2a`, with all seven final checks passing, 3,553 native
tests / 17 skipped, 44 daemon checks and four renderer cases. Combined-main validation is running.

R36's hosted checks passed, but its local full gate failed on a recycled PID/thread fixture name
and an import receipt retained from September 27. The failed log and receipt remain preserved.
Test-only [PR #166](https://github.com/idosams/Mesh/pull/166) atomically reserves separate parent
namespaces; all 26 focused tests and its full local gate passed. R36 needs validation with that fix.

Fresh result queries bind exact assignment/checkpoint, native peer identities, bounded objective
limits and a nonce/deadline. The native resident route returns a signed observation of the existing
immutable offer or unknown result, checking guarded facts again after signing. The caller rechecks
current context and rejects replay, wrong checkpoint, signature-domain substitution and stale time.
SSH uses the existing bounded transport with explicit reconnect and no automatic retries.

Three focused tests and full daemon lint passed. Initial compile failures (an error-conversion
closure and a consumed test registry) and missing public API docs were corrected; original logs
are retained. Full final-source checks and PR delivery remain pending. Discovery of checkpoint
identities, actual content transfer/reopening, exact candidate import and real remote acceptance
remain necessary; this increment is not complete output delivery.


## R38 resident saved-result publication (validation in progress)

New canonical work for [#163](https://github.com/idosams/Mesh/issues/163), based on merged
[PR #165](https://github.com/idosams/Mesh/pull/165), `d90cc25bcd992116bdbc2f4b81cea6e22f495d4d`.
No preserved source commits are replaced. PR #165 passed all seven checks with 3,556 native tests,
17 skipped, 44 daemon-demo checks and four renderer cases. Its combined-main run was superseded
by the later PR #167 merge; combined-main verification is running.

The actual native worker service publishes saved review offers from its original received owners
using configured worker custody. Per-owner attempts are spaced by one second; a wrapping page
cursor prevents a failed result from permanently starving later reviews. A bounded retained cache
avoids repeated exports/signatures, while durable offers remain the source of truth. Publication
errors are separate native observations and grant no retry, slot-release or protected-main rights.

The composed regression and daemon lint pass. The regression uses a controlled clock for retry
spacing and proves wrong-key refusal and durable replay without signing again. Full local and
hosted checks and PR delivery remain pending. Authenticated discovery, reopening/transfer, exact
coordinator import and remote/packaged acceptance remain in scope.


R38 initial full local gate passed on `ea38833`: 3,556 native tests, 17 skipped, desktop checks
and all 44 daemon-demo checks. The runner reported one slow test and one process-leak warning
(`project-attachment::repeated_observations_preserve_git_and_apply_exclusions_without_claiming_a_version`);
the log is retained and this is not a clean lifecycle claim. PR #167 merged as
`a370cdc77a6b80004ef83d477135ca94aee85743` after all seven checks, 3,559 native tests and four
renderer cases. This branch incorporates it by normal merge, retaining both appended documentation
sections. Final combined-source validation is running before delivery.


## R39 durable native result catalog (validation in progress)

New canonical work for [#163](https://github.com/idosams/Mesh/issues/163), stacked on published
[PR #168](https://github.com/idosams/Mesh/pull/168), `0bbe5eaca7e182bfe1dedc3e01de2d89ce8d6873`.
No preserved source commits are replaced. R38's final full local gate passed: 3,559 native tests,
17 skipped, desktop checks and 44 daemon checks. Its initial run reported a process-leak warning;
the final run did not. Neither is proof of a lifecycle fix. Hosted checks are still running.

A bounded per-launch catalog retains the exact signed offer after the original checkpoint record.
Native discovery cross-checks every row against guarded original facts, with a revision cursor and
at most sixteen results per page. Publication reports success only after both records are retained;
interruption between appends is repaired by explicit idempotent republication without resigning.
Legacy offers remain readable by known checkpoint but require republication for discovery.

The composed native regression passes pagination across eighteen offers, reopen, duplicate
publication, interrupted index append, invalid cursor and corrupted records. The first run exposed
missing explicit unknown-admission handling; it was fixed and the failed log retained. Full local
and hosted checks remain pending. Authenticated discovery, content reopening/transfer, exact
coordinator import and real second-machine/packaged acceptance remain required.


## R40 authenticated result discovery (validation in progress)

New canonical work for [#163](https://github.com/idosams/Mesh/issues/163), depending on published
[PR #169](https://github.com/idosams/Mesh/pull/169), `e7ae39f62481a6aeda396dccf6b014b0890727e1`.
No preserved source commits are replaced. R39's full local gate passed: 3,560 native tests,
17 skipped, desktop checks and 44 daemon-demo checks. Its hosted verification is running.
R38 merged through [PR #168](https://github.com/idosams/Mesh/pull/168) as
`5cdc12b4934d73fab282f8d0d5ba98edc5c0e6c4` after all seven checks passed.

The native resident connection and bounded SSH wrapper now serve separately signed catalog
queries/replies with fresh nonce/deadline, exact current assignment, bounded cursor and page
relationships. Every returned offer is signature-checked and bound to the same assignment;
duplicate checkpoint identities refuse. The worker rechecks retained catalog facts around signing.
No automatic retry, provider launch, content transfer or protected-main authority is introduced.

Three focused regressions passed, including the real resident route losing a reply and recovering
the same catalog, stale challenge replay, wrong keys/domains, malformed cursor/count relationships,
duplicate checkpoints, another run's signed offer and history mutation while signing. Full local
and hosted checks remain pending. Legacy catalog limits, content reopening/transfer, exact
coordinator review import and actual remote/packaged acceptance remain in scope.


## R41 native saved-result reopening (validation in progress)

New canonical work for [#163](https://github.com/idosams/Mesh/issues/163), following published
[PR #170](https://github.com/idosams/Mesh/pull/170), `8c19acd36d220858cceedf935c99cc96579a6cac`.
No preserved source commits are replaced. R40's full local gate passed: 3,563 native tests,
17 skipped, desktop checks and 44 daemon-demo checks; hosted verification is running.

Native result reopening binds guarded launch/offer facts to a separately admitted destination,
revalidates the retained initialization mapping, original input and allocation installation, then
uses the existing history-only reopen seam. The exact recorded review/version and reconstructed
manifest must match the signed offer. Returned read-only sources retain filesystem authority
without restoring provider execution, credentials, working files or a launch reservation.

The composed received-host regression passed after the original owner was dropped, with later
unsaved bytes excluded, altered mapping refusal and replacement-root refusal. An initial test
compile lacked a type qualification; the correction and original log are retained. Full validation
and canonical PR delivery remain pending. Resumable output transport, exact coordinator review
import and actual remote/packaged acceptance remain required.


## R42 native resumable result receipt (validation in progress)

New canonical work for [#163](https://github.com/idosams/Mesh/issues/163), depending on published
[PR #171](https://github.com/idosams/Mesh/pull/171). No preserved source commits are replaced.
R41's full local gate passed: 3,563 native tests, 17 skipped, desktop checks and 44 daemon checks.
Two local process-leak warnings remain tracked in [#172](https://github.com/idosams/Mesh/issues/172).
Its original seven hosted checks also passed, including 3,563 native tests and four renderer cases;
its updated dependency preserves the identical source tree.

The native result receiver verifies the exact signed offer/current assignment and manifest before
store effects, owns the private receiving lock, and reuses durable partial CAS receipt. Context and
root checks bracket each operation. It creates no synthetic input assignment or launch reservation
and cannot materialize working files. Complete verification is an observation, not a durable
completion/import receipt or retention pin.

The native regression passed resumable receipt after dropping the owner, exclusive ownership,
invalid manifest before store creation, unknown chunks, conflicting offsets and substituted roots,
with coordinator revision/run count and the allocation directory unchanged. An initial test-helper
lifetime failure was corrected; its log is retained. Full local/hosted checks and PR delivery remain
pending. Authenticated result serving/transfer, durable exact candidate import and real remote
acceptance remain in scope.


## R43 authenticated saved-result content transfer (validation in progress)

New canonical work for [#163](https://github.com/idosams/Mesh/issues/163), depending on published
[PR #173](https://github.com/idosams/Mesh/pull/173). No preserved commits are replaced.
[PR #171](https://github.com/idosams/Mesh/pull/171) merged as
`3c6bddf913767c2a1587d4bc5184971af5650d51` after all seven checks passed. R42's corrected full
local gate and seven hosted checks passed (3,564 native tests, 17 skipped, desktop and 44 daemon
checks); its main-base reconciliation preserves identical content. Process warnings remain tracked
in [#172](https://github.com/idosams/Mesh/issues/172).

A separately signed fresh transfer query selects one exact saved checkpoint. The native resident
reopens guarded immutable history, signs a query-bound header, and serves only declared chunks in
strictly increasing order. The coordinator verifies offer/manifest before native store effects,
resumes durable offsets, and verifies complete file content. Authorization stays bounded by the
original challenge lifetime; explicit reconnect creates a new challenge without retrying execution.

Three focused regressions pass. Initial resume-fixture assumptions incorrectly treated a 32 KiB
source chunk as a partial 64 KiB transfer; the corrected bounded synthetic fixture tests a real
65,536-byte retained offset. A missing digest trait/type qualification was also corrected. Failed
logs are retained. This is protocol/socket/native-store evidence, not actual second-machine proof.
Full validation and PR delivery remain pending; durable exact import receipts and candidate review
correlation remain required.


## R44 durable result content receipts (validation in progress)

New canonical work for [#163](https://github.com/idosams/Mesh/issues/163), depending on published
[PR #174](https://github.com/idosams/Mesh/pull/174), `37f1e2e8a755ef71fd699db35a33989faf83a6c2`.
No preserved source commits are replaced. R43 full local validation passed 3,567 native tests,
17 skipped, desktop checks and 44 real daemon checks; its hosted verification remains in progress.

Content receipt publication verifies complete files, preserves a bounded private canonical manifest,
and records the original offer plus exact manifest and physical store identity in the coordinator
ledger. Replays recover the same receipt; native reopen requires the retained manifest, ledger and
complete valid bytes. Network transfer now records this fact before its end frame. It does not
release execution capacity, materialize files or approve main. Partial/corrupt metadata is preserved
and refused, not overwritten; receipt durability is separate from content retention and import.

The expanded native regression passed durable replay, database reopen, partial-manifest preservation,
changed-manifest/content refusal and corrupt ledger tails. Full validation and PR delivery are
pending. Exact candidate mapping/import, retention and actual remote/packaged acceptance remain.


## R45 native result identity correspondence (validation in progress)

New canonical work for [#163](https://github.com/idosams/Mesh/issues/163), depending on published
[PR #175](https://github.com/idosams/Mesh/pull/175), `6183986f01c7723e25575d85a8741c8ab00ef452`.
No preserved commits are replaced. R44 final local gate passed 3,567 native tests, 17 skipped,
desktop checks and 44 daemon checks. [PR #174](https://github.com/idosams/Mesh/pull/174) merged as
`121849cad3041f1624d9f947c6ad3299aac1a547` after all seven checks passed.

Native exact-history reopening now returns original offer/content plus an opaque correspondence
descriptor. It reuses existing project-import identity rules across the verified initial/result
snapshots, with complete manifest comparison, duplicate rejection and retained-kind checks. The
bounded canonical evidence binds original input, worker initial operation and exact saved result.
No renderer/peer constructor or coordinator import authority is added.

Two focused identity regressions pass, including retained renames and a new object at the old path.
The composed received-host test passes native reopen after the owner is dropped. Its initial
expectation incorrectly assigned ancestry to a new file from an empty input; correcting that
fixture expectation required no production behavior change, and the failed log is retained. Full
validation and PR delivery are pending. Authenticated evidence transport, exact coordinator import,
retention and actual remote/packaged acceptance remain in scope.


## R46 authenticated result correspondence (validation in progress)

New canonical work for [#163](https://github.com/idosams/Mesh/issues/163), depending on published
[PR #176](https://github.com/idosams/Mesh/pull/176), `5756a26efbfe9c28d77c449190b6b0f9d8e451ef`.
No preserved commits are replaced. R45 full local validation passed 3,569 native tests, 17 skipped,
desktop checks and 44 daemon checks.

The new read-only evidence query/reply uses separate signing domains, a fresh exact-assignment
challenge and the original signed-offer digest. Native worker reopening produces bounded metadata;
authenticated headers bind its full digest/length and bounded chunk frames carry it. The receiver
verifies both manifests and exact initial/result identities, completeness, unique objects/origins,
kind compatibility and canonical encoding. It writes no import, execution or main state.

Focused real-socket protocol fixtures passed multi-frame metadata and malicious response refusal;
closed-decoder tests pass malformed path/identity/context refusal. An initial scoped-borrow error
in the test helper was corrected and retained. Native history reopening is covered separately, not
claimed as actual second-machine proof. Full validation and PR delivery are pending; durable
evidence/import correlation, retention and complete remote/packaged acceptance remain required.

## R47 durable authenticated result evidence (validation in progress)

New canonical work for [#163](https://github.com/idosams/Mesh/issues/163), depending on published
[PR #177](https://github.com/idosams/Mesh/pull/177), `1e540674f982e018af5c787949d72e01b8ba19fe`.
No preserved commits are replaced. R46 full local validation passed 3,573 native tests, 17 skipped,
desktop checks and 44 daemon checks. [PR #176](https://github.com/idosams/Mesh/pull/176) merged as
`e1c05bc5e7b851513ff28d5f2183d5c46a7af177` after all seven checks passed.

The coordinator retains the first authenticated correspondence attestation alongside complete
verified content, in a private create-only descriptor file and an exact ledger receipt. Receipt
identity binds the original offer, descriptor and native content receipt. Fresh response nonces do
not replace the first attestation; conflicting provenance for the same content refuses. Restart
rechecks signatures, assignment, manifests, metadata and content without renewing execution authority.
Missing, partial, linked or changed metadata is preserved and refused, not repaired.

A focused native regression passes complete-content gating, partial-file preservation, wrong-key
refusal, idempotent replay, conflicting validly signed provenance, actual SQLite reopen, missing
metadata refusal and hard-link refusal. Initial production error-conversion and test-helper borrow
errors were corrected; failed logs are retained. Full validation and PR delivery are pending.
Candidate materialization/import, retention policy and real remote/provider/packaged acceptance
remain required; this receipt grants none of those authorities.

## R48 private remote result materialization (validation in progress)

New canonical work for [#163](https://github.com/idosams/Mesh/issues/163), depending on published
[PR #178](https://github.com/idosams/Mesh/pull/178), `9523d306d593a26c0b5ed3400e43e92f38b8aa4b`.
No preserved commits are replaced. R47 full local validation passed 3,574 native tests, 17 skipped,
desktop checks and 44 daemon checks; its hosted checks are separate from this source validation.

A native result receiver can copy an exact retained result to a fresh private result allocation.
It revalidates existing content/evidence receipts before and after copying, preserving the existing
bounded native materializer's complete inventory, file hashes, modes and root identity checks.
The result-only handle has no worker-admission conversion. Existing or partial output refuses;
missing receipt metadata cannot be recreated by copying. The original project is never a target.

The native library check and focused regression pass exact copy, provenance identity, collision
preservation, changed-copy refusal, CAS independence and missing-evidence refusal before allocation.
Full validation and PR delivery are pending. Independent native history/review, original-project
candidate import, durable local/remote review correlation, retention and real remote/provider/
packaged acceptance remain required; a copied tree alone is not an imported review candidate.

## R49 independent native result history (validation in progress)

New canonical work for [#163](https://github.com/idosams/Mesh/issues/163), depending on published
[PR #179](https://github.com/idosams/Mesh/pull/179), `60fb2dc6bcec194624d15b88f816b5574d1c9630`.
No preserved commits are replaced. R48 full local validation passed 3,574 native tests, 17 skipped,
desktop checks and 44 daemon checks; hosted delivery is checked separately.

An exact result-only allocation can initialize one independent native workspace. A create-only
intent precedes import; the completed private receipt binds the authenticated evidence identity,
remote version/manifest, physical allocation/files identities and the new local initial operation
and installation. Native import verifies the complete manifest and settled initial history.
Revalidation checks pinned custody, immutable local history, original copy and exact private receipts.
There is no worker-admission fabrication, provider launch or protected-main approval.

The composed native regression passes from authenticated retained evidence through result copy and
native import, proves distinct local history identity and refuses changed input, missing receipt and
hard-linked receipt without repair. The library build's unused import warning was corrected before
the focused test. Full validation and PR delivery are pending. History-only reopening, local review
registration/correlation, original-project candidate import, retention and real remote/provider/
packaged acceptance remain required; this source step does not complete remote review delivery.

## R50 history-only result reopening (validation in progress)

New canonical work for [#163](https://github.com/idosams/Mesh/issues/163), depending on published
[PR #180](https://github.com/idosams/Mesh/pull/180), `f31d72f296256d7fb86d96d177705646329020a5`.
No preserved commits are replaced. R49 full local validation passed 3,574 native tests, 17 skipped,
desktop checks and 44 daemon checks, with one unresolved process-leak warning tracked in #172.

A native destination can reopen an existing local result using a separately retained mapping digest,
authenticated evidence receipt and exact remote manifest. It checks canonical bounded private
metadata, physical identities, exact local installation and immutable initial content, then returns
a source that retains allocation pins. It creates no daemon, provider, session or credential and
repairs no missing state. Reading history does not establish dependency eligibility or approval.

The library check and composed native regression pass reopening after the original daemon is dropped,
exact saved bytes/local operation, wrong mapping and missing/linked receipt refusal, and retained
source refusal after allocation relocation. Full validation and delivery remain pending. Local review
registration, durable coordinator correlation, original-project import, retention and real remote/
provider/packaged acceptance remain required.

## R51 exact native result review registration (validation in progress)

New canonical work for [#163](https://github.com/idosams/Mesh/issues/163), depending on published
[PR #181](https://github.com/idosams/Mesh/pull/181), `046dedd8a3cc1b5bd81ac3f7e9c68fb99bf2677c`.
No preserved commits are replaced. R50 full local validation passed 3,574 native tests, 17 skipped,
desktop checks and 44 daemon checks. [PR #179](https://github.com/idosams/Mesh/pull/179) merged as
`c2e35df46b4cf8af76b7084101edb4f9a3983657` after all seven corrected checks passed.

Received local result history can register an exact saved inspection review under native managed
workspace custody, with no agent generation or admission. Native installation, allocation and
complete immutable manifest are checked before reusing the existing saved-review persistence.
Agent review creation retains its original custody guard and shares only that persistence helper.
Repeats return the original bundle; review creation grants no protected-main approval.

The library and composed native regression pass review creation, idempotent replay, linked receipt
refusal, durable record reopening after dropping the daemon, exact subject operation and absent
protected main. Full validation and PR delivery are pending. Durable coordinator correlation,
review-panel discovery, original-project import, retention and real remote/provider/packaged
acceptance remain required.

## R52 durable remote/local review correlation (validation in progress)

New canonical work for [#163](https://github.com/idosams/Mesh/issues/163), depending on published
[PR #182](https://github.com/idosams/Mesh/pull/182), `40581ef98be68f2c865aaa4b733eb37a45e023f4`.
No preserved commits are replaced. R51 full local validation passed 3,574 native tests, 17 skipped,
desktop checks and 44 daemon checks. [PR #180](https://github.com/idosams/Mesh/pull/180) merged as
`7207e15fe3c117162f8b167aec0ef09cc8e6dfd8` after all seven final checks passed.

The coordinator ledger now retains a distinct closed remote/local review record binding the signed
offer digest, authenticated evidence/content receipts, native mapping/allocation and exact local
operation/review. It retains objective/lane/run attribution without inventing a local agent origin
or checkpoint. Duplicate requests return the exact record; another mapping and corrupt extra
history refuse. Historical reopening checks the exact native saved review and content independently
of current run state; it cannot authorize execution or original-project import.

The library and composed native regression pass correlation replay, actual SQLite reopen, exact
historical bytes after cancellation and corrupt-tail refusal. A second-allocation conflict case
is included in full validation. An initial edit-script indentation error was corrected before
compilation; no existing work was lost. Full validation and PR delivery remain pending. Native
review discovery/panels, original-project import, retention and actual remote/provider/packaged
acceptance remain required.

## R53 bounded offline remote review discovery (validation in progress)

New canonical work for [#163](https://github.com/idosams/Mesh/issues/163), depending on published
[PR #183](https://github.com/idosams/Mesh/pull/183), `9b36db11531f3ce7ee90f9b09d029b071c6a1f21`.
No preserved commits are replaced. R52 full local validation passed 3,574 native tests, 17 skipped,
desktop checks and 44 daemon checks.

A bounded objective index makes retained remote reviews discoverable without contacting a worker
or knowing each offer in advance. Index intent precedes per-offer correlation commitment; incomplete
commitment is explicit and never reported as ready. Pages contain at most sixteen entries against
a fixed snapshot, with a 4,096-entry ceiling, contiguous sequence checks and exact canonical binding.
Reads refuse malformed/conflicting history without repair. Old v1 direct lookups stay valid; exact
native re-registration can add the index without rewriting the original correlation.

Two focused tests pass snapshot paging across later appends, interrupted intents, replay, malformed
history preservation, SQLite restart and composed native review discovery. A missing mutable receiver
on the index writer was corrected after the initial library check failed; that log is retained.
An existing historical branch name was preserved by choosing a distinct new branch. Full validation
and PR delivery are pending. Native review-panel integration, exact original-project import,
retention and actual remote/provider/packaged acceptance remain required.

## R54: native historical remote review reader

Depends on R53 / PR #184. New canonical implementation; replaces no preserved legacy commit.
Retained remote/local correlations now expose the existing native recorded-review presentation and
immutable artifact reader. Each read checks the exact manifest, mapping, installation, allocation,
local subject and review before and after access. Artifact selection uses object identity and side,
never a supplied path. Missing metadata is preserved and refused. The received result tree is
explicitly labelled; it does not claim an original-project diff or import eligibility. No session,
worker, approval or writeback authority is created. Desktop routing, durable destination lookup,
original-project import and full remote acceptance remain outstanding.

Validation: composed native regression covers SQLite reopening, exact saved review/artifact bytes,
invalid path/object/side/manifest and missing metadata refusals. Full canonical gate pending.

## R55: retained native remote review destinations

Depends on R54 / PR #185. New canonical implementation, replacing no legacy commit. Registration
now records a bounded private destination binding before indexing or acknowledging the correlation.
It retains the admitted store/parent paths, exact directory identities and protected-root policy in
the native ledger, keyed by the immutable correlation. Exact retries preserve the record; conflicts
refuse. The paths never appear in renderer output.

Runtime and FleetHistory now discover and read remote reviews/artifacts by offer and correlation
identities alone. Reopening verifies the retained native location, exact manifest, local history and
review; unknown identities, replacement directories, missing metadata and malformed records refuse
without repair. Existing v1 correlations remain readable with their explicit native destination;
unbound legacy correlations cannot use identity-only lookup until authorized exact registration.
No execution, import or approval authority is added. Desktop routing and panels remain next work.

Validation: native composed regression covers SQLite reopen, FleetHistory reads, exact artifact
bytes, legacy unbound records, mismatched identities, replaced store and corrupt location tail.
Full canonical gate pending. External acceptance and reliability issues remain open.

## R56: desktop remote review lists and parallel panels

Depends on R55 / PR #186. New canonical implementation; replaces no legacy commit. The native
desktop exposes bounded remote discovery and exact review/artifact calls through FleetHistory.
Only objective, offer/correlation and object/side identifiers cross the boundary; private locations
remain native. Missing history does not initialize storage. Other platforms refuse explicitly.

The fleet view now lists received results with a fixed bounded snapshot, explicit interrupted
registrations and independent pinned panels. It reuses the verified saved-review/artifact UI without
fabricating local checkpoints. Late replies cannot revive closed lists/panels, changed identities
refuse, incomplete projections stay explicit, and new work never replaces a pinned selection. The
shared eight-panel admission limit includes local panels; loading a saved local set that would
exceed capacity requires closing remote panels first. Remote pins remain session-only in this
increment, clearly disclosed; durable remote selectors remain required follow-on work.

Received snapshots expose no feedback, project-import or protected-main approval action. Original
project comparison/import, complete ingestion composition, retention, signed packaged proof and
second-machine/provider acceptance remain open. Focused native, controller, React build/render and
artifact refusal tests passed. Full `npm test` passed: 3,576 native tests, 17 skipped,
153.593 s, desktop checks and 44 daemon-demo checks. One slow test completed successfully; no
leak warning occurred in this run. Earlier reliability issues remain open.

## R57: durable remote review selectors

Depends on R56 / PR #187. New canonical implementation; no legacy commit is replaced. Separate
closed v1 remote selectors retain exact objective/offer/correlation, local and remote review identity,
selected object and view layout. No content or path is persisted. Native storage binds records to
the physical private catalogue, bounds them to 64 KiB/eight selectors, compares revisions and
acknowledges only after file and directory durability. Interrupted, unknown, linked or foreign
records are preserved and refused. Existing local v1/v2 pin records remain unchanged.

Local and remote saves share the native initialization lock and enforce eight combined pins. The
desktop restores local then remote selections, rechecks native content, retains unavailable reviews,
and exposes retry/reload status. Lost save replies reconcile without duplicate publication; unknown
snapshots cannot be overwritten. A stored selector conveys no content, execution or approval
authority. Original-project import, full ingestion, retention and external acceptance remain open.

Validation: nine focused native pin tests passed, including concurrent local/remote capacity,
restart, stale revisions, malformed metadata and foreign storage. The full `npm test` gate passed:
3,579 native tests, 17 skipped, 151.050 s, desktop checks and all 44 real daemon-demo checks.
One slow test completed successfully; no leak warning occurred. Earlier reliability issues remain open.

## R58: remote results become original-project candidates

Depends on R57 / PR #188. New canonical implementation; no legacy commit is replaced. Native
receiver methods stage authenticated remote results through the existing private project-candidate
store and compile an existing candidate through the original-project operation compiler. Retained
worker attestation, complete content, local review correlation, exact project registration/input,
current assignment and main are rechecked. Cancellation or stopping refuses preparation while
historical review remains readable. Delegated remote inputs require a separate complete transitive
lineage integration and currently refuse; direct attached-project inputs are supported here.

The original project and independent received history have different object identities. Only an
exact manifest-equivalent copy may bridge their paths. Authenticated worker input origins preserve
renamed objects; a new object at the old filename is never adopted as the original. Extra/missing
entries, duplicate identities, changed bytes/modes or a different original operation refuse.
`mesh.remote-project-candidate/v1` provenance binds evidence, correspondence, exact review selection,
source project/version and observed main. Existing candidate/import formats are unchanged. The new
record confers no approval authority and is bounded by existing candidate/content limits.

The focused native test uses a real attached project and independently materialized native result
history with a signed worker fixture. It verifies private staging replay, actual compiled rename and
replacement operations, unchanged original files/history/main, stale-main refusal and cancellation.
A second test verifies complete-copy identity and content refusals. These are not a real second-host
or packaged graphical acceptance run. Signed import commit/recovery, remote candidate UI/actions,
transitive remote lineage, composed transport/ingestion and the remaining full-plan acceptance are
still required. Full `npm test` passed: 3,581 native tests, 17 skipped, 158.645 s,
desktop checks and all 44 real daemon-demo checks. One slow comparison test completed; no process
leak warning occurred. Earlier reliability issues remain open.

## R59: signed remote candidate import and recovery

Depends on R58 / PR #189. New canonical implementation; no legacy commit is replaced. Native
receiver APIs now inspect durable import outcomes, append the exact signed private project version,
and record/read its real original-project review through existing native import/recovery controls.
Original files and protected main remain unchanged. Import signing is distinct from human approval.

Signing may yield. The remote source is verified and pinned before acquiring original-project write
custody; the retained source roots, current remote context and revision are checked immediately
before append, after durable intent. Reopening a foreign workspace under original-project write
custody is deliberately avoided. Cancellation or replaced remote storage leaves the pending intent
intact and refuses append. Exact pending retries use retained signatures; completed retries return
the existing operation without signing again. Historical outcome inspection permits cancellation
and later main advancement, while new import/review creation requires current eligibility. The
current receiver still requires its retained assignment context; arbitrary older-attempt discovery
is not added by this increment.

Three focused native journeys passed: real signed import/review/reopen, cancellation during signing,
and remote allocation substitution during signing followed by exact pending recovery. Tests assert
original object identities in the actual imported version, unchanged original files and main,
no double signing, refusal with an unavailable key, retained pending intent, and no append across
cancelled or substituted custody. Worker evidence/signing keys are deterministic fixtures; this is
not OS-key, packaged or second-host acceptance. Persisted import/receipt formats are unchanged.
The full gate first stopped during compilation because the disk was full; its failure log is retained.
After reclaiming only the inactive incremental compiler cache, the same `npm test` gate passed with
`CARGO_INCREMENTAL=0`: 3,583 native tests, 17 skipped, 147.814 s, desktop checks and all 44 real
daemon-demo checks. One slow comparison test completed; no process-leak warning occurred. Earlier
reliability issues remain open. Direct-root input support does not complete transitive remote
lineage, graphical actions, composed transport/ingestion, retention or external acceptance.


## R60: composed native remote result ingestion

Depends on R59 / PR #190. New canonical implementation; no legacy commit is replaced. One native
coordinator operation binds an independently configured worker and current assignment to an exact
selected signed offer, receives complete saved content and authenticated correspondence, retains both
receipts, materializes independent local history and registers its review correlation. SSH connections
have separate bounded budgets. A different signed offer for the same checkpoint is refused before
accepting content. Existing transfer callers retain their original behavior.

A completed retry revalidates retained evidence, content and native history locally without signing,
network access or another allocation. Missing or replaced storage refuses without repair. A partial or
unacknowledged allocation remains preserved for reconciliation; this operation does not adopt it or
restart a provider. Existing persisted formats and protected-main approval boundaries are unchanged.

The composed native integration uses a real received worker, a fixture executable and test signing
keys. It checkpoints/reviews real history, publishes its signed offer, stops the worker and serves
content and correspondence through the native endpoint and connection handler. Assertions cover saved
bytes despite later unsaved edits, exact-offer substitution before content mutation, coordinator
restart/offline replay and replaced receiving storage. This is local native transport evidence, not a
real SSH/second-host or packaged acceptance claim. The full `npm test` gate passed: 3,584 native
tests, 17 skipped, 148.805 s, desktop checks and all 44 real daemon-demo checks. One slow test
completed; no process-leak warning occurred. Initial fixture-selector and Clippy failures were
corrected, with their logs preserved. Existing reliability issues remain open.
Desktop receive/import actions, transitive lineage, retention and full external acceptance remain open.

Delivery update: PR #189 passed all seven final checks and merged as
`70567590d03fcadefca30f153644290b940538e1`. Its combined-main run is still pending. PR #190's original
seven checks passed; its unchanged source tree is being checked after rebasing the PR target on main
through a normal ancestry-only merge (no history rewrite).


## R61: project actions from retained remote selectors

Depends on R60 / PR #191. New canonical code; replaces no legacy commits. Runtime and FleetHistory
APIs stage, inspect/import and read/create the imported project review from exact retained offer and
correlation digests. Native code resolves the previously bound receiving destination and complete
content receipt, reopens the exact signed offer against current assignment context, and derives the
input manifest from the independently admitted original project's saved version. Neither a renderer
path nor a peer-supplied manifest enters the API. Keys recovered from the local content record are
bound by its exact digest in the committed correlation and reverified with native content/history.

The existing direct-root eligibility, provenance, cancellation, signing/recovery, fixed-main and
custody checks remain in force. Historical outcome inspection remains distinct from mutation.
Missing/ambiguous local records, mismatched selectors and replaced native roots refuse without
repair. Persisted formats are unchanged. These APIs do not start or adopt workers, approve main,
apply original-folder changes or complete the graphical integration.

Three real native project journeys pass through retained-selector staging, signed import and
restart/outcome recovery, including cancellation and storage replacement during signing. Exact
correlation mismatch refuses; cancellation still permits historical inspection while staging/import
refuse. Original identities, unchanged source bytes/main and retained-signature recovery remain
asserted. Fixture worker evidence/test keys are used. Full `npm test` passed: 3,584 native tests,
17 skipped, 158.190 s, desktop checks and all 44 daemon-demo checks. One slow comparison test
completed; no process-leak warning occurred. Earlier reliability issues remain open.

Delivery update: PR #190 passed all seven final checks and merged as
`c2b032579093f1323cf48f11cd44a78addb518e5`; combined-main validation is pending. PR #189's combined-main
run passed. PR #191 is published at `49a0ef81af03e8693692def607ad97e6e7fa3849`, with original hosted
checks running. Its full local gate passed with 3,584 tests, 17 skipped, desktop and 44 daemon checks.


## R62: native desktop remote-project command boundary

Depends on R61 / PR #192. New canonical code; replaces no legacy commits. The asynchronous desktop
command `remote_fleet_project` accepts a bounded closed `mesh.desktop-remote-project-request/v1`
selection containing project/objective, exact offer/correlation, stable request, fixed observed main
and one named stage/inspect/import/review action. Paths, unknown fields, malformed identities and
approval/application/execution actions refuse. Native work runs off the renderer thread.

The native host resolves the admitted source and retained history with the same review trust used by
existing local project imports. A new recorded-outcome API verifies the actual durable import actor
and outcome without opening a private key. Completed imports return recorded truth; pending imports
use the recorded native identity; new imports use the existing native signer. Stage, inspect-import,
import, inspect-review and create-review are separate actions. No main approval, source write-back or
worker adoption is added. Responses bind the exact selection under
`mesh.desktop-remote-project-result/v1`; stored import formats are unchanged.

Three real native project journeys verify recorded actor/outcome recovery, including cancellation and
replaced storage during signing. Two desktop host tests cover closed-request refusals and missing-state
refusal without provisioning. Test keys/evidence are fixtures; this is not positive packaged UI or
OS-key acceptance. Full `npm test` passed: 3,586 native tests, 17 skipped, 148.478 s, desktop
checks and all 44 daemon-demo checks. One slow comparison test completed; no process-leak warning
occurred. Earlier reliability issues remain open. Graphical controls and durable remote action
retry state remain outstanding; the native command alone is not the completed user workflow.


## R63: durable remote project workflow in parallel review panels

Depends on R62 / PR #193. New canonical implementation; replaces no legacy commit. Remote saved
panels now prepare a result for its original project, save a private project version, create its
original-project review and open that exact review through the existing attachment review surface.
Native context resolution verifies the received result, independently admitted project and observed
main. The fixed base, offer/correlation and retry identity are retained before dispatch. Rendering
never resolves paths or grants main approval/application authority. English and Hebrew copy are added.

`mesh.remote-project-outbox/v1` holds at most eight closed, content-free requests under a native
catalog identity and revision check (131,072-byte record limit). Same-request project/selection/main
substitution refuses. Atomic create-only staging and durable rename preserve interrupted metadata;
foreign, malformed, stale and partial state refuses without repair. Existing outbox and pin formats
are unchanged. This is retry metadata, not a content-retention guarantee or execution authority.

Reopening loads pending inputs without automatically submitting actions. Explicit reads recover
status; exact retries preserve uncertain work. Closing panels does not remove pending requests.
Removing an entry deletes only metadata. Independent actions retain separate identities and results;
changed completed outcomes refuse while the last verified snapshot remains visible. Missing-state
native reads do not provision work. Private imports, exact reviews, human main approval and applying
original files remain separate boundaries.

Focused native storage tests cover restart, exact replay, stale revisions, changed fixed input,
foreign catalog copies and preserved partial state. Three native project journeys also verify context
and recorded recovery. Controller tests cover failed/lost save acknowledgments before dispatch, lost
import replies and restart without replay, disposal during persistence, concurrent independent imports,
substituted replies and exact project-review navigation. Rendered tests keep pending actions visible
without panels. The first broad desktop run exposed five existing fixtures that omitted the new
read-only outbox load; their exact allowed-call expectations were updated, retaining no-launch/no-write
assertions. Full `npm test` passed: 3,588 native tests, 17 skipped, 148.020 s, 170 rendered
tests, 598 desktop tests and all 44 daemon-demo checks. One slow comparison test completed; no
process-leak warning occurred. Earlier reliability issues remain open. This is not signed packaged
or second-host acceptance.

## R64: retained native ancestry for delegated remote input

Depends on R63 / PR #194. New canonical implementation; replaces no legacy commit. Native callers
can prepare a `RemoteProjectInput` from an exact local saved review and independently admitted
original project. It retains the original input, complete bounded local ancestry and every native
history allocation. Exported input identity stays distinct from the original-project predecessor;
object correspondence comes from the complete historical chain, never matching current filenames.

Every manifest, predecessor, correspondence and chunk read revalidates the recorded selection,
lineage and directory identities. Chunk reads also verify after reading. Later working edits do not
replace saved bytes. The handle can be reopened through retained history without worker adoption.
It introduces no persisted format, signing, execution, dependency eligibility or approval authority.
Holding directory handles is not a retention/GC guarantee; missing content still refuses.

The native delegated-project journey covers exact parent/child history, later unrelated parent work,
later unsaved child work, wrong-review refusal, replaced-ancestor refusal on both metadata and bytes,
and restart without acquiring execution. Focused verification passed (one journey, 2.122 s).
Full `npm test` passed: 3,588 native tests, 17 skipped, 150.903 s; 170 rendered tests,
598 desktop tests and all 44 daemon-demo checks. One slow test completed; no process-leak warning
occurred. Earlier reliability issues remain open. Composition with authenticated remote result correspondence,
transitive remote ancestors and current dependency/revocation checks remain the next required work;
this input handle alone does not enable delegated remote imports or complete remote acceptance.

## R65: remote results composed with retained local ancestry

Depends on R64 / PR #195. New canonical implementation; replaces no legacy commit. A received
remote result may now derive from a recorded local parent review. Native service code resolves one
complete parent checkpoint/review at the assigned input, opens its ancestry outside the fleet mutex,
then performs the closed action under that mutex. Missing or ambiguous parent reviews refuse; a
caller cannot substitute a renderer-selected manifest or ancestry. Validation inside the action does
not reacquire the fleet mutex.

The delegated input and original-project predecessor remain distinct. Authenticated remote object
correspondence is composed with complete native local ancestry: renames retain original object
identity while additions/replacements remain new. Native eligibility checks reject cancellation and
stopping/cancelled ancestors around signing. Replaced ancestry refuses before append; durable pending
intent remains recoverable after the exact original identity is restored. Historical inspection does
not grant mutation authority and does not erase a completed import after cancellation.

Direct candidate provenance remains `mesh.remote-project-candidate/v1`. Delegated candidates use
`mesh.remote-project-candidate/v2`, adding closed `mesh.remote-project-ancestry/v1` evidence with the
exact parent selection, manifest, original predecessor and bounded local steps. Existing candidate,
import and outbox envelopes remain unchanged. Desktop readers accept both provenance versions and
validate complete step linkage, unique lanes, exact root/leaf bindings and closed fields. Older
readers refuse the new provenance; there is no automatic data rewrite or approval migration.

Six native journeys passed (4.04 s): existing direct import/recovery, delegated service import and
recorded recovery, cancellation during signing, and ancestor replacement during signing. The
expanded fixture uses a real native allocated parent and saved review with fixture signing keys;
newly opened history services recover the same outcome without changing lifecycle state. Initial
fixture failures skipped a required running observation and retained a receiving-store owner during
reopen; both were corrected without changing production guards. Eight desktop workflow tests passed,
including malformed ancestry refusals. Full `npm test` passed: 3,591 native tests, 17 skipped,
150.044 s; 170 rendered tests, 599 desktop tests and all 44 daemon-demo checks. One slow test
completed; no process-leak warning occurred. Existing reliability issues remain open. The first
full gate stopped on a test-helper return-binding lint; it was corrected without weakening checks.

Initial hosted Linux compile/lint checks caught a macOS-only correspondence type referenced by
ungated import helpers. Five macOS-only helper guards now match their native consumers; shared
retained-input verification remains cross-platform. The corrected full local gate passed again:
3,591 native tests (148.927 s, 17 skipped), desktop and daemon-demo checks. Hosted Linux verification
is required before merge; the original failed logs and running macOS job were preserved.

Remote ancestors are not yet composed into this local ancestry chain. Complete remote dependency
revocation/selection policy, real second-host operation, retention and packaged end-to-end acceptance
remain required. These native/renderer tests do not establish those outcomes or human main approval.

## R66: immutable parent review binding at remote admission

Depends on merged R65 / PR #196. New canonical implementation; replaces no legacy commit. Replay now
captures the unique completed parent checkpoint/review at the lane's first authenticated remote
launch claim. Later saves, late completion/review submission and retries cannot replace that
selection. Native retained import and ancestry verification use the captured identity instead of
searching all current checkpoints by version. Root remote lanes and ordinary local delegation keep
their existing behavior.

The binding is a projection of the existing committed event prefix; command wire formats, persisted
records and candidate provenance formats are unchanged. Existing streams reconstruct the same
historical selection on reopen without appending or rewriting events. Missing or ambiguous evidence
at the first remote admission remains refused even if a later review would make today's search
unique; no read or retry silently adopts new authority. An explicit recovery path for such lanes
remains future work. This selection is historical evidence, not dependency eligibility or approval.

The focused cross-platform runtime regression passed across unique, ambiguous, unfinished and
unreviewed parent cases, subsequent duplicate checkpoints, retries and store reopen (0.047 s).
All six native direct/delegated import and recovery journeys passed (3.800 s), now including a parent
checkpoint completed and reviewed after admission. Full `npm test` passed: 3,592 native tests (144.926 s, 17 skipped), 170 rendered tests,
599 desktop tests and all 44 daemon-demo checks. One native folder-opening test reported a process
leak; the original log is retained and reliability issue #172 remains unresolved. Hosted checks
are pending.
Earlier remote ancestors, complete revocation policy, retention, packaged acceptance and real
second-host/provider measurements remain required.

## R67: unlocked native remote observations through retained fleet history

Depends on merged R66 / PR #197. New canonical implementation; replaces no legacy commit.
`FleetHistory` can prepare a single-use current-lease status query or bounded saved-result discovery
query using independently admitted native identities. The query retains its exact service and
assignment challenge. Its SSH exchange holds no fleet mutex; reply acceptance reacquires the service
and revalidates the current durable context. Slow/disconnected workers therefore do not hold the
service lock needed for views or cancellation. A changed run refuses the old reply.

No mutable runtime, generic signing payload or renderer-selected SSH policy is exposed. The caller
must supply native-admitted SSH configuration. No lease renewal, automatic retry, adoption, dispatch,
allocation or import authority is added. Dropped prepared queries perform no network work. Existing
query wire formats and persisted history remain unchanged. Full native operator configuration and
application controls still need integration; these APIs alone are not a completed operator journey.

Three focused native regressions passed (0.043 s), exercising both query kinds, unlocked transport,
no history mutation, direct store reopen, changed run context, disconnected/malformed replies and
identity/cursor refusal before signing. The first fixture reopen tried to reclaim an existing launch
and correctly refused; the fixture now reopens the ledger directly. Full `npm test` passed: 3,595 native tests (145.188 s, 17 skipped, one slow),
170 rendered tests, 599 desktop tests and all 44 daemon-demo checks. No process-leak warning occurred;
existing reliability issues remain unresolved. Hosted validation remains pending. Tests use authenticated fixture signatures and an in-process transport boundary;
real SSH, another machine, OS custody and packaged acceptance remain unproven.

The first hosted Linux compile/lint run exposed an ungated module referencing the existing
macOS-only remote transport APIs. The observation module and its reexports now use the same macOS
boundary. The corrected full local gate passed again: 3,595 native tests (147.920 s, 17 skipped),
170 rendered tests, 599 desktop tests and all 44 daemon-demo checks, without a process-leak warning.
Original failed logs and the ongoing original macOS verification were preserved. Fresh hosted Linux
verification is required before merge.

## R68: explicit native coordinator status and discovery commands

Depends on R67 / PR #198, including its platform correction. New canonical implementation; replaces
no legacy commit. Before GUI setup, the signed macOS app accepts `--coordinator status` and
`--coordinator results` with a private closed configuration. It reopens existing native installation
custody, admits strict SSH policy and opens retained fleet history through the existing guarded
catalogue. It uses R67 one-use unlocked observations. Another catalogue owner refuses without
adoption. No missing fleet, assignment or identity is provisioned by these commands.

The local `mesh.coordinator-observation-config/v1` has twelve exact fields. Arguments and page
cursors are bounded; private-file loading reuses the worker's permission/no-follow/size checks.
Existing worker configuration decoding still wraps that same shared reader. Output distinguishes
unknown work from a verified empty page and preserves signed offers for native follow-up. Output
contains private correlation, not authority. Existing persisted fleet and remote wire formats are
unchanged. No UI control, dispatch, lease renewal, automatic retry or result ingestion is claimed.

Nine focused desktop-native tests passed (2.690 s): exact command routing, closed configuration,
keys/paths/cursors, private-file permissions/symlinks/size and unchanged contents, unknown/empty
output, existing worker configuration/provisioning and unsigned refusal before any config/key access.
The parent platform correction was fast-forwarded with hashes proving all child changes unchanged.
Full `npm test` passed: 3,600 native tests (147.383 s, 17 skipped, one slow), 170 rendered tests,
599 desktop tests and all 44 daemon-demo checks, without a process-leak warning. The first full gate
passed all native tests but failed the existing exact startup-order assertion; that assertion now
requires the new coordinator route and retains every previous route and GUI-order check. Hosted
validation remains pending. Eligible signed-app custody, real SSH/second-host
operation, app controls and full fleet acceptance remain required.


## R69: independent connections retain the native fleet ledger authority

New canonical prerequisite for unlocked result ingestion; replaces no legacy commit. Based on
merged R67 / PR #198; independent of R68's coordinator observation command. A guarded fleet store
can reopen a separate connection using its own SQLite filename and the same retained native
authority. It accepts no new caller-selected path and never initializes missing history. Unguarded
stores refuse. Every connection retains the existing before/after identity checks, transactional
revision enforcement and exact request idempotency. No schema or wire format changes occur.

Three focused regressions passed (0.023 s): committed events are visible in both directions; stale
writers refuse and exact retries replay; authority survives dropping the original connection and
revocation reaches the remaining connection; missing/replaced history is not initialized or modified.
The initial focused build caught a mistyped existing method name in the test, corrected before these
runs. The full local gate passed on the PR #198 base: 3,598 native tests (146.401 s, 17 skipped),
170 rendered tests, 599 desktop tests and all 44 daemon-demo checks, with no process-leak warning.
The increment is now reconciled onto merged PR #199; combined validation and hosted checks are pending.
This storage API alone does not connect ingestion
to the native service, add operator controls, adopt workers or approve main. Those remain subsequent
increments, alongside the full outstanding fleet acceptance plan.


## R70: unlocked selected-result ingestion through retained fleet history

Depends on merged R69 / PR #200. New canonical implementation; replaces no legacy commit. The macOS
native `FleetHistory` surface now receives a selected authenticated result using a closed request
with native-admitted identities, input, destination and trust. It opens an independent connection
to the same guarded ledger under the service lock, then releases that lock before the composed
receive/evidence/materialize/review operation. The public caller gets no mutable runtime or generic
mutation closure. The existing ingestion flow retains revision checks, exact offer identity and
completed offline replay. It adds no schema or protocol change, renewal, retry, worker adoption,
original-project write or protected-main authority.

Three focused native regressions passed (2.198 s). A real private catalogue and attached project
exercise unlocked view/cancellation, refreshed cancellation in the second runtime, history-only
restart and replaced-catalogue refusal while original file bytes remain unchanged. The existing
authenticated native socket ingestion journey also runs through this service bridge: separate live
readers must finish within two seconds inside each transport exchange; exact result bytes and saved
review are checked; completed replay needs no network/signature, and replaced storage refuses.
The original direct-runtime journey still passes. Full `npm test` passed: 3,605 native tests
(148.366 s, 17 skipped), 170 rendered tests, 599 desktop tests and all 44 daemon-demo checks.
Two existing native tests reported process-leak warnings: the mismatched-saved-input remote-delivery
refusal and the configured-reviewer authority refusal. Logs are retained; reliability issue #172
remains unresolved. This is not a leak-free acceptance claim. Reconciliation onto merged PR #200
preserved every changed source byte and the parent tree. Hosted validation remains pending.

These fixture tests do not prove real SSH, a second machine, OS signing custody or packaged graphical
acceptance. Operator configuration, dispatch/receiving controls and the remaining complete fleet
plan are still required. This bridge does not itself configure a remote peer.


## R71: native operator command receives an exact selected remote result

Depends on merged R70 / PR #201. New canonical implementation; replaces no legacy commit. The
macOS pre-GUI coordinator command now supports `receive` with a bounded owner-private configuration.
It reuses the admitted coordinator/SSH/catalogue context, selects existing saved project or managed
review input, protects source/history and native ownership directories from destination overlap,
and invokes the unlocked R70 ingestion service. It accepts no authored manifest or arbitrary
per-file output paths and emits only local receipt/version/review identities after verification.
The existing status/results configuration and persisted/wire formats are unchanged.

Eight focused native tests passed (1.591 s): explicit CLI parsing, closed configuration/refusal,
private file rules, saved-versus-live input bytes, protected original/retained storage, result output
and unsigned-app refusal before config/key access. The initial build found two unsupported desktop
imports; they now use the existing daemon digest alias and software signer only in the test fixture.
The production path retains Apple custody and its eligible-signed-application requirement. Full
`npm test` passed: 3,608 native tests (149.185 s, 17 skipped, one slow), 170 rendered tests,
599 desktop tests and all 44 daemon-demo checks. No process-leak warning occurred in this run;
existing reliability issues remain unresolved. Hosted verification remains pending.

This adds explicit receiving, not automatic dispatch, renewal, worker adoption, original-project
import, human approval or GUI receiving controls. Native context configuration and exact assignment
identity remain operator prerequisites. Real signed-app/OS-key, SSH and second-host acceptance,
retention/revocation and the full remaining fleet plan stay required.


## R72: native initial remote input delivery and request recovery

Depends on merged R71 / PR #202 (`858bcb318559ad78016feeffd4e7673adbfb6fbe`). New canonical
implementation; replaces no legacy commit. R71 passed all seven PR checks and all seven combined-main
checks in run 37059772782. Original commits, dirty legacy work and deprecation history remain preserved.

The macOS pre-GUI `start` command resolves a saved original-project version, creates a native fleet
with an exact provider/limit policy, derives stable attempt identities and invokes authenticated
initial input delivery. A closed current-session service operation records dispatch before network
work and holds no service mutex during transfer. It retains an independent guarded ledger, rejects
any existing attempt or missing current workspace, validates fixed lease/key/input before dispatch,
and preserves all unknown outcomes without retry, renewal or worker adoption. Existing observation
and receiving configuration formats remain compatible.

The separate `created` command finds native catalogue facts by the retained creation request, including
after restart or lost output. It requires no original-project or transport access and cannot return
execution ownership. Allocation rejects protected original/history/identity roots. An input receipt
is explicitly not provider startup/completion or main approval. Fourteen initial focused regressions
passed (2.263 s). The final full `npm test` passed: 3,615 native tests (154.630 s, one slow,
17 skipped), 170 rendered tests, 599 desktop tests and all 44 daemon-demo checks. The final tests
also compare materialized saved bytes against newer original bytes and refuse cancellation during
transport and automatic execution reattachment after request lookup. No process-leak warning
occurred; existing reliability issues remain open. Hosted CI is pending. This does not claim actual
OS-held keys, SSH, a second machine, packaged UI or velocity/resource measurements.
The complete fleet objective, explicit restart reconciliation, GUI controls, retention and complete
remote dependency/revocation remain outstanding.


## R73: explicit original-assignment input reconnect after coordinator restart

Depends on merged R72 / PR #203 (`714e28d46191ba37e1cb92d49b2367eca0af3e86`). New canonical
implementation; replaces no legacy commit. R72's exact-head run 37061729924 passed all seven checks;
its combined-main run 37079919613 also passed all seven checks. The user's tested R71 checkpoint remains
fixed at `858bcb318559ad78016feeffd4e7673adbfb6fbe` with separate application state.

The native history service now composes the existing explicit input reconnect through an independent
guarded ledger. It exposes no dispatch/mutable runtime or replacement assignment/lease. The macOS
`reconnect-input` command reuses admitted peer and saved project/review configuration under the same
eligible-app/private-file rules. Its output reports input disposition only. Existing wire, observation
and receiving formats remain unchanged; no worker or local execution context is adopted.

Nineteen focused tests passed (2.771 s), including an authenticated chunk-loss journey followed by
catalogue restart. The same worker reservation materializes exact saved bytes while original files
have newer external edits. One admission/allocation/run and the complete unchanged coordinator state
are asserted, along with live-reader responsiveness and wrong-worker/missing-claim/replaced-catalogue
refusal. The initial build caught test cleanup ownership errors; their log is retained. Additional
negative ledger-state coverage refuses expired/renewed leases, cancellation, running or local
ownership, changed input and wrong runs before signing. The full `npm test` passed: 3,620 native
tests (151.018 s, one slow, 17 skipped), 170 rendered tests, 599 desktop tests and all 44 daemon-demo
checks. The original run reported one process-leak warning in
`attachment-background::repeated_signals_coalesce_while_a_save_is_in_flight`; issue #172 remains
unresolved. A bounded 30-repeat probe passed without reproducing the warning; this does not
establish a lifecycle fix. Hosted CI is pending. These fixtures do not establish
OS custody, real SSH, a second host or packaged operator acceptance. Lost worker reservations,
complete dependency/revocation/retention, GUI controls, real-provider acceptance and measurements
remain part of the full objective.

## R74: packaged existing-project graphical acceptance

R73 / PR #204 merged as `83fab700c675744fcd473360a45208a26772526a`; all seven exact-head
checks and combined-main run 37110888516 passed. R74 is new canonical verification work and replaces
no legacy commit. The existing CLI attachment proof cannot establish graphical attachment, and the
older window proof covers imported workspaces. A separate exact-bundle runner now exercises dirty
existing-project attachment, external edits, two parallel saved comparisons with exact text, an
independent line, restart, explicit resume and detach. Native proof reports/checkpoints are closed
and bound to their launch phase; renderer success alone is supplemented with independent file/Git
and persisted-pin checks. Focused native proof tests passed (13). The first full run ended with
72 failures under restricted hardware/socket/native-event access; its log is retained. The authorized
full `npm test` rerun passed: 3,621 native tests (158.960 s, one slow, 17 skipped), 170 rendered
tests, 601 desktop tests and 44 daemon-demo checks. One process-leak warning in
`attachment-background::stopping_during_signing_prevents_that_capture_from_being_committed` remains
unresolved under #172. A real packaged run remains pending. This does not complete the full fleet/provider/harness/main-approval objective.

## R75: presentation-independent native coordinator results

R74 / PR #205 merged as `6d6770c3c87670ec2a0cf742f15efb8ee2fcd2a1`. All seven exact-head checks
and combined-main run 37115147546 passed. Its sealed `57206a4e917929832575b2713318c1ab11a42a23`
app passed `mesh-attached-project-window-proof/v1`: dirty-project attachment, external edits, exact
saved text in two pinned comparisons, independently verified work-line bytes, stopped restart,
explicit resume/detach, unchanged Git/folder identity and retained pin records. This is automated
manual-work graphical evidence, not provider, harness attribution, remote-host or main-approval proof.

R75 is new canonical groundwork for graphical remote controls; it replaces no legacy commit.
Coordinator operations return verified native JSON before CLI presentation. Output failures cannot
restart an operation, and all six CLI shapes, response formats and recovery messages remain intact.
Native execution still checks application eligibility before configuration/custody access. No generic
renderer invocation, credential-path input or mutable runtime is exposed. Fifteen focused tests passed
(9.938 s), including short writes, partial/flush failure and direct native eligibility refusal.
The full `npm test` passed: 3,624 native tests (150.944 s, one slow, 17 skipped), 170 rendered
tests, 601 desktop tests and 44 daemon-demo checks. No process-leak warning occurred in this run;
that does not resolve #172. Native connection selection/profiles, graphical commands and complete
remote/provider/human-approval acceptance remain required. Hosted CI is pending.

Reliability issue #172 remains open: a separate instrumented full native run passed all 3,621 tests
without reproducing a leak. Earlier focused direct probes used a different dependency build closure
from the workspace suite and do not establish a cause or fix. No test threshold was changed.

## R76: native SSH file paths with spaces

R76 is new canonical setup work, replacing no legacy commit. It depends on R75 / PR #206
(`cb99e4d3d8681620350c7f8be0f5a7dfffca3ec5`) and is published separately against that parent.
Existing private identity and known-host files may live in directories such as `Application Support`.
The identity remains one argument; the known-hosts option contains one quoted literal path.
Expansion tokens, quotes, escapes and controls still refuse, and existing owner, mode, link, size
and metadata-change checks remain enforced. Mesh does not create keys or enroll host trust.
The new positive regression failed before the fix. All nine focused transport tests then passed
(0.104 s), including inspection by the installed SSH parser with real placeholder files and no
network connection. Full `npm test` passed: 3,626 native tests (150.432 s, one slow,
17 skipped), 170 rendered tests, 601 desktop tests and 44 daemon-demo checks. No process-leak
warning occurred; this does not resolve #172. Hosted validation is pending. This is setup
groundwork, not graphical connection management, authenticated SSH or second-host acceptance.

## R77: graphical native remote observation panel

R75 / PR #206 merged as `6b4d4048e3f050f6b242ffb582226bdabc71a35f` after all seven exact-head
checks passed; combined-main run 37116710513 also passed. R76 / PR #207 is separately published,
with its original run 37116690725 successful; retargeting and final main integration remain pending.
R77 starts from canonical R75 main and replaces no legacy commit. It is independent of R76.

The fleet UI can select an existing private coordinator configuration in a native file chooser,
then explicitly read authenticated worker status or first-page saved-result discovery. Native code
owns the immutable configuration, random session selector, exact directory/coordinator identity and
original SSH file admission. Only public labels and bounded observations cross to the renderer.
The existing application catalogue owner supplies the retained history, avoiding a competing open.
Unsigned applications refuse before configuration/custody access. Only status/results operations
are exposed; no start, renewal, receive, retry, adoption or protected-main authority is added.

Twenty-one focused native tests passed (10.501 s), including live catalogue reuse and refusal of
foreign/missing catalogue roots. Six controller tests cover opaque selection, forged/mismatched
replies, duplicate suppression, cancellation, forgetting and stale observations. The rendered panel
covers escaping, busy controls, Hebrew labels and honest historical/unknown status. The first full
gate stopped on three needless-return lint errors in the new command wrappers; they were corrected.
The full rerun passed: 3,627 native tests (150.953 s, one slow, 17 skipped), 173 rendered tests,
607 desktop tests and 44 daemon-demo checks. No process-leak warning occurred; #172 remains open.
Hosted verification is pending.
Persistent editable profiles, remote execution/recovery controls, result pagination/import, packaged
signed-app/SSH/second-host acceptance and the complete fleet plan remain required. The fixed user
checkpoint has not changed.

### R76/R77 combined-main integration

R77 / PR #208 merged as `0a3b4cb57fbbbbf1f029b8713e467080fe44b26f` after all seven exact-head
checks in run 37117438917 passed. Combined-main run 37118015370 is pending. The exact sealed
`aca0980fb53c089e14b0bb1ff157437e4abaa97d` app passed the existing-project packaged journey:
attachment/capture, two pinned comparisons, independent line, restart/resume/detach and unchanged
Git. This is graphical regression evidence, not remote-control/SSH/provider/main-approval proof.
Manual inspection of the remote panel was blocked by the locked Mac; no security setting changed.

R76 / PR #207's original stacked run 37116690725 and main-targeted run 37117509243 both passed
all seven checks. It now integrates R77 main after those runs ended. Both ledger entries are retained;
no implementation conflict occurred. Full local verification of the combined tree passed: 3,629
native tests (151.067 s, one slow, 17 skipped), 173 rendered tests, 607 desktop tests and 44
daemon-demo checks. No process-leak warning occurred; #172 remains unresolved. Fresh hosted checks
are pending. Neither older passing run substitutes for the new integrated revision.

## R78: native in-app remote connection setup

R78 is new canonical work, replacing no legacy commit, based on PR #207's verified integrated
head `f94470c3ba4418945626e7b0d674adae6d4510c6`. PR #208 and its combined-main checks passed.
PR #207's fresh run 37118616368 passed all seven checks; it merged as
`79c736d80e20abdfc0391021425dd4ea4af033f4`. R78 targets main after a content-identical ancestry
merge. The fixed user checkpoint is unchanged.

The remote observation panel now accepts public worker/account/port/identity fields and exact
fleet/lane choices without requiring a hand-written configuration file. Native choosers retain
private file paths; native code supplies the app catalogue. Picker replies rotate opaque draft IDs.
Original file/parent identities and metadata are rechecked before applying settings. Clearing rotates
the draft, so late picker replies cannot restore it; the active connection remains unchanged until a
new complete selection succeeds. No key/trust creation, network connection or agent launch occurs
when applying settings. Native signed-app eligibility remains mandatory before file/custody access.

The first focused compilation found a duplicate module declaration in the new test scope; corrected
before rerunning. Twenty-three focused native tests passed (6.104 s), including changed files,
replaced folders, stale/cleared drafts, closed form fields and unsigned refusal. Ten controller tests
and 174 rendered/typecheck tests passed before the final copy update. The first full run stopped on
the extracted helper appearing after the test module; it was moved without relaxing lint. The full
rerun passed: 3,634 native tests (149.087 s, one slow, 17 skipped), 174 rendered tests, 611 desktop
tests and 44 daemon-demo checks. No process-leak warning occurred; #172 remains unresolved.
Hosted validation and a packaged setup journey remain pending.
This remains session-only setup. Persisted profiles, start/receive/reconnect controls, real remote
packaged acceptance and all remaining requirements of the full plan remain open.


## R79: explicit saved remote connection settings

R79 is new canonical work, replacing no legacy commit. It builds on R78 / PR #209's exact published
`d1e1c31cdae8dea28d861798a433999661ced66d`. All seven R78 checks passed in run 37119239231;
PR #207 combined-main run 37119199807 also passed. R78 merged as
`30065a1e536bedd3cf0d47c5a44ba67a06aab064`. R79 integrates that main merge with an unchanged
implementation tree from tested commit `2016b93`; only this delivery record changes afterward.
R79 publication and R78 combined-main verification are pending.

The native catalogue holds up to sixteen named settings records with expected-revision atomic writes,
owner-only files, original directory/file/coordinator bindings and explicit interrupted-save recovery.
The UI loads, opens, saves and removes settings through closed native operations. Private paths stay
native-only; reopening re-admits the original identities and files and fills the editable public form.
None of these operations contacts, launches or adopts a worker. Removal retains credentials, work
history and the active session. Unknown/foreign/linked/stale/partial data is preserved and refused;
recovery publishes only an exact next revision. Existing catalogues need no migration.

Initial focused verification passed: 31 native tests (7.700 s), fourteen controller tests and 175
rendered/typecheck checks. The complete local gate passed: 3,643 native tests (149.230 s, one slow,
17 skipped), 175 rendered tests, 615 desktop tests and the real daemon demo. No process-leak warning
occurred; reliability issues #37 and #172 remain unresolved. Hosted checks remain pending. Signed-app
profile restoration, packaged remote operations and all remaining fleet acceptance are still open.
The fixed R71 user checkpoint and existing dirty legacy checkout remain unchanged.


## R80: graphical continuation of retained input transfers

R80 is new canonical work replacing no legacy commit. It depends on published R79 / PR #210 head
`2cc394ba245702b3ace0cd789d6de5034cbff8b4`. R79 passed all seven original checks and merged as
`8137082bde3f4af9f651d97196b6ce25e03cc3aa`. R78 combined-main passed. R80 integrates the R79 merge
with the implementation tree unchanged from fully tested `45d7877`; only this delivery record changes.
R79 combined-main and R80 hosted verification are pending.
The explicit recovery button sends only an opaque native selection. History resolves the original
attached-project version or complete reviewed parent checkpoint. The current catalogue owner and
existing reconnect transport preserve original assignment, peer, lease, cancellation and saved-input
checks; no worker adoption, replacement attempt or automatic retry is added. Receipt wording does
not claim provider liveness. Missing sources/parent proofs or lost worker reservations remain errors.

Seventeen controller tests and 176 rendered/typecheck checks passed. The first native compilation
caught a missing generation field in a new test fixture; corrected without changing production rules.
The focused rerun passed all 34 native tests (8.304 s), including interrupted transfer and coordinator
restart with no execution adoption. The full local gate passed: 3,646 native tests (152.402 s, one
slow, 17 skipped), 176 rendered tests, 618 desktop tests, repository/docs/license/storage checks and
the real daemon demo. No process-leak warning occurred; #37/#172 remain open. Publication and hosted
checks are pending. R79's original run 37120274025 and R78 combined-main run 37120248363 both passed.
Real signed-app
remote recovery and the complete remaining fleet plan are not established by fixture tests.


## R81: bounded remote result pages and non-empty-page cursor fix

R81 is new canonical work replacing no legacy commit, based on published R80 / PR #211 head
`4f307e5f74343f8da180d9efb8ab7eb7357e5a30`. R80 run 37120867263 passed all seven checks and
merged as `df8e609a3adab251b876fa3c8e00d21ea208cde0`. R79 combined-main run 37120839893 passed.
R81 integrates the R80 main merge with a content-identical implementation tree to tested `e67ed4f`;
only this delivery record changes afterward. R80 combined-main run `37121395516` passed.
R81 PR #212 passed all seven exact-head checks (run `37121419464`) and merged ordinarily
at `01b6e04b34bcf6ccf0bfa9c31dec6f89630101aa`; combined-main verification is pending.
The old controller expected a returned cursor of zero, incorrectly refusing valid non-empty result
pages. The new bundled v2 observation validates returned cursor/request/count/revision relationships,
limits rows to sixteen, rejects duplicate identities and revision rollback, and retains the previous
verified page on failure. Native public summaries expose offer/checkpoint/version/review/manifest
identities only. Explicit previous/next controls replace the current page; no content is downloaded.
No persisted format changes or receipt/import authority are added.

Twenty controller tests, 177 rendered/typecheck checks and fifteen focused native panel tests passed
(3.481 s). The full local gate passed, including the signed-offer summary regression: 3,648 native tests
(152.481 s, one slow, 17 skipped), 177 rendered tests, 621 desktop tests, repository/docs/license/storage
checks and the real daemon demo. No process-leak warning occurred; #37/#172 remain open. Hosted
delivery is pending. Real signed-app/SSH/second-host acceptance and graphical receipt remain unfinished.


## R82: exact saved-input preparation for result receipt

R82 is new canonical work replacing no legacy commit, based on published R81 / PR #212 head
`d14683b4482bc10b1754000eccb34b708144ad62`. PR #212 is now merged after all seven checks passed,
and R80 combined-main passed. R82 integrates canonical main `01b6e04b34bcf6ccf0bfa9c31dec6f89630101aa`
with a content-identical implementation tree to tested `6924348`; only delivery documentation
changes afterward. R81 combined-main and R82 hosted delivery remain pending.
Historical result input lookup now supports exact recorded completed/cancelled/superseded attempts,
without weakening reconnect's latest/claimed/launching/initial-lease rules. Native preparation
exports the original attached version or exact reviewed parent source, validates both input version
and manifest against the assignment, and rereads the selector/assignment before returning. The
reconnect panel consumes this verified source rather than independently rebuilding it. No new
transport, execution authority, renderer paths, persisted format or automatic adoption is added.

Eight focused native tests passed (2.301 s), including actual coordinator restart/interrupted-transfer
continuation using a newly reconstructed immutable source after the original project changed. Added
combined-gate coverage checks substituted projects, cancelled/expired work and wrong assignment
manifests without mutating history or working files. Full local validation passed: 3,651 native
tests (155.722 s, one slow, 17 skipped), 177 rendered tests, 621 desktop tests, repository/docs/license/
storage checks and the real daemon demo. No process-leak warning occurred; #37/#172 remain open.
Hosted delivery is pending.
Graphical receipt destination/intent management and download controls remain required, along with
all remaining fleet acceptance. The fixed user checkpoint is unchanged.


## R83: native receiving inbox lifecycle

New canonical work, replacing no legacy commit, based on R82 PR #213 head
`10d9904ab32d611be61cb13c79d29976fb42b72c`. Its original hosted checks remain active.
Create-only native inbox provisioning and exact-identity reopening supply the storage prerequisite
for graphical remote receipt. The caller must retain the new inbox identity outside the inbox
before transfer. Canonical owner-only `mesh.remote-result-inbox/v1` records bind all three physical
folders; existing partial state is never adopted or repaired. Current protected roots are reapplied
when reopening. No transport, execution, original-project write or main approval is granted.
Four focused native tests passed (0.124 s): receipt/materialization after reopening, duplicate
creation, replaced store/root, changed receipt, protected parent and unknown partial setup.
Full local gate passed: 3,655 native tests (149.379 s, one slow, two process-leak warnings,
17 skipped), 177 rendered checks, 621 desktop tests, repository/docs/license/storage checks and
the real daemon demo. The warnings occurred in `a_daemon_with_nothing_open_refuses_state_rather_than_answering_zero`
and `stopping_during_signing_prevents_that_capture_from_being_committed`; #37/#172 remain unresolved.
Hosted delivery pending. Native receipt intents and graphical controls remain
unfinished; the entire fleet acceptance plan and fixed user checkpoint remain unchanged.


## R84: immutable native result receipt intents

New canonical work, replacing no legacy commit, based on published R83 PR #214 head
`7959fa253456e8a7d2a64cd8cbbb62a77eaa47b3`. R82 PR #213 checks and R81 combined-main have passed;
R83 original checks remain active. Native inbox records preserve the exact signed offer and native
configuration before I/O with a stable allocation. Physical-root locking serializes create-only
records; repeated exact selection returns the original intent. Changed context, partial records,
foreign inbox copies and overflow refuse without repair or transfer. Records never claim completion
or authorize a peer, worker launch or protected main. New format `mesh.remote-receipt-intent/v1`
is additive; existing inbox records remain compatible and no unknown state is migrated.
Focused restart/idempotency/refusal coverage passed (0.080 s). The full gate passed, including
bounded-inventory/cross-inbox-copy regressions: 3,657 native tests (148.724 s, one slow, 17 skipped),
177 rendered checks, 621 desktop tests, repository/docs/license/storage checks and the real daemon
demo. No process-leak warning occurred; #37/#172 remain open. Hosted delivery is pending. GUI receipt and native application identity/
intent integration remain required, together with the complete outstanding fleet acceptance plan.


R84 hosted correction: original run `37123118390` found a Linux unresolved signed-result import.
Intent implementation and exports now use the same macOS boundary as the existing signed-result
and ingestion APIs; the underlying inbox remains Unix-capable. No gate is weakened. The failed
job logs are retained. Original run terminated failed; its macOS job passed. Canonical main
was integrated with an identical implementation tree before this correction. Local revalidation
passed: 3,657 native tests (153.018 s, one slow, one process-leak warning, 17 skipped),
177 rendered checks, 621 desktop tests and all repository/docs/license/storage/demo gates.
The warning occurred in `mesh-approval` text-diff context coverage; #37/#172 remain unresolved.
Fresh hosted Linux/macOS validation is required.


## R85: catalogue-owned inbox identity

New canonical work replacing no legacy commit. Based on published R84 PR #215
`a6f439dcbf7e2b8b15f5c5d9076d05723aaa5f80`; canonical main
`d58178cf4e3e00bc9ed8e5d39ce440176e5e94f1` was integrated on this separate branch with an
identical implementation tree, preserving running PR checks. R82 PR #213 is merged; R83 PR #214
original seven checks passed; R84 original checks and R82 combined-main remain active.
The native catalogue now creates/reopens an inbox only against its independently retained
external binding, protects catalogue and every saved original-project identity, and refuses
orphaned/partial/substituted state. No new transfer, execution, source write or main authority.
Three focused tests passed (0.101 s): lazy first provision and catalogue restart, renamed detached
source protection, and preserved unbound/substituted inbox refusal. One unused import observed in
the focused compile was removed before the pending full gate. New binding format is additive;
no automatic migration, repair or adoption. Local full gate passed: 3,660 native tests (151.629 s,
one slow, 17 skipped), 177 rendered checks, 621 desktop tests, repository/docs/license/storage
checks and the real daemon demo. No process-leak warning occurred. Dependency PR #215 has a
Linux compilation failure because intent exports did not match the existing macOS signed-result
API boundary; correction and hosted delivery remain required. Desktop
receipt and the complete remaining fleet acceptance plan are still required.


R85 reconciled delivery base: PR #214 is retargeted to main at content-identical head
`c12ece07893e5cb057475a098ad2fba5eff6f6e4`; corrected PR #215 head
`674060bf03d85e3ba5518e85d0e93397bb226f63` is published and integrated here. Their original
runs are terminal and fresh hosted checks are required. R82 combined-main passed. Both ledger
sections were retained when resolving their append-only documentation conflict. Catalogue
full-gate revalidation on the corrected platform boundary is pending.

Second R85 full gate passed on the corrected dependency: 3,661 native tests (153.103 s, one slow,
17 skipped), 177 rendered checks, 621 desktop tests and all remaining gates/demo; no leak warning.
Desktop startup inspection then identified a normal readable application-data parent layout.
Inbox creation/catalogue admission now allow read/search permissions on the parent while refusing
group/other writes; receiving folders and records remain private. Added an actual 0755-parent
create/reopen/private-child regression and 0770/0777 refusal checks. Final full revalidation passed:
3,662 native tests (149.419 s, one slow, one process-leak warning, 17 skipped), 177 rendered
checks, 621 desktop tests and all repository/docs/license/storage/demo gates. The warning occurred
in `a_refused_open_is_published_too_so_a_subscriber_learns_about_it`; #37/#172 remain open.
Corrected PR #215 hosted Linux tests and lint passed; macOS and full hosted delivery remain pending.


## R86: graphical exact-result download and explicit recovery

New canonical work replacing no legacy commit, based on published R85 PR #216 head
`97226882a4c3c6894f38cc651896c05c7edec17a`. Corrected R84 PR #215 original fresh checks passed;
R85 original CI remains active. A bounded native cache retains the authenticated signed page,
while the renderer supplies only selection/offer identities. Native commands reuse the app-owned
catalogue/history, retain inbox binding and exact receipt intent before I/O, prepare original saved
input, then use existing guarded ingestion. Explicit saved-attempt listing/recovery survives a
connection reopen; completed replay verifies local copies. Verified completion links to the
existing fleet result queue and parallel pinning. No paths/offers/manifests/allocations from the
renderer, automatic retries, execution, original-folder writes or main approval are introduced.

Twenty-four controller tests and 178 rendered/typecheck checks passed. Eighteen focused native
panel/ingestion tests passed (4.566 s), including actual host inbox reopening, owner reuse and
unsigned-app refusal before storage access. Full gate passed: 3,664 native tests (150.785 s,
one slow, 17 skipped), 178 rendered checks, 625 desktop tests and all repository/docs/license/
storage/demo gates. No process-leak warning occurred; #37/#172 remain unresolved. Hosted
delivery pending. Existing storage formats are
unchanged; native/UI receipt messages are paired v1 additions. Real signed packaged/second-host
SSH acceptance remains required, along with the complete outstanding fleet plan. The fixed user
checkpoint is not replaced.


R86 delivery reconciliation: R83 PR #214 merged ordinarily at
`a443da3dff36bc7dcbe6354f04c01d8baa7b91eb` after all seven exact checks passed. Corrected
R84 and R85 original CI passed. PR #215 now targets main at `1cb08ea115e9f48647fcaccec0a94badd351fbd0`;
PR #216 retains its dependency at `a0007a7638eb853f666711c636f1816006adc545`. Both ancestry
updates preserved their tested implementation trees and start fresh CI. R86 integrates that stack
with a tree identical to tested implementation `9c37676`; only this delivery record changes afterward.
R83 combined-main and the new exact-head PR runs remain pending.

## R87: retain original remote creation inputs before side effects

New canonical work replacing no legacy commit, based on published R86 PR #217 at
`d214e3454514e458c96f5697a9da10440d00b5d1`. Tracks fresh desktop start/recovery in
[issue #218](https://github.com/idosams/Mesh/issues/218). The native attachment catalogue now
retains immutable private creation inputs before allocation/custody/transport; the current native
start command uses it. An exact repeat is idempotent, changed inputs refuse. Records bind physical
catalogue identity, private bounded single-link files and the original 32-character creation key.
Capacity is 64 within the existing 256-entry catalogue discovery bound. No deletion, migration,
automatic retry, lease refresh or execution authority is introduced. Existing created inspection
remains read-only. Graphical setup/recovery and app-owned catalogue integration remain next steps;
all remaining fleet acceptance requirements remain open.

Focused verification passed six tests (0.527 s), including restart, changed input refusal,
partial/public/linked/copied/renamed/oversized records and capacity preserving exact replay.
An unused import observed during that compile was removed before the full gate. The original
focused process was preserved through completion. Full local and hosted validation pending.
PR #215 exact CI and R83 combined-main passed; dependent #216/#217 runs remain active.

R87 full local gate passed: 3,668 native tests (157.046 s, one slow, 17 skipped), 178 rendered
checks, 625 desktop tests and all repository/docs/license/storage/real-daemon demo gates. No
process-leak warning occurred; #37/#172 remain unresolved. Full log retained separately. R86
PR #217 original hosted checks passed; R85 #216 macOS CI remains active. Hosted validation of
this new increment and the complete graphical/remote acceptance remain required.

## R88: desktop fresh remote input creation and explicit inspection

New canonical work replacing no legacy commit, implementing the next part of #218 on published
R87 PR #219 `fb497eefb1fb6bf0c55525a0d0019b29490fc4d0`. A separate local branch integrated verified
canonical main `692b536d4e3ec31c01f69c5cfe87415dd50dca3b` with an identical tree before edits;
published heads and their original CI were not mutated. Publication will retain the dependency.

A native peer-only setup path enables preparation without an existing run. Native code fixes and
retains request/configuration/bindings/deadline before fleet allocation or network I/O. Explicit
send uses the app-owned catalogue and shared native transfer helper; already-dispatched attempts
refuse. Request listing and explicit original-attempt inspection support restart/lost-output
recovery without dispatching, renewing or adopting processes. Exact inspection opens the original
selection for existing status/reconnect controls. Public fields are validated and private fields
stripped; duplicate in-flight sends and uninspected restart sends are suppressed. English/Hebrew
controls distinguish confirmed input transfer from provider execution. Persisted request journal
format is unchanged; paired desktop configuration and public envelopes are additive v1 schemas.

Native compile passed. Twenty-eight controller tests, 179 rendered/typecheck checks and 37 focused
native tests (16.056 s) passed. Coverage includes fake-path refusal, peer setup without a run,
changed trust/stale draft, catalogue-owner reuse, journal restart, lost reply/duplicate suppression,
cross-request replies, escaped/localized views and unsigned refusal before storage/network.
Full gate pending. No signed packaged/real second-host or remote provider execution claim is made.
Missing assignments, expired leases, lost reservations and all prior acceptance gaps remain open.

R88 full gate passed: 3,672 native tests (155.235 s, one slow, 17 skipped), 179 rendered checks,
629 desktop tests and all repository/docs/license/storage/real-daemon demo gates. No process-leak
warning occurred; #37/#172 remain unresolved. Final presentation-only copy clarifies the existing
worker service's execution queue after materialization and exposes original request/worker IDs;
controller, rendered/typecheck and docs checks validate that clarification separately. No runtime
behavior changed after the full gate. Hosted publication and actual signed/second-host acceptance
remain pending.

R88 delivery reconciliation: R85 PR #216 merged ordinarily after all seven exact checks passed,
main `68e7e69e16639f927e689d3d2074721a5a719de5`. R87 #219 original CI and R84 combined-main
passed. R86 #217 now targets main at `089440f4d34943f61f770dca3776aa78fc4fb0c8`; R87 #219
uses that parent at `8f7fce6eec878b8a5d1679d5e327de7920be0cff`. Both preserve their previously
checked implementation trees and require fresh CI. R88 integrated the published parent with a
tree identical to locally verified `1c35c49`; only this delivery record changed afterward.

## R89: received-result review independent of fleet polling

New canonical work replacing no legacy commit, based on R88 PR #220 head
`59961a67d4e01f42751343a8f4b36f7f121dd341`. Corrects the download-to-review integration gap:
`remote-results` was silently ignored unless cached fleet status admitted its objective. The
existing native history reader now verifies the explicit bounded selection independently; queues
render even before their fleet card loads. Arrival of status avoids a duplicate list, while existing
pins remain fixed. Unavailable native history is visible and refuses pinning; no agent action,
storage creation, native authority or persisted-format change is introduced. Related to #163/#218.

Focused verification passed 31 controller/persistence tests and 180 rendered/typecheck checks.
Regressions cover missing/failed/unavailable fleet polling, no dispatch/repair, malformed selectors,
native failure, sixteen-queue capacity, unchanged existing pins, fallback visibility and localization.
Final rendered assertions additionally cover enabled verified pins, duplicate-list prevention and
failed-read refusal. Full local and hosted verification pending; signed packaged/real second-host
review and the complete remaining fleet objective are still required. Published CI remains intact.

R89 full local gate passed: 3,672 native tests (154.891 s, one slow, 17 skipped), 180 rendered
checks, 632 desktop tests and all repository/docs/license/storage/real-daemon demo gates. No
process-leak warning occurred; #37/#172 remain unresolved. Full log retained separately. R86
PR #217 fresh hosted checks passed; dependent #219/#220 and combined-main remain pending at
last inspection. This increment's hosted checks and all actual signed/second-host acceptance
remain required.

## R90 — durable acknowledged input allocation identity

New canonical implementation; replaces no legacy commit. Depends on R89 / #221 and tracks #222.
The receiving session records its original admission and native parent/allocation/files identities
in an immutable `mesh.remote-materialization/v1` ledger event before handing materialized input
to the broker. Bounded readback survives restart without reopening files or reconstructing a
reservation. Exact repeats retain one record; conflict, invalid context and malformed history
refuse. Earlier admissions are not backfilled. Missing records remain uncertain.

Focused receiving regressions passed (8 tests, 0.313 s). Full `npm test` passed with host
permissions: 3,674 native tests (156.092 s, one slow, 17 skipped), 632 desktop tests, rendered
checks and all repository/docs/license/storage/real-daemon demo gates. The restricted run was
retained after socket permission and native capture failures; no code or assertions changed
between those runs. Publication and fresh hosted checks remain required. Restart reconciliation, pre-launch initialization recovery,
uncertain provider execution, capacity release and signed real-host acceptance remain required.

## R91 — inspect acknowledged input after restart

New canonical work, replacing no legacy commit; depends on R90 / #223 and tracks #222. An
independent branch from main preserves the published dependency's tested source and CI. Native
inspection reopens only the recorded parent/allocation/files identities, validates the bounded
private manifest against the original assignment and reuses full input verification. No
admission reservation is reconstructed, and no file or ledger mutation occurs.

Three focused regressions passed (0.491 s): restart readback without initialization/launch,
byte-identical directory substitution refusal, and changed content/invalid manifest preservation.
Full `npm test` passed: 3,677 native tests (150.711 s, one slow, 17 skipped), 180 rendered
checks, 632 desktop tests and repository/docs/license/storage/real-daemon demo gates.
Publication and fresh hosted checks remain required. Native graphical reconciliation and
complete initialization/provider/second-host recovery remain required.

## R93 — explicit signed retained-input inspection

New canonical implementation; no preserved source commit is replaced. Depends on merged R90/PR223
and R91/PR224 and follows the R92 acceptance-map documentation. Adds status protocol v3 with
distinct query/reply signing domains and closed `input_inspection` facts. The explicit native SSH
request and resident worker endpoint verify original retained input before and after signing.
Malformed ledger evidence refuses; missing receipts remain unrecorded uncertainty; changed or
unavailable storage reports unavailable. None grants allocation, process ownership, retry, lease
renewal or capacity release. No persisted format changes.

v1/v2 encoding and ledger-only behavior remain unchanged. Old replies cannot satisfy a v3 request;
old workers refuse the unsupported schema. Input verification is intentionally explicit because it
reads the bounded complete input tree twice and can exceed the existing 30-second freshness window.
Expiry remains a refusal, not a retry instruction. The worker reports the originally materialized
input, not a later saved result or current provider liveness.

Focused status and endpoint suite passed 45 tests in 6.261s. Initial compile and test-authoring
failures are retained in delivery evidence. Full local `npm test` passed: 3,682 native tests in 152.638s (one slow, 17 skipped),
desktop checks and all 44 daemon demonstration checks. Fresh hosted checks remain pending.
Desktop presentation, signed packaged and real-host acceptance, interrupted initialization
and uncertain launch reconciliation remain required.

## R94 — explicit desktop and CLI original-input inspection

New canonical implementation depending on R93/PR226; replaces no preserved source commit. Adds a
separate native observation kind, worker-panel action and CLI `inspect-input` route. Fresh v3
replies revalidate the exact attempt after I/O outside the service lock. The renderer receives only
a timestamp and closed disposition, never private paths or execution authority. English/Hebrew
copy distinguishes input verification from process liveness and safe restart. Ordinary status and
saved-result observations remain independent; selection changes clear the inspection. No persisted
format changes. The existing panel envelope gains an explicit kind; unsupported old renderers
refuse it. Focused controller/rendered tests passed 40 tests; focused native observation tests
passed 40 tests in 10.853s. Initial compile failures are retained. Full `npm test` passed
3,683 native tests in 152.623s (one slow, 17 skipped), 181 rendered checks, 634 desktop tests
and all 44 real-daemon checks. Fresh hosted checks and actual signed packaged/real-host acceptance
remain pending.

## R95 — exclusive received-workspace initialization ownership

New canonical implementation following R94/PR227; replaces no preserved source commit. An
independent nonblocking native directory lock now serializes initialization of one physical input
allocation, including callers sharing the same worker installation. The original workspace retains
that owner through its native session lifetime, and revalidates the original directory on use.
Competing initialization refuses before intent or workspace writes. Read-only inspection is unchanged.

This is a prerequisite for purpose-specific restart recovery, not a recovery or relaunch API. Lock
release alone proves neither process termination nor safe retry. Existing launch/admission records,
leases, immutable inputs and preserved partial initialization must still govern any future recovery.
No persisted format changes or lock files are introduced; existing workspaces are not adopted.
The final focused received-workspace/host suite passed 19 tests in 2.694s. Full `npm test`
passed 3,686 native tests in 150.125s (one slow, 17 skipped), 181 rendered checks, 634 desktop tests
and all 44 real-daemon checks. Nextest reported one LEAK warning in the unrelated CAS round-trip
`every_size_promotes_and_reads_back_byte_for_byte` test; its log is preserved and the reliability
investigation remains open. An earlier version also passed its complete gate before removal of a
redundant pre-lock content scan. Fresh hosted validation, interrupted-initialization recovery and
full real-host acceptance remain pending.

## R96 — retain failed received initialization for recovery

New canonical implementation following R95/PR228; replaces no preserved source commit. Received
worker import used the ordinary project's rollback path, including cleanup on a dropped prepared
handle and on errors after journal ingestion. This could remove the partial worker copy and durable
history that purpose-specific interrupted-initialization recovery needs to inspect.

The native received-import purpose now retains created directories, copied content, pending markers
and any private history on failure or drop. Explicit rollback on that prepared received handle
refuses. Ordinary project imports retain their existing rollback behavior. Successful initialization
still performs all exact-content, physical-identity and receipt checks. No persisted format changes,
launch authority or automatic retry are added. Retained paths remain unavailable to create-only
initialization; native recovery must validate and continue the original attempt. The existing generic
`recover_pending_import` remains a rollback API and must not be used to resume received work.

Five new fault regressions cover dropped preparation, copy failure, changed working content,
post-journal confirmation failure and explicit prepared-handle rollback refusal. The focused import
security/received-workspace suite passed 23 tests in 0.880s. Full `npm test` passed 3,691 native
tests in 150.512s (one slow, 17 skipped), 181 rendered checks, 634 desktop tests and all 44 real-daemon
checks. No LEAK warning was reported in this run; it does not resolve the earlier reliability
investigation. Fresh hosted validation remains pending. Purpose-specific partial initialization completion, authenticated recovery controls and the
full fleet acceptance map remain unfinished.

## R97 — exact received initial-history completion

New canonical implementation following R96/PR229; replaces no preserved source commit. Received
initial ingestion now derives the complete canonical initial journal before writing it, accepts only
an exact existing prefix, verifies/promotes all required payloads through the existing durable commit
checks in a transient index, and appends only the missing suffix. A complete initial journal is not
appended again. Conflicting bytes or later records refuse without truncation or replacement.

The existing file is opened without create/truncate authority or blocking on a substituted FIFO;
regular-file, private permissions, single-link and exact descriptor identity checks bind the observed
prefix through append and final verification. Namespace changes refuse. Ordinary import ingestion
retains its existing behavior. Journal/receipt formats are unchanged.

Five new regressions include every byte cut of a real generated initial journal, repeated completion,
changed prefixes/later tails, linked/shared files, replaced generations and missing payloads. The
focused import/received-workspace suite passed 28 tests in 3.379s. Full `npm test` passed 3,696 native
tests in 158.456s (one slow, 17 skipped), 181 rendered checks, 634 desktop tests and all 44 real-daemon
checks. Nextest reported one LEAK warning in the existing background-capture test
`stopping_during_signing_prevents_that_capture_from_being_committed`; its log is preserved and the
reliability investigation remains open. Fresh hosted checks remain required. This is private initialization machinery, not an authenticated restart operation: original
admission/lease/launch checks, partial working-file completion, native workspace reconstruction,
receipt reconciliation and graphical recovery controls remain required by the full acceptance map.

## R98 — retained received-file prefix completion

New canonical implementation following R97/PR230; replaces no preserved source commit. The native
received-copy path can complete an existing exact input-file prefix by appending only its missing
suffix. It validates the complete original source and both file identities, compares the prefix
again through the writable descriptor, and verifies final source/content/identity before success.
Private canonical modes, a single regular-file link and the original source executable state are
required; interrupted executable-mode finalization is applied only to the original output inode.
Copies use fixed-size buffers rather than loading a potentially large received file into memory.

Conflicting or extra bytes, changed source, replaced paths, links, unexpected executable state and
noncanonical permissions refuse without truncating or replacing retained work. Ordinary import
collisions keep their existing refusal. No persisted formats or launch authority change. This is the
copy completion machinery needed by restart recovery; reopening original prepared-workspace markers,
index/receipt reconciliation and the authenticated original-attempt resume operation are still pending.

Seven new regressions cover every byte cut, repeated completion, executable and empty files, buffer
boundaries, conflicts, links/permissions, same-inode edits and replacement during inspection, changed
source/displaced parent, and ordinary-import refusal. The final focused suite passed 35 tests in
3.422s, including special-permission refusal. Full `npm test` passed 3,703 native tests in 151.832s
(one slow, 17 skipped), 181 rendered checks, 634 desktop tests and all 44 real-daemon checks. Nextest
reported one LEAK warning in the existing checkpoint-storage test
`a_manifest_cannot_enter_metadata_when_its_chunk_is_absent`; the original log is preserved and the
reliability investigation remains open. Fresh hosted checks and the full fleet acceptance map remain
required.


## R99 — require the corrected concurrent test runner

New canonical workflow correction based on merged R98/PR231 at
`33af10c7022a8b4e97c056424579329a1420e9b1`; replaces no preserved implementation commit.
The local runner was nextest 0.9.143. Upstream nextest 0.9.145 fixes inherited sibling capture
pipes on Apple platforms ([upstream PR #3553](https://github.com/nextest-rs/nextest/pull/3553)),
which can falsely report a test as leaking output handles. Mesh now declares 0.9.145 as its
minimum runner version, using nextest's own version check, and documents the setup requirement.
Hosted installs already use current nextest. No assertions, deadlines, leak settings, retries or
skips change; no product lifecycle defect is claimed fixed.

The official universal macOS 0.9.145 archive was verified against its published SHA256
`52ecaedb4f5af9267ef7ed02bc937d2a15a94ff96cb663080e81311f798c9905` and run from a separate
temporary location, preserving the existing 0.9.143 installation. On merged PR231, all 3,703
native tests passed in 153.495s (one slow, 17 skipped), without a leak warning. This is one native
suite observation, not full packaged/fleet acceptance or proof of all historical warning causes.
The original logs and tool identities are retained; #172 and #37 remain open. The built-in version
check refused 0.9.143 with exit 92 and accepted 0.9.145 with exit 0. An initial probe without Cargo
on PATH exited 102 and was retained, then corrected without changing the version requirement.
Full `npm test` passed on this increment with the isolated fixed runner: 3,703 native tests in
160.781s (one slow, 17 skipped), 181 rendered checks, 634 desktop tests, and all 44 real-daemon
checks; no leak warning. All seven checks also passed on the preceding merged PR231 main
([run 37140182566](https://github.com/idosams/Mesh/actions/runs/37140182566)). Fresh hosted
checks on this increment and its eventual combined main remain required.

## R100 — complete received-import finalization in place

New canonical implementation following R98/PR231 and the R99/PR232 runner correction;
replaces no preserved source commit. Received imports can now finish an exact canonical
confirmation-receipt prefix in place and recognize an existing empty derived-index placeholder.
The helper uses pinned directories, private mode 0600 and single-link regular files, compares
both bytes and inode through an independent writable descriptor, appends only the missing
suffix, syncs and verifies the completed record and namespace. It never truncates or replaces
an existing file. Populated indexes, conflicting receipts, links, changed permissions, replaced
files and displaced parents refuse while preserving retained work and pending ownership.
Ordinary user imports remain create-only. Persisted receipt formats are unchanged.

Nine regressions cover every real import-receipt byte cut, successful confirmation after
completion, repeated original ingestion without changing its journal or index inode, conflicting
finalization with retained ownership, ordinary-import collision refusal, and generic prefix,
empty-index, mode/link and replacement races. The focused folder-import suite passed 36 tests
in 3.273s. Full `npm test` passed: 3,712 native tests in 156.505s (one slow, 17 skipped),
181 rendered checks, 634 desktop tests and all 44 real-daemon checks. No leak warning was
reported. Fresh hosted checks and combined-main verification remain required.

This completes the low-level file/journal/index/import-receipt continuation mechanisms; it is
not the native original-attempt resume operation. Reopening original pending/confirmed import
ownership, completing the worker mapping receipt, guarded admission/current-lease checks,
pre-launch handoff, authenticated desktop recovery and uncertain-execution reconciliation
remain in the full acceptance plan. Populated derived indexes require the confirmed-workspace
reopen path; they are never reset by this unconfirmed-ingestion helper.


## R101 — retain original directories through import handoff

New canonical implementation following R100/PR233 at
`18357b0980beb0d63e0293aaf365e4cfd2c0a849`; replaces no preserved source commit.
Confirmed saved-work forks and received-worker imports retain their working and private-store
directory descriptors through recovery-database quiescence and durable workspace reopening.
Previously that handoff reopened the private index and recovery database through the displayed
pathname after dropping prepared import authority. The new private path uses a verified native
directory reference, while the original displayed directory remains a separate identity guard.
The returned workspace retains that guard for subsequent access and is checked again before
installation into the live daemon. Unconfirmed preparation still uses its transient index.

SQLite recovery paths preserve only recognized live native directory references: macOS volume/inode
references or Linux process-descriptor references whose descriptor remains held by the workspace.
Ordinary paths retain their existing canonicalization. Recovery/index alias checks also compare
parent directory physical identities when filenames match, so different spellings cannot admit
the same database even before it exists. This changes no persisted format, provider admission,
lease or protected-main authority. Native paths are an I/O binding, not execution authorization.

Six regressions cover descriptor lifetime through directory rename, durable index reopening,
refusal of an otherwise valid replacement workspace before mutable open, continued storage
identity checking even when the original working directory is restored, native recovery access
after rename, and recovery/index physical aliases. Fourteen focused tests passed in 0.863s.
Full `npm test` passed 3,718 native tests in 153.758s (one slow, 17 skipped), the rendered and
desktop suites and all 44 real-daemon checks, without a leak warning. After that full run, the
replacement regression was strengthened to use a complete independent workspace and passed
again in 0.252s; production implementation was unchanged. Original failed development logs are
retained, including the discovery that canonicalizing native references broke received imports.
Fresh exact-head hosted checks and combined-main verification remain required.

This is the safe handoff needed by original-attempt recovery, not a complete resume operation.
Reconstructing pending/confirmed ownership from bounded pinned receipts, guarded registry/current
lease checks, worker mapping completion, authenticated recovery UI, uncertain-process reconciliation
and the full real-host fault journey remain in the acceptance plan. The fixed user checkpoint at
`13fedd9f94706fc0f3ac6bc7f5e6d6302735faa8` does not include this source increment.


R101 hosted correction: original PR234 Linux run `37143422615` failed received-worker
initialization (998 tests passed before two failures stopped that job). The bundled SQLite unix
VFS resolves `/proc/self/fd` links in its full-path callback, which conflicts with the existing
`SQLITE_OPEN_NOFOLLOW` recovery opens. Merely dropping that flag would also leave SQLite using
an ordinary resolved pathname. The original failed run is preserved; this revision remains unmerged.

The correction registers a process-lifetime copy of SQLite's bundled unix VFS with only its
full-path callback specialized for a validated `/proc/self/fd/<fd>/<single filename>`. The native
caller retains the directory descriptor; the callback requires that it still names a directory,
rejects traversal, malformed descriptors and linked/nonregular/multiply linked database leaves,
and bounds the output buffer. All other paths use the original callback. SQLite's native file,
locking, sync, journal and WAL operations remain intact, including `O_NOFOLLOW`, and recovery
opens keep `SQLITE_OPEN_NOFOLLOW`. Both the ordinary index and recovery/read-only connections
select this adapter for Linux descriptor references. Registration is serialized and never replaces
the process default VFS. The small FFI boundary documents buffer and registration lifetimes.

New regressions keep two WAL connections and their sidecars in the original directory across a
rename/replacement, verify retained contents after reopen, and refuse database leaf links. A
cross-platform callback test also verifies ordinary-path delegation and symlink refusal. Its first
macOS run correctly refused the test's `/var` alias; the test now supplies the canonical ordinary
parent, as production recovery does. That failed test log is preserved. Updated full validation
and a fresh exact-head hosted Linux run are required before this correction is delivered.

The corrected focused macOS run passed all four applicable tests in 0.054s. Full `npm test`
also passed 3,720 native tests (one slow, 17 skipped), 181 rendered checks, 634 desktop tests
and all 44 real-daemon checks, without a leak warning. This validates the shared FFI adapter
and unchanged macOS path; Linux-specific descriptor execution still requires fresh hosted proof.
The original PR234 run is terminal: macOS and five other checks passed, Linux failed. The
preceding merged PR233 main run `37143376511` passed. Neither original run was restarted.


## R102 — native continuation of original worker initialization

New canonical implementation based on merged R101/PR234 at
`51babc5f47ff53d477de24b8a5aedd4b020095e0`; replaces no preserved source commit.
A freshly authenticated native receiving connection can continue its acknowledged original
initialization. An independently reopened guarded ledger, original materialization receipt,
current effective lease and absence of any launch record are required. The original allocation's
exclusive initializer lock is held before mutable work and retained by the returned workspace.
An active input-transfer reservation is refused intact; inspection-only receipts remain read-only.

The operation reopens recorded parent/allocation/input identities and verifies the original input.
It completes exact intent prefixes and reconstructs pending imports from complete canonical,
private ownership markers read through retained directory handles. Partial copies, initial journal
and confirmation receipts use the existing in-place continuation rules. Confirmed imports require
an exact canonical receipt and matching original work, then use R101's pinned durable handoff;
a populated derived index is never sent through unconfirmed ingestion. Both private directories
must retain mode 0700. Missing or incomplete ownership, changed roots, conflicting bytes and later
user edits are preserved and refused. The exact original worker mapping is finished in place.

Registry, lease, launch absence and allocation identities are checked again across phases, and
native input/content checks run again before return. The operation grants no process adoption or
launch permission; ordinary reserve_launch remains the atomic single-intent boundary. No persisted
format changes. Repeated completed recovery retains the same installation, initial operation and
receipt; a live prior owner excludes another initializer.

The first connected focused run passed 23 tests, including authenticated recovery before import,
partial copy/journal, confirmed workspace, torn worker receipt, repeated recovery, busy ownership,
unguarded/unauthenticated refusal, existing launch, changed user/input content, directory replacement,
permissions and conflicting receipts. The first fixture used an obsolete working-folder name; its
failure log was retained and corrected to the repository constant. The additional active-transfer
regression passed (five recovery tests total). The first full gate stopped on a needless-borrow
lint; its log is retained. After correction, `npm test` passed 3,725 native tests (one slow,
17 skipped), 181 rendered checks, 634 desktop tests and 44 real-daemon checks, without leak
warnings. Review then found the shared retained-file reader in a macOS-only module; it was
moved unchanged into the common worker module for Linux compilation. The final portability correction passed the same complete local gate: 3,725 native tests
in 157.390 seconds (one slow, 17 skipped), 181 rendered checks, 634 desktop tests and
44 real-daemon checks, without leak warnings. Hosted Linux/macOS checks and merge remain pending. The test authority retains and checks actual directory and
ledger identities; this is native integration evidence, not a packaged remote-host proof.

This source increment is not a desktop resume button or a complete broker recovery protocol.
A dedicated versioned recovery challenge must support the current renewed assignment while retaining
original admission provenance; the existing admission challenge binds the initial assignment and
cannot authenticate a renewed one. Native socket/frame routing, user recovery controls, expiry and
cancellation fault coverage, every-write/process-kill campaigns, uncertain launched processes and
real second-host reconnect acceptance remain required. The full acceptance map is unchanged.

## R103 — fresh proof for original recovery under a renewed lease

R102/PR235 merged at `5a40f181c7e5f07e52653bf36f139e7407a9226b` after all seven
exact-head checks passed on `85ca27d1ce3cf30ef8d3522e378248e92dfc301f`. Hosted Linux
passed 3,329 tests (293.163 seconds, two slow, seven skipped); macOS passed 3,725 native
tests (190.359 seconds, one slow, 17 skipped), the complete desktop/daemon gate and four
native renderer tests. Combined-main run `37146827118` subsequently passed all seven checks.
R103 is new canonical implementation on that merge and replaces no preserved source commit.

A separate bounded `mesh.original-recovery-challenge/v1` proof and mutation-specific signature
domain bind the immutable original admission/revision and materialization directories to the exact
current effective lease, a random worker nonce and an expiry of at most 30 seconds. Original
admission v1 and its sequence-one validation remain unchanged. A single-use native challenge owns
the guarded original registry and invokes R102 recovery only after signature verification; proof
expiry, exact lease equality, original materialization and launch absence are rechecked during
recovery phases. A renewal during recovery requires a new challenge. No input reservation is
recreated and no launch intent or capacity release is granted.

The coordinator compares the proof with the exact current claimed prelaunch attempt, input/bundle,
worker, provider, task, limits and original admission scope. Its signing callback is checked both
before and after execution against the full selected lane and limits, including cancellation and
lease changes. The proof is task-bearing and must not be logged. Decoding/cloning facts cannot
reconstruct worker authority; noncanonical/unknown envelope fields or versions refuse.

Six connected proof tests passed: renewed initialization preserves the original admission/history,
simulated authentication after the initial deadline uses only the renewed current lease, nonce
replay/wrong signer/stale lease refuse before initialization, cancellation during signing discards
the signature, and malformed proof/wrong native scope/unguarded ledger refuse. The signer-time
lease-change regression passed too. The combined focused set passed all 11 recovery tests
in 2.682 seconds. The complete local gate passed 3,731 native tests in 157.525 seconds
(one slow, 17 skipped), 181 rendered checks, 634 desktop tests and 44 real-daemon checks,
without leak warnings. Hosted checks and merge of this increment remain pending. The simulated clock test is not a real delayed
network or second-host recovery proof.

Broker/native socket routing, preserving recovered handoff ownership across lost acknowledgments,
supervisor integration, desktop recovery controls and cancellation delivery during remote work
remain required. A fresh bounded signature cannot prove instantaneous knowledge of a later remote
cancellation. Existing launch intents require separate uncertain-execution reconciliation. The fixed
user checkpoint is unchanged, and the full fleet acceptance map remains open.

## R104 — recovered workspace ownership through the native supervisor

This is a dependent canonical increment on published R103/PR236 at
`2472a89454ed05a646fc31189d0668e6b99838b2`. Its parent PR has passed six hosted checks, including Linux; macOS remains running
at this entry. It replaces no preserved source commit.

`RemoteRecoveredHandoff` retains the authenticated original workspace's exclusive initializer
ownership and guarded registry. `ReceivedWorkerHost::start_recovered` consumes that ownership
through the existing atomic `reserve_launch` boundary before provider startup. First-time input
initialization shares the same final path; no launch checks are bypassed or receipt-to-reservation
conversion added. The supervisor admits the original worker identity and capacity and records its
slot before effects, retaining that slot after failed startup. Existing or uncertain resident entries
refuse another start.

The bounded resident mailbox accepts an explicit native-only recovered start request. Full or
closed delivery returns the original request and held workspace. A lost/full startup reply does not
remove the provider owner or stop resident observation. Neither connection loss, a free initializer
lock, nor an expired lease is used as evidence of process termination or capacity release.

Three focused native regressions passed in 1.047 seconds: a real local fixture process starts exactly
once and finishes after startup-reply loss/control disconnect; mailbox backpressure returns the same
verified workspace without startup; and signer failure retains the slot plus single launch intent
while preserving original input. This is fixture-provider native integration, not a successful real
Codex/Claude or second-host recovery run. The full local gate passed 3,734 native tests
in 157.288 seconds (one slow, 17 skipped), 181 rendered checks, 634 desktop tests and
44 real-daemon checks, without leak warnings. Hosted checks and merged delivery remain pending.

The authenticated recovery wire exchange, worker-connection routing, coordinator action and desktop
controls remain required to expose this path to users. The fixed user checkpoint stays unchanged.

## R105 — authenticated original-recovery wire and native worker routing

This dependent canonical increment uses R104/PR237 at
`b527a623b5db7db1d8a2569b718c31dac4449662`. R104's original run `37148099153` passed
all seven checks; its unchanged source tree was reconciled with merged R103/main and retargeted
to main. Fresh exact-head run `37148801347` passed all seven checks. PR237 merged normally
at `b169f6e9a08aac9421bcdc3b10397eb855a84ece`; R105 was reconciled with that main
through a related-history merge with full source-tree equality. R103/PR236 merged normally
at `92c0caf5a08e22b25155610386c1a7884bfaffae`; its combined-main run `37148640109` passed.
R105 is new canonical implementation, replacing no preserved source commit. Preserved working
commit `9cc6db081e6ef8bd2a2a9149852d694864c61ce0` and related-history reconciliation
`26a446b69d9395574758b60c42e9f6983a83205c` retain its implementation history.

The bounded wire exchange binds a fresh signed coordinator request, worker-signed R103 proof,
coordinator recovery signature and worker-signed receipt to the exact original assignment. Closed
canonical v1 schemas reject unknown fields and oversized control frames. Worker policy, original
registry limits and current effective lease are independently checked. Coordinator cancellation and
full lane/limits changes are checked around signing and exchanges. Original admission encoding and
persisted formats are unchanged; older endpoints refuse the new distinct recovery request. Native
worker connections require explicit recovery opt-in and independently installed roots and keys.

Recovery retains the R104 exclusive handoff even if the final receipt signature/write fails.
Native mailbox backpressure retains the same request; repeat recovery/dispatch refuses while the
connection owner holds the original assignment. Launch still passes the original atomic intent and
lease checks. A local reply write is not peer acceptance, and the signed receipt observes recovered
initialization, not provider execution. No receipt adoption, automatic execution retry or uncertain
capacity release is introduced.

Eight focused regressions passed in 2.337 seconds: malformed/unknown/noncanonical requests,
independent policy and original-limit checks, stale requests, renewed-lease mapping preservation,
modified worker proofs/final receipts, cancellation during either coordinator signature, absent or
invalid commit, native opt-in refusal and real local fixture-process handoff after final-reply loss.
The native endpoint test retains ownership through closed-mailbox delivery and starts the fixture
once after delivery becomes available. This is native fixture integration, not real-provider, SSH,
Developer ID or second-host acceptance. The full local gate passed 3,742 native tests in 160.750 seconds (one slow, 17 skipped),
181 rendered checks, 634 desktop tests and the real-daemon checks. Hosted validation and
merged delivery remain pending.

The coordinator SSH/native application action and desktop recovery controls remain required.
Cancellation delivery during remote work, uncertain-execution reconciliation, the full fault campaign
and packaged second-host acceptance remain open. The fixed user checkpoint is unchanged.

## R106 — explicit native coordinator recovery action

This canonical increment depends on published R105/PR238 at
`7eeae97f52e684b7f0b531398f54dbbf8a9db8ad`; it replaces no preserved source commit.
The parent exact-head run `37149462676` has six passing checks; macOS remains running.
PR237 combined-main run `37149424198` passed all checks.

The native `recover-original` coordinator command selects the original attempt from the existing
closed private configuration and uses the admitted SSH destination plus eligible native signing
custody. FleetHistory opens a separately guarded connection, releasing the service mutex before
transport and signing so live views and cancellation continue. The wire client refreshes that
connection around signatures and exchanges. No generic runtime escapes, dispatch retry, input
resend, lease renewal, uncertain-capacity release or protected-main authority is added.

Ten focused tests passed in 4.603 seconds, including service-lock availability during exchange and
signing, unchanged history after disconnect and cancellation through the service during signing
preventing request output. Two earlier failed fixture runs are retained: the initial tests used
unguarded stores and correctly refused; the second still used the old call sites. Production
authority checks were not relaxed. The complete local gate passed 3,744 native tests in
160.137 seconds (one slow, 17 skipped), 181 rendered checks, 634 desktop tests and 44
real-daemon checks. Hosted validation and merged delivery remain pending.

This is native source integration with configuration/output refusal tests, not real SSH, native
signing success or packaged provider acceptance. Graphical recovery, uncertain-execution
reconciliation and the full fleet acceptance campaign remain required. The fixed checkpoint is
unchanged.

R106 hosted correction: original run `37150128362` passed macOS and five checks overall,
but Linux test/clippy compilation exposed a missing platform guard around the macOS-only
SSH coordinator service. The module and export now use the same macOS boundary as existing
SSH reconnect/start/observation services. Cross-platform wire and worker recovery remain
available and tested on Linux; no test assertion or required check was weakened. Failed logs
are retained. PR238 merged at `4bc99f27cbdb89ceb3afa109b6af543937546634`; R106 was
reconciled with that main by a related-history merge with identical complete source tree before
the two platform guards were added. The corrected full local gate passed 3,744 native tests in 160.379 seconds (one slow,
17 skipped), 181 rendered checks, 634 desktop tests and 44 daemon checks. Hosted Linux
verification and PR239 delivery remain pending.

## R107 — explicit desktop original workspace recovery

This canonical increment depends on published R106/PR239 at
`b0aa5262f77ecc1929da5c168a66addd9ddb9319`; it replaces no preserved source commit.
R105/PR238 exact-head run `37149462676` passed all seven checks. PR239's original
run `37150128362` remains running; neither parent is merged at this entry.

The remote panel now exposes a distinct original-workspace recovery action. It retains the native
selection, admitted peer and file identities; the renderer supplies only the opaque selected ID.
Native code checks the eligible application, selection and installation before and after signing
and after the exchange. The projection contains only the authenticated initialization disposition
and exact selection/attempt correlation, not private proof or filesystem contents.

English and Hebrew copy states that recovery may start the originally assigned agent and cannot
restart existing or uncertain execution. A successful reply does not claim the provider is running.
Duplicate clicks are suppressed; failure clears the new recovery result, preserves the selected
attempt and previous observations, and never retries automatically. Recovery does not replace
pinned reviews, download a result or approve protected main.

All 43 focused controller/rendering tests passed, including strict attempt/schema/disposition
matching, opaque bridge arguments, duplicate suppression, lost-reply uncertainty, preserved
observations and localized disabled/hidden controls. Both native panel signing-boundary tests
passed in 0.691 seconds. The full local gate passed 3,745 native tests in 155.588 seconds
(one slow, 17 skipped), 182 rendered checks, 636 desktop tests and 44 real-daemon checks.
Hosted validation and merged delivery remain pending. These are controller,
static-rendered and native refusal tests, not signed packaged or second-host acceptance.

The full remote fault/uncertain-execution campaign and packaged real-provider/second-host
recovery remain required. The fixed user checkpoint is unchanged.

## R108 — connected recovery signing-fault and lease-race coverage

This canonical test increment depends on published R107/PR240 at
`f31c2000fd0e612b038322606685a336bfa34cd7`. It replaces no preserved source commit
and changes no production behavior, protocol, threshold or test skip.

Three connected wire tests passed in 0.799 seconds across six scenarios. Worker signer failure
or a signature from the wrong configured key before recovery refuses without initialization or
coordinator history changes. The same failures during the final receipt retain the completed
exclusive handoff; another authenticated recovery refuses while that owner remains held.
Worker lease advancement during the proof signature refuses the old recovery proof without
initialization. Advancement during final-receipt signing suppresses that stale reply while
retaining the recovered handoff. Every scenario verifies original input bytes and absence of
launch intent.

These are Unix-stream native fixture exchanges with independently reopened guarded ledgers,
not actual process-kill, SSH second-host or packaged native-custody evidence. The full local
gate passed 3,748 native tests in 156.004 seconds (one slow, 17 skipped), 182 rendered
checks, 636 desktop tests and 44 real-daemon checks. Hosted validation and merge remain pending. Process termination/restart, remote cancellation delivery,
uncertain execution reconciliation and the full fleet acceptance campaign remain required.

## R109 — owned worker-process kill and original recovery

This canonical test increment depends on R107/PR240 at
`49faa84d352c2100ea42e82b1310138bada99372`, independently of the published R108 signing
fault tests in PR241. It replaces no preserved source commit and changes no product behavior.

An owned subprocess reopens the acknowledged original fixture input and guarded worker ledger,
then serves the real signed recovery exchange. The parent waits for an explicit phase marker,
sends SIGKILL only to that child and reaps it. The two phases are before the worker proof signature
and after completed initialization but before the final receipt signature. A new authenticated
exchange recovers after termination. Assertions preserve original allocation/file/store identities,
input bytes, immutable admission, exactly one allocation and no launch intent. When initialization
was completed before the kill, the entire recovered workspace mapping is identical. Child cleanup
kills and reaps on failure; streams have bounded deadlines and child output uses no inherited pipes.

The focused run passed both test entries in 0.630 seconds, including both kill phases. The child
entry is inert when invoked without the isolated fixture environment. Earlier failed builds and
runs are retained: receipt assertions were corrected without adding private Debug output, and
the accepted test socket explicitly switches from inherited nonblocking mode to bounded blocking
I/O. No production guard or timeout was weakened. The full local gate passed 3,747 native
tests in 159.753 seconds (one slow, 17 skipped), 182 rendered checks, 636 desktop tests
and 44 real-daemon checks. Hosted validation and merge remain pending.

This proves local Unix subprocess termination during prelaunch recovery. It does not prove death
after provider launch, safe uncertain-capacity release, every partial write boundary, actual SSH
second-host recovery, native signing custody or packaged real-provider acceptance. Those remain
required by the full fleet plan. The fixed user checkpoint is unchanged.

R108/R109 consolidation: both published originals passed all seven hosted checks
(runs `37152188050` and `37152165239`). After PR240 merged at
`8e3a912ffafb4b9d46670c0c78738154be62fa6c`, R108 was reconciled onto canonical main
with an identical complete source tree at `de7a3d3a2a35734a3903adf8ac68038812759a42`.
R109 now stacks on that published PR241 revision and retains both test modules and both ledger
entries. The combined 11 focused recovery tests passed in 1.441 seconds. The combined full
`npm test` gate passed 3,750 native tests in 159.586 seconds (one slow, 17 skipped),
182 rendered checks, 636 desktop tests and the real-daemon proof. Fresh hosted checks and
merges remain pending; no product behavior, assertion or skip changed during consolidation.

## R110 — process death preserves uncertain launch intent

This canonical test increment depends on published R109/PR242 at
`a690c3a67e7837232b848b76db5fed0d9c6d75f1`. It replaces no preserved source commit
and changes no production behavior.

The owned subprocess campaign adds a third explicit phase: after successful initialization
acknowledgment and atomic launch-intent reservation, before provider admission/spawn. The
parent observes that phase, SIGKILLs and reaps the child, then attempts a fresh authenticated
recovery. Both peers refuse recovery while the original launch receipt remains unchanged.
The test also preserves the exact workspace mapping, original admission/input identities and
bytes, single allocation and unchanged coordinator state. Earlier prelaunch phases continue
to recover normally. The focused run passed both entries and all three phases in 0.849 seconds.

This tests the uncertain intent boundary, not actual provider-process death, liveness or safe
capacity release. It grants no restart/adoption permission based on PID death or released locks.
The full local gate passed 3,747 native tests in 161.534 seconds (one slow, 17 skipped),
182 rendered checks, 636 desktop tests and 44 daemon checks. Hosted validation,
real-provider/SSH/packaged acceptance and terminal reconciliation remain required. The fixed user checkpoint remains unchanged.

R110 consolidation: original run `37152553178` passed all seven hosted checks.
This increment now stacks on reconciled R109/PR242 at
`553678fa3b145a4e742358376c02839a01935966`, retaining R108 signing-fault coverage.
All 11 focused wire tests, including three owned-process kill phases, passed in 1.729 seconds.
The combined full gate passed 3,750 native tests in 156.020 seconds (one slow, 17 skipped),
182 rendered checks, 636 desktop tests and the real-daemon proof. Fresh exact-head hosted
checks and merged delivery remain pending. No production behavior or verification threshold
changed; real-provider, second-host and capacity-release acceptance remain unfinished.

## R111 — retained original execution observations

This canonical increment starts from merged R108/PR241 at
`84ed9df8e1034f62489d9298cc73db4a107d6b14`. It replaces no preserved source commit.
PR241 merged normally after exact-head run `37152877276` passed all seven checks.
A temporary-directory loss interrupted the old checkout before feature edits. Surviving metadata
and files were archived, prior all-ref bundles retained, and canonical Mesh was cloned into a
durable development directory. Original dirty Mesh-internal and the fixed user checkpoint remain
untouched. No unrelated histories were merged or completed implementation discarded.

The native registry reads the exact original launch/session namespace without opening workspaces,
constructing services, issuing credentials, dispatching processes or changing capacity. It validates
the original setup prefix and exact lane/run/provider/input/installation configuration, replays
bounded pages to the starting revision, and rechecks the launch receipt and revision before return.
Missing session records and interrupted setup remain explicitly uncertain. Run observations are
historical facts; even terminal states do not establish descendant termination or release capacity.
The session namespace and persisted command schemas are unchanged. Signed wire and desktop
projection follow separately; full terminal reconciliation, packaged and second-host acceptance
remain required.

All nine focused tests passed in 2.239 seconds, covering every setup prefix, persisted successful,
failed and cancelled outcomes, stopping after cancellation, malformed/cross-assignment history,
revoked native ledger authority, completion beyond one replay page and retained admission capacity.
Existing local and broker fixture-provider journeys now assert the independently read completion,
including a lost broker final reply. The corrected full `npm test` gate passed 3,753 native tests
in 155.431 seconds (one slow, 17 skipped), 182 rendered checks, 636 desktop tests and the
real-daemon proof. No production checks or thresholds were weakened. Failed fixture compile/path
and broker-registry configuration runs are retained. The first full attempt also exposed missing
generated Tauri files in the damaged temporary cache; validation moved to a fresh durable cache.
Hosted checks and merged delivery remain pending. This does not claim a real commercial provider,
signed packaged application, remote SSH acceptance or capacity release.

R111 consolidation: original exact-head run `37154645067` passed all seven hosted checks.
PR242 merged at `ee3ae12d1df9eff259e3316247c80422f11ccc21`. This increment now stacks
on published R110/PR243 at `19e803229a1e1ce59eb77aeba2e3bd2d26de5917`, preserving
all recovery tests and the R109/R110/R111 migration records. All 20 combined focused tests passed
in 3.304 seconds. Full `npm test` passed 3,755 native tests in 161.913 seconds (one slow,
17 skipped), 182 rendered checks, 636 desktop tests and the real-daemon proof. Fresh hosted
checks and merge remain pending. This reconciliation changes no product behavior or gate.

## R112 — signed historical execution status

This canonical increment depends on published R111/PR244 at
`474289bc96aa403b2aaa5d76d65997c6c264f2ef`; it replaces no preserved source commit.
The additive `mesh.worker-status-query/v4` and reply use independent signing domains and the
existing bounded, fresh exact-attempt exchange. They carry original admission/launch/current-lease
facts plus the historical session revision and state. The worker compares the facts again after
signing; changed worker history suppresses the reply. Changed coordinator context, stale responses,
wrong signers and older response versions refuse. v1/v2/v3 remain unchanged. No allocation,
workspace inspection, service construction, launch, retry or capacity release occurs.

The native connection routes v4 without creating transfer or recovery ownership. The original
input-inspection routing test is retained unchanged; execution-status routing is covered by a
separate new module. No existing assertion was removed. New schema checks reject extra fields,
execution without a retained launch, invalid revision/state combinations and out-of-range revisions.
All 50 focused status/compatibility tests passed in 7.976 seconds. The full `npm test` gate passed
3,758 native tests in 157.971 seconds (one slow, 17 skipped), 182 rendered checks, 636 desktop
tests and the real-daemon proof. Hosted checks and merged delivery remain pending. This is native
fixture/protocol evidence; desktop projection, real SSH second-host acceptance, native custody
and capacity-release reconciliation remain required. The fixed user checkpoint is unchanged.

R112 consolidation: original exact-head run `37155381526` passed all seven hosted checks.
R111 combined-head run `37155813980` also passed all seven. PR243 merged at
`63f98e152a1514314c159042846f04e223be988f`; the related R111 reconciliation at
`ff29d6da53a39134b82c743f103fe7c972fa183a` has a source tree identical to checked
`7d950ed3e90c118e7c4247d70690e97b3ff58111`. This increment retains all R109–R112
records and recovery tests. All 61 combined focused tests passed in 8.596 seconds. The full
`npm test` gate passed 3,760 native tests in 159.966 seconds (one slow, 17 skipped),
182 rendered checks, 636 desktop tests and the real-daemon proof. No assertion or product
behavior changed during reconciliation. Fresh exact-head hosted validation remains required.


## R113 — coordinator and desktop recorded execution

Depends on R111/PR #244 and R112/PR #245. This increment exposes signed v4 worker
execution history through the fleet service, explicit `--coordinator execution` command,
and selected remote worker panel. Existing status, input inspection and result discovery
remain separate operations. It introduces no replacement for preserved source commits.

The renderer receives only the native-selected identity, observation time, admission/launch
facts and the original session revision/state. Revisions remain decimal strings. Unknown
progress, incomplete setup, stopping and recorded terminal states have distinct English and
Hebrew presentation. Reads cannot allocate, adopt execution, release capacity, restart an
agent or approve work. Selection changes clear prior execution observations; failed reads
retain prior facts with an explicit stale-data message.

Validation passed: 45 focused native tests, 49 focused controller/rendered tests, and the full
`npm test` gate: 3,764 Rust tests in 160.533s (1 slow, 17 skipped), 183 rendered tests,
641 desktop tests, and the real daemon demo. Hosted exact-head checks and merge remain pending. Real signed packaged SSH observation, multiple remote workers and terminal
reconciliation remain required by the full plan. The fixed user checkpoint is unchanged.

## R114 — keep fleet observations available during native signing

Tracks [issue #247](https://github.com/idosams/Mesh/issues/247). A regression reproduced
that observation preparation held the shared fleet service mutex inside native signing,
preventing concurrent views and cancellation. The failing baseline is retained separately.
Preparation now opens an independent connection to the same guarded native ledger before
releasing the service lock. Signing retains its existing pre/post context checks; transport
and final reply validation preserve their existing boundaries. No unguarded fallback, new
history initialization, peer-selected path, execution adoption or capacity release is added.

This is new canonical work on R112/PR #245 and replaces no preserved implementation commit.
The initial base covered current lease, input inspection and results. Reconciliation now includes
R113/PR #246 and extends the same signing/cancellation/authority regressions to recorded execution. Initial validation passed: eight focused tests
and the full gate with 3,764 Rust tests in 158.100s (1 slow, 17 skipped), 182 rendered tests,
636 desktop tests and the real daemon demo. The baseline failure, inspection-fixture correction
and test-only lint failure are preserved. Combined validation and hosted delivery remain pending.
Packaged responsiveness and the full parallel fleet acceptance journey remain required.


R114 combined validation: reconciled with published R113 head
`aca8930e04aba8ce9055ee723e578a200d9a1143`, preserving both test groups and documentation.
All four observation kinds are covered by the signing availability, concurrent cancellation
and lost-authority regressions. All 49 focused native tests passed in 0.431s. The full gate
passed 3,768 Rust tests in 160.764s (1 slow, 17 skipped), 183 rendered tests, 641 desktop
tests and the real daemon demo. Exact-head hosted validation and merge remain pending.


## R115 — refresh real four-worker native acceptance

No implementation is transferred or replaced. On merged canonical main
`1c0bcd1788878bcbca8936db1c436e8c772fb841`, the existing real Codex acceptance driver
passed a serial/parallel pair with four workers. Exact source tree, bridge/driver/provider
hashes, measured values and explicit unmeasured fields are recorded in
[evidence/fleet-four-worker-2026-10-04.json](evidence/fleet-four-worker-2026-10-04.json).
The first current-run attempt passed in 249.53s: native execution was 184.925s serial and
64.458s parallel, with observed peaks of one and four workers. Both generated fixture
histories and raw output are preserved separately; the user's checkpoint is unchanged.

This is native real-provider evidence, not packaged interactive review or protected-main
acceptance. The full fleet plan, second provider/host, matched harness baseline, cost,
resource and human coordination measurements remain required. This documentation-only
increment uses the actual acceptance run plus documentation checks; it does not claim a new
full repository gate or a statistically repeatable speed improvement.


## R116 — retain remote assignment identity in the fleet overview

First source increment for [issue #250](https://github.com/idosams/Mesh/issues/250), based on
R114's published reconciled head `56fd1784928734b4a3436798bef4fb84706d1662` (PR #248).
New canonical implementation; no preserved source commit is replaced. The existing native
catalogue adds nullable remote assignment metadata to its current run projection. Exact u64
lease values cross the renderer boundary as decimal strings. Persisted formats and execution
commands are unchanged; legacy replies omitting the field remain readable.

Controller and rendered baseline regressions failed because assignment metadata was dropped and
matching local activity could be shown for a remote lane. Cards now retain the original native
identities and disclose that remote execution is not observed by this view. Read-only snapshots
of a real persisted assignment must retain execution state and its single original attempt.
Independent signed multi-lane observations, packaged proof and the full plan remain unfinished.
Validation passed: 20 controller tests, 14 rendered fleet tests and the native persisted-assignment
regression. The full canonical gate passed 3,769 Rust tests in 158.580s (1 slow, 17 skipped),
184 rendered tests, 642 desktop tests and all 44 real daemon demo checks. Baseline controller and
rendered failures, the empty native filter and corrected pre-claim fixture failure are retained.
Hosted PR validation and merge remain pending; the fixed user checkpoint is unchanged.


## R117 — independent authenticated observations across remote lanes

Continues [issue #250](https://github.com/idosams/Mesh/issues/250) on R116/PR #251 head
`baf2cc6597d6b7cb9e77c1d244d3d1c4f543dfdf`. New canonical work; no preserved implementation is
replaced. Native reads select the exact retained lane/run/assignment and one matching saved
connection, preserving original file/directory bindings and rechecking settings and assignment
after the authenticated exchange. Four bounded read permits and per-attempt exclusion leave the
selected-worker panel independent. There is no launch, lease renewal, process adoption or capacity
release. Existing wire signatures and persisted schemas are unchanged; the desktop command adds
`mesh.remote-fleet-observation/v1`, carrying the existing signed execution projection.

Visible-view polling retains dated observations per lane, bounds concurrent reads and per-lane
frequency, drops replaced-attempt replies and refuses backward/contradictory execution history.
Errors remain per lane without replacing saved reviews. Initial native and controller regressions
pass; the rendered baseline failed as expected before card integration. Full validation passed:
3,771 Rust tests in 159.009s (1 slow, 17 skipped), 185 rendered tests, 646 desktop tests and all
44 real daemon demo checks. The focused native panel suite passed 24 tests, and controller tests
passed 24. The incorrect initial library-target invocation is retained separately. Hosted checks
and merge remain pending. Real signed packaged/second-host acceptance and all other phases
remain required; the fixed user checkpoint is unchanged.


## R118 — discover the current installed Codex CLI bundle

Tracks [issue #253](https://github.com/idosams/Mesh/issues/253). Based on merged canonical main
`97e263ffd602b4232c5b1dc77fa54bc686deec18`; new implementation replaces no preserved source commit.
All four old desktop candidates were absent on the acceptance host, while the regular executable
existed in the nested `codex-cli/CodexCLI.app/Contents/MacOS/codex` layout. Native real-provider
acceptance used an explicit executable path and therefore did not prove desktop discovery.

Discovery supports both bounded app layouts in system/user Applications, prefers the current
layout within each app, and retains final-link refusal and provider-adapter admission. No
renderer path or arbitrary command search is added. Native filesystem tests cover all supported
locations, legacy/current precedence, missing/directory/link candidates and fallback; two baseline
tests failed before the correction. All three focused tests pass. The full canonical gate passed
3,771 Rust tests in 160.643s (1 slow, 17 skipped), 183 rendered tests, 641 desktop tests and all
44 real daemon demo checks. Hosted delivery and corrected packaged provider launch remain pending.

The pre-fix main package was preserved with its exact embedded revision and valid ad-hoc seal.
Its actual visible-window existing-project journey passed: capture external edits, two immutable
pins, independent lane, restart/resume/detach and original Git preservation. It explicitly reports
no provider launch and no protected-main approval. This is separate evidence, not proof of the
provider discovery correction or completion of the fleet plan. The fixed user checkpoint remains
unchanged.


R118 combined acceptance validation: reconciled the original discovery implementation
`eec5b09feea199008611f000f12aa9b9c87aaaf4` with R117's published current-main head
`87fb29bed8751b9c708bbc176fa6e58d7291fab2`. All implementation files merged without conflict;
both documentation records were preserved. The full gate passed 3,774 Rust tests in 160.031s
(1 slow, 17 skipped), 185 rendered tests, 646 desktop tests and all 44 real daemon checks.
The original published PR254 head remains fixed until its original CI is terminal. The combined
package and actual provider-launch acceptance are still pending; no signing or full-plan exit is
claimed.


## R119 — refresh the full acceptance and checkpoint map

Documentation-only reconciliation on published R118/PR #254 head
`3f66a4115912c52abfe261e47a405fcdd4551172`. No implementation is transferred or replaced.
The acceptance map now records merged PR252, the current native four-worker measurement,
the repeated `97e263f` window proof, and fixed `c2641c6` package identity separately.
It corrects the older launcher's incomplete home isolation without replacing user test state.
Original R118 CI completed successfully before its source-identical ancestry update; fresh
checks and merge are pending. The map preserves every full-plan exit, including manual/harness
acceptance, real packaged fleet execution, native approval, second provider/host, retention,
terminal reconciliation and legacy deprecation. Documentation and repository checks validate
this reporting increment; no new runtime or full-gate result is claimed by a documentation edit.


## R120 — collector process-crash campaign

Tracks [issue #256](https://github.com/idosams/Mesh/issues/256) on R119/PR #255 head
`4f21cca374646bbf87afabb8e5b71dfa4e21abe3`. New canonical verification; no preserved
implementation commit is replaced and production collection behavior is unchanged.
A disposable child performs real collection and acknowledges each of eight named boundaries;
the parent confirms SIGKILL, reopens and checks exact retained bytes, removed subsets and
old/new journal state before completing collection and journal compaction. A bounded handshake
and owned-child cleanup prevent a missed stop point from passing or leaving a test process.
Both focused tests passed in 0.556s. Disabling the production reference veto made the campaign
fail on the missing retained chunk; production source was then restored byte-for-byte. The initial
test compile error, lint failure and mutation failure are retained. The full gate passed 3,776 native
tests in 158.640s (1 slow, 17 skipped), 185 rendered tests, 646 desktop tests and all 44 daemon
checks. Hosted delivery remains pending. This is process-crash evidence, not power-loss,
storage-exhaustion, native fleet retention
policy or daemon scheduling acceptance. All full-plan exits remain intact.


## R121 — retain buffered and disconnected recorded history

Tracks [issue #258](https://github.com/idosams/Mesh/issues/258), independently based on canonical
main `d61421d74de3d51c63668ad010af06783edd91fb`. New canonical correction; no preserved source
commit is replaced. Three regressions reproduced omitted buffered/disconnected payloads and an
unresolvable conservative root set for an actor with no causally ready head. The default roots now
include an existing full-history retention window for each known actor, plus its head when ready.
Windows resolve against recorded actor existence; explicit head roots still require a ready head,
and unknown actors remain errors. Persisted formats and causal readiness are unchanged.

All 13 focused retention/independent-GC tests pass, including later parent arrival, genuine orphan
collection and explicit narrowing. The full gate passed 3,777 native tests in 161.085s (1 slow,
17 skipped), 185 rendered tests, 646 desktop tests and all 44 daemon checks. Hosted delivery remains
pending. This fixes
the default retained-set computation; it does not connect daemon cleanup, choose fleet policy,
resolve writer coordination or complete the storage-exhaustion and full fleet acceptance journeys.


R121 combined validation: local related-history reconciliation
`4de1efcd5fd9634dccaee16fd28ea34ebe019ac1` includes published R120 head
`b437963c1e6b96c2ac18098035b8a59677174c7a`, preserves the R119/R120/R121 documentation
and combines the crash campaign with the retention correction. The full gate passed 3,779 native
tests in 168.394s (1 slow, 17 skipped), 185 rendered tests, 646 desktop tests and all 44 daemon
checks. Original published PR heads remain unchanged while their exact-head CI runs finish;
publication of this reconciliation and final hosted validation/merge remain pending.


## R122 — collection storage-exhaustion recovery campaign

Tracks [issue #256](https://github.com/idosams/Mesh/issues/256), based on published R121 / PR #259
head `45c5c475c7c33066bdcf741a5def60342b2a3f64`. New canonical verification; no preserved
implementation commit is replaced. Production collector behavior and persisted formats are unchanged.
A real-file test injects Unix ENOSPC at eight cleanup boundaries: first/second unlink, chunk-directory
sync, replacement-journal staging, partial staging, file sync, rename and journal-directory sync.
It requires the exact error operation/path/code, exact removed subset, retained review bytes,
old/new journal boundary, successful reopen/retry, explicit compaction and a later promotion.
The focused eight-case campaign passed in 0.614s. Temporarily swallowing the production journal
rewrite error made it fail on a false success; production source was restored byte-for-byte.
The full gate passed 3,780 native tests in 159.314s (1 slow, 17 skipped), 185 rendered tests,
646 desktop tests and all 44 daemon checks. Hosted delivery is pending. This is bounded injected-failure
coverage, not a physically full volume, daemon scheduling, cross-process writer coordination,
checkpoint-wide storage-pressure recovery or full fleet retention-policy acceptance.


## R123 — native guarded orphan cleanup

Tracks [issue #256](https://github.com/idosams/Mesh/issues/256), based on canonical main
`ee6fb900d752dff293e2b33bd7a051f98d124db2` (merged R121 / PR #259). New native implementation;
no preserved source commit is replaced. This connects retention planning and CAS deletion under
exact workspace identity and native custody, retains every recorded version, refuses pending
recovery/incomplete history, and handles at most 256 candidates per explicit call. A separate
all-record digest veto protects against a wrong plan. Absent arrival entries are retired under the
same authority. Native view/checkpoint locks are released before deletion; custody remains held.

Nine focused regressions pass: history/dry-run/reopen, bounded batches with absent entries,
stale identity and custody refusal, fresh cross-daemon journal validation, durable pending recovery,
concurrent view reads and second-daemon acquisition exclusion, nested-call refusal, torn history,
and directory substitution after planning. Initial compile and fixture/recovery-reader failures are
preserved. The full gate passed 3,788 native tests in 162.475s (1 slow, 17 skipped),
185 rendered tests, 646 desktop tests and all 44 daemon checks. Hosted delivery is pending.
No persisted format or IPC changes.
This is an explicit native operation, not automatic scheduling, history expiration, bounded scan
latency, real cross-process fault acceptance, packaged UI proof or completion of fleet retention.


R123 merged through [PR #261](https://github.com/idosams/Mesh/pull/261) at canonical main
`4be3da5f5e27c41383f9cccd9991b5b8120684af` after all seven exact-head checks passed.
R122 / PR #260 original head `7beb0bd8b12647042cdbb2a9325d2849376ab4c5` and its
source-identical reconciliation `60fc9d5e5ca6fadd0a354875ec0510be590cd44f` both passed all seven
hosted checks before this related-history combination with merged R123. The append-only records
and both retention-contract changes are preserved; no production code conflicts occurred.
Combined source `5655fe5` passed the full gate: 3,789 native tests in 165.911s (1 slow,
17 skipped), 185 rendered tests, 646 desktop tests and all 44 daemon checks. R122 hosted delivery
remains pending on the final combined head.


## R124 — keep review reads available during cleanup preparation

Depends on published R123 / PR #261 at `0191b0e9a74594e3849c43954afe5262f42943d4`.
New canonical refinement replaces no preserved source commit. Cleanup now captures an independent
pinned journal descriptor and small schema ledger, then releases the live view and checkpoint
locks before recovery inspection, fresh record folding, payload verification and candidate selection.
Native writer custody remains held. A replaced journal is refused even when its bytes are identical;
a stale durable digest still refuses without rewriting the cached view or its indexes.

All eleven cleanup regressions pass in the full gate: 3,790 native tests in 163.208s (1 slow,
17 skipped), 185 rendered tests, 646 desktop tests and all 44 daemon checks.
A deliberate reintroduction of the view lock across preparation made the new read-availability
test fail; the implementation was restored byte-for-byte. Hosted delivery is pending. This removes the bulk scan from the review lock but does not bound storage latency or
provide the still-pending nonblocking admission, background scheduler or complete fleet acceptance.


R124 original PR #262 head `ae6a1189ad37268f52c10b7e2c5d8e502c57ba2f` passed all seven
hosted checks before combination with R122 / PR #260 head
`054a0f8e12a6d1d273e3b322afef5aacfe402f0d`. Related-history combination
`e92def7434afc02eaaaeb096d56094c657c8be1c` resolved only this append-only ledger; no production
conflicts occurred. Its full local gate passed 3,791 native tests in 161.334s (1 slow, 17 skipped),
185 rendered tests, 646 desktop tests and all 44 daemon checks. PR #262 is stacked on PR #260
while that parent's original macOS verification remains running; its published head is preserved.
Fresh hosted checks and merged delivery remain pending for the combined R124 head.


## R125 — defer cleanup when native admission is busy

Tracks [issue #256](https://github.com/idosams/Mesh/issues/256), based on published R124 / PR #262
head `8e9c0b4713eb06692dd3dd1cbcdcf7275ce93f69`. New canonical implementation replaces no
preserved source commit. A native try-entry point defers busy directory custody, workspace-open,
managed-edit, checkpoint and live-view locks, and assigned-agent custody. Exact identity,
nested-mutation refusal, fresh recovery inspection and conservative retention remain enforced.
Explicit and try-entry points share preparation/deletion execution. No IPC or persisted format
changes. All 13 focused cleanup tests pass. A deliberate blocking-lock mutation fails the new
response-before-release regression; source was restored byte-for-byte. Full validation and hosted
delivery are pending. Admission does not bound filesystem or admitted writer-exclusion latency,
and automatic scheduling and pressure policy remain separate unfinished work.

R125 full local gate passed 3,793 native tests in 162.513s (1 slow, 17 skipped),
185 rendered tests, 646 desktop tests and all 44 real-daemon checks. The restored implementation
is the tested source. Hosted checks and merge remain pending; checkpoint builds are unchanged.


## R126 — periodic orphan cleanup with one native owner

Depends on R125 / PR #263 at `d4adb9a142c57e9daa1d49e5cfa40b7890a8eacf`, staged on canonical
main `f1e1ee3d9273adc2faf18440a6e47dc98bb2e0d8` through related-history merges with identical
source trees. New canonical implementation replaces no preserved source commit. Desktop and
headless daemon retain one worker, waiting 60 seconds between attempts, selecting exact current
workspace identity through a try-lock, and calling R125 admission. Native status is redacted;
no IPC or persisted-format change. Duplicate owners refuse. Sleeping workers do not retain the
daemon; owner drop wakes and joins the worker before resources can be abandoned.

All 17 focused cleanup tests pass, including four new scheduler tests covering real deletion,
retained history/reopen, busy retry, torn-history refusal, single ownership and shutdown. A worker
changed to dry-run only fails the deletion assertion; source restored byte-for-byte. Full validation
and hosted delivery are pending. Admitted disk I/O can still delay writers or shutdown. This does
not expire history, traverse closed workspaces, establish pressure policy, prove packaged operation
or complete the full fleet storage-pressure and fault acceptance requirements.

R126 full local gate passed 3,797 native tests in 162.557s (1 slow, 17 skipped), 185 rendered
tests, 646 desktop tests and all 44 real-daemon checks. The initial full gate identified six
desktop fixtures missing the new owner; those fixtures now start the real worker, and the failure
log is preserved. Hosted delivery remains pending. R125's original head passed all seven hosted
checks before its source-identical main reconciliation; no active CI head was replaced.


## R127 — cancel the owned provider process group

Based on published R126 / PR #264 at `cc5608f3eddb04d001cb204e2403f4d76e58a31f`. New canonical
implementation replaces no preserved source commit. Both adapters use a dedicated process group.
Cancellation and failed-launch abort signal it only while the original child remains unreaped,
which pins the group number against PID reuse; the direct child is also stopped if it moved groups.
An observed exit permanently refuses later numeric group signaling. No persisted schema, protocol,
credential, capacity-release or main-approval authority changes.

Four focused provider regressions pass. The new real-process test starts a descendant with inherited
output pipes, requires prompt shutdown before natural child exit, keeps an independent process alive,
and refuses signaling after reaping. Restoring direct-child-only stop fails the timely-shutdown
assertion; source restored byte-for-byte. Full validation and hosted delivery are pending. This is
not containment or proof of escaped descendant termination. Remote terminal capacity remains reserved;
full cancellation/restart acceptance and issue #172's intermittent lifecycle warning remain open.

R127 full local gate passed 3,798 native tests in 162.715s (1 slow, 17 skipped),
185 rendered tests, 646 desktop tests and all 44 real-daemon checks. No lifecycle warning was
reported in this run; this does not resolve issue #172's intermittent earlier observations.
Hosted delivery remains pending. User checkpoints and their application state remain unchanged.


## R128 — worker progress inspection without a shared scan lock

New canonical implementation for [issue #267](https://github.com/idosams/Mesh/issues/267); no
preserved source commit is replaced. Depends on scheduler PR #264 and the merged provenance audit
PR #266. The existing missing-file read now releases the fleet mutex before native custody and
filesystem inspection, then refreshes and checks the exact retained grant and active run before
returning. A native-only progress classification reports unchanged, changed or resolution-needed.
Neither path saves content, appends checkpoint events, publishes reviews or advances main.

Four native regressions pass, covering real file/directory changes, missing files and unsupported
links, read-only history/ledger preservation, parallel fleet reads, cancellation, credential
rotation/revocation and substituted roots. Restoring the shared scan lock fails the concurrency
regression at its two-second deadline; tested source was restored byte-for-byte. Full `npm test` passed 3,801 native tests in 162.149s (1 slow, 17 skipped), 185 rendered
tests, 646 desktop tests and all 44 real-daemon checks. Hosted delivery is pending. No IPC schema,
dependency or persisted-format change.

This is an inspection prerequisite, not automatic worker saving. Per-lane capture, save authority
during cancellation, scheduling/final-save semantics, recovery, presentation and packaged provider
acceptance remain open. Source and mutation evidence are retained with the delivery archive.


## R129 — local signed capture without the shared fleet mutex

New canonical implementation for [issue #267](https://github.com/idosams/Mesh/issues/267), stacked
on PR #268; no preserved source commit is replaced. Local explicit checkpoint capture retains
the exact grant/workspace, performs native scanning and signing outside the shared fleet mutex,
and refreshes authority before and after every signing callback. While native custody is held,
contended fleet admission refuses rather than waiting in the inverse lock order. Original begin/
finish records, exact request replay, partial capture and review boundaries remain in use.

Four native regressions pass: paused signing permits parallel fleet reads; exact retry does not
sign again or rewrite later work; cancellation/revocation during signing suppresses that payload;
real concurrent credential rotation completes without deadlock and preserves unsaved files; and
authority checks refuse a held fleet mutex within the test deadline. Routing capture through the
previous implementation fails the parallel-read deadline. Source was restored byte-for-byte.
Full `npm test` passed 3,805 native tests in 162.930s (1 slow, 17 skipped), 185 rendered
tests, 646 desktop tests and all 44 real-daemon checks. Hosted verification is pending. No IPC,
dependency or persisted-format change.

This increment changes local explicit capture. Received remote sessions keep their original
authority path. Contention can produce an incomplete checkpoint; completed authorized appends
remain durable, including work admitted before cancellation. Automatic ordinary progress capture,
checkpoint-budget policy, scheduling, live presentation and packaged acceptance remain open.


## R130 — automatic private saving for owned local workers

New canonical implementation for [issue #267](https://github.com/idosams/Mesh/issues/267), stacked
on PR #269 head `92eaef391cb534073725f54e34de4472a2ce0d90`. No preserved source commit is replaced.
The host captures ordinary progress with one in-flight job per local worker, five seconds between
completed attempts and one final attempt before recording provider completion. Native journal
recovery and the existing Saved event retain versions without consuming explicit checkpoint slots.
Unchanged polls append nothing. Exact grant and native fold checks prevent stale or revoked saving
and prevent an older completion from replacing a newer saved version. No persisted-format change.

Five native service regressions and a real running-provider fixture cover capture, empty polling,
missed observation recovery without resigning, explicit handoff separation, missing files,
cancellation/revocation and concurrent newer checkpoints. Disabling periodic saving fails the
live-worker assertion; original source was restored byte-for-byte. Controller parsing covers the
additive nullable observation and malformed values. Rendered tests keep saving separate from
execution and handle an unavailable timestamp. The first full run passed 3,811 native tests in
163.739s, then caught a nullable desktop timestamp type error. That failure log is preserved;
the corrected display passes all 186 rendered tests. The final full `npm test` passed 3,811
native tests in 167.333s (1 slow, 17 skipped), 186 rendered tests, 647 desktop tests and all
44 real-daemon checks. Hosted delivery remains pending.

Incomplete saving and pending acknowledgments remain visible; final failures are not retried
after session revocation. Cancellation can leave already-admitted appends retained. Shutdown joins
admitted saves and has no storage/signing latency bound. Remote sessions, external harnesses,
protected main and packaged provider acceptance remain separate unfinished requirements. The
user's fixed test checkpoint is unchanged.


## R131 — bounded final save acknowledgment recovery

New canonical implementation for [issue #267](https://github.com/idosams/Mesh/issues/267), stacked
on PR #270 head `d15b28081df5c6c49ee673c188bda45cf1ceed16`; no preserved source commit is replaced.
A complete native final save whose fleet acknowledgment is pending no longer immediately ends the
session. The same native owner permits three total attempts, one second apart, with one job in
flight and unchanged credential/capture authority. Success clears the pending observation;
exhaustion stays visible. Cancellation prevents another attempt. Incomplete captures, signing
failures and provider execution are not retried by this acknowledgment policy. No protocol,
dependency or persisted-format change.

All ten focused regressions pass: four owner/backoff tests, five native save tests and the real
live/final provider-process fixture. Reducing the limit to one fails the recovery assertion;
source restored byte-for-byte. An initial fixture digest-import compile failure is preserved.
Full `npm test` passed 3,815 native tests in 164.031s (1 slow, 17 skipped), 186 rendered tests,
647 desktop tests and all 44 real-daemon checks. Hosted delivery is pending. An attempt may still wait on native filesystem
or signing work; an attempt count is not a wall-clock shutdown bound. Packaged, remote and
protected-main acceptance remain open, and the fixed user checkpoint is unchanged.


R127 delivery consolidation: checked cancellation head `2ff2e1a826a9602ebe8c1444d842b289b706a96e`
(all seven hosted checks passed) is combined with the published saving/retry stack through PR #271
head `cfcca75aab183c7a07985868401a568610a89814`. Combined implementation
`9d411a84360f711cb815e9cf4d2cba7d7088514c` passed full `npm test`: 3,816 native tests in 164.518s
(1 slow, 17 skipped), 186 rendered tests, 647 desktop tests and all 44 real-daemon checks.
All source commits remain ancestors; no production conflicts needed manual resolution. PR #265
uses PR #271 as its delivery base so cancellation remains a focused review and the full combination
is validated once. Fresh hosted checks and merged delivery remain pending. User checkpoints remain fixed.


## R132 — immutable ordinary saved-progress reads

New canonical implementation for the Phase 3 live-review objective, based on PR #265 head
`ad2d75f499bcb1f6f8a2b57faef3a474fb06ee3b`. No preserved source commit is replaced. Native
FleetHistory/FleetService readers enumerate verified causally ready operations in pages of 50
and compare an exact selection to the bound starting version, using existing 200-change and
262,144-byte selected-text limits. Exact digest cursors reject unknown or noncanonical IDs.
History I/O is outside the fleet mutex; pinned allocation/root identity and the lane binding are
verified before returning. Reads create no checkpoint, review, execution session or approval.
No persisted-format change or new public IPC command. Page/comparison response schemas are new
native-only v1 shapes. Intermediate captures are retained progress, not complete handoffs.

Two native regressions pass, covering later saves, unsaved bytes, revocation, restart, read-only
fleet state, unchanged original/desktop selection, substituted roots, unknown selectors, bounded
pagination and another lane's version refusal. Mutating exact selection to the latest operation
fails the immutable-read assertion; source restored byte-for-byte. An initial fixture path-type
compile failure is preserved. Full repository validation and hosted delivery remain pending.
Separate progress panels, live changed-file summaries, packaged multi-panel acceptance and the
remaining full fleet plan are still required. The fixed user checkpoint remains unchanged.

R132 implementation `3a44acc44d1b0486dda51ffaae6f72be10c7ce74` passed full `npm test`: 3,818 native tests in 165.247s (1 slow, 17 skipped), 186 rendered tests, 647 desktop tests and all 44 real-daemon checks. Hosted delivery remains pending.


## R133 — desktop saved-progress boundary

New canonical implementation based on PR #272 head
`23c9663c4c94034f93cde9c81a1c5272977fcd19`; no preserved source commit is replaced.
Two additive read-only Tauri commands expose retained progress through FleetHistory, without
execution adoption or arbitrary paths. Distinct progress validators bind fleet/lane/source,
starting version and exact target, reject invented authority, and enforce causal page identity.
The existing bounded immutable comparison content validator is shared without fabricating a
checkpoint or review-bundle selector. Existing review parsing retains its separate envelope.
No persisted-format changes; older desktop calls remain supported.

Eight focused tests pass, including existing review comparisons and new progress page/correlation,
bounds and unsafe-content refusal. Full validation and hosted delivery remain pending. Independent
progress panels, persistence and packaged parallel inspection remain required in following increments.
The fixed user checkpoint is unchanged.

R133 implementation `e029a34ae41831bfa3e945f68e6fb349c458830e` passed full `npm test`: 3,818 native tests in 167.317s (1 slow, 17 skipped), 186 rendered tests, 651 desktop tests and all 44 real-daemon checks. Hosted and packaged delivery remain pending.


## R134 — independent ordinary-progress panels

New canonical implementation built on PR #273 head
`89f59d911e463aedc6cbea979c18d6e4008a3654`, reconciled locally with canonical main
`e816aa5d161fc13fac6cf15ea81c3361dbf3dbac` without changing the source tree or running published
CI heads. No preserved source commit is replaced. The fleet view offers retained-progress lists,
up to four exact immutable comparison pins and at most 32 open lists. Lists refresh independently;
pins never follow a newer save. Each pin owns its request generation, exact retry and selected
content. The existing native comparison presentation is reused through a separate event handler,
without exposing handoff, review, import, execution or approval mutations.

Three controller regressions cover two fixed pins while a third save arrives, stale/closed request
suppression, failure retention, exact retries, input refusal and bounds. Removing request-generation
checks fails the stale-response regression; original bytes restored. Two rendered regressions cover
parallel selectors, authority-free controls, intermediate-save labeling and missing-starting-version
refusal. Existing fleet and rendered suites passed during iteration. Full validation and hosted
delivery remain pending. Pins are explicitly session-only in this increment; persistence, live lane
summaries and actual packaged parallel-agent inspection remain required. Fixed checkpoint unchanged.

R134 implementation `256bdf754b0f834a338aad6f355e83526f510cb0` passed full `npm test`: 3,818 native tests in 166.674s (1 slow, 17 skipped), 188 rendered tests, 654 desktop tests and 44 real-daemon checks. Hosted and packaged validation remain pending.


## R135 — native progress-selector persistence

New canonical implementation based on PR #274 head
`ab62121eea5dfd6d40865a749e578c5dc79f78bf`; no preserved source commit is replaced.
A separate native store retains at most four exact progress selectors and layout/object/cursor
choices, never file contents, paths, checkpoints or review authority. The new projection is
`mesh.desktop-progress-pin-selectors/v1`; the private catalog-bound file is
`desktop-progress-pins.json` with an exclusive pending sibling. Existing attachment/local/remote
review records are unchanged. Reads reject foreign catalog identity, links, malformed/oversized
records and incomplete first writes. Writes lock the catalog, require the current revision,
sync the file and directory, and retain failed staging evidence. Two additive desktop commands
expose this bounded store. Panel restoration is not yet wired, so visible panels remain session-only.

Five native regressions pass: restart and namespace independence, schema/duplicate/bound refusal,
corrupt/copied/linked/interrupted evidence preservation, concurrent writers and coexistence with a
full review pin set. Disabling the revision check fails the concurrent-writer assertion; original
bytes restored. Initial fixture formatting failure was recorded and corrected. Full validation,
hosted delivery and packaged persistence acceptance remain pending. Fixed user checkpoint unchanged.

R135 implementation `c0013200689280be4a23ee0a31730dc1270b7d33` passed full `npm test`: 3,823 native tests in 166.966s (1 slow, 17 skipped), 188 rendered tests, 654 desktop tests and 44 real-daemon checks. Hosted delivery and panel restoration remain pending.


## R136 — restore and save progress panels

New canonical implementation based on PR #275 head
`a4dc6d46c2aabf2ea6e2f48a481d50d87de1d451`; no preserved source commit is replaced.
Progress panels load their native selector snapshot on opening the fleet view, independently of
live ownership. Each saved version, source, starting version, layout, page cursor and selected
object is retained; content is read again through the exact native comparison. No cached content,
paths, handoff or approval authority is persisted. The new strict renderer parser matches the
native v1 schema. Restore freezes edits, stale/disposed replies remain ignored, and missing native
history leaves selectors retained with visible errors. Save failures show explicit retry/reload;
lost acknowledgment recovery reads current selectors without duplicate writes. Existing review
pin protocols remain unchanged.

Six progress persistence regressions cover restart without live fleet ownership, missing history,
lost save acknowledgment, unreadable state/disposal, malformed selectors and independent cursor/
object restoration. Retargeting a restored version fails the regression; source restored exactly.
Existing panel/fleet tests and all 189 rendered tests pass. Initial integration fixtures still
expected six startup reads; updated them for the seventh read-only progress-selector load and
preserved the failure log. Full validation and hosted/packaged delivery remain pending. Live lane
summaries and the full fleet acceptance plan are still required. Fixed user checkpoint unchanged.

R136 implementation `df9d0c5053cd16d1412b860117863e06479e9dab` passed full `npm test`: 3,823 native tests in 166.583s (1 slow, 17 skipped), 189 rendered tests, 660 desktop tests and 44 real-daemon checks. Hosted and packaged delivery remain pending.


## R137 — exact latest-save lane overview

New canonical implementation built on PR #276 head
`c222afe8b289ac00b79e038d758df368b583c8d5` and source-identically reconciled with main
`2cdc40668c4a944ac5408873c7968cb509a27c30`. No preserved source commit is replaced.
Native lane summaries add nullable saved_version from the durable ledger. The renderer accepts
older absent fields as unknown and rejects malformed values. The polled overview displays the
latest acknowledged version independently of live worker ownership and can pin that exact clicked
version. History reads obtain the bound source/starting identity; a newer acknowledgment does not
retarget the request, even when the chosen version lies beyond the first 50-operation page.
Duplicate in-flight requests are suppressed, concurrent requests are bounded, and disposal or
selector restoration invalidates pending panel creation. No persisted-schema change or mutation
of provider execution, working files, checkpoints, reviews or approval state.

Focused native verification covers the projected version after a real save. Controller/parser
and rendered regressions cover older replies, malformed values, later-save races, duplicate and
closed reads, restored ownership and loading controls. Substituting the newer acknowledgment for
the clicked version fails the regression; source restored exactly. Full gate and hosted delivery
remain pending. Changed-file/activity summaries, packaged multi-agent acceptance and the rest of
the full fleet plan remain required. User checkpoint unchanged.

R137 implementation `7c7a128f6e9253a14841e41bfc8436ff91890964` passed full `npm test`: 3,823 native tests in 169.654s (1 slow, 17 skipped), 190 rendered tests, 663 desktop tests and 44 real-daemon checks. Hosted and packaged delivery remain pending.


## R138 — real-provider ordinary save acceptance

New canonical acceptance test built on PR #277 head
`36ed79c0d8c7dd10826311e00a1809b86194d8a8`, source-identically reconciled with canonical main
`d364f552b31c4d9417bf14fd5432099f1b2965c4`. No preserved source commit is replaced.
An opt-in installed Codex worker writes an intermediate edit, waits 15 seconds, then writes a
final edit; its normal explicit handoff is allowed only after that command finishes. The native
host must expose immutable intermediate bytes while the worker is active and before any explicit
checkpoint. After completion, the same selection must still show those bytes while the working
file holds the final edit. One worker run, original project and selected desktop state are checked.
The fixture and structured measurement are retained for later inspection. Account configuration is
used in place; no credentials are copied. Test signers do not confer human-approval authority.

The real run passed with Codex CLI 0.158.0-alpha.2.1 in 55.10s. First live save observation was
26,069ms after native-host run start, not a measurement of filesystem-event latency; total host
journey was 54,550ms. Explicit checkpoint count was zero at the live observation and one after
completion. Thus this proves ordinary saving before handoff, not absence of all later handoffs.
The immutable comparison remained exact after the final write; original/desktop selection remained
unchanged. Preserved fixture copy verified all file bytes and link targets. This is non-graphical,
local single-provider evidence; full gate, hosted delivery, packaged parallel panels, second-provider/
second-host and protected-main acceptance remain separate requirements. Fixed checkpoint unchanged.

R138 final test source repeated the real journey successfully in 59.64s: first live save at
25,971ms, total host journey 59,221ms, zero checkpoints at the live observation and one after
completion. Both fixtures are retained. The initial full gate caught an unnecessary unwrap in
the test; an edition-incompatible formatting attempt was corrected before a second provider run.
Both failure records are preserved; production behavior was unchanged.

R138 final implementation `21eab5996d1865dd4b28c99cdaaa9ae1b9178563` passed full `npm test`: 3,823 native tests in 166.174s (1 slow, 18 skipped), 190 rendered tests, 663 desktop tests and 44 real-daemon checks. The new ignored provider test was separately run successfully twice, including once against final source. Hosted delivery remains pending.


## R139 — fixed saved-progress test checkpoint

Documentation-only evidence increment based on PR #278 head
`5052009ee4fe57a468bca4adec43de6f93c4c6a0`; no preserved source commit is replaced.
A clean canonical checkout built the local macOS app with `npm --prefix apps/desktop run
 tauri:bundle-local`. Existing target output was copied and verified before replacement; running
older apps and the fixed c2641c6 checkpoint were left intact. The new separate checkpoint includes
an isolated-data launcher, sample project and concrete parallel-progress/reopening guide. No
installed application or account configuration was replaced, and no credentials were copied.

Artifact identity: revision `5052009ee4fe57a468bca4adec43de6f93c4c6a0`, executable SHA-256
`93f8f470db4aa07fd2a034c02599310be9566db55f7098e7c63c13d6e56f3c09`, 22,742,368 executable bytes.
The local app verifier confirmed its revision, embedded interface and ad-hoc resource seal before
and after the packaged test. `packaged_desktop_bridge_delegates_and_reviews_attached_versions`
passed in 1.35s using that exact checkpoint executable and expected revision. Its scope was the
native packaged bridge: two child lanes, signed test checkpoints, pinned review, feedback,
proposed revision, work decision, deletion, empty-result review, exact retry and revoked-session
refusal. It did not launch a provider or open a graphical interface, and does not prove the newer
saved-progress commands through the packaged UI. Build/proof logs and identity receipts are retained
in the private delivery archive. The launcher syntax and old checkpoint executable hash passed.

Graphical inspection was attempted but the Mac was locked. This is a usable development checkpoint,
not completed graphical acceptance, a notarized release or protected-main authority. Feature PRs
#274–#278 were still open at checkpoint delivery; their final merge receipts remain separate.
The full native source gate and two real Codex ordinary-save runs are recorded under R138 and do
not substitute for this package's remaining visual journey. The complete fleet plan stays open.


## R140 — exact saved file and folder counts

New canonical implementation based on PR #279 head
`e09229de263c1d1038f98e72f19caa677174221c`; no preserved source commit is replaced.
Tracks issue #280 and the changed-file summary requirement in phase 3. Native starting-version
comparisons add file_total and folder_total over the complete immutable changed-object set, before
pagination or selected-content reads. Folder totals never masquerade as files. The same projection
serves handoff and ordinary-progress comparisons. Desktop validation accepts absent legacy pairs as
unknown, refuses partial/malformed/inconsistent pairs, checks visible entries against global counts,
and prevents a known count from changing or disappearing on a subsequent exact comparison request.
Parallel panels present the native split with English/Hebrew labels and retain the legacy object
count fallback. No new filesystem scan, authority, persisted format or provider control is added.

The native regression creates 202 changed files and one changed folder, checks both pages and a
selected-object read, saves later edits including another folder, and verifies that the old pinned
comparison stays exact. Original project content remains unchanged. Parser and rendered tests cover
legacy unknown, malformed counts, per-page totals, stable navigation and independent panel fallback.
Substituting zero for the native file count fails the parser regression; source restored exactly.
The initial fixture wrongly expected ordinary saving to confirm deletion; the existing conservative
incomplete-save behavior was preserved, the fixture changed to supported later edits, and the
failure log retained. Full validation and hosted delivery are pending. These are exact saved-entry
counts, not a live working-tree inventory or a completeness claim. Live lane summaries, packaged
parallel-agent acceptance and the remainder of the fleet plan remain unfinished. Fixed checkpoint
5052009 and previous checkpoints are unchanged.

R140 implementation `d32385e54f8b0619b8e24149dff45761affdeb46` passed full `npm test`: 3,824 native tests in 168.747s (2 slow, 18 skipped), with rendered, desktop and all real-daemon checks passing. The mixed multi-page regression passed in 78.130s under the full suite. Hosted delivery and packaged graphical acceptance remain pending.


## R141 — bounded exact saved-progress summary boundary

New canonical implementation based on PR #281 head
`dda9560fc55946765b6357a02939ca4b165bd509`; no preserved source commit is replaced.
Tracks issue #282 and the phase-3 live-overview requirement. A native summary projection reuses the
same immutable change-set calculation as the full comparison but returns before constructing any
named entry or selected content. FleetHistory and FleetService expose saved_progress_summary for
one exact retained operation through the existing custody-verified, outside-lock history read.
Unknown versions, unbound starts and substituted roots still refuse. The read does not adopt a
worker, start capture, create a checkpoint, or grant handoff/approval authority.

The read-only summarize_fleet_saved_progress desktop command delegates to that history boundary on
the blocking pool. The new mesh.fleet-saved-progress-summary/v1 response carries source/start/target
identities, separately observed latest acknowledgment and complete saved file/folder totals. Its
strict parser limits serialized replies to 2 KiB, rejects unexpected summary fields, malformed or
inconsistent counts and mismatched selections, and never retargets to a newer acknowledgment.
No persisted schema or existing comparison response is changed by this increment.

The focused native regression passed in 60.13s: 202 files and one folder produce the same aggregate
as both comparison pages; the summary has exactly the expected fields, fits the response bound and
remains fixed after a later save. Existing restart/substituted-root and cross-lane regressions also
exercise the new read. Parser tests cover bounded output, wrong identities, malformed counts,
unexpected entry fields and false authority. Replacing the selected version with the latest
acknowledgment fails the regression; source restored exactly. Full gate and hosted delivery remain
pending. This is the summary read foundation: independent bounded lane refresh and live presentation
still follow, and the complete packaged/second-provider/second-host acceptance plan remains open.
The fixed user checkpoint is unchanged and does not include this new command.

R141 implementation `849bbbd21aaa932bf4e4353d0adeeb56833f7012` passed full `npm test`: 3,824 native tests in 170.334s (2 slow, 18 skipped), 191 rendered tests, 665 desktop tests and all 44 real-daemon checks. Restart, substituted-root and cross-lane summary refusals passed in the complete suite. Hosted delivery and overview presentation remain pending.


## R142 — independent local lane saved-change overview

New canonical implementation based on PR #283 head
`7c5d6c2282d860599335ee4053d93f9f3a184258`; no preserved source commit is replaced.
Tracks issue #284. Local allocated lanes with an acknowledged save now read the exact native
summary independently of fleet status/commands and review pins. The scheduler admits at most two
reads, fairly rotates across the entire bounded catalogue (16 fleets times 1,024 lanes), coalesces
newer saved versions while an older request is active, and caches successful immutable summaries.
Failures retain previous verified counts and wait at least five seconds after failure before retry.
No new native dispatch occurs while hidden/disposed or while catalogue status is unavailable.
Uncancelled in-flight calls retain their slots until completion; stale scope/version/disposed replies
cannot replace current results. Restored local lanes use history only; remote assignments and lanes
without a verified local save do not enter this summary queue.

Lane cards join the exact source and acknowledged version. They distinguish current saved counts,
earlier saved counts, queued/reading states and unavailable/unknown counts, with exact summary-version
details and English/Hebrew labels. Counts do not describe unsaved working files or approve results.
The new controller never captures, launches, stops, adopts or mutates pinned comparisons. Existing
status/command queues remain responsive while native summary reads are slow.

Focused verification passed 47 scheduler, controller and rendered tests, including two-read capacity,
fairness, coalesced updates, stale replies, error backoff, hidden/disposed dispatch, source changes,
restored versus remote eligibility, the catalogue-size bound and explicit start during a pending
summary. Current/older/error/unknown rendered states and source/version correlation are covered.
Increasing concurrency to three fails the regression; source restored exactly. Native regression
also binds the catalogue base to the history summary source. Full gate and hosted delivery remain
pending. Packaged real-agent parallel review, measured responsiveness, richer activity/validation/
dependency overview and the full fleet acceptance plan remain open. Fixed checkpoint unchanged.

R142 implementation `94af7d6c329ae7b9440a4f44bfea52512f871ef5` passed full `npm test`: 3,824 native tests in 169.965s (2 slow, 18 skipped), 193 rendered tests, 671 desktop tests and all 44 real-daemon checks. Hosted delivery and packaged graphical acceptance remain pending. The fixed 5052009 user checkpoint remains unchanged.


## R144 — exact handoff and feedback overview

New canonical implementation based on PR #285 head
`8ab2f84fd9787b0551e5e766ed4f5522702569d3`; no preserved source commit is replaced.
Tracks issue #287. The native fleet snapshot now adds per-lane handoff status computed in one pass
over the already-refreshed checkpoint and review-request records. Complete/incomplete handoffs and
submitted reviews are bound to the lane's exact latest saved version; older-version records are
excluded from those counts. Pending capture intents and open explicit change requests span the lane.
Confirmed decisions and reopen operations update the request count; provider completion and proposed
revisions do not. No feedback text, credentials or file contents enter this aggregate.

This is an additive snapshot projection with no persisted-format change, filesystem scan, capture,
worker adoption or new authority. The strict desktop parser enforces closed fields, exact version
correlation, safe counts, review subsets and false approval authority. Omitted legacy projections
remain unknown; contradictory/malformed replies refuse. Lane cards explain current-version versus
lane-wide counts in English and Hebrew and distinguish native capture completeness from tests or
approval. Independent comparisons and progress pins remain unchanged.

The focused native projection test covers old/current/new/absent saves, complete/incomplete/pending
captures, cross-lane isolation, explicit feedback decisions and reopening, read-only state and privacy.
The existing real native feedback lifecycle test checks the snapshot against durable commands. Both
pass; 44 focused controller/rendered tests passed. The initial native test fixture needed an explicit
closure parameter type; its compile failure is preserved. Removing the current-version filter fails
the native regression; exact source was restored. Full gate and hosted delivery remain pending. Private dependency eligibility, validation evidence, packaged graphical
acceptance and every remaining fleet-plan exit remain required. Fixed checkpoints are unchanged.

R144 implementation `49cc6a84f5b5ccf90b0c5e4f4b0c40924ea60370` passed full `npm test`: 3,825 native tests in 174.256s (2 slow, 18 skipped), 194 rendered tests, 672 desktop tests and all 44 real-daemon checks. The native feedback decision/reopen projection passed within the full suite. Hosted delivery and packaged graphical acceptance remain pending.


## R145 — refresh managed approval history inside native custody

New canonical implementation based on merged PR #285 (`9cc808ab4813497d16179a30d20e70af59acb925`);
no preserved source commit is replaced. Tracks issue #290 and the publication-boundary prerequisite
of private-dependency issue #289. Managed approval now refreshes record-derived history through the
already-open descriptor-pinned journal while holding existing native custody, before checking the
displayed root, digest and installation and before admitting or appending any approval. It does not
select a replacement pathname, alter receipt formats or weaken native signing/current-folder checks.

The new two-client regression failed before the fix: after the first valid receipt committed, a
second independently opened daemon appended a different receipt from its cached pre-approval state
and only then returned `publication-save-failed`. The fix refuses `stale-workspace` before any append.
The test checks journal byte equality, stale retry refusal, reopened main and exact original receipt
recovery. The existing native suites passed 44 attachment-approval and 7 human-approval tests. An
initial test-only receipt accessor compile error is preserved separately. Full gate and hosted
delivery remain pending. This is native fixture evidence with test credentials, not packaged human
presence or complete dependency-policy enforcement. The fixed user checkpoint is unchanged.

R145 implementation verification: Full `npm test` passed on `19486d421f1c32f8409f6737ef3f19d7be206013`: 3,825 native tests in 170.061s (2 slow, 18 skipped), 193 rendered tests, 671 desktop tests and all 44 real-daemon checks. Hosted delivery remains pending.

## R146 — bounded native custody sets

New canonical implementation based on PR #292 (`3d53c3d8dfda9a3e0f7184b09e90713162930e96`);
no preserved source commit is replaced. Tracks issue #293 and the first prerequisite of private
input issue #289. Native initialization can lock up to 32 requested descriptor-pinned roots, with
identity deduplication and deterministic physical-identity order. It uses the same kernel directory
locks as existing single-root custody and routes ordinary initialization through the same path.
Every admitted namespace is verified before and after acquisition. Nested expansion is refused;
exact already-held roots can be opened without acquiring another lock. Borrowed guards verify
continued membership. Failed partial acquisition releases every acquired lock. Lock guards cannot
move between threads because their native custody membership is thread-local.

Twelve focused custody tests pass, including separate-process single/set and set/set contention,
both acquisition orders, reverse input order, both member roots, duplicates/bounds, unrelated
nesting, expired borrowed guards and root replacement while a set waits after acquiring its first
member. Replacing exclusive locks with shared locks makes the process regression fail; tested
source restored byte-exact. The thread-confinement compile check identified an existing test that
sent a live guard across threads; the test now performs acquisition on its owning thread and sends
only the resulting generation. That compile refusal and a separate test-only path accessor compile
failure are preserved. No persisted format or agent API changes. The primitive grants no mutation,
dependency or publication authority; project policy, exact closure and race-safe publication still
require integration. Full gate and hosted delivery pending. Fixed user checkpoint unchanged.

R146 implementation verification: Full `npm test` passed on `cd830a86a98174e0952277f9442020f6329f9b80`: 3,830 native tests in 173.261s (2 slow, 18 skipped), 194 rendered tests, 672 desktop tests and all 44 real-daemon checks. Hosted delivery remains pending.

## R147 — required dependency journal and index foundation

New canonical implementation based on PR #294 at
`d0b4176d0a7347ba6d2a2e7bfce448e9e5de4d80`; no preserved source commit is replaced.
Tracks issue #295 and the persistence prerequisite of #289. Required journal kind 8 encodes a
fixed 105-byte envelope with authority, ledger ordinal, previous/current payload and closed kind.
Additive index migration 3 reconstructs envelopes. Replay rejects contradictory or noncontiguous
history before mutation and preserves idempotent exact retries without rewinding ledger heads.
The payload still requires native semantic validation; no grant or enrollment API is enabled.

Native open and refresh refuse dependency-bearing history, direct dependency appends refuse, and
cached approval rereads the pinned journal before writing. Unknown transitive roots refuse
collection. Old disposable-index schema compatibility is not promised. Exact pre-change storage
source at the base revision was independently built and refused the new record kind with unchanged
fixture bytes; this is scanner evidence, not an already-running old desktop enrollment proof.
Future enrollment must install and verify a mandatory old-writer custody fence before its journal
record, including both crash boundaries. That requirement remains unimplemented.

The complete storage crate suite passed, including real SQLite reconstruction and existing
crash/recovery campaigns extended with dependency records. The native cached-approval/reopen
regression passed. Allowing a missing previous payload or disabling cached approval inspection
made their respective regressions fail; source was restored byte-exact. Two initial test fixture
compile failures are preserved. Full repository and hosted checks remain pending. The fixed
user checkpoint is unchanged; full private-input authorization and publication enforcement remain
required by the fleet plan.

R147 implementation `af708ababc2f42a8e75a06ebd0d8152d9bfd4d13` passed full `npm test`: 3,837 native tests in 173.217s (2 slow, 18 skipped), 194 rendered tests, 672 desktop tests and all 44 real-daemon checks. Hosted validation and merge remain pending.

## R148 — native dependency payload validation and historical replay

New canonical implementation based on PR #296 at
`d04729e505007fd04aebcbdad43417e385b9c2e7`; no preserved source commit is replaced.
Tracks issue #297 and the native policy prerequisite of #289. Canonical bounded payloads must match
the exact storage digest, authority, kind, ordinal and predecessor. Enrollment matches an independently
supplied native registration binding. Replay separates ledger order, exact input/destination grant
generations and per-input decision revisions. It rejects contradictory requests, stale/revoked grants
and stale/ineligible review vectors before changing state. Exact historical replay remains idempotent
at capacity. Rejection and replacement preserve direct operation/policy references and earlier reviews.

Nine focused native tests pass, including independent byte vectors, malformed/truncated/oversized
payloads, external-binding and envelope substitution, atomic refusal, distinct installation versus
stable-work identity, unrelated decisions, replacement, revocation and full record/aggregate-byte
limits. Weakening eligibility or current-grant enforcement makes the intended regression fail; exact
source restored. The initial missing digest-trait import compile failure is retained. Full repository
and hosted validation pending. This read-only projection does not authenticate control writes, verify
source ancestry or complete transitive closure, fence older writers or authorize consumption/publication.
Existing native dependency refusals and the fixed user checkpoint are unchanged.

R148 implementation `f7978bf33cb7ed82dd40ecb70c12191b14863170` passed full `npm test`: 3,847 native tests in 171.254s (2 slow, 18 skipped), 194 rendered tests, 672 desktop tests and all 44 real-daemon checks. Hosted validation and merge remain pending.

## R149 — durable preparation fence for older native writers

New canonical work based on main `60441cb0013df707d71f8781b27964514cb32310` (merged #294);
no preserved source commit is replaced. Tracks #299 and the older-writer prerequisite of #289.
A thread-bound native preparation guard installs a required custody v2 marker under the existing
physical-directory lock, binding installation/directory/dependency-authority identity. Existing generic
custody parsing remains unchanged and refuses the marker. Exact retry syncs the file and directory
before acknowledgement; errors retain the fence, and dropping the guard never restores write access.
Assigned, conflicting, malformed and substituted workspaces refuse. No enrollment journal writer,
agent/renderer/CLI entry point, policy-aware write capability or automatic migration is added.

Eighteen focused custody tests pass, including targeted retry-sync failure, exact retry, interrupted
staging, assigned/root substitution refusal and existing cross-process custody contention. Bypassing
retry durability makes the new regression fail; exact source restored. A cached native writer first
edits successfully, then refuses edit/create/review/agent acquisition after preparation, leaving file
and journal unchanged and not invoking signing. A separate pre-change source archive from the base
revision verifies all 1,413 tracked files against Git before adding the test probe; the same cached
paths refuse its exact marker. Removing that marker fails the refusal assertion; restored probe
passes. Its executable SHA-256 is
`efc788aab6865b6bc70c0cc44e08c3f6deb0d17454fb09a5bdea5b1843be061e`.

The initial cached fixture lacked checkpoint configuration, then required unwrapping its fallible
runtime constructor. Both failures are preserved. An erroneous one-hour fixture idle interval was
identified in source and a stack sample; only that verified owned test process was stopped, its
fixture/log retained, and the established one-millisecond native test interval restored. These are
fixture corrections, not product latency evidence. Full and hosted validation are pending. Valid
human-approval refusal, every historical writer, complete two-write enrollment crash recovery and
packaged acceptance remain unproven. The fixed user checkpoint is unchanged.

R149 implementation `ae1e2c0aa20380b4a63ae31113442ebc269d0584` passed full `npm test`: 3,837 native tests in 175.729s (2 slow, 18 skipped), 194 rendered tests, 672 desktop tests and all 44 real-daemon checks. Hosted validation and merge remain pending.


R149 delivery update: [PR #300](https://github.com/idosams/Mesh/pull/300) merged as
`ba0178935a0fdc9848fe80135169a16ac50c6856` after all seven hosted checks passed in run 37191604924.
Additional unchanged-source tests prove valid prepared managed approval refusal (with test P-256
credentials), including exact retry and unchanged journal/main. Removing the custody marker fails
the regression and the restored probe passes. This is not packaged human presence. Attached approval
was separately found to ignore that marker before enrollment; it needs R150's distinct preparation.

## R150 — required attached-history preparation

New canonical implementation based on merged R149 `ba0178935a0fdc9848fe80135169a16ac50c6856`;
no preserved source commit is replaced. Tracks #301 and #289. Preparation retains the original
attachment-history binding in a required v3 envelope under native store custody. It validates native
registration and retained history, publishes through a private staged file and atomic rename, and
requires file/directory durability before acknowledgement and exact recovery. Conflicting authority,
stage, invalid file mode, changed binding and replaced source/store refuse. Guard drop retains the
fence. Journal/main bytes and original source are not rewritten. Generic history readers remain
unchanged and refuse the required binding; no runtime entry point or complete enrollment is enabled.

Three focused regressions passed: prepared valid approval/capture refusal with journal preservation,
external editor continuity and exact retry; changed binding/source refusal; and injected initial/retry
sync failure. Removing the sync call makes its regression fail; original implementation restored.
An unchanged pre-change reader separately accepts attachment approval with only the managed custody
marker and refuses the required attachment-history binding. Its probe and binary are retained with
R149 evidence. An initial Rust error-message borrowing compile failure is preserved. Full repository
and hosted validation remain pending; the fixed user checkpoint is unchanged.

R150 implementation verification: Full `npm test` passed on implementation `cead2c5542f729c32cf458bf4b79c31fccbfe040`: 3,840 native tests in 171.744s (2 slow, 18 skipped), 194 rendered tests, 672 desktop tests and all 44 real-daemon checks. Hosted validation and merge remain pending.

R147 reconciliation with merged R149: implementation `291e09663237c65c15084d431dae5d2b7fe190b6` passed full `npm test`: 3,844 native tests in 175.049s (2 slow, 18 skipped), 194 rendered tests, 672 desktop tests and all 44 real-daemon checks. Both independent documentation sections were retained; no native conflict required resolution. Hosted verification of the reconciled head is pending.

R148 reconciliation with merged R149 and published R147 `6320642214f4bcbc31c6dbc4f266e397b2645f92`: Full `npm test` passed on reconciliation `6c7779b5f022b70047933352ba9a507483903618`: 3,853 native tests in 175.953s (2 slow, 18 skipped), 194 rendered tests, 672 desktop tests and all 44 real-daemon checks. Both independent documentation sections were retained and native code merged without manual resolution. Fresh hosted validation remains pending.

R150 reconciliation: based on PR #298 `3d3d26672f29a0f99359f148149726f4b0c68ff4`, including merged #296. Full `npm test` passed on combined revision `bdc156363e6ff147ad9936c1553b157c27e5ee8d`: 3,856 native tests in 172.064s (2 slow, 18 skipped), 194 rendered tests, 672 desktop tests and all 44 real-daemon checks. The ledger conflict retained both records; native code required no manual conflict resolution. Fresh hosted validation remains pending.


## R151 — recoverable native project enrollment

New canonical work based on PR #302, published parent
`044a600a908427d32686ba03e61e38470eeb8de2`; replaces no preserved source commit. Tracks #303 and #289.
A content-addressed canonical intent binds native registration/installation and exact clean journal
inode, length and digest. Its digest selects the local authority. Canonical enrollment payloads are
staged before generic and attachment fences; only then may the required record append. Recovery
accepts only the original journal prefix and this frame's exact suffix, writes missing bytes, syncs,
rereads and semantically validates before returning native facts. Source work and accepted history
are not rewritten. Generic readers/collection still refuse enrolled history; no runtime caller,
automatic migration, grant, private consumption or publication capability is enabled.

Five focused transaction tests passed, including all 146 frame-prefix cases in real files, staged,
between-fence and lost-acknowledgement recovery, explicit sync failure, exact reopen/retry, unrelated
fragments, changed prefix, identical-byte journal replacement and corrupt CAS intent. Generic custody
refuses pending mutation/cleanup after its marker. The new private borrowed-fence helper requires the
exact live initialization root; unrelated or expired custody refuses. Twelve combined enrollment
regressions passed in 27.07s, and the native accepted-main retention/refusal test passed in 0.26s.
A journal-sync bypass mutation fails the intended regression; source restored byte-exact. These are
deterministic fault injections, not a power-loss or full historical-writer campaign. Full repository
and hosted validation are pending. The fixed checkpoint remains unchanged.

R151 implementation verification: Full `npm test` passed on implementation `beda134de27be869a94e6aeda19574f9fb98a904`: 3,863 native tests in 199.444s (2 slow, 18 skipped), 194 rendered tests, 672 desktop tests and all 44 real-daemon checks. The accepted-main regression also preserves staged/dirty Git status and exact index/HEAD bytes. Hosted validation and merge remain pending.


## R152 — validated enrolled-history inspection

Tracks [issue #305](https://github.com/idosams/Mesh/issues/305), following native enrollment in
[PR #304](https://github.com/idosams/Mesh/pull/304). This increment extends its implementation;
it does not replace or drop a preserved source commit. R150 [PR #302](https://github.com/idosams/Mesh/pull/302)
merged as `86de1feca2c4bbf908018ca77b5dbc830488d4fc` after all seven hosted checks passed.

A sealed native proof verifies registration, both required fences, canonical retained intent,
original journal identity/prefix and bounded policy replay. The workspace opener binds the exact
whole journal even if it is replaced by a legacy-only prefix. Immutable journal/CAS handles and a
transient index serve saved versions/files, file inspection/comparison, exact saved reviews and
historically accepted main. Post-enrollment legacy approval records refuse until dependency-aware
publication is implemented; old accepted main retains its existing trusted-reviewer validation.
Prepared-only fences, torn frames, substituted journals and corrupt intent/payloads refuse unchanged.

Four focused native regressions passed, and the existing accepted-main integration test now proves
these reads across reopen with staged/dirty Git preserved. Removing the exact journal proof check
caused the replacement regression to fail (exit 101); production bytes were restored exactly.
The shared writer helper is unchanged: capture, review creation, approval and source-integration
admission remain fenced. No auto-enrollment, new grant/consumption API or complete dependency closure
claim is introduced. Full local verification and hosted delivery remain pending.

R152 implementation verification: Full `npm test` passed on implementation `6c61818cda5d078723e8a624f92cf80848457840`: 3,867 native tests in 195.008s (2 slow, 18 skipped), 194 rendered tests, 672 desktop tests and all 44 real-daemon checks. Hosted verification and merge remain pending.


R152 pre-merge admission correction: the restored inspection helper was also used by four native
consumption paths. A new regression demonstrated that manual lane allocation, agent input
validation/materialization and remote export all incorrectly admitted enrolled history. These
callers now use a separate helper retaining the original fenced admission; saved inspection keeps
its immutable reader. The same regression passes after the correction and proves no work-lane
allocation, destination files, journal change or source change. The earlier full gate did not cover
this gap; a fresh full gate is required for the corrected revision. No enrollment control is exposed
and neither the initial reader PR nor this correction has merged at this point.

R152 admission correction verification: Full `npm test` passed on corrected implementation `3902d7abde439f64545cb6d5064c1af82ab1793b`: 3,868 native tests in 200.938s (2 slow, 18 skipped), 194 rendered tests, 672 desktop tests and all 44 real-daemon checks. Hosted verification of this correction and merge remain pending.


## R153 — native saved-input eligibility decisions

Tracks [issue #307](https://github.com/idosams/Mesh/issues/307), extending R152 and replacing no
preserved source commit. The initial local writer was preserved as `e8491361150eba0472c5117f7a30b80945320b4e`;
this increment adds its required interrupted-append recovery before delivery. R151 merged in
[PR #304](https://github.com/idosams/Mesh/pull/304) as `7216af5f89353c3f32d6e6468cdd7cb1be1ab1e4`;
all seven exact-head checks and merged-main CI passed.

The native host can explicitly reject, replace or revalidate an exact saved version of an enrolled
registered root work. The root work identity is its native project registration, separate from
installation and provider/run identity. Native history verifies both selected and replacement
operations; caller-selected policy paths or foreign operation identities cannot select authority.
Decision revisions and predecessors are per input; authority ordinals remain separate. Exact
historical request retry returns its original fact without rewinding later decisions.

Canonical payloads are staged before a private transaction intent and journal append. The intent
binds request, original journal inode/device, exact prefix length/digest and payload. Native-only
recovery verifies that prefix, enrollment, both fences, canonical payload and exactly its own
eligibility-frame suffix before resuming. Unknown/conflicting intent, foreign suffix, changed source,
replaced journal and corrupt payload refuse without truncation. Normal readers still refuse torn
history. Rechecks immediately before append and before intent cleanup preserve substituted work.
Journal synchronization and exact replay precede acknowledgement; complete retries resync. Enrollment
retry after valid decisions returns the original enrollment without appending or rewinding.

Five focused real-storage tests passed, including all 146 frame-prefix interruption positions,
restart, sync/lost-ack recovery, stale/foreign/conflicting inputs and damaged recovery evidence.
Additional last-moment source/fence/intent refusal and accepted-main/dirty-Git preservation tests
pass. Removing journal synchronization makes the durability regression fail; source restored exactly.
An initial test-only direct inspection omitted custody and correctly refused; its corrected fixture
holds custody and passes. Full local and hosted verification remain pending.

This method has no agent, renderer or CLI caller. It does not yet enforce downstream closure at
publication, authorize consumption, bind other work memberships, or enable enrolled capture. All
those admission paths remain fenced. No physical power-loss or packaged GUI claim follows from the
native fault injections. The fixed user checkpoint remains unchanged.

R153 implementation verification: Full `npm test` passed on combined implementation `e57247f7cc2ea18cdfc7426a8d2c92b38d31723e`: 3,874 native tests in 228.506s (3 slow, 18 skipped), 194 rendered tests, 672 desktop tests and all 44 real-daemon checks. Hosted verification and merge remain pending.


## R154 — native root and descendant work selection

Tracks [issue #309](https://github.com/idosams/Mesh/issues/309), depending on R153
[PR #308](https://github.com/idosams/Mesh/pull/308) at
`0c3d17d725b5546eca7b3fbcfeaefb3e456090b6`. This new increment replaces no preserved
source commit. Native catalog selection binds enrolled root ownership, exact installations and
manual or delegated allocation ancestry independently of provider/run identity. Historical allocation
is correlation evidence only; it never retroactively grants consumption or publication.

Selection reopens exact registrations and verifies every saved source version under one ordered
custody set. Eight ancestry edges require at most 27 roots. Foreign work, descendant root authority,
cycles, overflow, changed receipts and replaced source/store/allocation containers refuse. Immutable
bindings hold no lock; consuming transactions still need their own custody and current policy checks.

Five real-storage regressions cover nested manual work, restart/editor writes, foreign and unenrolled
owners, receipt/source/store substitution, the exact depth boundary and allocation replacement with
unchanged work/installation. Removing allocation identity from the correlation makes the replacement
regression fail; the original source was restored exactly. Full local and hosted gates remain pending.
No grants, new allocation protocol, closure enforcement, runtime entry point or packaged claim is
introduced. The user checkpoint remains unchanged.

R154 implementation verification: Full `npm test` passed on implementation `ee53b1b346ee23803f261a31898a06c76f243210`: 3,879 native tests in 281.518s (4 slow, 18 skipped), 194 rendered tests, 672 desktop tests and all 44 real-daemon checks. Hosted verification and merge remain pending.


## R155 — native exact-input grants and revocation

Tracks [issue #311](https://github.com/idosams/Mesh/issues/311), extending R153/R154 at
`f2ec4491ba80a3b5c6e1b9a23f3c6df719382c06`. Replaces no preserved source commit. R152
[PR #306](https://github.com/idosams/Mesh/pull/306) merged as
`64d1139db22cc25b0d0d51b2b4fd6b8ad61d1b4a`; its seven exact-head checks and merged-main
CI passed. The local grant branch reconciles that shared history without rewriting published heads.

The trusted native host can grant, revoke or regrant an exact saved input to an existing native
destination work. Source and destination ancestry are prepared together, then revalidated under the
complete bounded custody set before staging and immediately before append. Requests bind native
project/work/installations and physical correlation; historical retries return the exact recorded
outcome without rewinding later revocations. Stable work/installation alone cannot reuse a grant
after an allocation container is replaced. Grants remain separate from decisions and publication.

New grant payloads use `mesh.dependency-policy/v2` with two required nonzero native correlation
digests. Existing v1 history remains readable without upgrading its unbound grants into native
authority. The shared decision/control transaction retains the original decision intent schema and
uses `mesh.dependency-grant-intent/v1` for grants. Both use the existing private pending-file slot;
only the complete journal record establishes an outcome. Exact prefix recovery, synchronization,
replay and request verification precede acknowledgement. Unknown pending work is preserved.

Six grant regressions cover progression/restart/editor preservation, self/foreign/stale/conflicting
requests, all 146 partial-frame boundaries, synchronization/lost acknowledgement, changed destination
receipts and replacement with unchanged work/installation. Six existing decision regressions passed
after extracting the shared transaction; the final full gate must cover the later grant extension too.
Ten policy tests passed, including strict v2 bindings. A shared-custody test rejects incomplete or
released guards. Removing custody-membership checking and removing grant synchronization each cause
their regression to fail; source restored exactly. The exact previous policy validator from
`f2ec4491ba80a3b5c6e1b9a23f3c6df719382c06`, compiled with the current crate types in a temporary
test module, refuses a bound v2 grant and accepts the legacy positive control. This is semantic-reader
compatibility evidence, not a previous packaged application run. The initial temporary fixture had
the wrong module location; the corrected run passes and both logs are retained.

Full local and hosted validation remain pending. No agent/renderer/CLI caller exists. No content is
materialized, child history enrolled, grant consumed or publication enabled by this increment. Native
allocation/consumption, old-writer fencing, exact starting operation, full inherited closure, all-path
publication/recovery and packaged acceptance remain open. The fixed user checkpoint is unchanged.

R155 implementation verification: Full `npm test` passed on implementation `af9a40ddd7cd94c5d1b25d5e16368364db511ba1`: 3,887 native tests in 369.689s (5 slow, 18 skipped), 194 rendered tests, 672 desktop tests and all 44 real-daemon checks. Hosted verification and merge remain pending.


## R156 — owning-project decisions for native child work

Tracks [issue #313](https://github.com/idosams/Mesh/issues/313), extending the published grant
increment [PR #312](https://github.com/idosams/Mesh/pull/312) at
`9345d52a7c6891a2264c8f5bbc4595a2d0952ec9`. Replaces no preserved source commit.

The native host can reject, replace or explicitly revalidate an exact saved input of a manual or
delegated child work in the owning project's journal. Native ancestry and exact saved operations
are checked under one complete custody set, including after staging. Decision keys use stable
source work, installation and operation; they do not collapse child identity into the project root.
Replacements must belong to that same source work. Existing root request identity and payload schema
are retained; historical retries never rewind later decisions. Child history and ordinary editor
content are not rewritten by an eligibility change. Grants remain separate.

Five native tests pass: child rejection/replacement/revalidation with exact policy-key inspection;
foreign input/replacement and descendant-root refusal; catalog-reopen recovery at frame-prefix
positions 0, 1, 72, 144 and 145; changed ancestry between staging and append; and existing root API
request compatibility. The shared transaction's exhaustive frame tests remain part of the full gate.
Changing the persisted child work identity to its owning root makes the regression fail; source
restored exactly. Full local and hosted verification remain pending.

No agent/renderer/CLI route or downstream eligibility enforcement is enabled. This supplies child
decisions that the still-required full closure, consumption and publication paths must consult.
Private allocation, exact starting-operation/consumption receipts, retention, all publication/import/
recovery checks and packaged acceptance remain open. The fixed user checkpoint is unchanged.

R156 implementation verification: Full `npm test` passed on implementation `8036b089600600665ba904f28874ddd10e6f90e2`: 3,892 native tests in 268.630s (5 slow, 18 skipped), 194 rendered tests, 672 desktop tests and all 44 real-daemon checks. Hosted verification and merge remain pending.


## R157 — current native grant inspection under retained custody

Tracks [issue #315](https://github.com/idosams/Mesh/issues/315), extending R156
[PR #314](https://github.com/idosams/Mesh/pull/314) at
`fc7db8fa52445c654b34ceda08a23e80b6306085`. Replaces no preserved source commit.
R153 [PR #308](https://github.com/idosams/Mesh/pull/308) merged as
`5bc0e79ee5b3f24a149ef9bbfc4b7a607034d954`; its seven exact-head checks passed, with
merged-main verification still pending at this entry. The local branch reconciles that shared history
without modifying published branches that are under verification.

A native synchronous read callback enters only for the exact current allowed grant and matching
source/destination physical correlation. The complete native custody set remains held through the
callback; source history is read-only and its snapshot is fixed. Revoked, superseded, unbound, wrong
destination and unfinished-control evidence refuse before callback entry. Native associations and
current access are revalidated before returning its result. Callback errors do not roll back any
caller side effects; this is neither a consumption transaction nor publication authority.

Six native tests pass, covering root and child snapshots, immutable bytes after editor changes,
read-only source enforcement, distinct access and eligibility decisions, callback non-entry on
denial, pending-control preservation, retained/released custody, changed ancestry and replacement
with unchanged stable work/installation. A policy regression refuses a legacy unbound grant. Removing
the current allowed-generation predicate causes the revoked-grant callback regression to fail; source
restored exactly. Initial compile checks exposed the existing file reader's concrete output type and
an anonymously imported trait; the corrected generic output retains its native chunk verification.
Failed and passing logs are preserved. Full local and hosted validation remain pending.

No renderer/agent/CLI invokes this API, no destination is materialized or launched, and no consumption
receipt is recorded. Full inherited closure, exact starting operations, allocation recovery, retention
and all publication/import/recovery enforcement remain required. The fixed checkpoint is unchanged.

R157 implementation verification: Full `npm test` passed on implementation `c62f57711174e82fc5c00bac1e558ac43bc9ab0c`: 3,899 native tests in 264.898s (5 slow, 18 skipped), 194 rendered tests, 672 desktop tests and all 44 real-daemon checks. Hosted verification and merge remain pending.
R153 merged-main CI `37201308084` subsequently passed on `5bc0e79ee5b3f24a149ef9bbfc4b7a607034d954`.

## R158: Native capture after dependency enrollment

Tracks [issue #317](https://github.com/idosams/Mesh/issues/317). Builds on merged R157
[PR #316](https://github.com/idosams/Mesh/pull/316), merge
`035dc70169d7144fd53e17ff8097c77c8e9e0a39`. Replaces no preserved source commit.
R154 #310, R155 #312 and R156 #314 also merged through their seven exact-head checks and successful
post-merge main checks. R157 post-merge main verification `37204965382` subsequently passed on that merge.

The new native prepare/commit path keeps inspection read-only, signs outside custody, and refreshes
exact enrollment, registration and capture position before appending authenticated private progress.
Immutable content and framed records are staged before a private exact-request intent. Recovery
completes only the identical journal suffix; retained request receipts recover historical results
without duplicating appends or rewinding later saves. Current native control and capture pending
writes exclude each other. No runtime enrollment, destination allocation, consumption or publication
is enabled by this increment.

Six checkpoint regressions passed after separating preparation from persistence. Eight native
capture/recovery tests passed, including every byte prefix of a small capture, exact snapshot bytes
after editor changes, a signer without custody, stale policy/basis refusal, historical request retry,
failed synchronization, and preservation of foreign suffixes. Removing the commit sync makes the
sync-failure regression fail; the implementation was restored exactly. Initial compilation and
private-fixture permission failures are retained with the corrected passing evidence. A further
source-replacement recovery regression and the complete repository gate are pending.

The full allocation/consumption, closure/retention, publication, runtime and packaged acceptance
scope remains open. This increment stays unpublished until its complete checks pass. The fixed
checkpoint stays unchanged.

R158 initial full verification on `965753f65a30407e5e1a150bf437ef38ba1806f0` passed:
3,909 native tests in 274.263s (5 slow, 18 skipped), 194 rendered tests, 672 desktop tests and all
44 real-daemon checks. Review then identified an unnecessary conflict when only dependency policy
changed during signing. The corrected regression failed before refinement and passes afterward.
Commit now refreshes current policy while requiring the same native enrollment and exact signed
authoring basis; an advanced actor cannot be reused even if a mutable capture line is rolled back.
Ten focused capture/recovery tests pass, including physical source replacement and every frame
prefix. The complete gate on final implementation `1d0c000875c33956c2f01423e491d03245ec9347`
passed 3,910 native tests in 275.798s (5 slow, 18 skipped), 194 rendered tests, 672 desktop
tests and all 44 real-daemon checks. Hosted validation and merge remain pending.


R158 delivery: [PR #318](https://github.com/idosams/Mesh/pull/318) merged at
`027c8d2debb06cda81fd5e32be2c28c3a10f4473` after all seven hosted checks in run `37206573170`
passed on published head `9b006c690220211f24e7ffff4f3679f640355867`. Post-merge run
`37207400781` passed on that merge. Earlier pending statements above are historical development entries.

## R159: Reserve a native destination before granting or copying input

Tracks [issue #319](https://github.com/idosams/Mesh/issues/319), builds on merged R158, and replaces
no preserved source commit. Initial implementation `090fcd5` passed five focused tests; ancestry-only
reconciliation `693124f` incorporated the canonical R158 merge without dropping local work.
The refined local implementation passed 14 reservation/ancestry regressions in 5.203s.

The destination starts as empty files plus its own native enrolled history inside private allocation
staging. The ordinary project catalog sees that history only after required writer fences are durable.
Descriptor-relative exclusive rename refuses an occupied target, including an empty directory.
Reservation intent, physical identities and a distinct reserved receipt establish native lineage;
no legacy ready receipt or consumed-input claim is produced. A completed reservation remains stable
through manual captures and can parent another exact reservation within the existing ancestry bound.
Native grant/revoke binds the destination while source content remains uncopied.

Exact retries handle initialized, fenced, published and acknowledgement-lost states. Partial evidence
without identity receipts or conflicting user work is retained and refused. Child history enrollment
is separate from the owning project's policy authority; descendants cannot self-adopt as root owners.
Unknown fields and stale parent correlations refuse. Retry validation reuses the complete held custody
set instead of trying to acquire a nested set. The earlier failing nested-set regression and initial
compile failure are preserved. Replacing exclusive publication with ordinary rename made the collision
regression fail; the implementation was restored byte-for-byte before further validation.

The full Mesh gate, hosted delivery and post-merge proof are pending. Actual input consumption still
requires complete inherited closure and retention, authenticated starting operations, owning-project
consumption receipts and exact crash recovery. Empty starting snapshots, runtime integration,
dependency-aware publication/import and all remaining fleet acceptance stay in scope. The fixed
user checkpoint is unchanged; no installed application or repository settings were modified.

R159 full gate on `7ef5088b885cf86224353b55d409487a9d604335` passed: 3,918 native tests in 271.700s
(5 slow, 18 skipped), 194 rendered tests, 672 desktop tests and all 44 real-daemon checks, including
repository/docs/license/storage/fmt/clippy checks. The native interruption fixtures do not establish
power-loss or packaged acceptance. Hosted checks and merged reservation delivery remain pending.


R159 delivery: [PR #320](https://github.com/idosams/Mesh/pull/320) merged at
`227fc60d7ce4f5730d74055380567daf1ef0d2f2` after all seven hosted checks in run `37208335397`
passed on head `953b5cd285e386a3dc1b990859545b711f3b59e2`. Post-merge verification
`37209051599` is running. The earlier pending R159 entries above are historical.

## R160: Complete native input ancestry and retained roots

Tracks [issue #321](https://github.com/idosams/Mesh/issues/321), builds on merged R159, and replaces
no preserved source commit. Initial local helper commit `0c5ba6e` verifies immutable signed operation
facts, including exact parent/header agreement and bounded native CAS reads; three focused tests
passed in 0.136s. An initial test CAS type-inference failure was corrected. Removing the signed-parent
comparison makes its refusal test fail on an emptied journal parent list; exact source was restored.
Ancestry-only merge `7b1769d` reconciles that work with canonical R159 without changing its file tree.

This is unfinished groundwork, not complete dependency closure or a passing full-gate increment.
The reader is now connected to bounded native graph inspection. Typed historical consumption facts
add native-bound source edges; receipt input declarations are checked against independent traversal.
A real two-save history regression first exposed an incorrect workspace identity derivation; using
the capture writer's existing domain-separated function fixed it. The focused ancestry/policy/graph
run passed 17 tests in 2.475s without unused-code warnings, including reopen and unsaved-editor
isolation. Retain both failing and passing logs. This does not establish retained-root completeness.
Do not claim a flat caller-provided input list proves complete ancestry. Native work resolution,
qualified DAG traversal, typed owner-consumption facts, bounds/conflict/cycle checks, manifest/chunk
retention, pending roots and rebuild/rejection tests remain to implement in this increment.
Reservation lineage alone is not a consumed-input edge. The full consumption/starting-operation,
old-writer pending-copy fence, all-path publication/import/review and runtime/acceptance scope remains
open; the fixed user checkpoint is unchanged.

R160 content groundwork verifies journal manifests and streams historical chunks without quarantine,
including content replaced in later saves. Logical manifest identity, contiguous chunk layout, chunk
length/hash and whole-file digest must agree. The graph carries qualified chunk facts, with bounded
per-manifest references and a shared 1 GiB read budget. Six focused tests passed in 0.493s; missing
or corrupt earlier content refuses and corrupt bytes remain untouched. The initial compile failure
(missing streaming digest trait import) and passing log are preserved. This still does not establish
complete policy/pending-object retention, rebuild or cross-work coverage and is not ready for PR.

R160 selected-graph retention facts now qualify authority/policy payloads, operation payloads,
logical manifests and chunks by native work, installation, physical store and correlation. Policy
rejection preserves content and graph identity. Early reference bounds refuse before loading more
nodes. Focused policy/graph/ancestry tests passed 18/18 in 2.443s. Extended native graph tests passed
4/4 in 1.95s with three real native works and test-only owner consumption replay fixtures, exercising
incomplete closure and missing intermediate-work refusal, transitive roots and owner/child store
separation. This is not production consumption transaction proof. Attachment indexes rebuild in
memory: an explicit removal-count assertion caught the initial vacuous disk-index test. Its replacement
proves corrupt on-disk caches are ignored and preserved, and absent caches yield identical facts.
Omitting policy roots makes the regression fail on the missing rejection payload in 0.51s; exact
passing source restored. Full pending/completed transaction retention and full-gate/PR delivery remain
unfinished. All failing and passing logs are retained.

R160 capture recovery-root groundwork adds an exact native read-only request inspector for pending
and completed receipts. It reconstructs known torn capture frames in memory and verifies local
authenticated operation/manifests/chunks through the same helpers as graph reads; the journal,
sidecars and newer source files remain unchanged. The result includes the staged frame CAS object
and qualified local recovery evidence, not collection permission or a whole-project closure.
Combined focused verification passed 18 tests in 15.532s. After wiring the signed-payload byte budget,
11 capture tests passed without warnings in 15.10s, including root inspection before recovery at
every frame-byte boundary and historical receipts after later saves. Omitting the frame root makes
the native regression fail in 0.24s; exact passing source restored. Failed compilation and unused
budget intermediate logs are retained. Pending control objects and complete graph/recovery-root
composition still need implementation before full validation and R160 PR delivery.

Pending grant/eligibility control roots are now inspectable under native custody without applying
the transaction. Exact request, physical prefix, staged payload and policy replay must agree; local
policy CAS roots remain distinct from references into other works. Both existing every-byte-prefix
recovery tests now inspect retention before recovery and assert unchanged journal bytes. Changed
pending intent, source/journal substitution, foreign suffix and corrupt payload also refuse. Focused
verification passed 12 tests in 80.930s (two slow). Full validation and a draft PR follow; R160 remains
unmerged and incomplete until atomic graph/recovery-root composition and outstanding #321 proof
are finished. No runtime copying, publication, generic collection or checkpoint package changed.

Full Mesh validation passed on implementation `9a6f4b7a240775b81757d41ced49f641caacca6f`: 3,926 native
tests in 272.003s (five slow, 18 skipped), 194 rendered, 672 desktop and 44 daemon checks, with all
static gates passing. The initial full run failed Clippy's map-entry rule; `9a6f4b7` corrected the
lookup and the complete rerun passed. The draft PR publishes this verified groundwork before further
R160 refinement. It does not close #321 or authorize merge while composition/fault coverage remains
unfinished. Subsequent documentation-only evidence changes pass docs and diff checks.

Draft [PR #322](https://github.com/idosams/Mesh/pull/322), head `1cbbab33934e7d6491b0518431b850e7fedcb72a`,
publishes R160 groundwork with full local gate evidence. Initial hosted run `37211764661` had five
passing checks and Linux/macOS tests still running; no merge or published-head replacement occurred.

Further local R160 composition ties completed capture sidecars and frame CAS objects to the same
graph custody snapshot, and retains complete pending control sidecars awaiting acknowledgement.
Receipt inventory and aggregate verification work are bounded. A new native regression initially
failed because two receipt requests could claim one operation; duplicate detection now refuses.
The final focused graph suite passed five tests in 2.31s, including corrupt recovery-object preservation,
byte-identical installation substitution/conflicting handles, restoring original native identity, and
legacy provenance refusal without losing saved work. Logs preserve the failing-before case and
passing refinements. Full validation and publication of this refinement are pending; R160 remains
draft until remaining recovery composition and executable #321 evidence are complete.

R160 local refinement now composes fully journaled pending capture evidence with graph retention;
torn or unappended captures require the exact recovery inspector and cannot produce a complete
graph. Signed multi-parent operation fixtures verify shared-parent deduplication and missing-parent
refusal. An actual legacy allocation regression reproduced a copied lane incorrectly being reported
as dependency-free after enrollment. Graph inspection now refuses that copied origin until explicit
migration evidence exists, preserving its journal and files. Allocation identity for an empty native
reservation remains distinct from consumption. The five focused graph tests pass (2.67s); full
validation and publication of these refinements are pending. Draft PR #322 remains unmerged, with
seven successful checks on its older published head `1cbbab33934e7d6491b0518431b850e7fedcb72a`.

R160 composition implementation `30d658add820195b9d0a43ee2c9bc94ecbc4ffe4` passed the full `npm test`
gate: 3,927 native tests in 274.864s (five slow, 18 skipped), 194 rendered tests, 672 desktop tests,
44 real-daemon checks, and repository/docs/license/storage/format/lint checks. This completes local
validation of the graph/recovery composition and legacy-copy refusal refinement; publication and
hosted validation of this revision are next. The earlier draft CI is not evidence for this revision.
Native consumption copying, publication/import/review enforcement and the full fleet acceptance
scope remain open. The fixed user checkpoint is unchanged.

R160 [PR #322](https://github.com/idosams/Mesh/pull/322) merged normally at
`5afe7757150eb4f6c079768c9473b5da3cc8b14e` after all seven checks passed on head
`0a0f2b211d295189cce2670d4cbc03c0907e7a33` (run `37213698088`). Post-merge run `37214307093`
is still active; this is not yet a post-merge pass.

R161 [issue #323](https://github.com/idosams/Mesh/issues/323) tracks native consumed starting
versions, required older-writer fencing and exact interrupted-copy recovery. Local groundwork
separates native graph/grant selection from validation under an already-held complete custody set.
The ordinary public entry points still acquire their own guards and refuse nested acquisition.
Preparation conveys no permission: validation rechecks physical bindings and current grants,
including a grant revoked after preparation. A native composition fixture verifies graph and
immutable granted-content reads under one guard, and refuses source-only custody when destination
custody is missing. Five graph tests (2.75s) and seven grant tests (1.43s) pass. Full validation and
publication are pending. No consumption copying, starting-operation transaction or new persisted
fence is enabled by this groundwork; the entire #323 scope and fleet goal remain open.

R161 custody groundwork `a3a99f8e35e86d230aa578c53baa0d742f03bf44` passed full `npm test`:
3,928 native tests in 269.544s (five slow, 18 skipped), 194 rendered tests, 672 desktop tests,
44 real-daemon checks and all repository/docs/license/storage/format/lint gates. Publication as a
draft follows so the unfinished consumption increment remains reviewable. This does not complete
#323 or enable consumption. Main's post-R160 run still has macOS verification active; its other
six jobs have passed.

Draft [PR #324](https://github.com/idosams/Mesh/pull/324) publishes the R161 custody groundwork at
`81babaa25ea86064a7e40b831f41f3d5fafa7970`; its hosted run is `37215060435`. R160 post-merge main
run `37214307093` has now passed all seven checks on `5afe7757150eb4f6c079768c9473b5da3cc8b14e`.

Further local R161 proof uses an authenticated `InitializeWorkspace` operation in a real empty
native reservation. Its saved snapshot and qualified graph reopen identically twice, with no files
or directories invented in the destination. All five focused graph tests pass in 3.20s. An initial
fixture omitted native read custody and correctly failed; it was corrected without relaxing the
reader. This is signed native journal replay, not a delivered consumption transaction. Copying,
required persistent fencing, owner consumption commit and exact recovery remain unfinished. The
published draft head and fixed user checkpoint remain unchanged while hosted checks run.

Local R161 snapshot preparation now builds deterministic initial operations and prepared manifests
from exact granted immutable content, with an explicit workspace-root declaration, file/entry/total
budgets, and an output sink that refuses growth beyond the admitted file length. It does not write
destination files or history. Eight grant tests passed (1.51s); the new stream-bound test passed,
and the granted-snapshot test passed again through that sink (0.40s). Unsaved source edits are not
used and the owner journal remains unchanged.

This internal builder is not yet wired into production consumption/signing or recovery. Full
validation and publication of this local refinement remain pending; the earlier full gate covers
only the published custody groundwork. A further transaction requirement is explicit binding of
the copied snapshot's ignore rules: the empty reservation already bound its original capture policy,
so copied rule files cannot silently change that policy or break later captures. Resolve that in the
required consumption binding together with the pending-writer fence. The full #323 scope remains
open and the published draft head remains fixed while hosted checks run.

R161 signed preparation now connects complete graph/current-grant checks, exact empty native
reservation validation and bounded snapshot construction to authentication outside custody. Its
result is an immutable candidate, not saved work: no history, policy or destination files are
written. Revalidation reacquires the complete set and refreshes the graph, current grant, physical
binding, enrollment and empty destination. Native fixtures prove that signing can independently
acquire custody, unexpected destination edits remain preserved/refused, revoked candidates cannot
be reused, and an empty source prepares a signed candidate without creating a saved version or fake
files. Five graph tests pass in 4.47s; the initial compile check passed without warnings.

The candidate computes a prospective history binding from saved ignore-rule files without changing
the reservation's current binding. Persisting and verifying that transition belongs to the upcoming
required transaction fence; dedicated rule-binding fault coverage is still needed. There is no
commit API yet. Full validation of these local refinements is pending. All seven hosted checks on
the older published draft head `81babaa25ea86064a7e40b831f41f3d5fafa7970` passed (run `37215060435`);
that result must not be attributed to the newer local signed-preparation implementation.

Dedicated R161 ignore-rule coverage now verifies that the signed workspace identity uses the saved
rule file's prospective binding, excludes ignored saved input, rejects invalid signatures, and
leaves the current reservation marker and both journals byte-identical. Later unsaved rule edits do
not alter the candidate identity. The test passed in 0.77s. A negative mutation signing with the old
reservation configuration failed the exact workspace-identity assertion in 0.54s; the unmodified
source was restored byte-for-byte. Full validation of this refinement follows. This does not yet
persist the policy transition or enable copying; #323 remains open.

R161 signed-preparation implementation `b27eb7d7175989aee6ddd022f0ae01514994d1b9` passed full
`npm test`: 3,931 native tests in 271.702s (five slow, 18 skipped), 194 rendered tests, 672 desktop
tests, 44 real-daemon checks and all repository/docs/license/storage/format/lint gates. Draft PR #324
will be updated with this verified refinement; new hosted validation remains required. It remains
draft because persistent fencing, materialization, owner/destination commit ordering and recovery
are unfinished. The fixed checkpoint and full fleet goal are unchanged.

R161 required consumption storage now adds destination start/completion subtypes 5 and 6 to the
existing required dependency envelope, with migration 4 preserving populated indexed rows. Migration
3 is unchanged. Commit `9415a315e164fa72057d0e6ce1beedafe4dfd8ee` passed 342 storage tests (one timing
benchmark skipped) and all-target daemon compilation. Reopening and raw journal reconstruction are
verified separately; the in-memory index is not implicitly loaded on open.

The local destination policy projection now binds a start to an exact request, owner enrollment,
destination installation, granted source, correlation bindings, original/prospective configuration
digests, complete-closure digest, signed starting operation and staged-object digest. Start is
allowed only as the first policy record after enrollment; completion must name that exact start
and its owner receipt. Unrelated policy advancement while pending, conflicting completion,
second start, malformed identities and duplicate request reuse refuse atomically. Exact replay
retains the same historical result and all direct references. Native admission still refuses both
pending and completed-looking records until cross-store consumption verification is implemented.
A native capture/control test confirms this refusal preserves editor bytes and history; removing
the admission check makes that test fail because capture incorrectly reaches preparation.

These are staged R161 foundations within draft PR #324, not a completed consumed version. Durable
materialization, independent owner-receipt verification, recovery, actual previous-writer proof,
full current-tree validation and merged delivery remain required. The fixed user checkpoint and
all graphical/provider/remote acceptance requirements are unchanged.

R161 required-record/provenance implementation `2a39f854ac14dbc503d5367a6410c50636261d8d`
passed full `npm test`: 3,937 native tests in 273.860s (five slow, 18 skipped), 194 rendered tests,
672 desktop tests, 44 real-daemon checks and all repository/docs/license/storage/format/lint gates.
The earlier full-gate lint failure and native-gate negative mutation remain preserved. This gate
validates the currently refusing foundation; it does not prove consumption commit/recovery or
actual previous-binary compatibility. Draft PR #324 remains unmerged and issue #323 remains open.

R161 previous-native compatibility is now executable evidence. The version-independent fixture
`required_consumption_prefixes_fence_native_read_prepare_commit_and_control` passed on current
source (3.12s) and exact previous implementation `220f2da76d7e1e35c8b2801db0b8dedc63d1aec8`
(2.89s). The audit verified that the entire tracked previous tree differed only by the appended
fixture; production code and schema were the previous revision. It recognized neither new subtype
and refused all 145 nonempty prefixes of each valid-checksum frame. Native read, new preparation,
commit of an already signed candidate and control mutation all refused while preserving journal
and editor bytes. A control capture after restoring the fixture's own baseline still committed.
The current tree recognizes both frame types and passes the same refusal campaign.

All overwritten current source bytes were backed up and restored with SHA-256 checks; the branch,
HEAD, published PR and fixed user checkpoint were unchanged. The compatibility fixture, overlay
manifest, exact previous revision and logs are retained with R161 evidence. This proves the required
journal boundary in the real previous native implementation, including an already prepared writer;
it is not a packaged desktop upgrade test. The production transaction must still durably append
that boundary before its first materialized byte and recover interrupted copying/owner receipts.

R161 compatibility implementation `62031d610e854ac72ae04f2e1dd1b3a0f442de46` passed full
`npm test`: 3,938 native tests in 315.123s (five slow, 18 skipped), 194 rendered tests, 672 desktop
tests, 44 real-daemon checks and all repository/docs/license/storage/format/lint gates. The previous
published head `8989b942eba30e07af8cc37022a41f778f47f387` also passed all seven hosted checks in
run `37217784394`. Compatibility proof does not close issue #323: durable staging/materialization,
owner consumption, cross-store admission and exact interrupted recovery remain unfinished. Draft
PR #324 remains unmerged; the full fleet goal and fixed user checkpoint are unchanged.

R161 now extends the existing retained-tree addition helper with explicit bounded staging and
canonical recovery receipts. Existing callers retain their 64-entry/64-MiB caps; the new internal
seam accepts admitted limits up to 100,001 entries including the staged root, 1 GiB total and
64 MiB per file. Entry ordering, duplicate/traversal paths and byte budgets are checked before
creating a private tree. A receipt binds source/recovery roots, exact target path, parent identity
and metadata, bounded tree evidence and original entry identities. It is evidence only: a consuming
native transaction must authenticate and durably retain it before authorizing installation.

Recovery reconstructs either the exact private stage or the exact already-installed tree. Retry
synchronizes both parents and preserves the same installed inode. Missing/both-present stages,
substitution, changed content, extra entries, different roots/path or parent policy refuse without
cleanup. All 46 related retained-replacement tests passed, including a 100-file tree and separate
processes: one installer exits immediately after rename, then two fresh recovery processes use only
the saved receipt and retain the same tree identity. Removing installed-tree retry handling makes
that process test fail; correct source was restored byte-for-byte.

This is a materialization/recovery primitive for the unfinished R161 transaction, not consumption
commit or runnable-lane admission. It is not yet connected to the required consumption journal
fence, source closure, signed start or owner receipt. Those cross-store steps and the full fleet
acceptance scope remain required; draft PR #324 and issue #323 remain open.

R161 bounded-tree implementation `d232d2697b251a255cac71047fc35c4217e1ea0d` passed full
`npm test`: 3,942 native tests in 274.126s (five slow, 18 skipped), 194 rendered tests, 672 desktop
tests, 44 real-daemon checks and all repository/docs/license/storage/format/lint gates. Previous
published head `ba88b38049f21318638eccdfbf4b744c7cdb8ef6` passed all seven hosted checks in run
`37218506637`. These results validate the staging primitive and existing behavior; full consumed
start commit, journal-before-install ordering, owner receipt and cross-store recovery remain open.
Draft PR #324 is not merged, and the fixed test checkpoint is unchanged.

R161 now provides the corresponding exact retained-file recovery seam. Canonical receipts bind
source/recovery roots, target path, parent policy, original allocation identity (including its
creation-time discriminator), file metadata, byte length and content digest. Resume requires
independently retained saved bytes within the caller's admitted limit. Wrong bytes or receipts,
same-byte replacement files, missing/both-present entries, changed roots/path or later editor edits
refuse without overwriting or cleanup. Already-installed retries synchronize both parents and
recheck identity/content; an edit during synchronization cannot be acknowledged. Empty files work
with a zero-byte budget. Receipts remain evidence only, not consumption or write authorization.

All 50 retained-replacement tests passed. A file installer exits in a separate process immediately
after rename; two fresh recovery processes reconstruct from saved receipt/content and preserve the
same allocation identity and executable state. Disabling installed-file retry handling makes that
process test fail; the correct source was restored byte-for-byte. Existing creation/restoration
callers use the same exact recovery validation without widening their admission policy.

Tree and file primitives now cover both top-level entry types. They still need native integration
with the signed starting snapshot, staged transaction/source closure, required journal-before-install
ordering and owner consumption receipt. No consumed version or runnable lane is acknowledged by
these helper changes. Draft PR #324 and issue #323 remain open; the fixed checkpoint is unchanged.

R161 retained-file implementation `c62c293303d1554c9f25ba7e729cb3f934ba37ae` passed full
`npm test`: 3,946 native tests in 314.880s (five slow, 18 skipped), 194 rendered tests, 672 desktop
tests, 44 real-daemon checks and all repository/docs/license/storage/format/lint gates. The first
full attempt found a redundant import; its failed log is retained alongside the corrected passing
run. Published predecessor `1a28f903f3f0015b6c8f9f6086b704b3c30b1330` passed all seven hosted checks
in run `37219440756`. These results cover the exact retained-file recovery seam and existing behavior,
not a completed cross-store consumption transaction. Draft PR #324 and issue #323 remain open;
the fixed user checkpoint remains unchanged.

R161 preparation and revalidation now reconstruct an exact materialization plan from the authenticated
initial operation and retained checkpoint objects. The verifier binds the prospective workspace,
genesis sequence/parents/epoch/time/base and independently derived resulting head. It accepts only
the complete initial-tree operation grammar: explicit empty root, newly created directories and
files, exact links and matching file versions. Missing parents, aliases, duplicate paths/objects,
extra operations and unrelated records refuse. The plan contains paths, manifest identities and
executable flags; it does not contain a second copy of file bodies.

Content verification checks each logical manifest, chunk layout, retained chunk digest, reconstructed
file digest, per-file and whole-tree byte limits, and exact referenced-object coverage. It streams
hashing over retained chunks. Reconstructed paths have a 64-MiB aggregate bound checked before each
path allocation; retained objects have the admitted content budget plus the existing 16-MiB signed
payload ceiling. Bounds refuse explicitly without truncating the tree or writing destination files.

Eight focused tests passed, including real signed nonempty/empty sources, preserved saved ignore
rules, tampered/missing staged content, invalid bounds, nested executable plans and conflicting
paths/versions. Valid signatures over a false genesis base or resulting head are refused. Removing
the resulting-head check makes the signed refusal test fail; the exact source was restored. Full
repository validation is pending. This connects signed preparation to a verifiable materialization
plan; durable staging, journal-before-install ordering, owner consumption and cross-store recovery
remain unfinished in draft PR #324 / issue #323. The fixed user checkpoint remains unchanged.

R161 signed-plan integration `11854f4c7374da54a7c95a3e431680c3e1416d1e` passed full `npm test`:
3,948 native tests in 274.899s (five slow, 18 skipped), 194 rendered tests, 672 desktop tests,
44 real-daemon checks and all repository/docs/license/storage/format/lint gates. The exact negative
head-check mutation and successful focused/full logs are retained. Published predecessor
`59eeee06b6334372f675e413f4e29cd26fa1d630` passed all seven hosted checks in run `37220272359`.
This validates signed-plan reconstruction and existing behavior; the durable consumption transaction
and complete fleet acceptance remain unfinished. Draft PR #324 is unmerged; the checkpoint is fixed.

R161 now connects prepared signed versions to durable private staging through
`PreparedNativeConsumedStart::stage`. Staging uses the native reservation allocation on the same
filesystem as its unchanged destination root. A private attempt records request, operation, physical
identities, grant and closure before content construction. It retains signed objects, exact operation
frames, original/prospective configuration, source graph/retention evidence and recovery receipts.
Only a complete synchronized bundle is published into the stable request slot by exclusive rename.
Interrupted scratch attempts remain preserved; retries never overwrite or adopt their incomplete
contents. No destination file or journal is changed, and no lane becomes runnable.

Retry independently checks the bundle, signed objects, frames, exact root/entry identities and
source basis. Staged directory paths, kinds, content digests, lengths and executable state are also
compared with the authenticated materialization plan; matching rewritten local receipts cannot
substitute different content. Lookups are built once per verification pass, and subtree selection
uses ordered ranges instead of rescanning every entry for every directory. An exact retry returns
the same staging receipt and allocation. Empty snapshots create no placeholder destination content.

Integrated tests passed for signed nonempty and empty sources, nested executable/empty directories,
three interrupted construction boundaries, damaged frames, rewritten tree receipts and untouched
destination/history. A publisher process exits immediately after bundle publication; two fresh
processes reopen native handles and recover the same receipt and physical bundle. Bypassing the
signed-tree comparison makes the forged-receipt test fail; original source was restored exactly.
Focused process tests passed in 6.392s. The full repository gate is pending.

This is the private staging portion of the unfinished consumption transaction. A staging receipt
is evidence, not continuing permission or a consumption acknowledgement. Required start-record
synchronization before installation, owner consumption, completion, cross-store admission and the
complete interruption/revocation campaign remain required. Private-stage retries currently compare
the exact retained-source snapshot; transaction recovery must additionally handle intervening owner
history and committed outcomes without rewriting that evidence. Draft PR #324 remains unmerged,
issue #323 remains open, and the fixed checkpoint remains unchanged.

R161 private-staging implementation `a89419f3bc3bc7b7d5203124c5f25e8cfe40cdb3` passed full
`npm test`: 3,948 native tests in 277.987s (five slow, 18 skipped), 194 rendered tests, 672 desktop
tests, 44 real-daemon checks and all repository/docs/license/storage/format/lint gates. The existing
native integration tests now additionally cover private staging and separate-process restart.
Published predecessor `4d176d29087916f7499849c535d27df024fa13f3` passed all seven hosted checks in
run `37221161431`. Successful and negative-mutation logs and complete Git bundles are preserved.
This validates private staging, not destination installation or consumed runtime admission. Required
start ordering, owner receipt, completion and cross-store recovery remain open in draft PR #324 /
issue #323; the full fleet objective and fixed checkpoint are unchanged.

R161 private staging now holds explicit native custody for the destination history, destination
files and reservation allocation during retained-entry preparation, verification and private bundle
publication. The retained-entry seam requires all three roots in the still-held guard; incomplete
custody refuses before creating entries. The private barrier is released before full graph/grant
revalidation, so it cannot extend a held set or accidentally retain permission for later installation.
The final commit still requires its own complete cross-store barrier.

Three focused integration tests passed in 6.719s. At each of three interrupted construction
boundaries, another native writer remains blocked until staging releases custody, then proceeds.
Incomplete root sets refuse without changing allocation entries. A real native grant revocation
after custody release prevents acknowledgement and preserves the staged bundle; a late editor file
also prevents acknowledgement and stays untouched. Separate-process publication/recovery and empty
source coverage continue passing. Taking custody on the wrong workspace makes the concurrency test
fail, and removing final revalidation makes the revoked-request test fail. Both negative logs are
retained, and exact production source was restored before the full gate.

Full repository validation is pending. Required start ordering, owner consumption, destination
completion and cross-store read/capture admission remain unfinished in draft PR #324 / issue #323.
These staging safeguards do not install files, launch agents or advance protected main. The fixed
checkpoint remains unchanged.

R161 custody implementation `6cf11cc0e474d07cafd2d498902180c31d89191f` passed full
`npm test`: 3,949 native tests in 272.512s (five slow, 18 skipped), 194 rendered tests,
672 desktop tests, 44 real-daemon checks and all repository/docs/license/storage/format/lint gates.
Published predecessor `687ab1c9ed970f11635086e2b37409523745d30b` passed all seven hosted
checks in run `37222451265`. Focused concurrency/revocation and negative-mutation evidence is
preserved with the full log and Git history. This proves private staging custody and refreshed
permission checks; required start ordering, installation, owner consumption, completion and
cross-store recovery/admission remain unfinished. Draft PR #324 is not merged; issue #323 and
the full fleet objective remain open. The user checkpoint stays fixed.

R161 now retains the complete selected owner/source/destination custody set through a synchronous
validated transaction callback. Destination ancestry and all of its roots are selected before the
single lock acquisition. Signing still runs outside custody; public revalidation remains a
point-in-time read. The private callback is a transaction integration seam, not a committed version.

Both signed-source integration tests passed in 4.996s. They require catalog, owner, source,
destination and allocation roots inside the callback, block a competing native writer until an
intentional callback failure releases custody, preserve unexpected editor work without invoking
the callback, and refuse callback admission after native grant revocation. Removing both destination
emptiness checks makes the new regression fail (1.116s); exact source was restored. Full repository
validation is pending. Required journal-before-install ordering, durable owner/completion receipts,
recovery and cross-store admission remain unfinished; PR #324 stays draft and the checkpoint fixed.

R161 transaction-custody implementation `55fe74f5528355c45bb9f6e0b4797b7bb14fbdfb` passed
full `npm test`: 3,949 native tests in 316.666s (five slow, 18 skipped), 194 rendered tests,
672 desktop tests, 44 real-daemon checks and all repository/docs/license/storage/format/lint gates.
Published predecessor `a3ff44a5e95f733a7c5bd391ee98fa5a8f781616` passed all seven hosted checks
in run `37223540971`. Positive and deliberately failing regression logs and complete Git history
are preserved. The validated callback retains complete custody but does not itself install files,
append consumption or acknowledge a runnable version. Draft PR #324 remains unmerged; issue #323
and the full fleet goal remain open. The fixed checkpoint is unchanged.

R161 recovery inspection now separates exact local native journal facts from workspace admission.
The shared parser still verifies registration, the enrollment fence, canonical policy payloads and
exact journal identity/bytes. Its non-admitting result can inspect pending or completed-looking
consumption policy; the private conversion to an ordinary read proof refuses either. No generic
ignore-tail flag, consumption permission or completed cross-store proof is introduced.

Six native integration tests passed in 1.426s, including exact local-policy inspection while ordinary
read/capture/control remain refused, changed bytes and torn-suffix refusal without repair, replaced
journals, missing fences and corrupt policy payloads. Removing the conversion's consumption refusal
makes the integration regression fail in 0.204s; exact source was restored. Full repository validation
is pending. Exact consumed-prefix recovery, required journal-before-install ordering, materialization,
owner/completion receipts and cross-store admission remain unfinished in draft PR #324 / issue #323.
The user checkpoint and full fleet objective are unchanged.

R161 native-facts implementation `971ed406a085a035d54d4ee259c195c6f58012a5` passed full
`npm test`: 3,949 native tests in 273.566s (five slow, 18 skipped), 194 rendered tests,
672 desktop tests, 44 real-daemon checks and all repository/docs/license/storage/format/lint gates.
Six focused native tests and the deliberately failing admission-gate mutation are preserved with
complete Git history. Published predecessor `d521b18626bf4417eb9a4a5d0aeb4a0def8273d5` has
six successful hosted checks; macOS remains running in run `37224305162` at this evidence update.
The factual reader does not grant consumed workspace admission or recover a torn consumption
append yet. Required start ordering, materialization, owner/completion receipts and cross-store
recovery/admission remain unfinished; draft PR #324 and issue #323 stay open. The checkpoint is fixed.

R161 now inspects exact interrupted required-start frames through a distinct canonical consumption
intent. Recovery facts bind the original journal identity, prefix length/digest, request, payload and
destination configuration. The observed suffix must be a byte-for-byte prefix of that exact frame;
unknown bytes are neither skipped nor repaired. A pending start remains explicitly consumption even
when replay stops at the pre-start prefix, so it cannot mint an ordinary native read capability.

Six focused integration tests passed in 1.684s. They cover every start-frame prefix from zero bytes
through the full frame, changed suffixes, an otherwise canonical start for another configuration,
unchanged editor/journal bytes and continued ordinary read/capture/control refusal. Removing pending
consumption admission fencing makes the new regression fail in 0.210s; exact source was restored.
Full repository validation is pending. This is read-only prefix verification, not the durable start
writer, materialization, owner/completion transaction or cross-store admission. Those remain required
in draft PR #324 / issue #323, together with complete restart and fault proof. The checkpoint is fixed.

R161 start-prefix implementation `eef22fe60b0e2f05ff76be69b33a2854c21058fe` passed full
`npm test`: 3,949 native tests in 275.019s (five slow, 18 skipped), 194 rendered tests,
672 desktop tests, 44 real-daemon checks and all repository/docs/license/storage/format/lint gates.
Published predecessor `41923e7ae4f9d135abd262b4981815d48198fcaa` passed all seven hosted checks
in run `37225144094`; the preceding run `37224305162` also completed all seven successfully.
Focused prefix/refusal tests, deliberately failing admission mutation and complete Git history are
preserved. This validates read-only exact-start recovery facts, not a durable start writer or an
acknowledged consumption transaction. Installation, owner/completion receipts, full restart recovery
and cross-store admission remain required in draft PR #324 / issue #323. The checkpoint is unchanged.

R161 now has a native durable start-fence writer for an exact prepared candidate and private stage.
Under complete owner/source/destination custody it rechecks the current grant, immutable graph,
reservation binding, empty destination and signed staged content. It retains the transaction's
explicit limits, stage receipt, signed objects and configuration bindings, then writes and synchronizes
its exact intent before appending the required start record. Exact retries append only missing bytes;
post-sync verification compares the complete expected journal, configuration and record identity.
This writes no destination files or owner consumption and grants no ordinary read/run permission.

The real byte-by-byte test first found that reservation-origin verification incorrectly demanded an
ordinary read after a partial start. Origin now checks exact native enrollment facts for consumed
reservations as correlation only; independent histories retain the original full read checks. All
nine focused reservation/start tests then passed in 26.154s. Coverage includes every interrupted
frame boundary, failed synchronization, lost acknowledgement, repeated exact retry, editor work,
foreign suffixes and a real grant revocation while consumption is still uncommitted. Removing journal
synchronization fails the regression in 23.995s. Failed and successful logs are preserved and exact
source was restored. Full repository validation is pending.

The writer currently retries using the same authenticated candidate and stage handles. Fresh-process
candidate reconstruction, exact exclusive installation, signed destination history, owner receipt,
completion and full cross-store admission remain required before this is an acknowledged consumed
version. Draft PR #324 / issue #323 and the full fleet scope stay open. The user checkpoint is fixed.

R161 durable start-writer implementation `4ec8638ff010ff63840698311453532dcf229e91` passed
full `npm test`: 3,949 native tests in 278.939s (six slow, 18 skipped), 194 rendered tests,
672 desktop tests, 44 real-daemon checks and all repository/docs/license/storage/format/lint gates.
Published predecessor `56b26e4d7aa9a6f2eb580887581b449786fad6a2` passed all seven hosted checks
in run `37225845167`. Positive, initial-failure and deliberately failing synchronization logs are
preserved with complete Git history. This proves durable start fencing and exact retries with the
same candidate/stage handles. Fresh-process reconstruction, installation, destination/owner/completion
commit and cross-store admission remain required. Draft PR #324 / issue #323 and the full fleet goal
remain open; the user checkpoint stays fixed.

R161 now reconstructs a fenced starting candidate in a fresh process without invoking a signer.
Recovery requires the exact retained request and original limits, native owner/source/destination
bindings, current uncommitted grant and empty reservation. It authenticates the retained checkpoint,
independently reconstructs the expected signed statement from the granted saved source and saved
ignore rules, and verifies the original stage/transaction bindings. Object names must match the
checkpoint references; total and per-object bounds are enforced before reading. Recovery creates no
new signature, installation, owner receipt or ordinary history permission.

Two focused integration tests passed in 27.149s. A child exits after appending one start byte; a fresh
process reloads and synchronizes the complete start, then exits before reply. Two further processes
recover the same record and physical stage. Larger retry limits, corrupted retained content and a
revoked grant refuse while preserving journal/editor work. Bypassing the retained descriptor check
makes the limits regression fail in 5.357s; exact production source was restored. The prior focused
run also passed (26.558s), and all evidence is preserved. Full repository validation is pending.

This implements fresh-process recovery of the start phase only. Exclusive file installation,
destination checkpoint history, owner consumption, completion and cross-store read/capture/runtime
admission remain required in draft PR #324 / issue #323. The full fleet goal and fixed checkpoint
remain unchanged.

R161 fresh-process start recovery implementation `a75f32855297f85e608438869658228eec9f105b`
passed full `npm test`: 3,949 native tests in 293.909s (six slow, 18 skipped),
194 rendered tests, 672 desktop tests and the remaining repository, documentation, license,
storage, format, lint and real-daemon gates. Published predecessor
`1e0e3e06bcd7b7671ff4c0c66d8531716801c757` passed all seven hosted checks in run
`37226972531`. Focused, negative-regression and full-gate logs are preserved with complete Git
history. This proves reconstruction of the original authenticated start without a new signature;
it does not prove installation, destination/owner/completion commit or cross-store admission.
Draft PR #324 / issue #323 remain open for those requirements. The full fleet objective and fixed
user checkpoint remain unchanged.

R161 now connects exclusive file/tree installation to the durable start writer under the same
complete custody guard. Before copying it retains the exact owner-consumption body and source
closure. Each move uses the original authenticated receipt, verifies native identity/content,
refuses unexpected destination/recovery entries and synchronizes both sides. Retries recognize
already installed entries without reallocating or copying them. Fresh-process reconstruction can
inspect an exact partial installation behind a complete required start. Final checks revalidate
stage, owner intent, journal, current grant and custody; ordinary history/runtime remain fenced.

Two focused tests passed in 28.057s after the final receipt checks. Separate children exit after
one entry and after all entries before reply, then two further processes recover the same native
allocations. Unknown editor work, foreign recovery entries, altered owner intent and changed
installed content refuse and remain preserved. Removing the destination-name check makes the
regression fail in 26.225s because entries were installed beside unexpected editor work; production
source was restored byte-for-byte. An explicit revoked-install retry assertion is included in the
full-gate candidate. Full repository validation is pending; no installed result is acknowledged as
a consumed version. Destination history, owner receipt, completion and cross-store admission remain
required by draft PR #324 / issue #323. The fixed user checkpoint and full fleet scope are unchanged.

R161 installation implementation `e7969bf8d13b3942bda2e79f0404fcdf01fc4301` passed full
`npm test`: 3,949 native tests in 324.505s (six slow, 18 skipped), 194 rendered tests,
672 desktop tests and all remaining repository/docs/license/storage/format/lint/real-daemon gates.
This includes the explicit revoked-install retry assertion. Published predecessor
`56c638c9105fb1f7ffb48c951c81a5ef100453d9` passed all seven hosted checks in run
`37228220453`. Complete Git history and positive/negative/full verification logs are preserved.
Installation and exact partial-install recovery are verified at native integration scope;
destination checkpoint, owner receipt, local completion, cross-store admission and packaged
acceptance remain required. Draft PR #324 / issue #323 remain open and the user checkpoint is fixed.

R161 now commits the original signed destination checkpoint after exact installation while retaining
the same complete custody guard. A distinct durable history intent binds the native journal identity,
complete start prefix, request and retained frame digest. Recovery accepts only the exact checkpoint
suffix authenticated by the original start/stage chain; unrelated records or changed bytes refuse.
The reader returns pending local facts only, and ordinary history/capture/runtime remain fenced.
Retries append only missing bytes and revalidate installation, owner intent, journal and current grant.
No owner receipt or completion is created by this step, and the prospective configuration transition
is still pending before ordinary admission.

Two focused tests passed in 32.560s. Separate children exit after one checkpoint byte and after
synchronization before reply; two further processes recover without duplicate history or file
allocation. Every checkpoint-byte prefix is accepted and every changed last byte refused by the
exact prefix inspector. Sync failure and a foreign trailing byte also refuse while preserving bytes.
Removing the suffix equality check makes the regression fail on foreign prefix 1 in 29.829s;
production source was restored byte-for-byte. Full repository validation is pending. The owner
receipt, completion, configuration/capture reconciliation, cross-store admission and full acceptance
campaign remain required in draft PR #324 / issue #323. The user checkpoint stays fixed.

R161 destination-checkpoint implementation `814f1993e90b9d0c391443e17459cf8af25a414a` passed full
`npm test`: 3,949 native tests in 281.112s (six slow, 18 skipped), 194 rendered tests,
672 desktop tests and all remaining repository/docs/license/storage/format/lint/real-daemon gates.
Published predecessor `3aab0858223914f5334a2acb0d7a14d55ce954e5` passed hosted run `37229406625`.
Complete Git history and focused, deliberately failing mutation and full-gate logs are preserved.
This proves the exact signed destination checkpoint commit and its native restart recovery;
owner receipt, completion, prospective configuration/capture reconciliation, cross-store admission
and packaged acceptance remain required. Draft PR #324 / issue #323 and the full fleet goal remain
open. The user checkpoint stays fixed.

R161 now commits the exact owning-authority consumption receipt after the signed destination
checkpoint under the same complete custody guard. A dedicated owner-prefix context verifies partial
receipt history for graph, source and grant inspection without changing ordinary read behavior.
Immutable per-request/per-payload attempt files preserve earlier staged evidence when valid owner
history advances before append. Restart selects only one exact matching attempt; unrelated or
ambiguous suffixes refuse. Pending owner payload/sidecar references are included in inspected graph
retention. A committed request is recognized before current permission checks, with historical input
recovery bound to the exact source, grant, native associations, destination and starting operation.
This historical outcome conveys no new consumption or runtime permission.

Two final focused tests passed in 32.451s. Real children recover after one owner-frame byte and a
synchronized lost reply; repeated retries return one receipt. An intervening owner decision and the
original staged attempt stay unchanged. A later grant revocation retains the exact historical receipt;
a different starting operation refuses. An explicit owner-sync failure prevents success. Ignoring that
failure makes the regression fail in 32.485s; production source was restored byte-for-byte. Earlier
failed test logs are preserved: fresh-process setup and result assertions initially omitted their
required read custody, which was corrected without weakening the read fence. Full validation is
pending. Local completion, prospective configuration/capture reconciliation, complete cross-store
admission and the full acceptance campaign remain required by draft PR #324 / issue #323. The user
checkpoint and full fleet objective remain unchanged.

R161 owner-receipt implementation `d22d4815514b4caa892f1d367d3abdc6b24fd6db` passed full
`npm test`: 3,949 native tests in 305.019s (six slow, 18 skipped), 194 rendered tests,
672 desktop tests and all remaining repository/docs/license/storage/format/lint/real-daemon gates.
Published predecessor `c1cc955d7bcac1147c3e6b19c7216c30ac50aac1` passed all seven hosted checks
in run `37230271789`. Complete history and positive, diagnostic, mutation and full validation logs
are preserved. Native receipt commit/recovery, intervening-owner-history preservation and exact
historical retry after revocation are verified. Local completion, prospective configuration/capture
reconciliation, final cross-store admission and the full acceptance campaign remain required.
Draft PR #324 / issue #323 stay open and unmerged; the user checkpoint remains fixed.

R161 now appends the local consumption completion record after the exact installed checkpoint and
verified owner receipt, under the same complete custody guard. Its immutable recovery intent binds
request, journal identity, exact preceding bytes and canonical completion payload. Recovery accepts
only the exact completion-frame prefix and appends missing bytes; earlier phase APIs refuse once
completion is staged. The checkpoint retry preserves only a separately verified completion suffix.
This local receipt does not admit ordinary history, capture, runtime or publication. Prospective
configuration/capture reconciliation and cross-store admission remain required.

Two focused tests passed in 36.953s. Coverage includes every partial completion-frame boundary,
changed suffix bytes, a canonical completion naming the wrong owner receipt, fresh-process one-byte
interruption, synchronized lost reply, repeated recovery without changed installed identities or
owner history, and explicit completion-sync failure. Ordinary history remains refused even after
local completion. Removing the suffix equality check makes the regression fail on foreign prefix 1
in 34.101s; the production source was restored byte-for-byte. The initial failed run exposed a test
mode collision with the older start-recovery fixture; distinct completion-mode matching corrected
that fixture without changing production authority. Both failed logs are retained. Full repository
validation is pending. Draft PR #324 / issue #323, packaged acceptance and the full fleet objective
remain open; the fixed user checkpoint is unchanged.

R161 local-completion implementation `1c1bab93464068213ebf99d0dccc7f74380af3b2` passed full
`npm test`: 3,949 native tests in 276.872s (six slow, 18 skipped), 194 rendered tests,
672 desktop tests and all remaining repository/docs/license/storage/format/lint/real-daemon gates.
Published predecessor `6f2802d21df32ae208b01930211e605e4fc62320` passed all seven hosted checks
in run `37231827134`. Complete history, the initial fixture failure, positive recovery, deliberate
corruption failure and full validation logs are preserved. This verifies the local completion
receipt and exact restart behavior; configuration/capture reconciliation, cross-store admission,
runtime integration and packaged acceptance remain required. Draft PR #324 / issue #323 and the
full fleet objective stay open. The user checkpoint remains fixed.

R161 now has explicit native-catalog APIs for reading the completed starting version and its saved
file bytes. Recovery retains the complete custody set while reconstructing the signed initial
snapshot from exact saved source content and exclusions. A private typed read proof is created only
after joining the destination start/checkpoint/completion with the independently replayed exact
owning receipt and full source closure. The effective saved configuration comes from that verified
starting record; the original enrollment marker is retained unchanged. Historical inspection does
not reinstall entries, consult live file content or renew a revoked grant.

Two focused tests passed in 38.953s. Two fresh processes read the original saved bytes after an
existing working file is edited and a new file is added, preserving journal, enrollment marker and
both editor files. A locally canonical completion naming a different owner receipt refuses. Removing
the exact receipt join makes that refusal test fail by returning a saved version (36.874s); source
was restored byte-for-byte. Explicit refusals before completion and after a one-byte completion are
included in the full validation now pending. This API currently reads the exact completed initial
history and retains the original staging evidence. Generic history/capture/runtime paths, later
capture-state reconciliation, transitive consumed-source admission and packaged acceptance remain
required before readiness. Draft PR #324 / issue #323 and the full fleet objective remain open.
The fixed user checkpoint is unchanged.

R161 completed-history read implementation `bba69dad1550fa986a4c6da460917e43db17e295` passed full
`npm test`: 3,949 native tests in 283.428s (six slow, 18 skipped), 194 rendered tests,
672 desktop tests and all remaining repository/docs/license/storage/format/lint/real-daemon gates.
Published predecessor `4a8a1dd297d0b16e46dd6506bf2ed1d2276af5d8` passed all seven hosted checks
in run `37232967331`. Complete history and focused, deliberate mutation and full validation logs
are preserved. Native initial-history reads now verify across stores and survive editor changes and
fresh processes. Generic read/capture integration, later captures, transitive consumed-source
admission, runtime controls and packaged acceptance remain required; this is not a merged release.
Draft PR #324 / issue #323 and the full fleet objective remain open. The user checkpoint stays fixed.

R161 now prepares, commits and recovers later private captures through an explicit native catalog
context. Preparation releases all custody before signing; commit and recovery reconstruct the
completed starting transaction and reacquire its full custody set. The existing capture writer
uses fresh verified configuration/history at each boundary, including exact partial-capture
recovery. Immutable completed-start inspection is separate from later capture validation and never
creates ordinary admission on its own. Native allocation correlation remains inspectable during
later interrupted captures without treating that inspection as access. Saved configuration comes
from the authenticated start; enrollment identity and current editor files are not rewritten.

Fifteen focused consumption/capture tests passed in 44.932s. The new scenario saves later edits,
interrupts after one capture byte, recovers in fresh processes, retries without duplicate history,
rejects a stale signed candidate, creates newer saves and retries an older request without rewinding
the capture position. It also refuses a synchronization failure and recovers the exact staged save.
Ignoring that sync failure makes the regression incorrectly acknowledge a save and fail in 41.328s;
source was restored byte-for-byte. An additional owner-history change after signing must refuse
before any destination append; it is included in the full validation now pending. Existing independent
capture behavior remains covered by the shared writer tests. Generic desktop/harness context wiring,
transitive consumed-source graph admission, runtime controls and packaged acceptance remain required.
Draft PR #324 / issue #323 and the full fleet objective remain open; the user checkpoint is unchanged.

R161 later-capture implementation `fba8e013a6e8886d75010456c87add8192b72ff7` passed full
`npm test`: 3,949 native tests in 280.261s (six slow, 18 skipped), 194 rendered tests,
672 desktop tests and all remaining repository/docs/license/storage/format/lint/real-daemon gates.
Published predecessor `2e7d68bcde473ffdeb4325cd1a53c05ac5df289e` passed all seven hosted checks
in run `37233954493`. Complete history and positive, mutation and full validation logs are preserved.
The full run also passed the after-signing owner-history damage refusal without a destination append.
Later native private saves and exact fresh-process recovery are verified; generic context wiring,
transitive consumed-source graph admission, runtime/publication controls and packaged acceptance
remain required. Draft PR #324 / issue #323 and the full fleet objective remain open. The fixed user
checkpoint is unchanged.

R161 now inspects a graph rooted in a completed consumed lane through its already verified native
history context. Each use refreshes pinned history facts, registration identity and the original
configuration binding; the effective configuration must match the authenticated start. Parent
inspection can use that same context without relaxing independent readers. Graph traversal includes
the lane's later captures and its owner-recorded input edge, and retains exact local start/stage/
configuration/frame objects and recovery sidecars. Owner-only grant and receipt references remain
qualified to the owner store. The complete custody set remains held and cannot grow during traversal.

Thirteen focused graph, identity and consumption tests passed in 46.381s, including repeated fresh-
process inspection after later saves. A changed original marker invalidates a previously verified
context while restored exact bytes remain readable. Omitting configuration from the native facts
made that regression reuse the stale proof and fail in 44.140s; production source was restored
byte-for-byte. Extra retention assertions check the exact local objects and exclusion of owner-only
grant payloads in the full validation now pending. The initial test build's ownership error is
preserved; retaining the before-state fixed the post-read comparison. This supplies explicit consumed-
lane graph inspection and retained references; automatic multi-lane context resolution, downstream
reservation/grant/start integration, generic callers, runtime/publication controls and packaged
acceptance remain required. Draft PR #324 / issue #323 and the full fleet goal remain open. The user
checkpoint stays fixed.

R161 consumed-graph implementation `35f447e142f2e26dfe9768db359af6f1ea524fe0` passed full
`npm test`: 3,949 native tests in 279.218s (six slow, 18 skipped), 194 rendered tests,
672 desktop tests and all remaining repository/docs/license/storage/format/lint/real-daemon gates.
This includes effective-configuration validation, original-marker freshness and exact local retention
objects without misclassifying owner-store grant payloads. Published predecessor
`0fc65bf9aaf197b74b27d24a079e06bf1cbbd919` passed all seven hosted checks in run `37235110193`.
Complete history and focused, mutation, failed diagnostic and full logs are preserved. Explicit native
consumed-lane graph inspection is verified; automatic multi-lane resolution, downstream reservation/
grant/start, generic desktop/harness integration, runtime/publication controls and packaged acceptance
remain required. Draft PR #324 / issue #323 and the full fleet goal remain open. The fixed user
checkpoint is unchanged.

R161 graph inspection now resolves completed consumed histories from retained native start records
without a caller supplying each transaction request. Resolution validates native handles against a
single complete custody set, derives bounded request/source/limit selectors, reconstructs signed
starts through already verified histories, and admits a context only after the exact owner/completion
join. Each successful pass resolves at least one history; unresolved or damaged inputs refuse the
whole result. Recovery can reuse an already-held guard without acquiring or extending locks. Grant
inspection can read an explicitly verified source context while preserving exact current or historical
permission checks. This is history inspection, not new consumption or runtime authority.

Fifteen focused graph/grant/consumption tests passed in 56.459s. Automatic graph inspection equals
explicit inspection after later saves and in fresh recovery processes; incomplete custody and torn
owner history refuse. Removing the resolver made the actual fresh-process graph regression fail in
48.005s, and source was restored byte-for-byte. The initial compile warning and strict lint refusal
for an unnecessary test clone are preserved and corrected; full validation is pending. Multi-level
child reservation/grant/start execution and its acceptance tests, generic desktop/harness wiring,
runtime/publication controls and packaged acceptance remain required. PR #324 / issue #323 and the
full fleet objective stay open; the fixed user checkpoint is unchanged.

R161 automatic-history resolver `abf82e54dbabe910ea614c65d3f30a710c8baea3` passed full
`npm test`: 3,949 native tests in 282.578s (six slow, 18 skipped), 194 rendered tests,
672 desktop tests and all remaining repository/docs/license/storage/format/lint/real-daemon gates.
Published predecessor `2a531da3f849b6fb044c7dd95f9b9a7d5640930b` passed all seven hosted checks
in run `37236395813`. Complete history and positive, negative, failed lint and full logs are preserved.
This proves automatic reconstruction for graph inspection, including later saves and fresh processes;
it does not establish end-to-end multi-level lane creation. Downstream reservation/grant/start/recovery
integration and tests, generic callers, runtime/publication controls and packaged acceptance remain
required. Draft PR #324 / issue #323 and the complete fleet goal stay open. The checkpoint is fixed.

R161 can now reserve an empty child from a completed consumed lane's later saved version. The native
reservation API accepts transitive native handles, computes the full input custody set before
allocation, and resolves the parent's verified history afresh under each allocation/recovery guard.
No read proof survives a released guard. Exact saved-operation membership and native correlation are
rechecked before acknowledgement; a retry cannot replace the original version selection. Existing
reservation entry points retain their signatures and use the same verification path.

Nine focused reservation/consumption tests passed in 59.705s. The new scenario interrupts before
publication, recovers through a fresh process twice, and retries with the same physical destination
and exactly one additional registration. The child stays empty and fenced; the parent's marker,
journal and current editor bytes stay unchanged. Existing unexpected-work, replacement, depth and
recovery regressions also passed. Omitting the native history inputs made the consumed-child test
fail in 55.391s; source was restored byte-for-byte. An initial test assertion compile failure is
preserved and corrected. Full validation is pending. Downstream grant/start/materialization and
multi-level transaction recovery, generic callers, runtime/publication controls and packaged
acceptance remain required. PR #324 / issue #323 and the full fleet goal remain open; the checkpoint
is unchanged.

R161 consumed-parent reservation `b4e1e4117abe5d69bd541764aec6bb9452372180` passed full
`npm test`: 3,949 native tests in 284.709s (six slow, 18 skipped), 194 rendered tests,
672 desktop tests and all remaining repository/docs/license/storage/format/lint/real-daemon gates.
Published predecessor `5337e96191813e7cf13a98a71b673b45efb57481` passed all seven hosted checks
in run `37237396357`. Full history and focused, mutation, failed diagnostic and full logs are
preserved. Empty child allocation from consumed progress and exact restart recovery are verified;
downstream grant/start/materialization and multi-level consumption recovery, generic callers,
runtime/publication controls and packaged acceptance remain required. Draft PR #324 / issue #323
and the complete fleet objective stay open. The user checkpoint is unchanged.

R161 native grants can now select a consumed lane's saved progress for an exact child destination.
The control callback reconstructs source history from a context independently matched to its current
owner proof, including an exact interrupted control prefix. Completed-parent admission requires the
already journaled owner receipt; the pending record itself cannot substitute. Historical input
reconstruction during pending control requires the exact prior consumption relationship. Current
access still refuses unfinished control, stale grants and revoked grants. Destinations remain
correlation-only selections under complete custody, so empty and interrupted destinations do not
need readable source history. This corrects the initial focused run's premature destination reads.

Sixteen focused grant/admission/consumption tests passed in 83.858s, including the existing full
interrupted-frame campaign. The consumed-parent scenario covers one-byte grant interruption,
fresh-process recovery and historical retry, revoke with lost reply, regrant, exact saved bytes and
unchanged source/destination journals with an empty child. An added stale-owner-proof refusal fails
in 61.820s when its exact proof check is removed; production source was restored byte-for-byte.
Strict daemon linting passed in 9.28s. The initial failing compatibility run and all diagnostic logs
are preserved. Full validation, including the added stale-proof and ordinary-read refusal assertions,
is pending. Start/materialization and multi-level consumption recovery, generic desktop/harness
callers, runtime/publication controls and packaged acceptance remain required. Draft PR #324 / issue
#323 and the full fleet objective stay open. The fixed user checkpoint is unchanged.

R161 consumed-source grants `9b504eac3ab2545684a8420efad2b1a1fd1cf711` passed full `npm test`:
3,949 native tests in 296.016s (six slow, 18 skipped), 194 rendered tests, 672 desktop tests and all
remaining repository/docs/license/storage/format/lint/real-daemon gates. This includes the additional
stale-owner-proof and ordinary torn-history refusal assertions. Published predecessor
`b453de9cd7d52181fa9cdbb7c16407abd0aa3fea` passed all seven hosted checks in run `37238362613`.
Complete history and failed compatibility, focused, mutation and full logs are preserved. Exact
consumed-source grants and restart recovery are verified; child materialization/start and multi-level
consumption recovery, generic callers, runtime/publication controls and packaged acceptance remain
required. Draft PR #324 / issue #323 and the full fleet goal stay open. The checkpoint is unchanged.

R161 now carries resolved native source histories through consumed-child preparation, commit,
recovery and completed graph inspection. The same held custody context validates the source graph,
destination correlation and exact input grant; post-read comparisons refresh those facts. Recovery
reconstructs the intermediate lane before the child and never grows its held root set. The selected
saved bytes remain distinct from current parent editor contents, and signing remains outside custody.

Seven focused graph/consumption tests passed in 94.838s. A real second consumed lane installs its
parent's later saved bytes, interrupts its owner receipt after one byte, restarts and interrupts local
completion, loses the completed reply, and retries twice without duplicate owner/destination records
or changed installed identities. Explicit and automatic graphs agree across three retained native
stores. Omitting the intermediate lane refuses inspection. Later child captures preserve the original
saved bytes and leave the source history/editor and owner history unchanged. An added revoke-after-
signing case refuses staging without a child file or journal change, then prepares under a new grant.
Removing parent-context resolution from commit made the scenario fail in 75.080s; exact source was
restored. Strict daemon linting passed in 9.27s. Full validation is pending. This verifies native
materialization/recovery, not provider/runtime permission, desktop integration or packaged acceptance.
Generic desktop/harness callers, runtime/publication/review/import/remote controls and the remaining
fault/acceptance campaigns stay required. PR #324 / issue #323 and the full fleet goal remain open;
the user checkpoint stays fixed.

R161 chained consumed starts `68979980c931a390fbcd4f3c7e62177173da2e44` passed full `npm test`:
3,949 native tests in 299.421s (six slow, 18 skipped), 194 rendered tests, 672 desktop tests and
all remaining repository/docs/license/storage/format/lint/real-daemon gates. This includes the
revoke-after-signing refusal and multi-process, two-level consumption recovery assertions.
This increment is delivered separately on `idosams/chained-consumed-start`, based on published
`idosams/native-consumed-start` at `935f8820a26e62d82202a5c44bcda264df08175c` (prerequisite PR #324).
It replaces no preserved fleet source commit; it extends the canonical native implementation.
The prerequisite passed all seven hosted checks in run `37239592857`; the child increment still
requires its own hosted checks and merge. Full goal and issue #323 remain open: generic callers,
runtime/publication/review/import/remote controls and packaged acceptance remain required.
The fixed user checkpoint is unchanged.

R161 empty-input acceptance now completes a real empty consumed snapshot, reopens its native
catalog/owner/source/destination handles, retries completion without journal growth, and verifies a
two-operation graph with no manifests/chunks and three retained stores. A first later save produces
readable bytes while the initial version remains empty, with no source files or owner-history change.
This closes a gap in the prior test, which stopped at signing/staging. The focused graph journey
passed in 14.663s. A mutation acknowledging completion without its durable transaction failed at
reopened recovery in 5.373s; production source was restored byte-for-byte. This is in-process native
reopening, not a new fresh-process or packaged-app claim. Full validation is pending. The increment
is based on published PR #325 (`e27f41302850e643040062c8454fd6aa57f76e76`) and replaces no preserved
fleet commit. Both dependency PRs and issue #323 remain open; all runtime, UI, publication and
packaged acceptance requirements remain. The user's checkpoint is unchanged.

R161 empty-input acceptance `891b7e35e6e1a940b61e28ffbc9f9e51ec244aa2` passed full `npm test`:
3,949 native tests in 301.441s (six slow, 18 skipped), 194 rendered tests, 672 desktop tests,
44 real-daemon checks and all remaining repository/docs/license/storage/format/lint gates.
Focused and false-acknowledgement mutation evidence, full logs and complete history are preserved.
This increment will be published separately against PR #325; hosted CI and normal merge remain
required. The complete fleet objective and issue #323 remain open, with the fixed checkpoint unchanged.

R161 native transaction/grant foundation PR #324 merged normally into canonical `main` on
2026-10-04 at `e92adb547a86a3059182be301c30ed632017892d`, after all seven checks passed on
`935f8820a26e62d82202a5c44bcda264df08175c` (run `37239592857`). Post-merge CI is separate.
Chained recovery PR #325 passed all seven checks on `e27f41302850e643040062c8454fd6aa57f76e76`
(run `37240702369`). Its reconciliation merge `938415c322e9c585ca12238d95ff52c207317569`
has the identical complete tree; only delivery documentation changes follow. It now targets main
and requires current-head hosted checks before merge. Empty-input acceptance PR #326 is published
at `60eeb73e9abbb3f9d35d523768003be4f44ca3cf`, with CI running. The native foundation is merged;
the full fleet objective and issue #323 remain open for generic callers, runtime/publication/review/
import/remote controls, remaining fault campaigns and packaged acceptance. The checkpoint is unchanged.

R161 native catalog reads now list saved dependency versions and immutable file bytes from exact
registered owner/work/input IDs, without requiring a reconstructed consumption transaction request.
The API reopens registrations, establishes one bounded complete custody set, resolves consumed
histories, validates correlations and refreshes every selected history and retained sidecar after
reading. Missing intermediate inputs, interrupted consumption and descendants claiming root authority
refuse; these read APIs do not grant capture/runtime/publication authority or recover pending work.
Native input-ID discovery and desktop/harness wiring remain subsequent work, not completed behavior.

The real chained-consumption journey passed in 104.207s, comparing root history, initial and later
child saves and exact immutable bytes. Added assertions cover interrupted-read refusal and an owner
journal change during the read, with fixture restoration before assertions. Removing post-read
validation made the freshness assertion fail in 92.271s; production was restored byte-for-byte.
Strict daemon lint passed in 9.76s. Full validation, including the new freshness and interrupted-read
assertions, is pending. This increment is based on published PR #325 at
`26d61d4fb97c99ee7b52f607f15721e07acf63ab`, replaces no preserved fleet source commit, and will get
its own PR before the next substantial increment. Issue #323 and the full fleet objective remain open.
The fixed user checkpoint is unchanged.

R161 native catalog reads `8d35bd9d9ed25a98d6fccc9f2eddc95738b1e8e8` passed full `npm test`:
3,949 native tests in 320.087s (six slow, 18 skipped), 194 rendered tests, 672 desktop tests,
44 real-daemon checks and all remaining repository/docs/license/storage/format/lint gates.
The full run includes the added owner-history freshness and interrupted-consumption refusals.
Focused/mutation/lint/full evidence and complete history are preserved. Foundation PR #324 also
passed post-merge run `37241537647`; empty-input PR #326 passed run `37241441884` on its published
head but still needs base reconciliation and merge. The catalog-read increment requires its own
hosted checks and normal merge. Automatic input discovery, desktop/harness wiring and the full
remaining fleet acceptance remain open in #323. The fixed checkpoint is unchanged.

R161 automatic input discovery selects required native stores from owner consumption facts plus
allocation ancestry, using the same canonical work/installation encoding as guarded validation.
Candidate correlation hints are collected before the owner-only policy guard. The complete read
then rechecks discovery under its existing guard without adding custody roots, and refreshes history
and sidecars afterward. Unavailable unrelated registrations are excluded; missing/ambiguous required
inputs refuse. Callers still select the native owning root; no capture/runtime/publication permission
or pending-transaction recovery is granted by these read APIs.

The real discovery journey passed in 129.383s: two-level saved history, immutable old/new bytes, a
peer consuming work from a different allocation branch, exactly four required stores, unrelated
offline work, missing required input and recovery after its identity is restored. An added stale-
selection case changes real grant policy before guarded read, refuses the old selection before its
callback and preserves completed history after revocation; full validation of that addition is pending.
Removing consumption-edge discovery failed the peer read in 121.059s; source was restored byte-for-
byte. Strict daemon lint passed in 9.18s. Initial compiler failures and owner-fence/nested-custody
failures are preserved alongside their fixes. Full `npm test`, publication and hosted checks remain
required. This increment depends on published #327 and replaces no preserved fleet source commit.
Local reconciliation `1a5176420741b3015a31f50567c0efb2e5bb840a` incorporated main #325 without
changing #327's tested tree. Desktop/harness wiring and all broader fleet acceptance remain open.
The fixed user checkpoint is unchanged.
R161 chained recovery PR #325 merged normally into canonical `main` on 2026-10-04 at
`39f75b33ba2586ac2c55b5222ac97ed39196fee6`, after all seven hosted checks passed on
`26d61d4fb97c99ee7b52f607f15721e07acf63ab` (run `37241603535`). Post-merge CI is separate.
Empty-input PR #326 passed all seven checks on `60eeb73e9abbb3f9d35d523768003be4f44ca3cf`
(run `37241441884`). Reconciliation `15c9c75b26d4dc884f92c94c5f8e71cc746ea1cc` preserves its
production and test source exactly; only documentation differs, with both append-only delivery
histories retained. It now targets main and needs current-head hosted checks before merge.
Catalog-read PR #327 is published at `c462e8f1ee573f0647db2cc65c3a317881781f23`, with CI running
and base reconciliation still required. The full fleet scope and #323 remain open; native input
selection, desktop/harness integration, runtime/publication and remaining acceptance are unfinished.
The user checkpoint remains fixed.

R161 empty-input acceptance PR #326 merged normally into canonical main on 2026-10-04 at
`7ffbd244346b3ce7e534cf509b016dcfe9103386`, after all seven checks passed on
`428582e967503470591b27db0078fba82e8f1b02` (run `37242735253`). Chained recovery #325 post-merge
run `37242679496` also passed. Catalog-read #327 passed all seven checks on
`c462e8f1ee573f0647db2cc65c3a317881781f23` (run `37242611845`). Its local reconciliation
`ecdb29ae6bd696cf59f209acdc791fc978c36870` adds only the merged empty-input acceptance test and
delivery notes; all catalog-read production code is unchanged. Both documentation histories were
retained. Full validation of this combined tree is running before republishing against main.
Automatic discovery and desktop/harness wiring are not delivered by #327; the full fleet objective
and issue #323 remain open. The fixed checkpoint is unchanged.

R161 catalog-read reconciliation passed the complete `npm test` gate on
`7f7c10446fb91650426b89626a073d069c5a40e4`: 3,949 native tests in 322.939s
(six slow, 18 skipped), 194 rendered tests, 672 desktop tests, 44 real-daemon checks
and all repository/docs/license/storage/format/lint gates. This combines the catalog-read
implementation with merged empty-input acceptance on main `7ffbd244346b3ce7e534cf509b016dcfe9103386`.
PR #327 is being republished against main; current-head hosted checks and normal merge remain
required. Automatic discovery remains a separate preserved, unpublished increment awaiting its
combined full gate. Issue #323 and the complete fleet acceptance remain open. The checkpoint is fixed.


R161 discovery reconciliation `15f2a2ec20e9a4343474b2ae7cee1e0fea14147d` passed full
`npm test`: 3,949 native tests in 345.982s (six slow, 18 skipped), 194 rendered tests,
672 desktop tests, 44 real-daemon checks and all repository/docs/license/storage/format/lint gates.
The real consumption journey passed in 192.603s, including the added stale-selection refusal
before its read callback and successful historical reads after revocation. The full source tree was
held fixed during verification. This increment preserves implementation `67f05c5add78bd981474ec74912f4cf6874e0593`
and depends on published catalog-read #327 at `62cdcadd9afa70df0d5325114d0eb72628eac2eb`.
Post-merge CI for #326 passed (run `37244087413`). Discovery still requires publication, hosted
checks and normal merge; desktop/harness integration and the full issue #323 remain open.
The fixed checkpoint is unchanged.


R161 owning-root selection now follows bounded recorded allocation ancestry from a registered lane
before using the existing complete dependency read guard. Callers can request versions or saved
bytes using only the lane ID; candidate root selection itself grants no authority. Enrolled root,
chained lane and cross-branch consumed history passed the focused journey in 136.561s, including
unenrolled-work and missing-input refusal. An added missing-owner refusal/restoration case awaits
full validation. Deliberately stopping ancestry traversal after one edge failed the real root identity
assertion in 104.417s; production source was restored byte-for-byte. Full validation, publication and
hosted checks are pending. This local increment depends on #328 and replaces no preserved source
commit. Desktop review/capture wiring and all remaining fleet acceptance stay open in #323.


R161 owning-root full validation passed on `8ec5d23af3f32d70ab62cb498ea7d3288466f2ef`:
3,949 native tests in 341.828s (six slow, 18 skipped), 194 rendered tests, 672 desktop tests,
44 real-daemon checks and all other canonical gates. The real consumption journey passed in
187.691s, including missing-owner refusal and exact-identity restoration. Reconciliation with
merged catalog-read #327 preserved the complete source tree of implementation `25c15ea`.
PR #327 merged normally at `e6ebb4f4ee048e7ac98063b81715dd07b7b65363` after all seven checks
passed (run `37244695215`); post-merge CI is tracked separately. Discovery #328 still requires
base reconciliation and normal merge, and owning-root selection still requires its own PR and
hosted checks. Desktop/harness integration and full fleet acceptance remain open in #323.
R161 discovery PR #328 passed all seven hosted checks on
`67e6c0cc3b765378f7f9d89f38a4b9fd68c313f7` (run `37245205962`). Reconciliation
`bb83272fc1e56466627ef5b93b518d9b75c71372` incorporates merged catalog-read #327/main
`e6ebb4f4ee048e7ac98063b81715dd07b7b65363`; its entire tree is identical to that tested head.
The following update records evidence only. PR #328 now needs fresh current-head checks against
main before normal merge. The full local gate remains 3,949 native, 194 rendered, 672 desktop and
44 daemon checks. Owning-root selection is separately preserved and fully locally tested but not
published yet. Desktop/harness integration and the complete fleet scope remain open in #323.


R161 desktop saved-review integration now shares bounded entries, text preview and comparison
rendering across ordinary and dependency-aware histories. The native registered reader selects the
history format from verified native facts, retains exact registration identity, and never falls back
to independent reads after a dependency failure. Desktop recovery, versions, inspection and
comparison use it; selected storage/history handles are retained before releasing the registry lock.
Recovery remains stopped. Capture/start, approval/publication and provider permission are unchanged.

Native consumed-review coverage passed in 146.504s (old/new immutable text, entries, comparison,
invalid cursor/version/path and missing required input). The real desktop host journey passed in
12.354s: ordinary-project compatibility, native captured input, consumed-lane review, later capture,
stable old content, fresh host reopen, stopped state and missing-source refusal/restoration. Its
initial fixture correctly failed when attempting legacy input consumption without migration evidence;
that refusal was retained and the fixture now captures fresh native input in an enrolled reserved lane.
Disabling dependency-aware routing caused the expected preview failure in 3.385s. Production source
was restored byte-for-byte. Compilation passed; full canonical validation and hosted PR checks remain
required. This is host-level evidence, not packaged graphical acceptance. The fixed checkpoint is
unchanged and the complete fleet scope remains open in #323.
R161 discovery PR #328 merged normally at `8e5f662f7c1f7897d29c1548b8c5773a16ef28d2`
after all seven checks passed on `c15d7b43c0d46f30486380b9feb9fb163ac1b6a4`
(run `37246187227`). Owning-root PR #329 passed all seven checks on
`dec2ac442e37381459f4ccbd418031ada57cb156` (run `37246233164`). Reconciliation
`ea146e3b0e92dd1829fa743fb5c1f9a676d6815d` incorporates main while preserving that entire tested tree; only the
following evidence update changes documentation. PR #329 now requires fresh checks against main
and normal merge. Desktop review integration is separately preserved: its first full local gate
passed 3,950 native tests in 351.698s, 194 rendered, 672 desktop and 44 daemon checks; the separate
changed-path action still needs integration before publication. Full fleet acceptance remains open.


R161 desktop review's first full gate passed on `d8d12c3e8e84f6ad027b3ed2d0bf2de54117ad40`:
3,950 native tests in 351.698s (six slow, 18 skipped), 194 rendered, 672 desktop and 44 daemon
checks plus all other gates. The native consumed-review journey passed in 196.610s. A follow-up
call-site audit found the separate selected comparison-path action still using independent history;
it now uses the same registered review context. Its expanded real host regression passed in
13.704s, covering exact changed-path selection, absent-path refusal and missing-source refusal.
Reconciliation `bf655d5c0a5ba18d2c49e48aa755f42d15b6d674` retained both documentation histories
without changing prior production/tests. The final comparison-path edit requires a new full gate
before publication. No packaged graphical claim is made; the fixed checkpoint remains unchanged.


R161 final desktop-review gate passed on `adeb579a1155412f87376ecd023e5ee79d361b95`:
3,950 native tests in 349.830s (six slow, 18 skipped), 194 rendered tests, 672 desktop tests,
44 real-daemon checks and all repository/docs/license/storage/format/lint gates. The native consumed
review journey passed in 196.616s; the expanded desktop-host journey passed in 23.332s within the
full concurrent suite. This includes recovery, version listing, preview, comparison and selected
changed-path review with ordinary-project compatibility and missing-input refusal. Focused,
mutation, initial-failure and both full-gate logs plus complete history are preserved. This increment
is ready for publication against #329, followed by hosted checks and normal merge. Packaged visual
acceptance, harness capture/start, runtime/publication/import/remote integration and full fleet
acceptance remain open in #323. The user checkpoint stays fixed.


R161 native capture discovery now reconstructs candidate source/grant/request/limits from retained
start records, selects native registrations through owner consumption facts and allocation ancestry,
and obtains the source version from verified history. These hints never admit history: existing
capture preparation/commit/recovery still reconstruct and authenticate the complete consumption
transaction under custody. Missing start intent cannot downgrade consumed work to independent
capture. Public enrolled prepare/recover entry points accept the retained registration and exact
capture request; no caller-built original start request is required. No runtime or main authority is
added, and automatic desktop/harness capture scheduling still needs integration.

The focused real journey passed in 143.942s, exercising torn append and sync recovery in fresh
processes, exact retries, stale prepared work, wrong-request refusal without journal changes and
stale owner refusal at commit. Added deep-lane and desktop capture cases plus missing-intent
refusal before signing await full validation. Disabling reconstructed context failed the actual
capture in 47.095s with native transaction verification required; production source was restored
byte-for-byte. Compilation passed in 6.62s. Full validation, publication and hosted checks remain
required; #323 and the full fleet scope stay open. The fixed checkpoint is unchanged.


Capture-discovery full validation completed on `295a42070020298472d0390ab138eaa058a88b99`: 3,950 native tests passed in 348.848s (18 skipped), plus the rendered, desktop and 44-check real daemon gates. This includes automatic deep-lane capture, desktop consumed capture and missing-intent refusal before signing. The first full attempt stopped on strict lint for a type declared after a test module; moving that declaration before the test module fixed it without changing behavior. Both logs are preserved. Publication and hosted validation remain required; automatic capture-service integration and the full fleet acceptance scope remain open.


Registered automatic capture now retains the native storage/registration authority in the worker. Desktop attach, lane capture and explicit resume use this path. Each save dispatches from verified history format, recovers the exact retained native request before new input, recognizes unchanged native content without another signature or version, and otherwise uses the registered dependency writer. Ordinary projects are not enrolled implicitly. The legacy metadata-only harness entry point remains separate and still needs integration. No execution or protected-main authority is added.

The real desktop host test passed in 16.013s through resume, automatic consumed-lane save, unchanged rescan, stopped state, immutable prior review and missing-owner refusal before signing. Disabling worker registration authority caused the expected SaveUnavailable-versus-Saved failure in 4.945s; production source was restored byte-for-byte. Added sync-failure recovery coverage checks recovery without signing or duplicate history. The first compile failed on a missing import; the corrected focused build passed. Full repository validation and publication remain pending. The user checkpoint is unchanged.


Registered capture-service full validation passed on `afbfb1991ceacd14f34603915e6e1119d7375661`: 3,950 native tests in 350.021s (six slow, 18 skipped), 194 rendered tests, 672 desktop tests and 44 real-daemon checks, with all repository/docs/license/storage/format/lint gates. The consumed-lane recovery scenario passed in 195.426s and the expanded desktop host scenario in 26.675s. This validates native service behavior, not packaged graphical or external-harness acceptance. The increment is ready for its own PR against #331; hosted checks and merge remain required.


Registered harness commands now expose capture/watch/versions against an explicit native storage root and catalog ID before graphical initialization. They share registered save/recovery, worker and saved-review paths with the desktop. Existing metadata-only commands retain their independent-history behavior. No enrollment, input grant, agent launch or main approval is added. Five focused parser/control/legacy/registered-host tests passed in 27.421s; the expanded consumed-lane journey passed in 27.419s through unchanged capture, new save, listing, watched edit, joined stop, old desktop review and missing-owner refusal. Routing the registered command back to the independent writer failed in 15.408s; production source was restored byte-for-byte. Compilation passed. Full validation, publication and hosted checks remain required; packaged/external-provider acceptance remains open.

Post-merge #329 CI run `37249215287` reported Linux cancelled at its 15-minute job limit even though the log records all 3,512 native Linux tests passing in 785.664s and successful test/cleanup steps. The other six jobs passed. The terminal job was requested for rerun; this is not a resolved timing issue or a green combined-main claim. Logs and job state are preserved.
