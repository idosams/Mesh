# Local alpha validation record

Date: 2026-09-25. Tested code candidate: `d942533b4727d4b662690e9e59cbac024ca92195`.
Public baseline: `8541601d1e14f225d25e20adfbad38adce009291`. Branch:
`idosams/next-phase-validation`. Subsequent changes that publish this record are documentation only;
the locally built application embeds the tested candidate above, not that later documentation commit.

## Result and scope

The [local-alpha readiness phase](phase-assessment.md) passed its local acceptance criteria.
This validates the existing macOS local journey and the repaired validation process. It does not
claim a new networked product, protected human-approval support in an ad-hoc build, or production
release readiness. The original working checkout and installed app were preserved.

## Evidence

| Check | Command or scope | Observed result |
|---|---|---|
| Complete local suite | `npm test` | Exit 0; 3,031 Rust, 92 React, 439 desktop, five documentation regression tests; license and storage gates; 44 real-daemon assertions |
| Supported minimum runtime | `npm exec --yes --package=node@22.18.0 --call 'node --version && npm run verify:desktop'` | Node 22.18.0; 92 React and 439 desktop tests passed |
| Native document integrations | `npm run test:macos-renderers` | Four PDFKit/Office tests passed using the checked-in synthetic fixtures |
| Explicit measurements | `cargo nextest run -p mesh-store -p mesh-sync-engine --run-ignored only --test-threads 1 --no-fail-fast --success-output immediate` | Two passed; recovery 242 ms for 4,006 records and 12,008 rows, below unchanged 5,000 ms budget |
| Local app build | `npm --prefix apps/desktop run tauri:bundle-local` | Exact embedded source revision, interface resources, and ad-hoc bundle seal verified |
| Packaged journey | `npm --prefix apps/desktop run tauri:prove-local` | Two consecutive final-candidate passes, eight processes each; one included a screenshot, one ran alongside recovery measurement |
| Dependency policy | `cargo deny check`; desktop UI `npm audit --audit-level=moderate` | Passed on unchanged dependency lockfiles; existing unmaintained-package exceptions remained explicit; npm reported zero vulnerabilities |
| Documentation | `npm run verify:docs` | Local Markdown targets, documented literal npm scripts, and checker mutation regressions passed |

The packaged checks exercised onboarding, restart, process/endpoint ownership, native Files actions,
agent handoff and saved results, exact private-export bytes and executable mode, review, version
selection, pinned agent context, native execution, and preservation of the original folder.

The app executable SHA-256 is
`b1f9e204abc02b4607b4d47456edcc2ec66e20e96fd905f8a7de58f676140e96`.
The retained Files screenshot is 2,400 × 1,586 pixels, 290,934 bytes, SHA-256
`64c4e2be802303da74619cd9fda28c322d7c98bf61aab8a3e9806d52be581839`.
These identify local evidence; they are not a published release or an authenticated distribution.

## Failures found and fixed

- Recovery initially took 6,912 ms on a quiet machine. Ordinary appended operations repeatedly
  scanned their complete ancestry. A derived parent lookup avoids that scan when no existing edge
  can close a cycle; out-of-order cycle checks and rejection atomicity remain tested. The initial
  repaired quiet measurement was 236 ms. See the [budget](../benchmarks/budgets/recovery.md).
- A packaged failure initially looked like successful export with missing files. Retained native
  logs established that the renderer had reported a refusal, while the outer harness proceeded to
  check export receipts. The renderer could treat temporary unverified startup as a completed
  refusal. It now waits within the existing deadline and the harness requires the exact expected
  outcome. Regression tests fail on the former behavior and pass on delayed verification; a
  permanently unverified workspace does not produce a success report.
- The export proof also accepted a notice for another destination. Its regression test now requires
  current controls, the selected destination, and finished confirmation state. Independent on-disk
  checks remain mandatory. Native authority and human confirmation were not bypassed.
- Stale native fixture expectations, broken documentation links, obsolete commands, and omitted
  CI checks were repaired. No test limit, dependency exception, or integrity check was weakened.

## Skips and remaining release boundaries

The ordinary Rust suite lists ten ignored tests. Four native document integrations and two
measurements were run explicitly as recorded above. Three are child-process helpers exercised by
their ordinary parent tests. The remaining `write_published_documents` entry deliberately rewrites
the compatibility corpus; running it would accept an encoding change rather than test one, so it
was not executed. Existing compatibility comparisons passed.

Hosted GitHub Actions have not run for this local branch. Linux/Windows runtime packaging was not
validated on this Mac. Developer ID signing, notarization, a downloaded clean-Mac first launch,
and protected native human approval still require the appropriate signing and user environment.
This ad-hoc app correctly reports approval unavailable. No publication, merge, tag, installed-app
replacement, or updater delivery was performed.

The [user playbooks](user-playbooks.md) cover first session, agent handoff, recovery, review/export,
build switching, uninstall, and support. Follow their independent export checks before removing
any original or managed copy. Raw failed-proof homes can contain seed-project files; retain them
privately for diagnosis and remove them once no longer needed.
