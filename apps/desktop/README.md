# apps/desktop

**Maturity: development proof.** This is a real local Tauri journey, not a supported distribution.
The macOS bundle has an ad-hoc resource seal but is not signed with an Apple Developer ID or
notarized; Windows packaging is not ready, and remote team delivery is not exposed. See
[Project status](../../docs/project-status.md) and the
[user guide](../../docs/user-guide.md).

The Mesh desktop client. Management and read-only state use the local IPC surface in
`crates/mesh-daemon`. The Tauri host additionally owns one native-only managed-text boundary:
the webview can read and atomically replace a selected regular file through `LiveDaemon`, but it
cannot open the database, store, filesystem or socket itself. Plan §8.1 and §8.3.

## 0. Run the desktop app

Desktop development requires Node.js 22.18 or newer, matching this package's `engines.node`
boundary and its direct TypeScript test/runtime commands. The narrower command-line demo described
in `docs/demo.md` remains compatible with Node.js 20 or newer.

The shipping local-folder journey is now a Tauri 2 window. It starts the existing local-only
daemon inside the app lifetime, accepts a native folder choice where that control is reliable or
an absolute typed path on macOS, previews an exact folder summary,
creates durable private history in a verified managed copy, reopens that state after restart,
keeps that private history outside the ordinary `Mesh Version - Working Folder`, reveals that folder to
Finder, editors, terminals and agents, reconstructs any durable workspace point as an independent
native folder without rewinding the current workspace,
creates text files and folders, renames or moves entries without changing file identity, deletes
files or empty folders while retaining saved file versions,
edits a selected UTF-8 file in the managed OS folder, preserves exact recovery bytes, signs and
appends durable private versions, restores and undoes exact retained versions in that working
copy, explicitly exports one current saved file or one reviewed saved tree back into an ordinary
folder, and
rolls an unchanged managed copy back without touching the original.
Opening a file re-reads the operating-system bytes and compares their exact BLAKE3 digest with the
current durable manifest. A change made in another local editor is shown as **Working** and can be
saved privately without first routing or rewriting those bytes through the webview.
Inspection applies to every tracked regular file. UTF-8 files up to the editor bound can also be
edited in the window; binary and larger files expose only their exact byte count, digest, durable
version, and modification truth, while the same authenticated save control captures their current
operating-system bytes.
Create, move, rename, and delete additionally write one fsynced private mutation intent before the
native filesystem change. Restart uses immutable journal truth to roll an uncommitted change back
or finish a committed one. A malformed marker or changed file identity is preserved, shown as
`Needs attention`, and pauses every managed write control instead of guessing or deleting bytes.

The imported source remains an untouched backup until a person explicitly previews and confirms
updating one saved file or saved tree. Mesh remembers that destination folder separately for each
managed workspace,
prefills it after restart, and carries the hint into independently opened workspace versions; the
person may still choose another destination. The hint grants no write authority. Mesh never
enables background or two-way synchronization. Update original
reconstructs current saved bytes from immutable history, requires each managed working file and
folder to still match, and binds the chosen destination directory and each destination. A whole-tree
preview lists missing folders first. One create-only confirmation installs those folders in depth
order through exact parent identities, then Mesh automatically re-previews the files. A second
confirmation atomically installs only changed files. A later race stops either stage and reports
the confirmed prefix; it never claims tree-wide atomicity. Continue real work in
the opened `Mesh Version - Working Folder`. To compare or experiment with older state,
choose a durable point under **Workspace versions** and open it. The native host allocates an
owner-only app-managed checkout by default, retargets the same stable folder, and keeps a custom
private location as an optional advanced choice. The result has independent private history, so an
agent can safely work there without changing either the source backup or the current workspace.

The **Review** section is also the only desktop path to the shared version. In an Apple-signed build
with a stable, validated application identity, **Set up approvals** creates or reuses one app-scoped
P-256 key inside the macOS Secure Enclave; it approves nothing. The ad-hoc build shows **Approval
unavailable** and never offers this action.
After **Record reviewed version**, Mesh labels the current approval action **Approve to shared
version**. An earlier point instead offers **Review this saved point again**: Mesh verifies that
historical point and opens an independent working folder, where a new review must be recorded before
approval while newer private work remains private. Approval
recomputes the exact workspace, review bundle, actor head, validation digest, and policy epoch,
shows that native summary, and then
asks macOS for Touch ID or password presence. The resulting ES256 receipt is verified again by the
daemon before the shared version moves. Cancellation, stale workspace truth, key substitution,
tampering, replay, and a generic agent IPC call append no approval. This alpha has no software-key
fallback and no non-macOS approval implementation. The card leads with the readable file-change
summary; exact bundle, presentation, operation, actor, reviewer, and sequence identities remain
available under **Technical proof** without competing with the decision.

```bash
npm --prefix apps/desktop run tauri:dev
```

