# Run the local Mesh demo

This is the shortest truthful first-run proof of the product surfaces already in this repository.
It runs the real `meshd` service and `meshctl` client as separate processes, saves an exact file
version through the shipped checkpoint helper, restarts over durable state, performs an exact
local review, proves a software-held key cannot publish it, restarts again, and previews a redacted
support bundle.

## Run it

You need macOS or Linux, Node.js 20 or newer, and the Rust toolchain pinned by
`rust-toolchain.toml`. From a fresh checkout:

```bash
node examples/local-daemon-demo.mjs
```

The first run builds the required Rust binaries and can therefore download missing Cargo packages.
The script finds rustup's usual `$HOME/.cargo/bin/cargo` location; set `CARGO` to override it.

A passing default run prints `PASS — 44 checks, all green`. The executable checks this count
against both quickstart documents before it starts, so the public promise cannot silently drift as
the proof grows. It proves:

1. local restriction and `.meshignore` explanations;
2. daemon start with explicit reviewer trust and explicit checkpoint thresholds;
3. health, method discovery, and startup reporting from separate client processes;
4. workspace open, `records.mesh`, `metadata.sqlite`, and ordered live service events;
5. an explicit file capture, process restart, recovered private version, exact durable review, and
   a fail-closed software-key publication attempt;
6. a second restart with the review intact and the protected shared version honestly unavailable;
7. a local support bundle with a stable schema, correlation digest, and no raw paths, file bytes,
   or key material; and
8. clean daemon shutdown and socket removal.

The thresholds are deliberately supplied on every daemon start. The save itself uses the shipped
`capture-checkpoint` example, so this proof does not relabel explicit recovery capture as measured
idle settlement.

## Bounded and offline modes

After the binaries have been built, the fastest local smoke is:

```bash
node examples/local-daemon-demo.mjs --skip-build
```

Once Cargo has cached the packages, this rebuilds and runs without network access:

```bash
node examples/local-daemon-demo.mjs --offline
```

`--offline` fails if the local Cargo cache is incomplete. `--skip-build` fails with the exact
missing binary and corrective build command instead of continuing with a partial proof. They are
alternatives and the script rejects using both together.

The optional mounted proof requires Docker, permission to run a privileged Linux container, and
`/dev/fuse`:

```bash
node examples/local-daemon-demo.mjs --mounted
```

The wrapper exports a complete local Cargo workspace, including all path dependencies, and runs
the build and proof with Docker networking disabled. It captures an ordinary write through the
kernel FUSE view, materializes it, opens the exact review, verifies an immutable shadow and a
warm mount, then restarts over the configured state. Preparing the Docker image for the first time
may require network access; the proof inside the container does not.

Other useful options:

```bash
node examples/local-daemon-demo.mjs --keep      # retain a passing workspace
node examples/local-daemon-demo.mjs --verbose   # print raw JSON Lines
node examples/local-daemon-demo.mjs --help
```

On failure, the script names the stage, exits nonzero, reports how many checks passed, and retains
the temporary workspace. Output is plain text when piped or when `NO_COLOR` is set.

## Inspect the surfaces manually

Build once and start a daemon in the first terminal:

```bash
cargo build -p mesh-daemon --bins --example capture-checkpoint
mkdir -p /tmp/mesh-demo-workspace
target/debug/meshd \
  --workspace /tmp/mesh-demo-workspace \
  --endpoint /tmp/mesh-demo.sock
```

Then query it from another terminal:

```bash
target/debug/meshctl --endpoint /tmp/mesh-demo.sock status
target/debug/meshctl --endpoint /tmp/mesh-demo.sock describe
target/debug/meshctl --endpoint /tmp/mesh-demo.sock startup
target/debug/meshctl --endpoint /tmp/mesh-demo.sock state
target/debug/meshctl --endpoint /tmp/mesh-demo.sock support-bundle /tmp/mesh-demo-workspace
```

Local explanations do not require the daemon:

```bash
target/debug/meshctl restrictions
target/debug/meshctl exclusions /tmp/mesh-demo-workspace target/debug/app
```

