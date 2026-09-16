# Contributing to Mesh

Thank you for helping test and improve Mesh. Start with the [developer guide](docs/developer-guide.md)
and [project status](docs/project-status.md), then open an issue before beginning a substantial
change.

## Build and test

```bash
npm test
```

For dependency changes, also run:

```bash
cargo deny check
```

Every behavior change needs a regression test that fails without the change. Reliability and
performance claims need a reproducible measurement. Do not weaken an assertion, threshold, or
security check merely to make a change pass.

## Pull requests

- Keep one coherent change per pull request.
- Describe the user-visible outcome and the failure mode.
- Include the exact commands and observed results used for verification.
- Update the user guide and project status when behavior, setup, security posture, or maturity
  changes.
- Never include credentials, private workspace contents, or unredacted support material.

Changes to cryptography, key handling, authorization, approval, publication, protocol schemas,
licensing, CI, or packaging require a named human reviewer. The author of a change does not own
its final security or compatibility oracle.

## Review evidence

A pull request should make its evidence reproducible by someone who did not author the change.
Record the exact revision, platform, commands, and results. When a test depends on macOS dialogs,
Secure Enclave presence, filesystem events, or another host capability, state that boundary and
include the closest deterministic test that can run in CI. A passing mock is not evidence that the
native integration ran.

Security-sensitive changes need both a positive test and a refusal test. The refusal should prove
that stale generations, replaced workspaces, mismatched paths, incomplete inspection, or missing
human presence cannot reuse earlier authority. Never replace a failing end-to-end check with a
source-text assertion unless the behavior is genuinely impossible to execute in automation and
the limitation is documented.

## Code and compatibility

Keep public interfaces small and explicit. New protocol fields need a documented compatibility
story, canonical encoding tests, bounds, and malformed-input cases. Filesystem changes need tests
for files, directories, missing sources, binary or unsupported entries, and ambiguous native
replies. UI changes must preserve keyboard access, focus ownership, live announcements, and the
exact-generation action boundary.

Run formatting before requesting review. Avoid unrelated rewrites in the same change, preserve
existing user data and dirty worktrees, and do not delete or rewrite history as part of an ordinary
bug fix. If migration is unavoidable, document its recovery path and test both old and new state.

## Reporting results

Use precise maturity language. Say whether evidence is a unit test, integration test, packaged-app
run, or clean-machine acceptance. Do not describe planned work as shipped, an unsigned local build
as a distributable release, or an ignored platform test as passing. A human reviewer must be able
to distinguish automated evidence from manual verification without reconstructing the run.

## Licensing

Mesh is licensed under Apache-2.0. By submitting a contribution, you license it under the same
terms under section 5 of the license. There is no contributor license agreement or copyright
assignment. Do not submit code or generated output whose provenance and license you cannot account
for.

Security reports must follow [.github/SECURITY.md](.github/SECURITY.md), not a public issue.