Opening Mesh again while this build is already running brings its existing window forward without
starting another workspace service. When a newer build finds an older Mesh process that cannot
receive that request, it shows a visible compatibility message and changes no workspace data; quit
the older copy completely before starting the newer build.

On macOS, choose **Choose a folder** for the native folder picker. If Mesh is already open on a
zero-history folder containing ordinary files, choose **Preview this folder** instead; Mesh uses
the exact verified folder without making you select it again. You can also enter an absolute
source folder path in the import card and choose **Preview folder**,
then choose **Create workspace and open folder**. The native host allocates the private copy below
Mesh's owner-only application storage; **Use a custom private location** remains available as an
advanced option. Existing managed workspaces can likewise be chosen natively or opened by absolute
path. Native chooser controls remain the primary journey while the visible typed paths are the
fallback when a system dialog is unavailable. Both routes go through the same daemon-authorized
preview, import, open, and verification boundaries; the webview receives no direct filesystem
access.

To produce and verify a local macOS application bundle, install the pinned Tauri CLI once and run:

```bash
cargo install tauri-cli --version 2.11.4 --locked
npm --prefix apps/desktop run tauri:bundle-local
npm --prefix apps/desktop run tauri:prove-local
open target/release/bundle/macos/Mesh.app
```

For a technical alpha tester on another Mac, create a transport archive and its exact checksum
manifest from the same clean commit:

```bash
npm --prefix apps/desktop run tauri:archive-alpha
```

That command prints one exact `-delivery.zip` path and the independently verifiable inner `.zip`,
`.json`, and `.sha256` paths after it has extracted and verified the application archive. Send the
single delivery ZIP. Expanding it creates one revision-named folder containing the three inner
verification files and a reviewed
`START-HERE.txt` with checksum, Gatekeeper, agent-isolation, saved-version, and signing-limit
instructions; `FIRST-SESSION.txt`, a short observable agent-review-export mission and feedback
checklist; and `VISUAL-GUIDE.html` with three reviewed React-interface screenshots. Verification
refuses an absent, linked, or changed guide or screenshot. To re-run verification later,
pass the exact printed `.json` path to `npm --prefix apps/desktop run tauri:verify-alpha --
--manifest /absolute/path/to/the-printed-manifest.json`. The verifier refuses a changed
archive, an unexpected archive path, a changed application resource seal, or an executable that
does not embed the manifest's exact source revision. This is suitable for a consented technical
alpha, not an unattended public install: the bundle is ad-hoc signed and unnotarized, so a
downloaded copy requires macOS 11 or newer and the tester must explicitly choose **Open**. Do not ask testers to
disable Gatekeeper globally or strip quarantine metadata. A normal click-through installation
still requires the separate Developer ID signing and notarization lane. The checksum detects a
changed transfer only when the tester receives the manifest through a channel they already trust;
it is not a substitute for Apple signing or an authenticated release service.

The same signing limit excludes the protected approval loop from this ad-hoc archive. Apple's
data-protection keychain derives an app's private access group from its validated application
identity; an ad-hoc seal supplies no such identity or cross-build continuity. The archive manifest
therefore records `human_approval_available: false`, and the desktop shows **Approval unavailable**
instead of a setup button. Testers can exercise import, native editing, isolated agents, private
saves, reviews, and saved-version switching, while the original folder stays untouched. Approval
and **Update original folder** require a separately Developer-ID-signed build with a validated,
stable application identity. Do not treat the checksum or ad-hoc resource seal as that identity.

Before attempting that separate distribution build, verify the external Apple prerequisites without
putting certificate or notarization secrets in the repository or command line:

```bash
npm --prefix apps/desktop run tauri:signing-preflight -- \
  --profile /absolute/path/to/Mesh.provisionprofile \
  --notary-profile 'Mesh Notary'
```

The preflight fails closed unless the selected full Xcode installation exposes `notarytool` and
`stapler`, exactly one Developer ID Application certificate is installed (or selected by its SHA-1
fingerprint), the non-symlink provisioning profile authorizes `dev.mesh.desktop` and its default
data-protection keychain group for the same team, and the named Keychain profile authenticates with
Apple's notary service. It prints no certificate private key, provisioning payload, notarization
credential, or Keychain item. Passing this preflight is necessary but is not itself a signed,
notarized, or approval-tested build.

After the preflight succeeds, create the protected-approval archive with the same external inputs:

```bash
npm --prefix apps/desktop run tauri:archive-signed-alpha -- \
  --profile /absolute/path/to/Mesh.provisionprofile \
  --notary-profile 'Mesh Notary'
```

