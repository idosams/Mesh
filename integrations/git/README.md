# Git integration

**Maturity: bounded functional alpha.** Importing a Git project gives every native Mesh workspace
an independently owned copy of the imported history, current branch, index baseline, and repository
configuration. Normal `git status`, local branches, and commits work inside that folder without
sharing locks, refs, or mutable metadata with the original project or another agent folder.

After the person records and approves an exact Mesh review, the desktop can create
`mesh/approved/<shared-version>` in the original repository. The export writes verified Git objects
and creates that review branch atomically; it does not switch the original checkout, change its
index, rewrite its working files, push a remote, or merge anything. The ordinary **Update original
folder** flow remains a separate previewed file-tree operation.

This is not a general synchronization adapter. Git submodules are refused for independent setup,
agent-local Git changes are not automatically copied back, and remote publication remains outside
the alpha. The implementation lives in `crates/mesh-git-bridge`; the [desktop guide](../../apps/desktop/README.md)
documents the user journey and its current limits.
