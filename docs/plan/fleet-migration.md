# Fleet consolidation and delivery ledger

Canonical repository: **idosams/Mesh**. Mesh-internal is deprecated for development and remains
historical evidence. The full [fleet objective](fleet-orchestration.md) is unchanged.

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

On 2026-09-27 the owner made tested, merged delivery an explicit goal requirement and authorized
eventual merges once checks and required reviews are satisfied. This supersedes the earlier lack
of merge authorization; it is not a completed code review. Integrate dependencies in order, refresh
checks after base changes, and verify the final combined canonical main revision. Keep source PRs
and replacement mappings traceable when a correction is delivered through another increment.
Repository-required named human reviews remain outstanding; do not infer approval from absent
GitHub branch rules. Until final main and the full acceptance journeys are verified, the goal stays
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
| R03 | `76d24877696474f2383429ca0b80c8eeab3ba3f4` | feat(fleet): record exact review change requests for originating lanes | Pending transfer |
| R03 | `367ec923ba6e1bcd3d15d62429ddec7ca49bda06` | feat(fleet): link proposed saved results to review change requests | Pending transfer |
| R03 | `f14d5344ebb5826a63aa9f2878902676e6f0140a` | feat(fleet): confirm reversible review request decisions | Pending transfer |
| C01 | `302f6cbb0a795b9743ef6c72447d803cdc7860ab` | feat(fleet): verify original project input correspondence | Pending transfer |
| C01 | `ee7b74fc47a831a3b7eb3827647f7aa841621fa7` | feat(fleet): trace delegated results to original project inputs | Pending transfer |
| C01 | `0f79d16c0456d83e707e6652e8bd4e3774683eea` | feat(fleet): stage exact project candidates outside capture history | Pending transfer |
| C02 | `27b5efc316a84a896189cb85952fa468c5e5b57c` | feat(fleet): review candidates against fixed project main | Pending transfer |
| C02 | `2263d5db328d3aab957218642c61e3e3b94d6047` | feat(desktop): expose fixed fleet project candidate reviews | Pending transfer |
| C02 | `c53e4fdc3494eb0037fa43326d2a56e4c7c3a77b` | feat(desktop): pin project comparisons with durable preparation inputs | Pending transfer |
| C03 | `3aef42a9373ff0ebb84fd482819d39536b022ab2` | feat(history): prepare operations against exact saved ancestry | Pending transfer |
| C03 | `d3b34517d6f9f390f2834a141717254861bc5358` | Keep attachment captures independent of candidate branches | Pending transfer |
| C03 | `0d6114701a778f1277cbd69ff2bc3280feb2a716` | Compile fleet candidates with original project object identity | Pending transfer |
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