The command verifies a clean revision, checks out a separate local clone at that exact commit,
installs its exact lockfile in private staging, pins and re-verifies the provisioning profile, then
replaces the development seal with the exact Developer ID Application identity, enables the hardened runtime
with a secure timestamp, submits the app to Apple's notary service, requires an accepted issue-free
notary log, staples and validates the ticket, and requires Gatekeeper assessment before any public
artifact path changes. It then verifies the extracted ZIP again, including the certificate
fingerprint, exact application and keychain entitlements, embedded profile, source revision,
staple, Gatekeeper result, desktop controls, and real application journey. Only the ZIP, canonical
manifest, and checksum are published, with the checksum last. The signing private key and notary
credential remain in Keychain and are never accepted as command-line values.

The result is named `Mesh-signed-alpha-<revision>-macos-<architecture>.*` and contains the separate
`SIGNED-ALPHA-START-HERE.txt` guidance as `START-HERE.txt`. Verify an already-produced result with:

```bash
npm --prefix apps/desktop run tauri:verify-signed-alpha -- \
  --manifest /absolute/path/to/Mesh-signed-alpha-<revision>-macos-<architecture>.json \
  --prove-rendered
```

This repository does not claim that such an artifact exists merely because the pipeline is
present. A release is signed/notarized only when the command completes against real external Apple
authority and the resulting manifest, checksum, stapled app, Gatekeeper assessment, and extracted
journey all verify.

The tester does not need the repository. Expand the one received `-delivery.zip`, open Terminal in
the resulting revision-named folder, and verify and extract the inner application archive:

```bash
shasum -a 256 -c ./Mesh-alpha-*.sha256
ditto -x -k ./Mesh-alpha-*.zip .
```

Require the checksum result to say `OK`, then read the extracted **START-HERE.txt**. Quit every
running copy of Mesh before extracting or opening a replacement build: macOS may otherwise route
the Open request to the already-running older process. Keep each build in its own new folder and
never extract over a running `Mesh.app`. In Finder, try to open the extracted **Mesh.app** once.
For this ad-hoc build on macOS 15 or later, a developer-verification block requires the current
System Settings route: open **Privacy & Security** within about one hour of that blocked attempt,
scroll to **Security**, choose **Open Anyway** for Mesh, authenticate, read the warning, and choose
**Open**. Continue only for the cannot-verify-developer warning. Stop if macOS says the app is
damaged, will damage your computer, or contains malware; never disable Gatekeeper or remove
quarantine metadata. Do not continue after a checksum failure or an unexpected extra file. The app
footer shows the first 12 characters of the exact source
revision; compare them with the same 12 characters in the archive and manifest filenames before
starting the test. Keep the original project folder as the untouched backup, import it into Mesh,
and do alpha work only in the native working folder Mesh opens. Use **Update original folder** only
after reviewing and approving that exact saved version, then review the explicit file-by-file
update preview before confirming any write.

The local bundle command refuses tracked source changes, embeds the exact Git commit in the native
binary, and displays its first 12 characters in the app footer. The verifier reports the full
revision and checks that it is present in the actual `.app` executable alongside the executable
bit, product identifier, display name, version, nonempty icon resource, and the production React
onboarding, workspace-overview, and review-workbench markers. This independently refuses an alpha
archive whose build hook omitted the new interface even if the established controller tests pass. Before archive
verification exercises the extracted app, it requires the complete desktop control suite from the
same clean Git revision. The windowed app proof
launches that exact bundled executable
twice with an isolated home directory, queries the embedded daemon over its local Unix socket, and
requires the same remembered workspace digest and record count after the second process starts. It
also uses CoreGraphics to require the exact process's layer-zero on-screen window at the configured
dimensions. When macOS grants window-title access, it also requires the exact
`Mesh — Local workspace` title. Pass `--screenshot /absolute/path.png` after `--` to retain a
picture of that window; macOS Screen Recording permission is required. The windowed proof drives
the authenticated daemon boundary rather than clicking browser controls; the separately reported
desktop control suite covers those controls and their fail-closed states. The build applies and
strictly verifies an ad-hoc resource seal so the executable, Info.plist, and icon form one
internally valid local application bundle after copying. It remains intentionally unsigned by an
Apple Developer ID and unnotarized: the seal proves local bundle integrity, not that a
distributable installer or Apple trust chain exists. Gatekeeper acceptance for a downloaded alpha
requires a separate Developer ID signing and notarization lane with external Apple credentials.

