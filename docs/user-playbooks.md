# Local alpha user playbooks

Use an Apple-silicon Mac and a disposable or independently backed-up project. Install the
revision-bound technical alpha using the [installation guide](launch/public-alpha.md). Keep the
original folder and managed workspace until you have verified any exported files. These playbooks
cover local work; they do not imply remote delivery or saving while Mesh is closed.

## First session

1. Quit any older Mesh copy and open the intended build. Record its version and source revision.
2. Import a small folder containing an ordinary text file. Inspect the preview before confirming.
3. Open the managed native folder from Mesh. Edit that copy, not the original folder.
4. Return to Mesh, inspect the change, and save it privately. Wait for **Saved privately**.
5. Quit and reopen Mesh. Confirm the same project and saved bytes are available.
6. Inspect the original folder independently; importing and private saving should not change it.

Expected result: the managed copy contains the saved edit after restart, with retained history.
If the folder is empty, unsupported, or changes during import, preserve it and resolve the reported
problem before retrying. Do not delete private storage to clear an error.

An import must contain at least one included file or subfolder. An empty folder, or a folder
containing only excluded Git metadata, cannot establish the current journal's directory tree.
Choose a folder containing project content. Alpha.4 reports this case as “the imported journal
did not materialize the verified tree exactly”; the subsequent source fix refuses it clearly at
preview/preparation instead. Refresh alone does not add the missing project content. If that
message appears for a populated folder, preserve it and report its path and entry counts; do not
assume the empty-folder diagnosis applies.

## Work with an agent

1. Select the exact version and native folder the agent should use. Start Codex from Mesh.
2. Confirm the folder shown to the agent. The optional read-only Mesh tool must report that same
   folder. A missing tool is a context warning; it does not grant a different folder authority.
3. Let the agent work in its pinned folder. Switching versions in Mesh does not move the running
   agent; return to that exact folder to inspect its work.
4. Stop the agent and every related editor or terminal that can still write there.
5. Choose **Finish agent handoff** and follow the inspection/save prompts. The finish sequence
   checks for final edits before releasing the folder. Resolve any remaining **Needs attention**.
6. Review the resulting saved version before exporting anything.

Expected result: the finished agent changes are saved privately and inspectable. If files keep
changing, stop their writer and inspect again. Never claim an agent finished solely because its
chat message says it did. See the [Codex integration](../integrations/codex/README.md).

## Save and recover local work

Automatic private saving covers supported changes while Mesh is running. It is not a backup of
all edits, and it does not prove that an arbitrary editor has finished writing. Check the visible
state and use explicit inspection/save when needed.

After a crash, reopen the same managed workspace and inspect the reported recovery state before
editing. Retain the workspace and diagnostics if Mesh refuses a replaced path, link, or incomplete
mutation. Do not repair the SQLite database or remove recovery markers manually.

To recover an earlier result, inspect the saved version and use the offered restore or native-folder
action. Preserve any current unsaved work first. Compare the recovered bytes before continuing;
retained history and a successfully opened folder are different checks.

## Review and export

1. Select the exact saved version. Inspect every changed file and the available before/after view.
2. For Office files or PDFs, use exact-copy inspection when layout or unsupported document features
   matter. A text representation is not proof that the complete document is unchanged.
3. In the ad-hoc technical alpha, expect **Approval unavailable**. Export a private copy only to a
   separate empty destination and check its contents independently.
4. In an eligible Apple-signed build, complete the exact review and native user-presence approval
   before previewing **Update original folder**. Recheck the destination and changes before applying.

If the workspace or reviewed version changes, request a fresh preview and review. A prior approval
does not authorize newer bytes. Original-folder update and Git review-branch export require the
approval-capable build; the technical-alpha archive cannot validate those journeys.

## Switch builds or uninstall

Stop agents and editors, save and verify needed work, export a separate recovery copy, and quit
Mesh before opening another build. Keep the old application and workspace data until the new build
has reopened the workspace successfully. Do not assume a downgrade can read newer persisted data.
To uninstall, remove the application only after exporting needed work; retain application-support
data until those exports have been checked. There is no automatic updater.

## Report a problem

Record the exact build revision, macOS version, the action, expected result, actual result, and
whether the original or managed folder was involved. Use **Current → More workspace actions →
Copy safe diagnostics**, inspect the JSON, and attach only the information needed to reproduce the
problem. Keep raw project files and private keys out of reports.

Use [GitHub Issues](https://github.com/idosams/Mesh/issues) for functional problems. Use the
[security policy](../.github/SECURITY.md) for security findings. If work might be lost, stop writes
and preserve the affected folder before retrying.
