# Mesh agent workflow

The canonical product and development repository is **idosams/Mesh**:
https://github.com/idosams/Mesh. Mesh-internal is deprecated for ongoing development.
A folder called Mesh is not repository identity. Preserve old checkouts and history.

## Verify the destination before every action

Before edits, pushes, PR creation, or merges, verify origin fetch AND push URLs and the intended base:

```bash
npm run repo:target -- --action edit --base main
npm run repo:target -- --action push --base main
npm run repo:target -- --action pr --base main
npm run repo:target -- --action merge --base main --pr 123
```

Use the actual published parent branch as `--base` for a stacked PR. The edit check uses local
Git refs; fetch the canonical base before starting a new increment. Delivery checks also query live
GitHub identity and remote refs. A stale/missing base, wrong remote, unrelated ancestry, or unpublished
head refuses. Stop that action and resolve the mismatch; do not rename a folder to satisfy it.
The check performs no mutation and grants no merge authorization. Inspect the exact PR checks and
review state separately. Never self-approve, bypass required checks, or merge without explicit user
merge authorization. Use explicit `--repo idosams/Mesh` for GitHub delivery commands.

If the check is absent in an older checkout, inspect `git remote -v`, live GitHub repository identity,
HEAD, the intended base and their merge base manually. Preserve dirty files and running verification;
create a clean worktree from the verified canonical base. Do not blindly merge unrelated histories.
An explicitly requested deprecation-only PR to Mesh-internal is a separate, narrowly scoped task,
not permission to develop product features there or change repository settings.

## Deliver in reviewable increments

Keep the complete [fleet plan](docs/plan/fleet-orchestration.md) intact. The
[migration ledger](docs/plan/fleet-migration.md) maps preserved source commits to canonical delivery.
Transfer coherent changes with dependencies and source commit IDs recorded in each PR. Preserve
canonical alpha.5 changes, including localization and import/recovery fixes. Older local test results
are historical evidence, not validation on the new base.

Push and open one coherent PR before starting the next substantial increment. Dependent PRs may be
stacked in this repository, with their base and dependency PR named explicitly. Update the ledger
with replacement commits and PRs. Report CI, remaining issues and merge status. Local commits,
passing tests, published branches, open PRs and merged delivery are different states.

## Preserve the native authority boundary

Rust/native code owns filesystems, identity, custody, authorization and exact human approval.
React presents typed native facts; it does not resolve arbitrary paths or grant authority. Agent
credentials cannot advance protected main. Preserve unknown or conflicting work and keep raw private
contents and secrets out of logs. Document persisted-format compatibility and refusal behavior.

## Validate the actual repository

Read [CONTRIBUTING.md](CONTRIBUTING.md), [project status](docs/project-status.md), the nearest README,
and relevant decisions. Use this checkout's commands, not commands remembered from Mesh-internal.
Run focused regressions during iteration and `npm test` for substantive code, persistence, security,
CI or workflow changes. The full gate includes repository, docs, license, storage, Rust, desktop and
real daemon demo checks. Do not weaken a gate to pass. Native, packaged, signing and installed-app
claims need evidence at that scope. State limitations explicitly and retain failed verification logs.