The webview contains no storage, filesystem or socket client. Native commands provide folder
selection and proxy management/state through the versioned Unix-socket surface. The managed-file
commands stay in the native host and call the same desktop-owned `LiveDaemon`; they include four
narrow signed file-lifecycle operations (create text, create folder, move/rename and delete) and
are not daemon IPC methods. Preserve edit atomically replaces the managed file, records its replacement boundary
and persists a verified recovery envelope under the selected 50 ms / 65,536 byte / 25 ms
checkpoint parameters. Save privately uses a software-custodied Ed25519 actor key, signs the exact
canonical ChangeSet statement, verifies and stores its public envelope with the manifest and file
chunks, and advances **Saved privately** only after the durable acknowledgement and settling
interval. The secret key is never exported or persisted; restarting the app starts a new local
actor that causally follows the retained workspace. Authoritative mounted editing remains visibly
unavailable. The envelope is intentionally a local MVP record: the current sync transport does not
advertise or replicate it, so this phase makes no shared-publication claim.
Earlier-version restore uses the same honest boundary: it re-verifies retained manifests and CAS
chunks, atomically hydrates the exact bytes into the OS folder, and preserves a recovery pointer
without rewriting immutable history. The status badge reads the restored checkpoint state, so an
open recovered edit remains **Working** after application restart instead of falling back to the
older durable-version label.
Create, move and delete are serialized with editing, preflighted against both the durable model and
the confined OS path, signed by the same desktop actor, and reported as **Saved privately** only
after their exact ChangeSet and acknowledgement are durable. Delete accepts files and empty folders;
immutable file content remains in CAS/history after its materialized entry is removed. Moving an
entry preserves its object identity. This local phase does not expose a general filesystem command
over daemon IPC and does not claim remote replication of these signed envelopes.

Refresh also performs a metadata-only, non-symlink walk of the presented working folder. Regular
files created by an editor or agent appear as **new native file** without being treated as history.
One scan presents the complete supported new directory tree for review. **Save all privately**
consumes that reviewed queue in parent-first order, rechecking every directory identity and each
file's exact identity, bytes and executable bit before it signs and appends the new
file/version/name operations. Work that arrives during the save remains visible for a later review
instead of being silently absorbed. Discovery itself grants no write authority. Directory deletion
and moves remain explicit rather than inferred.

The native-only update commands follow the same exact-workspace binding as edit and restore but
grant no general filesystem authority to the webview. Preview exposes bounded UTF-8 text when
possible and always exposes exact sizes, BLAKE3 digests and executable state for both sides.
Confirmation rechecks those fields plus the destination directory identity before using the
atomic replacement or create-new boundary. Before the prepared file or directory inode can acquire
its ordinary destination name, Mesh durably records immutable update provenance for that exact
inode in the descriptor-pinned private workspace. A provenance failure therefore leaves the
destination unchanged, while a process loss after installation remains restart-verifiable.
Whole-tree preview first composes a create-only folder
plan in depth order. Each folder is verified against durable source history and its exact native
identity; confirmation rechecks or discovers its exact destination parent immediately before
creation. Mesh then re-previews every current saved file, excludes identical destinations, and asks
again before applying the stable path-ordered file set. Missing ancestry that is not in the reviewed
saved folder plan, symlinks, nested managed targets, unsaved source bytes and identical single-file
no-ops all refuse. A failed later batch item leaves
the already-confirmed prefix installed and reports the uncertain item for inspection. Pulling work back changes
neither immutable Mesh history nor the managed working file. The alpha tree export is deliberately
two-stage rather than silently pruning. After current paths are installed, Mesh derives former
paths from the append-only operation fold and presents a separate removal preview. A file is
eligible only while its exact bytes, executable state, parent and inode still match the last saved
value at that path. A folder is eligible only by exact identity and removal is non-recursive, so it
must be empty at confirmation. Changed, linked, non-regular and unrelated entries are preserved.
Moves therefore install the new path before the old path can be reviewed for removal.

After a successful import or manual open, the native host writes one canonical owner-only recent
workspace record under its application data directory. Its v6 entries pair each managed workspace
with its optional ordinary export-folder hint, write-once project identity, optional exact
agent-handoff installation, and optional authoritative source-point ordinal. Earlier v1 through v5
records remain readable and migrate on the next successful write. The next application process asks the real
daemon to reopen and validate that durable workspace before the webview renders it. A malformed,
linked, shared-permission, stale, or damaged selection is never trusted as workspace truth. The
reader also requires the application-data directory itself to be a real owner-only directory and
binds its read to the exact regular-file identity it inspected. The UI shows any refusal and leaves
manual folder selection available. A successful empty-state response still marks the embedded
service ready; any later state-refresh failure retains the last display for context but disables
every managed write until a fresh state succeeds. Successful rollback removes only the exact
matching recent-workspace record.

The native status returns the legacy path list plus an additive per-workspace projection of those
same stored navigation hints. Each entry keeps a write-once project-folder label separate from its
mutable update destination, so exporting a saved version elsewhere cannot rename that workspace
family. The browser uses the projection only for navigation labels: recent copies are named by
project, saved point, and neutral working-copy number while the exact real path remains inspectable.
The number reflects an app-managed destination collision, not proof that an agent created or owns
the folder: ordinary version navigation also allocates a new copy when existing work must be
preserved. The active friendly identity also remains visible in the page heading and operating-system window
title when the recent-workspace chooser is collapsed. Neither
path grants authority and the workspace is reverified before any switch; older clients ignore the
additive projection and newer clients fall back to the legacy list when it is absent.
If two ordinary project roots share the same final folder name, the selector adds only the shortest
distinguishing parent suffix (for example `acme/app` and `lab/app`) instead of showing two ambiguous
`app` rows or exposing app-private storage as the primary label.

