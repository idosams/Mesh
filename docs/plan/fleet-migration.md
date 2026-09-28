# Fleet consolidation and delivery ledger

Canonical repository: **idosams/Mesh**. Mesh-internal is deprecated for development and remains
historical evidence. The full [fleet objective](fleet-orchestration.md) is unchanged.

## Current delivery checkpoint (2026-09-28)

The sections below retain the migration's historical observations. Later verified delivery supersedes
earlier statements that no PR has merged or named human review is still required. The owner explicitly
authorized merging without human review for this effort; required checks remain mandatory, and no
self-approval or check bypass is authorized. Canonical main is verified through
[PR #70](https://github.com/idosams/Mesh/pull/70) at
`46d473879e07b24f51bef5e9a2bb816c1448b02e` (merged 2026-09-28 at 14:37:23 UTC).
PRs #56–#70 were merged in dependency order after each exact head passed all seven hosted checks.
Reconciliation preserved original commits and implementation trees. This checkpoint does not claim
that the remaining source increments or acceptance journeys are complete.

PR #70's focused compiler and delegated-catalog regressions executed successfully locally. An
identity-substitution mutation failed the compiler regression; restoring the implementation passed.
Its full local gate remains live: compilation completed, but test discovery is delayed before test
code starts. A sampled process was at `_dyld_start`; the cause is not established. Preserve this run.
The earlier full #69 run failed a macOS native-watcher test and remains unresolved in
[issue #37](https://github.com/idosams/Mesh/issues/37); hosted success does not erase that failure.

[PR #71](https://github.com/idosams/Mesh/pull/71) is published at
`985adffffd29b3202c6cff22e6964c7454820d2c`, replacing source
`a8d3bd00105941a9ceb23bd9215b87d367645d5a`. Its Linux job executed 3,042 passing tests,
including the signed pending-import, invalid/aliased receipt and nonprivate/oversized receipt
regressions. Six checks are complete; the macOS job passed its test steps and is still finishing.
Local native/mutation/full validation is queued behind #70. It is not merged at this checkpoint.

PRs #14 and #16 were closed as superseded, not merged: their complete changed files are identical
to canonical main at `5ea158f9d53b4d247d457f50a1808bb56489f02d`. Their branches and history remain
preserved. The incorporated corrections remain traceable through the validation/source mappings below.
Mesh-internal's deprecation PR remains open because its CI jobs could not start due to GitHub's
reported payment/spending-limit restriction. No billing or repository settings were changed.

Remaining work includes desktop import/review and recovery, deletion, grouped integration,
restoration, real second-provider/four-worker and remote acceptance, latest packaged/native approval
journeys, resolution of local native failures and final combined-main validation.

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

The IDs below identify original work, not commits already delivered to canonical main. The status
column becomes the canonical PR and replacement commit mapping as each increment is transferred.

| Batch | Preserved source commit | Change | Canonical replacement |
|---|---|---|---|
| F01 | `c4e5b9450962b5bec818022d6e7936f2d5204b89` | feat: establish durable fleet lifecycle and isolated lane allocation | [Mesh PR #3](https://github.com/idosams/Mesh/pull/3), `c0050814fe8b0c96b3a33065d269faea6a0a07ff`; native foundation replaces runtime/ADR; stacked on #2; not merged |
| F02 | `e1b1aeed6461af349fc943299c31376b5c358739` | feat: connect scoped agent delegation to native workspaces and MCP | [Mesh PR #4](https://github.com/idosams/Mesh/pull/4), `905db96883ca8989eedbddcd8650332e1d24da3c`; scoped delegation; stacked on #3; not merged |
| F03 | `766852b17e8e06fbd01d5cf8b2504947fb19a280` | feat: capture private agent files under exact native custody | [Mesh PR #6](https://github.com/idosams/Mesh/pull/6), `451191520d67a35d2f66e4b5d286eb3622da3653`; private capture/checkpoints; stacked on #4; not merged |
| F03 | `4be9ae2f8a0de2515e10ee7fdd9be486ff8ddaa1` | feat: capture agent workspace additions with explicit partial results | [Mesh PR #6](https://github.com/idosams/Mesh/pull/6), `451191520d67a35d2f66e4b5d286eb3622da3653`; private capture/checkpoints; stacked on #4; not merged |
| F03 | `b0da3c728cfb84dcd1d98dbe2e214a5977016c79` | feat: expose durable signed checkpoints through scoped MCP sessions | [Mesh PR #6](https://github.com/idosams/Mesh/pull/6), `451191520d67a35d2f66e4b5d286eb3622da3653`; private capture/checkpoints; stacked on #4; not merged |
| F04 | `4741d895b3eb34a5f9c8d6e7a8c482c527cbc8a6` | feat: submit completed agent checkpoints as immutable reviews | [Mesh PR #7](https://github.com/idosams/Mesh/pull/7), `592dfbf3d7363053073a106d4918e1169af8e9af`; immutable review submission; stacked on #6; not merged |
| F05 | `0d6127f1371bec3cbf150bcf2540097fbf980aad` | feat: launch scoped Codex workers with durable ownership claims | [Mesh PR #8](https://github.com/idosams/Mesh/pull/8), `5f39c955f4e3c982c762a39b125a442f616b486c`; native provider execution/scheduling; stacked on #7; not merged |
| F05 | `ca9b48a59f14934e9b05dacef1ecbaa9c93453f3` | feat: schedule delegated Codex workers within native fleet limits | [Mesh PR #8](https://github.com/idosams/Mesh/pull/8), `5f39c955f4e3c982c762a39b125a442f616b486c`; native provider execution/scheduling; stacked on #7; not merged |
| A01 | `c6e62e0be68307fa315928d0d90bb6eaf8852b94` | docs: prioritize non-disruptive existing-project attachment | [Mesh PR #9](https://github.com/idosams/Mesh/pull/9), `7ce3b0c484ef482ca5a39385d11e38f8206a64d0`; registration/observation/capture; direction retained by #2; stacked on #8; not merged |
| A01 | `09f4d863796aa52c7fc60530dfd1a73a68e94178` | feat: register existing projects without moving or taking custody | [Mesh PR #9](https://github.com/idosams/Mesh/pull/9), `7ce3b0c484ef482ca5a39385d11e38f8206a64d0`; registration/observation/capture; direction retained by #2; stacked on #8; not merged |
| A01 | `920a71f36fa21e4646129a75d5bfd086630c9c25` | feat: observe attached projects through bounded native inventories | [Mesh PR #9](https://github.com/idosams/Mesh/pull/9), `7ce3b0c484ef482ca5a39385d11e38f8206a64d0`; registration/observation/capture; direction retained by #2; stacked on #8; not merged |
| A01 | `ddf6086860b38c104a4e199e4b770800fa34b715` | feat: capture immutable inputs from attached projects without taking custody | [Mesh PR #9](https://github.com/idosams/Mesh/pull/9), `7ce3b0c484ef482ca5a39385d11e38f8206a64d0`; registration/observation/capture; direction retained by #2; stacked on #8; not merged |
| A02 | `41bc6bf58615da3ff73922be2bb968b92925bd42` | feat: commit captured file batches as one durable signed version | [Mesh PR #10](https://github.com/idosams/Mesh/pull/10), `f6bda8b1b915367bd7f9b6e0f981ecf4a1a64521`; external signed history; stacked on #9; not merged |
| A02 | `f5860ae7d0647f6539616890037b3b4ec1080ffc` | feat: save attached-project versions in external native history | [Mesh PR #10](https://github.com/idosams/Mesh/pull/10), `f6bda8b1b915367bd7f9b6e0f981ecf4a1a64521`; external signed history; stacked on #9; not merged |
| A03 | `9e3e367c9be2f72fecd2fcd6304fbbac5ac31035` | feat: reconcile attached projects with a native background capture controller | [Mesh PR #11](https://github.com/idosams/Mesh/pull/11), `eb344f5460a332d7daa545e1d051b17368227367`; A03a native controller; stacked on #10; not merged |
| A03 | `8e595348462ebcdc772e71f455f5b1f457aae109` | feat: expose native attached-project capture commands for harnesses | [Mesh PR #12](https://github.com/idosams/Mesh/pull/12), `4259c8b8c8815d8145e457167e7ec7990d081ff2`; A03b harness controls; stacked on #11; not merged |
| A03 | `f8ec9022b9dbe6a44b6a8fa8894758e6962b1a62` | feat: provision native external attachment history storage | [Mesh PR #13](https://github.com/idosams/Mesh/pull/13), `5f4fe06673e476223ad53edad515b7f4d411521b`; A03c native storage provisioning; stacked on #12; not merged |
| A04 | `76f453c67687525a88dd63171530e63a722eaead` | feat: add native desktop attachment session controls | [Mesh PR #15](https://github.com/idosams/Mesh/pull/15), `e2cf7a29922e772d4f4de41ad9429e1879c3a65e`; localized desktop attachment controls; stacked on #13; not merged |
| A04 | `367e86a1b933d167083a0cb396e62b0ac3695710` | feat: connect existing-project attachment controls to desktop UI | [Mesh PR #15](https://github.com/idosams/Mesh/pull/15), `e2cf7a29922e772d4f4de41ad9429e1879c3a65e`; localized desktop attachment controls; stacked on #13; not merged |
| A05 | `29caceb1bfcb353abe1a6d8927adc8a05fa7c3ca` | feat: browse exact attached-project version history pages | [Mesh PR #17](https://github.com/idosams/Mesh/pull/17), `c7a7b557a326d4754597649e9317e65e5fc02236`; localized stable version pages; stacked on #15; not merged |
| A05 | `d52640a58edc6fb2b9ab07dd59ddbcb087c31499` | feat: inspect immutable attached-project files in desktop | [Mesh PR #18](https://github.com/idosams/Mesh/pull/18), `493927c7461f5beeff1ae686c0e8d0b4cce43d5f`; localized exact saved-file inspection; stacked on #17; not merged |
| A05 | `923187d270568917c7805435b3e3020ab353d51c` | feat: compare exact attached-project versions with pinned previews | [Mesh PR #19](https://github.com/idosams/Mesh/pull/19), `8c4884efdbe7bd34d3f05079193324df1a5e534c`; exact localized comparison; canonical historical-path handling retained; stacked on #18; not merged |
| A05 | `9a68afb42be45259902fa5061970d297223c7eae` | feat: pin independent attached-project comparisons side by side | [Mesh PR #21](https://github.com/idosams/Mesh/pull/21), `548fc7b1c488dc9f123a5aee7195e9eaa8f74831`; independent localized pins, stacked on #20; full local gate passed (3,128 Rust, 563 desktop, 44 daemon checks); hosted CI: six jobs pass, macOS move-settling regression fails (run 36346038402); not merged |
| A06 | `894127b78ba9d1c4104b013fd3c3e31dce15a504` | feat: restore registered attachment projects stopped after restart | [Mesh PR #22](https://github.com/idosams/Mesh/pull/22), `397b98b0d3c22814c4f3b569d575e1b02de6ab66`; native catalog and stopped restart recovery; stacked on #21; full local gate passed (3,131 Rust, 564 desktop, 44 daemon checks); all seven hosted checks pass (run 36346924533); not merged |
| A06 | `d2d9e8c5ca43e592dd5eaeaa889fe21e067c3cec` | feat: persist native comparison pin selectors with revision checks | [Mesh PR #25](https://github.com/idosams/Mesh/pull/25), `c8bb6351ea8c11d30f52c07723a5fd8e0f5827ab`; bounded catalog-bound native pin snapshots; stacked on #24; full local gate passed (3,134 Rust, one passed/leaky, 564 desktop, 44 daemon checks); all seven hosted checks pass (run 36348877073); not merged |
| A06 | `db065842401b6743d4b44f6f71517306be09d33e` | Restore attached project comparison pins through native history | A06c: replacement `4cbd1ca0e87f35b31aa39743a2460cadc789acbb`, [PR #26](https://github.com/idosams/Mesh/pull/26), stacked on #25; localized native-backed desktop pin restoration; full local gate passed (3,135 Rust, 570 desktop, 44 daemon checks); all seven hosted checks pass at `1ed78329fd929dda28e6d4d00d101d1b10dfad31` (run 36349767079); not merged |
| A06 | `ba65636e1a097a5491172226859c6d61994d8539` | Persist project detachment while retaining history and ordinary workflow | A06d: replacement `a7bda0f5105d77fd048ef704cadec8cfe21ada54`, [PR #27](https://github.com/idosams/Mesh/pull/27), stacked on #26; localized persistent detachment; full local gate passed (3,138 Rust, 572 desktop, 44 daemon checks); all seven hosted checks pass at `74a1c0d6f90bfe0dbff17b9b8fac01a593011ed0` (run 36350651196); not merged |
| A07 | `e3a2dc84ca3fcc33997cb802a0d7ccb319f921c8` | Verify packaged attachment capture against exact sealed bundles | A07a: [PR #28](https://github.com/idosams/Mesh/pull/28), transfer `7ddffdee0c8e2f57f4c1c0b490c785c7f4630ccb` plus exact-identity fixes through `fda9df38835421d72ca71abff2ac0c568162297d`; stacked on #27; packaged CLI proof at `fda9df38835421d72ca71abff2ac0c568162297d` passed (three versions, 6,000 ms), wrong-revision/broken-seal refusals pass; final full gate passed (3,139 Rust, 574 desktop, 44 daemon checks); all seven hosted checks pass at `716de71ecfe96b6a078447f2c960ccc9e1795d7e` (run 36351777392); not merged |
| A07 | `f99295541624f312172947ec75458dd6ae01bcb3` | Wake attached project capture from native macOS filesystem events | A07b: [PR #29](https://github.com/idosams/Mesh/pull/29), native filesystem-event wakeups with periodic fallback and localized status; replacement `c80c9fab57a19f10589bdb8bd9cf6cbdebb5f9e4`, stacked on #28; full local gate passed (3,141 Rust, 576 desktop, 44 daemon checks) and packaged event capture passed; initial post-sign identity-check failure retained; all seven hosted checks passed at `052e0d24254ce7066b433997c2c50c524e4640d4` in run 36352654983, not merged |
| A08 | `e320c5c928ad01af204570566f546daf8a76a045` | Record exact review requests from attached project history | A08a: [PR #31](https://github.com/idosams/Mesh/pull/31), native durable requests, exact desktop reopen and localized saved-result inspection; implementation `129966e4d669ee969fa80312baabc814e3f02442`; includes the directory-lock prerequisite from `60b9234ee123d242980f472a2558766f0b62659f`; stacked on #30; corrected full gate passed (3,146 Rust tests, 13 skipped; 580 desktop tests; 44 daemon checks); all seven hosted checks passed at `2f0e92c49a86f659995a0e526766eec020de4de7` in run 36354651857, not merged |
| A08 | `0823850023496505f3c45074ea972c2b36c1dbc0` | Preserve pending review bases as shared main advances | A08 prerequisite: [PR #30](https://github.com/idosams/Mesh/pull/30), replacement `9b17bf50e10f4c972dd3dae371bb691c26913c6b`, transferred before review-request admission to preserve immutable pending reviews; stacked on #29; full local gate passed (3,142 Rust tests, 13 skipped; 576 desktop tests; 44 daemon checks), all seven hosted checks passed at `a054186c4ee0828239dfa247de420507077c3b10` in run 36353559670, not merged |
| A08 | `60b9234ee123d242980f472a2558766f0b62659f` | Add exact human approval for attached project main | A08b: [PR #32](https://github.com/idosams/Mesh/pull/32), replacement `893dca74c13f919036397dc7cf2af6baff88c667`, remaining native exact-approval implementation transferred on #31; full gate passed (3,150 native tests, 13 skips; 580 desktop tests; 44 daemon checks); all seven hosted checks passed at `f58b2107fd3da5667c816d11a701de559d560fe9` in run 36355574791, not merged. Directory-lock prerequisite already delivered with A08a (#31), not reapplied; desktop approval controls remain a separate increment |
| A08 | `16eda49617c2e1f146950c2e08b3b0169a63884e` | Wire attachment main review and approval into desktop | A08c: [PR #33](https://github.com/idosams/Mesh/pull/33), replacement `68ab496a804204111b3087ee7c7fc9a1640adb55`, native confirmation, verified main inspection and localized controls on #32; full gate passed (3,151 native tests, one passed with a lingering-handle flag, 13 skips; 587 desktop tests; 44 daemon checks); hosted CI pending, not merged |
| A09a | `c15c354d30178972b90bf919c051328f8ec2637b` | Preview accepted main against ongoing source work | [Mesh PR #35](https://github.com/idosams/Mesh/pull/35), `3119b8b6233d17309d283d30d6aa855c2a14b8ce`; stacked on #34; not merged |
| A09b | `38d0a9386e3fef2c3670ff371533e204e1bc76e3` | Retain displaced attachment files during native integration | [Mesh PR #36](https://github.com/idosams/Mesh/pull/36), `c7843205da566684a9541010e03e1a49148e442c`; stacked on #35; all seven hosted checks pass in run 36359358791; initial and unchanged local gates failed native startup waits; not merged |
| A09c | `7d06b8bd68d77ac9c358bf9002c829cd02995fa0` | Inspect retained integration recovery without replaying writes | [Mesh PR #38](https://github.com/idosams/Mesh/pull/38), `7ccdc4be19e66840bc005b6ba9302e7bc706807a`; focused checks passed, local startup and hosted Linux restart failures retained; not merged |
| A09d | `11df227f749aa8654fe89a0612e8f2b286c29b4a` | Restore retained work through a new preserving transaction | [Mesh PR #42](https://github.com/idosams/Mesh/pull/42), `c6dfcf3ba53a932c4f12e41fdea45c1ca16e5f55`; all seven hosted checks pass in run 36363835722; local native/full validation and review pending; not merged |
| A09b prerequisite | `633af5ca9d81e6c71532b24332fe4310dac0899d` | Preserve file allocation identity while copying native metadata | Included in [Mesh PR #36](https://github.com/idosams/Mesh/pull/36), `c7843205da566684a9541010e03e1a49148e442c`; do not apply twice |
| A09 | `1c332ba96d3a74c33701143ef54003f35895e1e3` | Connect attached-file recovery to native desktop confirmation | Transferred onto published #42; canonical localization and native-mode contract adapted; validation and PR pending |
| L01 | `2680f5b1c99e5a4b8c96f24b7678c7d2afe32518` | Open attached saved versions as independent work lanes | Pending transfer |
| L01 | `c728c41c9831e636a6bdfbb9b0212739ad6ba974` | Connect attached saved versions to managed fleet lanes | Pending transfer |
| L01 | `e987565983cc7f57be33fe211f9f4bb4d290fad2` | Expose scoped fleet MCP through the packaged desktop app | [Mesh PR #46](https://github.com/idosams/Mesh/pull/46), `1434d269759e0ca62d9bc14b62ec6202580133a5`; all seven hosted checks passed; local full/package validation and human review pending; not merged |
| L02 | `877188898d0151d958bb06230b2e3797611a5ff2` | Persist native fleet discovery without adopting uncertain workers | [Mesh PR #47](https://github.com/idosams/Mesh/pull/47), `046ba075efc9290f9402ca2f3e4269610d1053fb`; all seven hosted checks passed; local full/package validation and human review pending; not merged |
| L02 | `60a1c5bc5c573a4b87e57d9f8b0b9c7a1c28c3e9` | Connect desktop-owned fleet scheduling and activity | [Mesh PR #48](https://github.com/idosams/Mesh/pull/48), `f1578135da3da1aca14d77612117caf8ac031620`; all seven hosted checks passed; local full/package validation and human review pending; not merged |
| L02 | `c7922b3d74cae8715066e2868533ba714d67ddb0` | Connect fleet provisioning and live lane controls to project view | [Mesh PR #49](https://github.com/idosams/Mesh/pull/49), `4a6126868cbe26b59c751488e2aa56f4dbe5072f`; 620 local desktop/interface tests passed; all seven hosted checks passed; local full/package validation and human review pending; not merged |
| R01 | `7d617e60828c6d6cae0e71a52f60fd4095eaed9a` | Read pinned fleet reviews independently of live work | [Mesh PR #50](https://github.com/idosams/Mesh/pull/50), `c2c625a1ff7fe9bfc0acd42f0655e637890f24d7`; all seven hosted checks passed; local native/full and packaged checks pending; not merged |
| R01 | `2e8992f8bbb6c990e399a4a308969c7f88a95d37` | Connect exact fleet results to independent parallel review panels | [Mesh PR #51](https://github.com/idosams/Mesh/pull/51), `35f486661ed4aa82ea59327da7cec75e3a2da8f5`; 634 local desktop/interface tests passed; all seven hosted checks passed; not merged |
| R01 | `b1a166da5cfb714c6677b02b0343e816ea47fcf4` | Bind fleet comparisons to verified starting versions | [Mesh PR #52](https://github.com/idosams/Mesh/pull/52), `90be90858d8a8c1271160edc8bdccd3b160c26a5`; all seven hosted checks passed; not merged |
| R01 | `5a553e78177cc045f10af15a2da86bdb32c95bf0` | Show starting-version comparisons in pinned fleet reviews | [Mesh PR #53](https://github.com/idosams/Mesh/pull/53), `4fa921244a3c02322114c60e7e09c54764621a06`; 646 local desktop/interface tests passed; hosted checks running; not merged |
| R01 | `5b9f758c2db294b636bc2bfd9b4eb867a2fcb2ff` | Persist exact fleet review selectors through native storage | Pending transfer |
| R01 | `20b01d6023616e02e2c9e5cabdc82dc39bb82c3b` | Restore exact fleet review selections and independent view state | Pending transfer |
| R02 | `b6b388caf30c7ffb1cc4d23f09f7175e0a220089` | Persist verified lane starting versions for history recovery | Pending transfer |
| R02 | `ce73d00a2a762eb514d47fbb429e79673157e3e3` | Reopen saved fleet history without adopting execution | Pending transfer |
| R02 | `e961781d941c938de408b38866c4b43cee36f7fe` | feat(desktop): preview exact saved artifacts in parallel fleet reviews | Pending transfer |
| R03 | `76d24877696474f2383429ca0b80c8eeab3ba3f4` | feat(fleet): record exact review change requests for originating lanes | [Mesh PR #59](https://github.com/idosams/Mesh/pull/59), `27ce54b6912dd2da9aafae628cadfec52537e30e`; exact feedback; stacked on #58; not merged |
| R03 | `367ec923ba6e1bcd3d15d62429ddec7ca49bda06` | feat(fleet): link proposed saved results to review change requests | [Mesh PR #60](https://github.com/idosams/Mesh/pull/60), `dc315053ac01b86cc5125155644a4f726a2be2b6`; proposed results; stacked on #59; not merged |
| R03 | `f14d5344ebb5826a63aa9f2878902676e6f0140a` | feat(fleet): confirm reversible review request decisions | [Mesh PR #61](https://github.com/idosams/Mesh/pull/61), `3fbc74fe2c5ba8e86d0686807687394bf0b97ae1`; reversible decisions; stacked on #60; not merged |
| C01 | `302f6cbb0a795b9743ef6c72447d803cdc7860ab` | feat(fleet): verify original project input correspondence | [Mesh PR #62](https://github.com/idosams/Mesh/pull/62), `d68ea27f6876f3a206d23ec564e706d7098dd45a`; original input correspondence; stacked on corrected #61; not merged |
| C01 | `ee7b74fc47a831a3b7eb3827647f7aa841621fa7` | feat(fleet): trace delegated results to original project inputs | [Mesh PR #63](https://github.com/idosams/Mesh/pull/63), `a9bc1c78f486cdfc55a0c9e5985feddf4ca8aaf7`; exact delegated ancestry; stacked on #62; not merged |
| C01 | `0f79d16c0456d83e707e6652e8bd4e3774683eea` | feat(fleet): stage exact project candidates outside capture history | [Mesh PR #64](https://github.com/idosams/Mesh/pull/64), `7d2afb61254ef46c691f760ff66a6ed0d52f7ad4`; exact external candidate staging; stacked on #63; not merged |
| C02 | `27b5efc316a84a896189cb85952fa468c5e5b57c` | feat(fleet): review candidates against fixed project main | [Mesh PR #65](https://github.com/idosams/Mesh/pull/65), `de5e2f03e68a4d056d99551c1c9e52316c46201d`; fixed-main candidate review; stacked on #64; not merged |
| C02 | `2263d5db328d3aab957218642c61e3e3b94d6047` | feat(desktop): expose fixed fleet project candidate reviews | This increment, C02b; native desktop mapping/preparation/review routes; stacked on published #65; not merged |
| C02 | `c53e4fdc3494eb0037fa43326d2a56e4c7c3a77b` | feat(desktop): pin project comparisons with durable preparation inputs | Pending transfer |
| C03 | `3aef42a9373ff0ebb84fd482819d39536b022ab2` | feat(history): prepare operations against exact saved ancestry | Pending transfer |
| C03 | `d3b34517d6f9f390f2834a141717254861bc5358` | Keep attachment captures independent of candidate branches | Pending transfer |
| C03 | `0d6114701a778f1277cbd69ff2bc3280feb2a716` | Compile fleet candidates with original project object identity | [PR #70](https://github.com/idosams/Mesh/pull/70), replacement `46d473879e07b24f51bef5e9a2bb816c1448b02e`; full validation pending |
| C03 | `a8d3bd00105941a9ceb23bd9215b87d367645d5a` | Persist signed fleet candidate imports with exact retry recovery | Pending transfer |
| C04 | `3a3b0da0139b16fb527de4bf8a9ece0d9772f523` | Connect candidate imports to desktop fixed project reviews | Pending transfer |
| C04 | `15111733b9da3565d5a1ddd8aba6457b0b2f85b9` | Open exact project reviews directly from fleet panels | Pending transfer |
| C04 | `0ccc58c78b0278b4a8e84cc9de7fdaa073563ccc` | Persist exact pending fleet review operation inputs | Pending transfer |
| C04 | `7497890ae6ae63a64974e958d720c4e446caf9ef` | Recover pending fleet review submissions through the desktop | Pending transfer |
| D01 | `62e8c885ab9c72fa257b1f3cea2483b650e3a4a8` | Bind explicit file deletion resolution to agent custody | Pending transfer |
| D01 | `278beb6d5549a2380953f0cbfb2e1351566637df` | Add durable explicit agent file deletion recovery | Pending transfer |
| D01 | `6c40c1e1c6562a931d533219032d4c2f0e04f3f2` | Record exact packaged fleet deletion verification | Pending transfer |
| D01 | `c7ac2e33de18a20b2517e23b667b0b3847e0ed19` | Make deletion-only lane results inspectable without approval authority | Pending transfer |
| D01 | `c1835ae651f4ca57edd399be0697b3c73a17520d` | Record packaged empty-result inspection evidence | Pending transfer |
| I01 | `496debf33e758b05985c493a7ecb30a230fdcd42` | Add native accepted-review replacement groups and retained recovery | Pending transfer |
| I01 | `0a1af9129359a9bfbfe1ef40131d2c38d6ffd481` | Reuse bounded project captures across integration groups | Pending transfer |
| I02 | `319a4cd7a0d093689e6aa2d4f168830376a01337` | Retain approved file removals in native integration groups | Pending transfer |
| I02 | `b8e9d0dcf1cafd65a06a390a1222579941b3236b` | Add approved regular files through retained integration groups | Pending transfer |
| I02 | `425f2e06e2ab9ce2c7f01ed8013416527d8c7d3f` | Inherit destination permissions for approved file additions | Pending transfer |
| I03 | `59e0b5b13995e516ed112b09cd295311807523e3` | Restore retained files into absent attached paths | Pending transfer |
| I03 | `fc5b37d9bea6edf4311dea4a2914145da4e600ab` | Connect complete regular-file groups to desktop confirmation and recovery | Pending transfer |
| I03 | `9bfc1f43466ec5a45082a854e51421dab2ae32d2` | Reopen durable group execution evidence in recovery views | Pending transfer |
| I04 | `c711c6531220f61bfe5e9994f12df19c4450bd2c` | Integrate approved directory subtrees in review groups | Pending transfer |
| I05 | `a02b33d8c7454a440d3ca94ae22f34e762913cc6` | Retain approved directory removals in review groups | Pending transfer |
| I06 | `6ec9c8ccf9e273de25d1a84c323c0cd4f3f4b821` | Retain approved file and directory conversions in review groups | Pending transfer |

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
