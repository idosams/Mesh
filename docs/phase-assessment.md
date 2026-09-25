# Local alpha readiness assessment

Assessment date: 2026-09-25. Product baseline: public `idosams/Mesh` main at
`8541601d1e14f225d25e20adfbad38adce009291` (alpha.4). The older private repository has a different
history and is not the public product baseline. No open public issues or pull requests named a
next milestone at the time of assessment.

## Phase choice

The next bounded phase is local-alpha validation and operational readiness. Its purpose is to
make the existing local journey reproducible and its documentation usable before expanding into
networked collaboration. This is an engineering readiness phase, not authorization to publish,
replace an installed application, or describe the product as production-ready.

## System analysis

| Area | Existing implementation | Remaining boundary |
|---|---|---|
| Durable state | SQLite, immutable chunks, recovery, private versions | Tests do not establish arbitrary hardware failure tolerance |
| Native workspace | Verified imports, managed copies, saved-version reconstruction | Ambiguous mutations, links, and active writers require refusal or intervention |
| Agent work | Pinned native folders, read-only MCP context, finish handoff | Complete distinct-run identity and version-attributed agent reads remain incomplete |
| Review and publication | Exact review evidence and native human-presence authority | Ad-hoc builds cannot exercise protected approval or original-folder update |
| Desktop | React presentation with native authority and bounded intents | Renderer and packaged journeys require separate runtime verification |
| Git | Independent native history and approval-bound export | No general remote synchronization or automatic agent-history merge |
| Synchronization | Internal protocol and deterministic engine | No supported two-device or hosted user journey |
| Distribution | Revision-bound local alpha tooling | No updater, supported Windows package, or production Linux installation |
| Documentation | Public guides and substantial protocol references | Baseline had 25 dead local links, obsolete commands, and stale Codex configuration instructions |
| Validation | 3,029 ordinary Rust tests discovered in this baseline plus desktop checks | PR CI omitted desktop/React tests, macOS full-workspace linting, and the existing storage-budget gate |

Sources: [project status](project-status.md), [architecture](architecture.md),
[desktop implementation and proof](../apps/desktop/README.md),
[Codex implementation](../apps/desktop/src-tauri/codex_workspace.rs),
[Git adapter](../crates/mesh-git-bridge/), and [CI configuration](../.github/workflows/rust.yml).
Test discovery is not a passing-test claim.

## Acceptance criteria

- The normal test command includes documentation, license, storage-budget, Rust, and desktop checks.
- The documentation audit rejects planted broken links and invalid npm script examples.
- Public Markdown has no missing local link targets or nonexistent literal npm scripts within the
  audit's documented syntax coverage.
- Pull-request CI includes desktop/React checks and macOS full-workspace linting.
- User playbooks cover import, agent custody, recovery, review, export, build switching, and support.
- The complete local suite and real daemon demonstration pass on the candidate.
- Relevant macOS renderer integrations and the exact packaged application proof are executed, with
  platform prerequisites and any unverified behavior recorded explicitly.
- The final source revision, commands, results, and remaining release boundaries are reviewable.

## Validation status

The initial candidate passed the complete local suite: 3,029 Rust, 92 React, 439 desktop,
five documentation tests, the storage-budget mutation checks, and all 44 real-daemon assertions.
All four explicit macOS PDF/Office renderer integrations passed using checked-in synthetic
fixtures. The minimum supported Node 22.18 runtime passed the complete desktop suite. Dependency
checks passed under the existing advisory policy; no exception was added to make them pass.
Hosted CI has not run for this branch.

The separate recovery measurement exposed a regression that the normal cost counters missed:
6,912 ms on a quiet machine against the unchanged 5,000 ms limit. A derived parent lookup removes
redundant ancestry scans for ordinary appends while preserving out-of-order cycle checks. The
same workload then measured 236 ms; all 308 storage tests, including the explicit timing test,
passed. See [the recovery budget](../benchmarks/budgets/recovery.md) for measurement limits.

One packaged run reported private-export completion with the expected files absent. Two reruns
passed, including the original concurrent measurement setup. The proof also had a reproducible
verification gap: it accepted a completion notice for a different destination. A regression test
now rejects that notice; completion requires current controls and the exact selected destination.
Failed runs retain private diagnostics instead of deleting the evidence. This is not yet a
confirmed explanation of the first missing-file failure. The final rebuilt candidate and its
complete suite remain under validation; packaged readiness must not be inferred from earlier
passing reruns alone.

The [developer guide](developer-guide.md) defines the commands and evidence boundaries. The
[user playbooks](user-playbooks.md) are the operational acceptance path. Signing, a clean-Mac
installation, and protected human approval require evidence from the appropriate environment;
source tests and an ad-hoc bundle cannot substitute for them.

## Following product phase

After local readiness is verified, choose a separately scoped milestone for either complete local
agent identity/read tracking or a supported two-device synchronization journey. Each requires its
own compatibility, threat-boundary, failure-recovery, and end-to-end acceptance criteria. Neither
is implemented or approved merely by this assessment.