An explicit recent-workspace or typed-path switch activates the stable native path and asks the
operating system to open it after the new root, fold digest, and physical installation are verified.
The switch remains successful if the platform opener fails; Mesh shows the new current workspace
and an **Open working folder** retry instead of reverting to or displaying the prior folder. Editors
and agents that retain an old directory handle must still be pointed or reopened on the shown path.

The embedded alpha checks a visible verified native workspace every five seconds and also when the
window regains focus. The repeating timer is a read hint, not by itself a save boundary: it rotates through at
most 128 regular-file bodies per tick, prioritizes newly discovered files when the inventory changes,
and still surfaces the complete metadata-only directory and missing-file results. Hidden windows
suppress the repeating scan in review-first mode. After the person explicitly enables automatic
private save, a hidden window remains eligible for the same bounded scan while the Mesh process is
running; WebKit may defer its timer, and returning to Mesh still performs an immediate complete
check. An existing change queue, in-window drafts, managed mutations, launcher focus transitions,
and an in-flight scan all suppress it. Returning to Mesh, choosing **Find folder changes**, recording a
review, and approving a review retain complete exact scans; every eventual private save re-inspects
the selected bytes under the exact workspace identity. The explicit **Automatically save safe
native edits privately** option is stored by the owner-only native host rather than browser storage.
When enabled, a complete stable scan calls the same authenticated batch-save path and rescans after
the append. Missing tracked paths, unsupported entries, active agent custody, open drafts, and
unstable bytes remain visible and are never automatically admitted. Restart reloads the preference
and performs the normal startup scan; a full application quit performs no capture, and no claim is
made for time when the desktop process is absent.

The native host also maintains one app-owned `native-workspace/current` symbolic link below that
owner-only application-data directory. It is a stable navigation path for Finder and editors that
should follow the workspace selected in Mesh, not workspace authority: the daemon first verifies and
pins the real presented directory by root, digest, and installation, then the host atomically
retargets the link. The desktop can open or copy this stable path after re-verifying its exact
workspace binding; long-running agents should receive the independent real folder instead,
because an already-open directory handle does not follow a later symlink retarget.
The desktop's **Start Codex on this version** and **Open agent terminal** actions both hand off that exact real
folder after checking its root, fold digest, and physical installation immediately before and after
the structured operating-system launcher call. The terminal action gives any local CLI agent a
one-click starting directory without routing it through the moving stable link. One real folder is
for one long-running agent: **Start another agent copy** forks the current durable point into a fresh
app-owned native folder and opens that folder in Codex. It never reuses an older clean checkout,
because Mesh cannot prove that another process has released it. From an older workspace layout,
the same action creates the first isolated native folder directly; a separate upgrade step is not
required. A successful Codex, terminal, or **Copy agent path** handoff is remembered against that
exact physical installation across app restarts and workspace-version switches. Copying re-verifies
the native folder and records the assignment before exposing its path; afterward the control says
**Agent folder assigned** and cannot hand the same writable folder to another agent. A clipboard
failure after that boundary conservatively retains the assignment. Returning to the folder says
**Reopen assigned Codex folder** and refuses a second launch by default until the person explicitly confirms; a
replaced installation or fresh agent copy receives its own first handoff. After every Codex session,
terminal agent, and editor using that real folder has stopped, **Finish agent handoff** completely
inspects that exact physical installation, clears its warning, and privately saves the returned file
and folder changes only when none requires structural interpretation. A rename, deletion, symbolic
link, or unsupported entry remains visible for explicit review instead. Mesh does not guess process
liveness, and a stale window
cannot release a replacement folder at the same path. **Update original** says **Choose destination folder** until Mesh has a proven original-folder
association, and never guesses an unmanaged destination from nearby paths.
When the imported folder is a Git repository, each native Mesh version receives an independent
Git directory containing the imported history, branch and index baseline. `git status`, branches,
and commits therefore work normally inside the folder, but Git locks, refs, and configuration are
never shared with the original folder or with another agent. Mesh does not copy Git metadata back
through **Update original**; that remains an explicit preview and atomic file-tree export. Git submodules
are currently refused for independent Git setup and leave the native files usable with a visible
warning.

For alpha feedback, **Current → More workspace actions → Copy safe diagnostics** copies only the
daemon's closed `mesh-support-bundle/v1` recovery summary after the desktop reconstructs and
validates its exact allowlist. The copied JSON excludes file contents, file paths, configuration,
event history, and key material. A malformed, extended, or unverified summary disables or refuses
the action instead of copying partially trusted data.