After the daemon is stopped, the same support preview can still inspect the workspace without an
endpoint. That fallback never opens or repairs SQLite and therefore fails closed when live WAL
sidecars are present:

```bash
target/debug/meshctl support-bundle /tmp/mesh-demo-workspace
```

Type `stop` in the daemon terminal, or send end-of-input with Ctrl-D.

## Current boundary

The default quickstart proves an explicit save and durable local review. It deliberately refuses
publication from the software seed because this build has no verified human-held signing backend.
It does not prove that
ordinary editor writes are automatically captured after idle settlement, and the support bundle is
only previewed locally—it is never transmitted. The optional FUSE path is an advanced local proof,
not the default editing experience. Any further boundary reported by `meshctl state` is printed from
the running service rather than inferred by the script.

## Troubleshooting

- **`cargo` could not start:** install rustup, or set `CARGO` to the full cargo path.
- **A package is unavailable with `--offline`:** run the default command once while Cargo can fetch
  dependencies, then retry the offline proof.
- **A binary is missing with `--skip-build`:** run the exact build command printed by the script.
- **`could not listen ... Operation not permitted`:** a sandbox denied Unix-socket creation. Run
  the command in a normal terminal on macOS or Linux.
- **Inspect a failed run:** use the temporary path printed at the end.
- **Docker or `/dev/fuse` fails:** omit `--mounted` for the default proof, or grant the container
  runtime the privileged/FUSE access requested by the wrapper.

The process-level daemon regression suite is:

```bash
cargo test -p mesh-daemon --test binary
```

## Try the alpha desktop with an editor or agent

Run the graphical app from the repository root:

```bash
PATH="$HOME/.cargo/bin:$PATH" npm --prefix apps/desktop run tauri:dev
```

Then follow this exact journey:

1. Under **Import**, choose or type an existing project folder and select **Preview folder**. If
   Mesh already opened that exact folder and reports that its ordinary files are outside private
   history, select **Preview this folder** from **Next action** instead; no second folder chooser is
   required. The
   preview counts the content Mesh will bring into the native working folder: repository
   `.gitignore` rules apply first, `.meshignore` may refine them, and Git metadata is structurally
   excluded. Ignored build products and dependencies remain in the original folder and can be
   regenerated normally in the managed copy.
2. Select **Create workspace and open folder**. Mesh chooses an owner-only private location in its
   application storage, creates the managed workspace, opens its ordinary native working folder,
   and leaves the original project untouched. Use **Use a custom private location** only when you
   specifically need to control where Mesh stores the private copy. After the first import, the
   primary **Next** action becomes **Start Codex on this version**; the working-folder controls remain available
   for Finder, Terminal, and editors. After that exact workspace opens successfully in Codex,
   **Next** returns to **Open working folder** instead of launching the same agent again. A replaced
   workspace installation or a newly created agent copy gets its own first handoff.
3. Select **Open working folder** or **Copy working path**. Mesh exposes one stable app-owned
   `native-workspace/current` link that currently names the selected native working folder. Use that
   stable path with Finder, Terminal, or an editor that should follow the workspace you open in Mesh.
   For a long-running agent that must not be redirected when Mesh switches workspaces, use
   **Copy agent path** or **Start Codex on this version** so the process receives the independent real folder.
   It starts at the selected durable version and is writable, but its path never retargets when
   Mesh switches elsewhere. Private Mesh databases, records, and content storage remain outside the presented
   project folder and do not appear as project files.
   Give one independent folder to one long-running agent. To start a second agent from the current
   saved point, select **Start another agent copy**. Mesh refuses while native work is unsaved, creates a
   fresh app-owned copy instead of reusing any earlier checkout, verifies it, switches the stable
   human path to it, and opens the new real folder in Codex. The first agent remains on its prior
   folder. From an older workspace layout, the same action creates the first isolated native folder
   directly. Do not paste one copied agent path into two simultaneous agents.
   If Mesh identifies an older workspace layout, **Create native working folder** opens its current
   saved point as a new native folder; the stable link never exposes legacy private files. On
   macOS, **Create folder + start Codex** performs that upgrade and agent handoff in one step.
   Afterward, **Start Codex on this version** starts Codex directly on the exact real folder behind the current
   version. Mesh deliberately does not give Codex the retargetable link: that agent session stays
   on the independent version it opened even if you later switch the desktop to another version.
   Mesh also launches Codex with a session-scoped, read-only `mesh_workspace_state` context tool
   bound to that exact workspace installation. The same generated settings are retained in Mesh's
   private store for compatibility and appear at `.codex` only through a symlink, so local socket
   and app paths are never offered as workspace changes. A pre-existing `.codex` configuration is
   never overwritten, and launch-scoped settings do not change global Codex configuration. Durable
   saves in that folder remain visible to the same agent session; switching
   Mesh to another independent version does not redirect it.
