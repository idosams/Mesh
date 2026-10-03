# User guide

This guide covers the local Mesh proof that works today. The downloadable technical alpha remains a prerelease.
The default demonstration runs on macOS or Linux, uses a temporary workspace, and does not need an
account or service credential.

## What you can prove today

The command-line demonstration starts the real local service and client, saves an exact file
version, restarts over durable data, opens an exact review, proves a software-held key cannot
publish it, restarts again, previews a redacted support bundle, and shuts down cleanly. The separate
desktop alpha imports an ordinary folder without changing it, opens a stable native working path,
launches Codex in an independently pinned folder, privately saves unambiguous returned changes when
the person confirms that the agent has finished, and can return an approved version to the original
folder after a fresh preview in builds with a validated Developer ID identity. Protected approval
and updating the original folder are unavailable in the ad-hoc public alpha.

Desktop import requires at least one included file or subfolder. Empty folders and Git-metadata-only
folders are unsupported; see the [import troubleshooting playbook](user-playbooks.md#first-session).
The desktop offers English and Hebrew; see [language support](localization.md) for coverage.

It does **not** prove background saving of every editor change, another device receiving the work,
a hosted service, an unattended distributable installer, or a complete agent integration.
See [Project status](project-status.md) for the complete boundary.

## Prerequisites

- macOS or Linux
- Git
- Node.js 22.18 or newer
- the Rust toolchain selected by `rust-toolchain.toml`

The first run may download Rust dependencies. From the repository root:

```bash
node examples/local-daemon-demo.mjs
```

A passing run prints 44 `✓` checks. It exercises real `meshd` and `meshctl` binaries and removes
its temporary workspace when it finishes. If a check fails, the command exits nonzero, identifies
the failed stage, and retains the workspace path for inspection.

After dependencies are cached, the offline proof is:

```bash
node examples/local-daemon-demo.mjs --offline
```

Use `--skip-build` only when the required binaries have already been built. On a Linux machine
configured for FUSE, `--mounted` adds the privileged mounted-workspace proof. The default command
does not claim that privileged path.

For the expanded transcript and equivalent manual commands, read
[Run the local Mesh demo](demo.md). The separate
[mounted-workspace evidence](local-mounted-demo.md) records a privileged environment-specific
proof and its limits.

## Inspect an attached project's saved files (development builds)

The unmerged attachment increments in the [migration ledger](plan/fleet-migration.md) add this
flow to development builds. This is not a claim that the downloadable alpha contains it or that
its packaged UI acceptance is complete.

1. In the folder selection view, use **Choose project to attach**, or enter the existing project
   path and choose **Attach existing project**. Continue using the same folder in your editor,
   terminal or harness; Mesh keeps its history in separate storage.
2. Once a version is saved, choose **Show latest versions**. Use **Older versions** to page back.
3. Select a saved version identity to list its files and folders. **More files** opens the next page.
4. Select a file to inspect its saved text. This preview stays on that exact version even when the
   live folder changes or capture saves a newer version. Refreshing capture status does not replace
   the preview; select another saved version or file to change it.

Text previews are read-only and limited to 256 KiB. Binary and larger files show why text is
unavailable. To compare saved versions, choose **Use as base** on one version and **Compare with base** on
another. Select a changed path to see its saved before/after contents. The open comparison stays
fixed when capture advances or you choose the next comparison base. Missing files, folders and
unavailable text previews have explicit states. Choose **Pin comparison alongside others** to keep up
to eight comparisons open across projects. Each pin has its own page and selected file; you can pin
the same version pair twice to inspect different files. Close a pin to free a slot. Pin selections are saved separately from file contents and return after restart, with contents
verified again from native history. Wait for **Comparison selections saved** before quitting. If
saving fails, views remain open and the status offers retry or an explicit reload of the saved set.
Unavailable comparisons retain their version selection and offer retry rather than showing
unverified content. This view does not approve or apply changes. **Stop capture** and **Resume
capture** control the session without changing the original project's files. After restarting this
development build, registered projects return with capture stopped and their saved history available.
Choose **Resume capture** when ready to capture changes made while Mesh was closed. If the original
project or history is unavailable, Mesh keeps the registration visible and requires the exact original
folder and history before retrying; a replacement folder at the same path is not adopted.

Choose **Detach Mesh** to stop Mesh tracking this project across restarts. Mesh joins its capture
worker before confirming detachment and retains the original files, Git state, saved history and
comparison pins. You can keep using your existing tools. **Reattach project** checks the original
folder identity and restores the controls with capture still stopped; choose **Resume capture**
separately when ready. An unavailable or replaced folder cannot be reattached. If detachment cannot
be confirmed, refresh status and reconcile the reported problem rather than assuming it was saved.

While capture is running, Mesh reports whether file-change signals are active or it is using
periodic checks. On macOS, signals can wake capture sooner; periodic checks still catch missed
changes. This status does not mean an edit has been saved: check the latest saved version and
capture outcome. Mesh does not infer who made a change from filesystem events.

Capture continues while file-change monitoring is being prepared or is unavailable. The status
shows these separately. Once capture shows **Stopped**, it performs no further saves; monitoring
may still be finishing, and that status remains visible until it ends. Resuming starts a new
capture session. An older monitor cannot restart the stopped session.


In the unmerged review-history increment, an already-open pending review stays on its original
comparison base when another review advances main, including after restart. This keeps the reviewed
changes stable; it does not make an old approval valid against the new main version. A stale approval
must be prepared and reviewed again through the normal current-main flow.

In the unmerged desktop recovery increment, **Compare main with working files** can offer **Review
applying this file** for an existing text file that still matches the approved base. The native
confirmation shows the exact current and proposed content, file location and permissions. Confirming
changes that working file and retains its previous file for recovery, including later writes from an
already-open editor. Mesh main and Git are unchanged. Changed files, additions, deletions and grouped
changes cannot use this action yet.

Use **Refresh retained files** to inspect the last observed recovery state. **Review restoring
retained file** prepares a frozen copy of the selected retained work and requires another native
confirmation. The current working file becomes a new retained entry, so undo is another explicit
restoration. Existing retained files remain available. An exact recovery reference can inspect a
record outside the bounded overview. Missing results and later edits require inspection; they do not
prove the earlier action was rolled back. Nothing is retried or deleted automatically.

The native confirmation currently uses English and refuses binary, NUL-containing or oversized
content rather than omitting it. Recovery controls retain English/Hebrew UI text. The default
recovery location must be on the project's volume; offline folders and a location selector for
other volumes remain unfinished. Actual packaged confirmation and recovery acceptance are pending.

## The six words Mesh shows people

| Status | Meaning |
|---|---|
| **Working** | An actor is changing files now. |
| **Saved privately** | The work survived locally and is not shared. |
| **Available to team** | A peer can open it read-only; it is not in the shared version. |
| **Ready for review** | An exact change is waiting for a person. |
| **Needs attention** | A person must decide before work can proceed. |
| **Approved** | The reviewed bytes advanced the protected shared version. |

The ad-hoc technical-alpha archive exercises **Saved privately** and recorded review end to end.
The protected **Approved** transition is implemented but requires a separately Apple-signed build
with a stable, validated application identity; it is unavailable in that archive. The other words
define the intended product experience; they do not imply that team delivery or a hosted workflow
is available.

## Try the desktop technical alpha

The desktop app is a real Tauri surface for the local managed-folder journey. It can import a
folder, retain exact versions, recover across restart, work through ordinary native folders, open a
pinned agent copy in Codex, inspect and explicitly or automatically save supported external changes,
restore earlier versions,
and exercise local review. A separately Apple-signed, identity-continuous build additionally
enables macOS user-presence approval.

```bash
npm --prefix apps/desktop run tauri:dev
```

The development command above is for repository evaluation. A consented tester can instead use the
revision-bound archive described in [Alpha start here](../apps/desktop/ALPHA-START-HERE.txt). The
macOS app is ad-hoc signed and unnotarized, so a downloaded copy requires an explicit **Open**
confirmation; Windows support and unattended installers are not ready. Follow the
[desktop guide](../apps/desktop/README.md) for the exact journey and verification commands.

## Privacy and safety

- The default proof stays on the local machine and opens no hosted account.
- Saved content is retained in local storage; treat its directory like any other sensitive
  developer data.
- The support-bundle command is a local preview. Inspect the preview before sharing anything.
- The CLI demonstration key cannot publish. On supported Macs, an Apple-signed desktop build with
  a validated application identity uses a separate P-256 approval credential whose private key
  stays in the Secure Enclave and whose use requires fresh Touch ID or macOS password presence.
  The ad-hoc technical-alpha archive reports this approval path unavailable.
- Do not use this proof as the only copy of important work.

Report security findings through the [private security channel](../.github/SECURITY.md), not a
public issue. Follow that policy for the report contents and response targets.

## Troubleshooting

If the first build cannot download a dependency, restore network access and retry. If the offline
command fails, run the normal command once so Cargo can populate its cache. If a run fails after
starting, use the retained workspace path printed by the script and rerun without `--offline` to
separate an environment problem from a product failure.

For build-tool errors, check the prerequisites and commands in the
[developer guide](developer-guide.md). For ordinary work, agent handoff, recovery, and export,
follow the [user playbooks](user-playbooks.md).

For known limitations or to verify whether a capability has landed since this guide was updated,
check [Project status](project-status.md).

## Request review of an attached version (development builds)

In the unmerged review-request increment, open **Show latest versions** and choose **Request review**
on the saved version you want reviewed. Mesh records an exact request against its accepted main
(or the empty starting state before the first approval), without changing your working folder.
Use **Show review requests** and **Open review** to return to it after other edits or a restart.
**Inspect saved result** opens that request's saved files rather than the newest capture.

The queue shows up to 32 requests and reports omitted requests. A saved-version request can reopen
its exact current-base request beyond that overview. A selected request remains fixed while capture
continues. Incomplete or unavailable content is labeled; a path overview alone is not complete review.
Change authorship remains unknown. Native main approval and desktop confirmation are separate development increments; applying accepted
content to the working folder remains pending.

## Accepted main for attached projects (native development API)

The unmerged native approval increment can accept an exact saved review as Mesh main, even while your
editor or harness continues producing newer changes. Acceptance records the reviewed saved content
in Mesh history; it does not replace your working files or change Git. A review prepared before main
advances must be prepared again against the new main before approval. Returning to an old review is
still supported for inspection. Desktop approval controls are described below. Applying accepted content to the working folder
remains a separate pending increment.

## Confirm an attached version as Mesh main (development builds)

Choose **Refresh main and approval availability**. Mesh reports the last verified accepted version
and whether this build can approve. If eligible, **Set up approvals on this Mac** enables the native
credential flow. Open a complete saved review, inspect its content, then choose **Approve as Mesh
main…**. The native dialog shows the exact saved result and requires confirmation and Touch ID or
your Mac password. Your working files and Git remain unchanged.

Use **Inspect Mesh main** to reopen the accepted saved result. If confirmation is cancelled or the
response is uncertain, refresh main before retrying; an uncertain response does not mean acceptance
was rolled back. A main change requires a new review against the current main. Ineligible builds and
unverified main status keep approval unavailable. This development flow still needs packaged graphical
and actual platform-presence acceptance evidence.

## Open an attached version as an independent line (development builds)

In the unmerged lane increment, choose **Show latest versions**, then **Create line from this
version**. Mesh creates a separate ordinary folder from those exact saved bytes and displays it
alongside the original project. **Open folder** opens the native-verified folder for your editor or
existing agent harness. The original project, Git checkout and open editor files stay in place.

Each line has its own capture controls, saved versions and comparisons. Its recorded source is
allocation ancestry, not proof of who makes later changes. This increment assigns no managed agent.
The original project's accepted main remains separate; integrating a line's result is not yet
available here. Recovered lines remain stopped until you resume capture.

If creation cannot be confirmed, use **Retry creating this line** in the same session. Mesh reuses
the exact request and preserves edits made after a completed allocation. Incomplete or changed
allocations are retained and refused; they are not silently replaced. If ancestry cannot be
verified, the line remains visible with that uncertainty. Packaged graphical acceptance and the
managed fleet connection remain pending.

The subsequent managed-fleet bridge is a native development API. It can start an additional managed
lane from an exact attached saved version, retaining correlation to the original project through
child delegation. It does not relocate your original folder or convert your existing harness
session to managed custody. Desktop fleet launch controls and real-provider acceptance for this
entry point are still pending; the ordinary **Create line from this version** action remains usable
without selecting a provider.

The scoped desktop fleet bridge is a development integration mode for a native fleet host. The host
supplies a lane-bound session and local endpoint; the bridge never substitutes whichever workspace
is selected in the desktop. It can use the desktop executable instead of a separate development MCP
binary. This does not yet provide a desktop button to start a fleet, adopt an existing agent session,
or approve work. See the [provider verification procedure](../crates/mesh-daemon/tests/README-provider.md)
for the required exact-build checks and current acceptance limits.

In the native fleet-discovery development increment, saved fleet records can be reopened even when
the original project is offline. A restored record reports that its working contexts are not attached;
it does not mean its former agents have stopped, restarted or been adopted. Missing or changed
storage is retained and shown as unavailable. Provisioning creates the fleet record and managed input
without starting a worker. Desktop scheduling and live fleet controls follow separately.

The subsequent desktop scheduling increment adds native start, stop and activity commands for fleets
created by the current app session. It uses the installed Codex provider and Mesh's own scoped bridge.
Repeated start keeps the existing loop. A launch or polling fault requires attention and prevents new
dispatch; recorded observations keep their timestamps. Stop requests cancellation, but does not prove
all descendant processes have exited or release uncertain work. Restored fleets remain unavailable
for execution until reconciliation. Graphical controls and packaged scheduling acceptance are separate
from these development commands.

## Optional local fleets from an attached project

The development desktop's existing-project view includes an **Agent fleets** section. This interface
has source tests; its complete packaged graphical journey is not yet verified.

1. Attach your existing project and let Mesh save a version. Your editor, terminal and harness keep
   using the original folder.
2. Expand **Provision a fleet from saved work**, choose the project and an exact saved version, and
   describe the goal. To choose older input, load the project's saved-version list below.
3. Set the maximum lanes (including the coordinator), simultaneous agents, and delegation depth.
   **Provision fleet** creates additional work; it does not start a provider.
4. Inspect the fleet, then choose **Start agents**. This uses the installed Codex provider and account.
   The lane cards show execution status and the time each worker was last observed.
5. **Stop agents** stops scheduling and requests direct-worker termination. Recovery is still required
   before uncertain worker ownership can be released. A completed agent has not approved main.

If provisioning is unconfirmed, use **Retry this provisioning request**. It preserves the same input,
goal, limits and request within this app view instead of creating another fleet. After an app/renderer
restart, inspect retained fleet state before creating another request; pending-intent recovery across
renderer reload is not implemented. Restored fleets cannot restart workers automatically. Worker
recovery and integration into the original project's main remain unavailable in this view. Existing project history, manual lanes and their comparisons remain independently usable.

Saved fleet results remain independent of newer agent edits and desktop navigation, including after
cancellation or session revocation. Missing or replaced history refuses without changing working files.

For an available fleet lane, including retained fleets after restart, choose **Show saved results**, then **Pin saved review**. Up to eight
panels can remain open while fleet activity refreshes and agents keep working. Each panel keeps an
exact checkpoint/version/review selection and independent file and comparison-layout controls.
Result pages stay fixed until refreshed; they are ordered by checkpoint identifier, not creation time.
Closing a loading panel does not stop a worker. Failed reads retain the selection and label previously
verified cached content explicitly; retry reads that same result.

Within a pinned panel, choose **Compare with starting version** to inspect what that lane changed.
The comparison uses the exact verified local copy of the lane's original input and the pinned saved
result. Select a changed object for its saved before/after content, then choose inline or side-by-side
text comparison. Larger change lists have independent pages. Paths, byte counts and executable modes
remain visible when text is unavailable or unchanged. Binary, unsafe text and files above the 256 KiB
text bound are labeled; they never appear as an unchanged text comparison. A failed request retains
previously verified content and can retry the same request.

Expand **Recorded review against its original review base** to inspect the existing review bundle.
That base can differ from the lane's starting version. Both comparisons remain pinned while agents
work. Supported images, PDF pages and Office documents can request a native preview of the exact
saved bytes. Each panel renders independently; PDF navigation is bounded to the first 64 pages, and
Office previews are representative thumbnails with bounded extracted text when available. Unsupported
formats retain metadata. Rendered content is temporary and is not included in saved pin preferences.
Main approval and integration are not yet connected. Incomplete content
and omitted changes are labeled explicitly. Exact pin selections, selected objects, pages and comparison
layouts are saved outside the project. Reopening the view reloads those selections and rechecks content
through native history. Failed saves retain local selections and expose retry or explicit reload;
reloading replaces local choices with the saved set. Unavailable history keeps its pin visible.
After a full app restart, saved results are reverified through retained native history, including when
the original project is offline. Inspection preserves uncheckpointed work and does not restart workers.
Starting-version comparison after restart requires the starting identity recorded by newer allocations;
older allocations can still expose their recorded review, but Mesh does not guess a starting version.
Missing or replaced history remains unavailable without being recreated. The complete packaged graphical
interaction remains unverified.

Within a pinned fleet review, expand **Request changes to this saved result**. Read recorded requests
before adding feedback, then enter the requested revision and choose **Record change request**.
The request names that exact saved result. The originating agent can read it on its next context call;
Mesh does not claim delivery or restart a stopped agent. Up to 32 requests per lane are retained.
If recording is unconfirmed, use **Retry this exact change request**. The original message and request
identity are retained while the panel remains open. Closing does not cancel a submitted request.
After a restart or reload, read recorded requests before submitting another: saved feedback is durable,
but draft text and unconfirmed retry identities are session-only. Restored fleets support reading;
recording new requests requires recovered native ownership. This interaction still needs packaged
visual verification.

An agent can respond with a proposed saved result after saving and submitting its checkpoint. Choose
**Read recorded change requests** again to discover these proposals, then **Pin proposed result beside
this review** to inspect one independently. The original review stays fixed. Up to eight proposals can
be recorded for each request, within the existing eight-panel limit. A proposed result is not marked
resolved or approved, and a stopped agent is not restarted automatically. Unsupported or unavailable
history is reported when Mesh verifies the new pin.

After inspecting a proposed result, choose **Mark request addressed by this result**. Mesh asks for
native confirmation naming the original request and exact saved result. This changes the request's
work status; approval and integration into main remain separate. Use **Reopen change request** if
further work is needed. Agents see the recorded decision on their next context check.

If the decision response is unconfirmed, **Retry this exact decision** recovers the original receipt.
Mesh still shows the latest decision if someone has since reopened the request. **Read latest state
and choose again** abandons the pending retry only after verifying current history. Reloading or
closing a panel does not cancel a native confirmation already in progress. Pending retry identities
are session-only, and the native graphical confirmation journey remains unverified.

### Choose fleet providers

When provisioning a fleet from a saved version, choose Codex or Claude for the
coordinator and select the providers its workers may use. The coordinator must stay
selected. Each selected provider needs its installed executable and account.
Provisioning prepares lanes; **Start agents** begins provider usage. Review the saved
provider choices on the fleet card before starting. If provisioning is uncertain,
the pending request retains its choices and offers an explicit retry. Refresh a fleet
with unavailable provider choices before starting it; its saved work remains readable.

### Reviewing retained remote results

In the fleet view, choose **Show remote saved results**. Pages remain fixed while work continues;
refresh to discover newer results. An interrupted registration remains visible and cannot be pinned.
Choose **Pin saved review** to keep a received snapshot in an independent panel. File and preview
choices in one panel do not replace another. Native history verifies each exact saved review and
artifact; missing history shows an unavailable state rather than restarting a worker.

These panels show the received result tree. Choose **Prepare for original project** to verify its
original project, fix the observed main and privately prepare the received result. Exact retry inputs
are saved before the action. Under **Remote project actions**, choose **Save as a project version**,
then **Create project review** and **Open this project review**. The project's existing review surface
handles inspection and separate human approval. These remote panels cannot approve Mesh main or
replace working files.

Pending actions remain visible after closing a panel or restarting. Reopening never repeats them:
choose **Read saved import status** to inspect retained truth, or **Retry the exact request** to repeat
the saved action. If main or source history changed, the exact operation may refuse; it never silently
changes its base. **Remove retry entry** removes only retry metadata, not versions or running work.
Up to eight requests can remain retained. Unavailable or interrupted metadata requires reconciliation
and is preserved. This workflow is covered by source/native tests; packaged and real second-machine
acceptance remains unverified.

 Remote panel selections and view choices are saved privately. Reopening verifies the same native
history before showing content. If saving is uncertain, keep the current panels and retry; reloading
the saved set replaces unsaved local choices. Missing history stays listed without restarting agents.

### Remote results based on local lanes

A received result can be prepared for its original project when its input is one exact recorded
review from a local parent lane. Mesh verifies the complete local ancestry and keeps the original
project base fixed through preparation, import and review. An unavailable, ambiguous or replaced
ancestor stops the action; Mesh does not guess a different input. The parent review is fixed when
remote execution is first admitted. Later saves cannot change it. If no unique completed review
existed then, adding a review later does not silently unblock that remote lane. Cancellation or a stopping ancestor
prevents new imports, while completed outcomes remain available for inspection. Main approval and
applying files to the original folder remain separate actions. An ancestry chain containing earlier
remote results still requires additional native support.


## Saved remote worker connections

Under **Agent fleets → Inspect a remote worker**, set up a connection with the native file selectors
and public worker fields. **Load saved connections**, enter a connection name, and choose **Save
current connection** to keep its settings. After restarting Mesh, choose **Load saved connections**
and **Open saved settings** explicitly. Opening verifies the original files and identities; it does
not contact or start the worker. Use **Read worker status** when you want an observation.

Choose **Inspect original input** to explicitly check the original saved input on the worker.
Mesh shows a timestamp and whether those original files were verified, could not be verified, or
have no retained record. Missing records do not prove no work started. Verification does not show
that an agent is running or authorize restarting one. Large input checks can expire; keep the
original attempt and investigate uncertainty. This check does not change saved reviews or resume
an input transfer. The native equivalent is `--coordinator inspect-input <absolute-config>`.

To edit a saved entry, open it, change and apply the setup form, then save the current connection.
To create a separate entry, clear the selected setup files and select them again. **Remove saved
settings** keeps the active selection, credentials and work history. If a settings save was
interrupted, **Recover interrupted settings save** can finish the retained save; conflicting records
remain untouched. Changed keys or identity folders require a new native selection.

This route requires an eligible signed application and an already configured worker. The local
ad-hoc checkpoint does not establish that journey. Remote launch/result-receipt controls, lost-worker recovery
and real second-host acceptance remain unfinished.


If a worker input transfer was interrupted, explicitly open its original connection and choose
**Resume saved input transfer**. Mesh uses the saved version recorded for that attempt, even if your
working files have changed. The worker must still retain the original reservation. Input acceptance
does not mean an agent is running. If Mesh cannot confirm the outcome, inspect worker status before
another explicit resume; it does not request a replacement attempt or automatically retry.


**Find saved remote results** lists up to sixteen saved results at a time. Use **Next results** or
**Previous results** to browse; choose **Find saved remote results** again to refresh from the first
page. **Result identities** shows the exact saved version and review identifiers. A listed result
has not been downloaded or accepted. Failed reads keep the previous page and show a warning.

## Native remote creation recovery

For the native coordinator `start` command, Mesh now keeps the original creation configuration
privately before allocating or sending work. Keep the same configuration and request identity if
an outcome is uncertain; inspect it with `--coordinator created` before deciding what to do next.
Changing that request's saved version, worker, goal, limits, provider or deadline is refused.
A retained request is not evidence that input arrived or that an agent ran. There is no automatic
retry or cleanup. The desktop flow for starting new remote work is still being completed.

## Prepare fresh remote work in the desktop

Under **Agent fleets → Inspect a remote worker → Set up a worker connection**, choose the existing
native identity and SSH files and enter the worker's public connection details. **Prepare new
remote work** does not require a fleet or an existing attempt. Choose an attached project's exact
saved version, goal, provider and limits, then **Save remote creation request**. This retains the
inputs privately; it does not contact the worker or allocate a fleet. The fixed initial lease lasts
fifteen minutes from preparation and is never extended by retrying.

Under **Saved remote creation requests**, inspect the retained inputs and choose **Send saved input
to worker** explicitly. Mesh creates the independent fleet through the app's existing catalogue and
sends the exact saved input. Sending authorizes the configured worker to queue execution of this
attempt. Confirmed input delivery does not establish that a provider is running.
If a reply is lost, choose **Load saved requests**, then **Inspect original creation attempt**.
Inspection does not contact the worker. When the original attempt exists, it opens that selection
in the worker panel for explicit status reads or **Resume saved input transfer**. A second dispatch
of an already-started attempt refuses. Missing assignments, expired leases and lost worker
reservations still require reconciliation; they do not authorize a replacement attempt.

After restart, saved requests load without dispatching. Changed native files or identities refuse;
records and work remain preserved. To prepare a different request, clear and reselect the setup
files. This flow requires an eligible signed application and a configured worker; the fixed local
test checkpoint does not contain it. Signed packaged and real second-host acceptance remain pending.

Downloaded remote reviews remain accessible while live fleet status is missing or unavailable.
**Show downloaded reviews** reads the exact native retained history directly. Until the fleet's
status card loads, its result list appears under **Saved remote reviews**; it moves into the card
when status arrives. Existing pinned reviews stay fixed. A failed history read shows an error and
keeps pinning unavailable until a verified read succeeds. Reviewing does not start or recover agents.

Remote workers retain the original input allocation identity before acknowledging a newly
materialized input. This helps later recovery distinguish retained work, but it does not yet
make a lost worker reservation resumable. Preserve uncertain work and its original assignment;
a saved record or missing acknowledgment is not permission to start another agent.

Native recovery can verify whether an acknowledged remote input still occupies its recorded
allocation with its original contents. This read-only check does not resume an agent or authorize
a replacement attempt. Graphical worker-restart reconciliation remains unfinished.


### Remote lanes in the fleet overview

A lane with a retained remote assignment is labeled **Remote worker · last coordinator record**.
Its saved coordinator state is separate from a fresh worker observation. The details retain the
original assignment, worker identity and exact recorded lease values. A deadline does not prove
that execution stopped. Local activity is never presented as evidence for that remote lane.

While the fleet overview is visible, remote lanes refresh their authenticated recorded execution
independently, using one matching saved connection for each exact objective, lane, run and worker.
Save that connection through the remote worker panel first. Missing or ambiguous saved connections,
ineligible signing custody and unavailable workers are reported per lane; they do not stop local
review. At most four reads run concurrently. Reads are scheduled no more often than every five
seconds per lane, with transport deadlines; this is not a measured freshness guarantee.

Each card keeps its last verified observation time and revision beside any refresh error. A new
attempt clears old facts, and late or contradictory replies cannot replace the new lane's state.
Leaving the view stops scheduling new reads; already-started native reads may finish. Existing
saved reviews stay independently pinned. These are signed historical records, not proof of current
process activity, stopped ownership or permission to launch another attempt. The configured remote
connection's **Read recorded execution** action remains available independently.


### Installed Codex discovery

Mesh finds the Codex CLI in supported ChatGPT or Codex app bundles under system Applications or
your Applications folder. Both the current nested CLI bundle and the older resource layout are
supported; the current layout is preferred within each app. Mesh does not accept executable paths
from a fleet card or search arbitrary shell commands. The native provider adapter still checks the
selected executable before starting an agent. An installed provider account remains required.