On macOS, **Choose folder** opens the native picker for an update destination. You can also type
or paste an absolute destination and press Return to start the whole-workspace preview directly.
**Start Codex on this version** additionally passes a launch-scoped read-only MCP server to `codex app`, backed by
the same Mesh application executable. This is explicit because current Codex builds load MCP
servers from user configuration and `-c` launch overrides, not merely from a project's
`.codex/config.toml`. Mesh changes no global Codex settings. Its generated compatibility config is
stored below the private workspace store and the presented folder contains only a verified
`.codex` symlink to it. Native discovery skips that symlink, so machine-local executable/socket
paths can never be adopted or copied into the original project as content. Mesh creates the link only when
`.codex` is absent and preserves/refuses any existing user configuration. An unavailable Codex CLI
or optional context refusal never blocks the native Codex handoff: Mesh still opens the fixed
independent folder and states that the context tool was not supplied.
The server reconnects for each `mesh_workspace_state` call and requires the exact
root and physical installation captured at launch. Durable saves within that same folder remain
visible to the running agent, while switching Mesh to another independent version makes the old
session refuse context instead of following it. Calling the tool exposes only workspace metadata such as the local
root, opaque version and installation digests, and review status to the configured Codex provider;
it does not send file content and grants no Mesh mutation authority. This bridge is context-only:
agent file edits still enter Mesh through native inspection. The desktop's confirmed **Finish agent
handoff** may then use its separate authenticated private-save authority for an entirely unambiguous
queue; MCP itself receives no read-ledger, mutation, checkpoint, publication, or approval authority.
Selecting a recent agent workspace verifies it and immediately performs the bounded read-only
native-folder inspection. Mesh does not continuously watch inactive agent folders, and navigation
inspection saves work only when the person enabled the persisted automatic-save option. The
separately confirmed finish action also combines a complete scan with an automatic safe private
save; structural ambiguity remains for the person in both modes.
Each historical version remains a separate ordinary folder with independent private history.
By default those independent copies live under the native host's owner-only `workspace-versions`
directory, so choosing a saved point does not require inventing a storage path. An explicit custom
location still goes through the same exact daemon reconstruction and verification boundary.
Linked workspace targets, a linked or shared link directory, and a non-link replacement at the
stable path fail closed. Tools that retain an already-open directory handle must be reopened after
the person switches versions. Older flat or in-project private layouts remain readable, but Mesh
does not expose them through this link because an editor or agent could then see private records;
open their current saved point as a new native folder to upgrade the working experience.

The embedded endpoint is single-owner. A second live app or daemon process cannot unlink and steal
the first process's socket path: endpoint acquisition is serialized by an operating-system lock,
live listeners return `AddrInUse`, stale socket files remain recoverable after a crash, and
shutdown removes only the exact socket identity that process bound. This prevents the webview
proxy from reaching one daemon while native managed-file commands still hold another. The socket
parent must also be a real directory: a linked parent is refused without following it, changing its
target's permissions, or placing an endpoint there.

## 0b. Run the legacy terminal renderer

Two terminals, no install step, no network:

```bash
cargo run -p mesh-daemon --example serve      # terminal one: the background service
npm  --prefix apps/desktop start              # terminal two: the window
```

The window draws what the **running service** answered — whether it is serving, which version of
the service interface it speaks, what its last start-up found and how long that took — and
redraws it every two seconds and on every change of the link. Stop the service and the window says
so without losing the last reading; start it again and the window comes back on its own. Ctrl-C
closes either.

```bash
npm --prefix apps/desktop start -- --once                       # one frame, then exit
npm --prefix apps/desktop start -- --endpoint /tmp/mesh.sock    # a service somewhere else
npm --prefix apps/desktop start -- --help                       # every option
```

Both ends look for the endpoint the same way and in the same order: `--endpoint`, then
`MESH_DAEMON_ENDPOINT`, then `$HOME/.mesh/run/daemon.sock`. That is why neither command above
needs an argument. The service prints the endpoint it bound on its first line.

This compatibility renderer is still useful for a headless diagnosis. It is not a mock: every
number and every sentence in the frame arrives over the same Unix-domain socket and versioned
surface the Tauri host uses.

## 0c. Build and gate it

```bash
npm --prefix apps/desktop test
```

The established controller suite has no top-level desktop package dependencies, install step,
network access, or bundler. Node's own test runner and Node's own TypeScript type stripping keep
that fast safety boundary available on a clean checkout. The production alpha also embeds the
locked React/Vite interface described below, so this command alone is no longer the complete
application build.

That command is the controller and native-boundary gate, and it runs the product vocabulary gate
first. Check the React interface separately, or build Tauri to run the same production UI build
hook used by the alpha archive:

```bash
npm --prefix apps/desktop test        # vocab-lint --user-facing, then the full desktop suite
npm --prefix apps/desktop run lint:vocab   # the gate on its own
npm --prefix apps/desktop/ui-next ci --ignore-scripts
npm --prefix apps/desktop run ui:next:check
(cd apps/desktop && cargo tauri build --debug --no-bundle)
```