4. After an editor changes a file or creates a file or folder, keep Mesh visible or return
   to it. In review-first mode the window performs a bounded read-only check every five seconds
   while visible and checks again on focus. With automatic private save explicitly enabled, the
   same bounded check remains eligible while the window is hidden and the Mesh process is running;
   macOS may defer the interval, so returning still performs an immediate complete check. A full
   application quit performs no capture. The resulting scan shows the complete supported new
   directory tree without another command.
   **Find folder changes** remains the explicit retry when a scan is refused. Review the full
   queue, then select **Save all privately** once. Mesh admits directories parent-first and
   rechecks every directory identity plus every file's exact bytes and executable bit before it
   signs and retains anything. Work that arrives during the save remains queued for the next
   review; it is never silently folded into the current save. To remove this routine click, enable
   **Automatically save safe native edits privately**. The owner-only native host remembers the
   choice. A complete stable scan then uses that exact same signed save path, including after an
   app restart. Turning it off returns immediately to review-first mode. Deletions, renames,
   symbolic links, special entries, active agent handoffs, and changing files always pause and stay
   visible instead of being guessed.
   For a pinned agent folder, first stop every Codex session, terminal, and editor that uses it,
   then choose **Finish agent handoff**. That one confirmed action performs the complete inspection
   and privately saves an entirely unambiguous file-and-folder queue through the same rechecking
   path. If the queue contains a rename, deletion, symbolic link, or unsupported entry, Mesh saves
   none of it automatically and leaves the structural result for explicit review.
5. To save just one file, choose it under **Edit**, select **Inspect file**, and then select **Save
   privately**. Mesh re-reads its exact operating-system bytes at both steps. This explicit review
   boundary applies to both the one-file and **Save all privately** paths. Read-only discovery
   remains separate from mutation; the persisted automatic-save option and confirmed **Finish
   agent handoff** may invoke the authenticated save only for a completely unambiguous queue.
   Links or special files remain unsupported.
6. Under **Review**, inspect the automatic card and select **Record reviewed version**. A build
   signed with a stable Apple Developer identity and validated application identifier then offers
   **Set up approvals**. That action creates a device-only Secure Enclave credential and approves
   nothing. Select **Approve to shared version**, verify the exact workspace, review bundle, saved
   version, and policy epoch in the native dialog, and confirm. macOS asks for Touch ID or your
   password before signing. Cancel either dialog to prove that the shared version remains
   unchanged. A successful ceremony changes **Shared version** to the exact reviewed actor head
   and remains visible after restarting Mesh. The repository's ad-hoc local/archive build instead
   shows **Approval unavailable** because it has no validated application identity; it can test the
   recorded review and version workflow, but not protected publication or original-folder update.
   This path has no software-key fallback and is unavailable off supported Macs.
7. Under **Workspace versions**, choose a durable point and select **Open in working folder**, or
   choose **Start Codex on this point** for a direct agent handoff. The picker names the
   immutable history entry **Saved point N**. Mesh verifies and reconstructs it into an owner-only,
   writable checkout named **Copy of saved point N**, retargets the same stable
   `native-workspace/current` folder, and leaves both the immutable point and your previous working
   copy unchanged. You can still enter a custom private location before opening when you need to
   control where that independent copy lives. The Codex action receives the selected copy's
   independent real folder, so that agent remains pinned if Mesh later switches the stable
   working-folder link elsewhere.
