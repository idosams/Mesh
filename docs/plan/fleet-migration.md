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

Independent CI prerequisite: [Mesh PR #5](https://github.com/idosams/Mesh/pull/5),
`337d2f56caa719f903c32c1170c74111c25ca328`, isolates two recovery deadline tests
using the existing nextest scheduling policy. It changes no timer, assertion or product code.
It is based directly on canonical main, is not included in this fleet stack, and remains unmerged.

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
| A02 | `41bc6bf58615da3ff73922be2bb968b92925bd42` | feat: commit captured file batches as one durable signed version | Pending transfer |
| A02 | `f5860ae7d0647f6539616890037b3b4ec1080ffc` | feat: save attached-project versions in external native history | Pending transfer |
| A03 | `9e3e367c9be2f72fecd2fcd6304fbbac5ac31035` | feat: reconcile attached projects with a native background capture controller | Pending transfer |
| A03 | `8e595348462ebcdc772e71f455f5b1f457aae109` | feat: expose native attached-project capture commands for harnesses | Pending transfer |
| A03 | `f8ec9022b9dbe6a44b6a8fa8894758e6962b1a62` | feat: provision native external attachment history storage | Pending transfer |
| A04 | `76f453c67687525a88dd63171530e63a722eaead` | feat: add native desktop attachment session controls | Pending transfer |
| A04 | `367e86a1b933d167083a0cb396e62b0ac3695710` | feat: connect existing-project attachment controls to desktop UI | Pending transfer |
| A05 | `29caceb1bfcb353abe1a6d8927adc8a05fa7c3ca` | feat: browse exact attached-project version history pages | Pending transfer |
| A05 | `d52640a58edc6fb2b9ab07dd59ddbcb087c31499` | feat: inspect immutable attached-project files in desktop | Pending transfer |
| A05 | `923187d270568917c7805435b3e3020ab353d51c` | feat: compare exact attached-project versions with pinned previews | Pending transfer |
| A05 | `9a68afb42be45259902fa5061970d297223c7eae` | feat: pin independent attached-project comparisons side by side | Pending transfer |
| A06 | `894127b78ba9d1c4104b013fd3c3e31dce15a504` | feat: restore registered attachment projects stopped after restart | Pending transfer |
| A06 | `d2d9e8c5ca43e592dd5eaeaa889fe21e067c3cec` | feat: persist native comparison pin selectors with revision checks | Pending transfer |
| A06 | `db065842401b6743d4b44f6f71517306be09d33e` | Restore attached project comparison pins through native history | Pending transfer |
| A06 | `ba65636e1a097a5491172226859c6d61994d8539` | Persist project detachment while retaining history and ordinary workflow | Pending transfer |
| A07 | `e3a2dc84ca3fcc33997cb802a0d7ccb319f921c8` | Verify packaged attachment capture against exact sealed bundles | Pending transfer |
| A07 | `f99295541624f312172947ec75458dd6ae01bcb3` | Wake attached project capture from native macOS filesystem events | Pending transfer |
| A08 | `e320c5c928ad01af204570566f546daf8a76a045` | Record exact review requests from attached project history | Pending transfer |
| A08 | `0823850023496505f3c45074ea972c2b36c1dbc0` | Preserve pending review bases as shared main advances | Pending transfer |
| A08 | `60b9234ee123d242980f472a2558766f0b62659f` | Add exact human approval for attached project main | Pending transfer |
| A08 | `16eda49617c2e1f146950c2e08b3b0169a63884e` | Wire attachment main review and approval into desktop | Pending transfer |
| A09 | `c15c354d30178972b90bf919c051328f8ec2637b` | Preview accepted main against ongoing source work | Pending transfer |
| A09 | `38d0a9386e3fef2c3670ff371533e204e1bc76e3` | Retain displaced attachment files during native integration | Pending transfer |
| A09 | `7d06b8bd68d77ac9c358bf9002c829cd02995fa0` | Inspect retained integration recovery without replaying writes | Pending transfer |
| A09 | `11df227f749aa8654fe89a0612e8f2b286c29b4a` | Restore retained work through a new preserving transaction | Pending transfer |
| A09 | `633af5ca9d81e6c71532b24332fe4310dac0899d` | Preserve file allocation identity while copying native metadata | Pending transfer |
| A09 | `1c332ba96d3a74c33701143ef54003f35895e1e3` | Connect attached-file recovery to native desktop confirmation | Pending transfer |
| L01 | `2680f5b1c99e5a4b8c96f24b7678c7d2afe32518` | Open attached saved versions as independent work lanes | Pending transfer |
| L01 | `c728c41c9831e636a6bdfbb9b0212739ad6ba974` | Connect attached saved versions to managed fleet lanes | Pending transfer |
| L01 | `e987565983cc7f57be33fe211f9f4bb4d290fad2` | Expose scoped fleet MCP through the packaged desktop app | Pending transfer |
| L02 | `877188898d0151d958bb06230b2e3797611a5ff2` | Persist native fleet discovery without adopting uncertain workers | Pending transfer |
| L02 | `60a1c5bc5c573a4b87e57d9f8b0b9c7a1c28c3e9` | Connect desktop-owned fleet scheduling and activity | Pending transfer |
| L02 | `c7922b3d74cae8715066e2868533ba714d67ddb0` | Connect fleet provisioning and live lane controls to project view | Pending transfer |
| R01 | `7d617e60828c6d6cae0e71a52f60fd4095eaed9a` | Read pinned fleet reviews independently of live work | Pending transfer |
| R01 | `2e8992f8bbb6c990e399a4a308969c7f88a95d37` | Connect exact fleet results to independent parallel review panels | Pending transfer |
| R01 | `b1a166da5cfb714c6677b02b0343e816ea47fcf4` | Bind fleet comparisons to verified starting versions | Pending transfer |
| R01 | `5a553e78177cc045f10af15a2da86bdb32c95bf0` | Show starting-version comparisons in pinned fleet reviews | Pending transfer |
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

## Remaining full objective

Migration does not close any product phase. Native graphical approval and recovery, worker recovery
and wakeup, private dependency closure and combined review, a second real provider, measured four-worker
performance, remote execution on a second machine, and the final requirements audit remain explicit
work. Consult the full plan rather than treating this preserved commit inventory as the completion scope.