A forbidden term in any shipped string fails the build before a single test runs, measured rather
than asserted: planting *branch* in `src/strings/connection.ts` exits 1 with
`[forbidden-words] "branch" is not a product word` and runs no test.

## 1. What is here — and what is deliberately not

**Here, and running:**

| Path | What it is |
|---|---|
| `src/ipc/protocol.ts` | The six message shapes, their fixed key order, the framing, the input validation |
| `src/ipc/methods.ts` | The versioned method catalogue this client knows |
| `src/ipc/transport.ts` | The **one** file in this application that opens a socket |
| `src/ipc/client.ts` | Negotiation, correlation, queueing, reconnection, the context that survives a restart |
| `src/main.ts` | The entry point `npm start` runs: arguments, a connection, a window |
| `src/app/live.ts` | The window bound to a live connection — the only caller of the layer below it |
| `src/app/window.ts` | One pure function from what is known to what is on the screen |
| `src/app/facts.ts` | What the service answered, checked at the boundary rather than assumed |
| `src/app/endpoint.ts` | Where the service is, decided the same way the service decides it |
| `src/app/operations.ts` | Every interface operation, and the versioned IPC call each one is |
| `src/app/status.ts` | The six-state user model: the only route from an internal state to a word |
| `src/strings/status.json` | The six words, and the approved wording for every internal state |
| `src/strings/errors.ts` | Every sentence shown when something has gone wrong |
| `src/strings/window.ts` | Every label and sentence the window frame is built from |
| `src/strings/` | Every sentence a person reads, linted by `tools/program/vocab-lint` |
| `ui-next/src/atoms`, `molecules`, `organisms`, `layouts`, `pages`, `views` | The React source for the visible alpha shell and its real workflow pages |
| `ui-next/src/models` | Closed, bounded projection and intent adapters between React presentation and the hidden coordinator |
| `ui-next/vite.island.config.ts` | The production Shadow DOM island build invoked by Tauri before every desktop build |
| `test-support/fake-daemon.ts` | A stand-in service over a real Unix-domain socket |
| `crates/mesh-daemon/examples/serve.rs` | The service the window connects to, in one command |

**Still not here, and not stubbed:**

- **A native-authority React replacement.** React owns the entire visible shell, pages, and views.
  The established controller remains hidden as an authority adapter for native commands until each
  command moves behind an equivalent typed boundary; it can no longer appear as a visual fallback.
- **A Windows named-pipe transport.** `crates/mesh-daemon/src/ipc/server.rs` is `unix`-only, and
  so is the example that runs it.
- **A non-macOS graphical approval ceremony.** The alpha desktop renders the exact saved-version
  review, records that review, and on supported Macs advances the shared version only through a
  Secure Enclave credential plus explicit user presence. `meshd` and `meshctl` retain the local
  software-key protocol proof, but the graphical approval ceremony has no Windows or Linux
  equivalent yet.

## 2. The contract, and why it cannot drift

`crates/mesh-daemon/ipc-contract.json` is one file, published by the daemon because the daemon
owns the surface. Both implementations check themselves against it:

| Side | Check |
|---|---|
| Rust | `cargo nextest run -p mesh-daemon --test ipc` |
| TypeScript | `src/ipc/contract.test.ts`, inside `npm --prefix apps/desktop test` |

Both decode every published vector, re-encode it, and compare **byte for byte**. That is what
makes "one message has one encoding" a measurement rather than a claim, and it is why two
implementations of one format is a tolerable risk here.

What the corpus does **not** establish: that the Rust service and this client behave identically at
run time. The two encodings are identical by construction; behaviour is a different claim.

The end-to-end path is now **observed rather than derived**, and the distinction matters:

| | What was done | What it establishes |
|---|---|---|
| Automated | `npm --prefix apps/desktop test` against `test-support/fake-daemon.ts`, over a real socket | The window's framing, negotiation, correlation, reconnection and rendering, on every run |
| By hand | `cargo run -p mesh-daemon --example serve` in one terminal, `npm --prefix apps/desktop start` in another | That the **Rust** service and this window really do talk: the frame in the pull request carries the Rust `startup.report` sentence, verbatim |

The by-hand leg is not in `npm test` and is not claimed to be. A suite that shelled out to cargo
would exceed the gate budget and would not run on a machine with no Rust toolchain, so the
automated leg keeps the stand-in and the live leg is repeated by anyone who runs the two commands
in §0. What would close the gap properly is a separate end-to-end task that owns a runner outside
this package.

## 3. The four acceptance criteria, and where each is checked