8. Give the **Pinned agent folder** shown by Mesh to an agent when you want it to explore,
   modify, or add ordinary files and folders to that state independently. It is a writable workspace
   initialized from the selected durable version, not an immutable snapshot. Do not give a long-running
   agent the stable link: switching Mesh retargets the link, while an agent that already opened a
   directory may keep its prior directory handle. When every process using it has stopped, choose
   **Finish agent handoff**. Mesh scans the complete new folder tree and automatically saves
   unambiguous file and folder changes parent-first while rechecking every exact file. Structural
   changes remain visible and unsaved for your decision.
   Use **Switch to a recent native workspace** to move back to the workspace you came from. Mesh
   retains the eight most recently validated native folders across app restarts and revalidates a
   selected folder before enabling its controls. Entries use the original project name, working-copy
   provenance such as **Copy of saved point N**, and working-copy number instead of presenting private
   storage hashes as their primary label. Numbered app-managed instances use a neutral
   **Working copy N** suffix; this is a storage collision number, not evidence that an agent owns
   the folder. Legacy records that predate the saved-point ordinal use
   **Copy of saved version abcdef12**. Changing the update destination does not rename that project
   family, and the exact real path remains available as detail. Projects with the same leaf name gain
   the shortest distinguishing ordinary-folder suffix, such as `acme/app` versus `lab/app`. The
   active project and working-copy provenance remain visible in the Mesh
   heading and window title after the chooser is collapsed. Selecting an agent folder performs an
   immediate bounded read-only inspection, so
   work completed while another folder was selected appears without waiting for the focus or
   polling interval. Inactive folders are not continuously watched; selecting one only inspects it.
   The separately confirmed finish action is the only agent path that combines inspection and save.
   Only after verification does Mesh atomically
   retarget its private stable link and ask the operating system to open it. Reopen an editor or
   agent that retained an old directory handle. If the opener fails, the verified switch remains
   current and **Open working folder** retries only the presentation step. **Forget from list**
   removes an unavailable navigation entry without deleting or
   changing that folder.
   In Codex, ask it to call `mesh_workspace_state` before working when you want the agent to state
   which Mesh root, saved digest, and installation it is using. It edits ordinary files natively;
   after the agent stops, **Finish agent handoff** inspects and privately saves only an unambiguous
   result, leaving structural ambiguity for you.
9. In a Developer-ID-signed build with working approval custody, carry accepted results back to the
   original project by switching to the workspace containing those saved results, recording its
   current review, and approving that exact saved version. Only then
   select **Update original folder** when Mesh retained the import association.
   A manually opened older workspace instead says **Choose destination folder** and requires you to
   type or paste the destination once and press Return; Mesh never guesses it from a
   sibling path. Under **Update original**, Mesh prefills the source folder you originally imported when
   that association is known. Choose one current saved file and **Preview one file**,
   or select **Preview saved workspace**. When saved folders are missing from the destination,
   review the create-only folder list and select **Update changed files**. Mesh creates each exact
   folder in depth order, then automatically re-previews every current saved file and keeps only the
   changed set. Mesh prefills the original imported folder after
   restart and when opening an independent workspace version; you may type another ordinary folder
   instead. Review the exact
   saved bytes and destination state, then select the single-file confirmation or **Update
   changed files**. Mesh rechecks both sides and atomically installs each file. A batch stops on the
   first race and reports the confirmed prefix; the managed folder and immutable history do not
   change. Each exact future file or folder identity receives a durable private receipt before its
   temporary inode is published at the ordinary destination name, so a receipt failure changes no
   destination and a completed installation remains attributable after restart. Mesh then presents
   former saved paths as a separate removal review. It can remove only
   an old path proven by the original import or an exact durable update receipt: a file must
   still be byte-for-byte and metadata-identical to its last saved value, and a directory must be
   the exact old directory and empty at confirmation. Changed, replaced, and unrelated destination
   entries remain in place; recursive deletion is unavailable.

Mesh deliberately does not silently synchronize changes into the imported folder. The opened
`Mesh Version - Working Folder` is the native working project after import. **Update original** is an explicit
approved-version operation whose files and folders are previewed before writing. Current folders and files are confirmed first; former paths have a
second, independently confirmed cleanup plan. This remains a reviewed update flow, not an
ambient bidirectional synchronization engine.