| Criterion | Where |
|---|---|
| No direct database or store dependency | `src/app/architecture.test.ts` — import graph and source scan over the browser client, plus the native manifest assertion; `tools/program/arch-check` binds the Rust closure. Every reading the window shows comes back from a versioned call |
| Every interface operation is a versioned IPC call | `src/app/operations.test.ts` — checked in both directions, so neither an operation without a method nor a method without an operation can exist |
| A daemon restart reconnects without losing user context | `src/ipc/client.test.ts` — a real socket, a real service stop and start, and assertions on object identity, the session name, the queue and the subscriber list. `src/app/live.test.ts` asserts the same outage at the window: the last reading stays on the screen, and the wording changes rather than the data |
| No network listener | `src/ipc/no-network.test.ts` — a text scan, and a reading of `process.getActiveResourcesInfo()` with a live connection open, asserting this process holds no TCP or UDP handle |

## 4. Where user context actually lives

The client owns it. The daemon owns nothing about it, and says so: `welcome.resumed` is `false`
after every daemon restart, because the daemon's session registry is in memory on purpose. What
survives an outage survives because `DaemonConnection` never discards it — the context object, the
session name, every queued and in-flight call, and every state subscriber.

A call issued while the service is down is **held, not rejected**. An in-flight call is re-issued
only when its method catalogue entry marks that safe. Reads and exact review-bundle reuse recover
automatically. `review.approve` does not: if the link disappears after the request was sent, the
client reports an unknown outcome and requires a shared-state refresh instead of risking a second
publication.

## 5. The six words

User-facing status is exactly six words, in one order — **Working · Saved privately · Available to
team · Ready for review · Needs attention · Approved** — and nine terms from the internal model may
never reach a person. The words and the whole mapping are `src/strings/status.json`; `src/app/
status.ts` is the only route from an internal state to one of them.

Four checks hold it, and each one is a different instrument on the same rule:

| What could go wrong | What fails, and when |
|---|---|
| A seventh word in the catalogue | `vocab-lint` `six-state` over `$.states.*.status` — the build, before any test |
| A forbidden term in any shipped string, label or error message alike | `vocab-lint` `forbidden-words` over every shipped `.ts` and the catalogue — the build |
| An internal state with no wording | `src/app/status.test.ts` against PRD §4.1, both directions — the build |
| The six words drifting out of the order the product states them in | `src/app/status.test.ts` against PRD §4.3 — the build |

Measured, not asserted. Adding a row to PRD §4.1 with no wording here exits 1 with *"these
internal states have a row in the product requirements and no wording in
`src/strings/status.json`"*; a seventh value in the catalogue exits 1 with *"`Blocked` … is not one
of the six user-facing status words"*; deleting the catalogue exits 2, because its surface is
`required`.

### There is no fallback, on purpose

`wordingFor` answers with an approved wording or throws `UnmappedInternalState`. The obvious
alternative — return the internal name, or default to one of the six — is silent: it ships an
unreviewed word to a person and nothing fails, so nobody finds out until a user reads it. The
thrown error carries the unmapped name on a **field**, never in its message, because the name of a
state this application has never heard of is exactly the string the lint cannot check. Rendering
`error.message` is therefore safe by construction, which is the property a fallback cannot have.

The decision is the task's own acceptance criterion — *"an unmapped internal state is a build
failure, not a fallback string"* (`01KZC2SCYPFMTDBTR35V9NXYXN`) — so it is cited here rather than
re-argued in an ADR, and `docs/adr/` is outside that task's allowed paths in any case. What would
change it: a window that must stay up while one region of it has no words. Throwing inside a
render is a blank screen for a fault in one label, and the answer then is a boundary that catches
`UnmappedInternalState` and renders a reviewed *"Needs attention"* card naming no state — a
decision with a real trade-off, and an ADR when somebody takes it. It is not what ships here. The
window in §0 renders no status word at all, for the reason two bullets below: no method on surface
version 1 reports the state of anybody's work, so the throw is still unreachable in practice.

### What this does not cover — stated, not softened

- **The six words are still not on a screen.** There is a window now (§0) and it renders the LINK
  and the SERVICE, neither of which is a state a piece of work can be in. The six words stay in
  the catalogue, the mapping and the tests until a method reports the state of somebody's work —
  putting one of them in the frame today would mean inventing the state behind it.
- **Only four internal states have a status.** *Their work*, *Shared version*, *Approve to shared
  version* and *Earlier version* name a thing rather than a state, so `statusFor` answers
  `undefined` for them rather than borrowing one of the six. That is the model, not a gap.
- **No status arrives over IPC yet.** Surface version 1 answers three read methods and none of
  them reports the state of anybody's work, so nothing crosses the wire into this mapping today.
  The mapping is bound to `docs/product-prd.md`, which is what keeps it honest before there is a
  producer; the day a method reports a state, its values must be names from `$.internalStates`,
  and an unmapped one throws rather than rendering.
- **Notifications and onboarding copy do not exist**, so the lint covers no such surface. The
  manifest entry that would cover them belongs with the task that writes them.
- **`src/ipc/` diagnostics are now linted, and that is a widening rather than a claim of
  cleanliness.** Sentences like *"the line is over the 65536 byte limit"* were never reviewed as
  copy; they pass the vocabulary rule today, and they are in the surface so the next one has to.
