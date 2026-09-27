# Attached-project capture from an existing harness

Development entry points in the desktop executable use the same native history and key-custody
implementation as the application. They do not open its window or start an agent. Graphical
attachment and history inspection are implemented in source; packaged-window acceptance and
attachment approval remain outstanding. Packaged executable capture proof is available below.

Build from the checkout:

```sh
cargo build -p mesh-desktop -p mesh-daemon --bin mesh-desktop --bin meshctl
```

Register a project where it already lives. Both directories must exist, use absolute paths, and
the dedicated private metadata directory must be outside the project. Initial history creation
requires that directory to contain only Mesh's registration receipt.

```sh
target/debug/meshctl attach /absolute/project /absolute/private-metadata
```

Save a complete bounded capture once, or list the resulting immutable version identities:

```sh
target/debug/mesh-desktop --mesh-attachment capture /absolute/private-metadata
target/debug/mesh-desktop --mesh-attachment versions /absolute/private-metadata
```

Start background capture. macOS uses native change signals, with reconciliation every five seconds
to catch missed changes; unavailable native signals fall back to periodic reconciliation:

```sh
target/debug/mesh-desktop --mesh-attachment watch /absolute/private-metadata
```

The watch command emits JSON lines with capture phase, health, last saved identity, and observation
age. Send one of these newline-terminated commands on its standard input:

- `capture`: request a rescan; multiple in-flight requests coalesce.
- `status`: emit current status.
- `stop`: request stop, wait for the capture worker, emit stopped status, and exit.

Closing stdin also requests a joined stop. A capture already entering a durable write may finish;
a stop request is distinct from confirmed termination. An external signer or slow filesystem can
delay termination. A successful watch exit means it stopped cleanly, not necessarily that a version
was saved. Read `saved_version` and `last_outcome` to determine the capture result.

A fresh native software-held actor key signs each capture invocation or watch session. It is never
written to a key file, the project, or command output. This identity attests to captured content;
original file authorship stays unknown. Captures cannot approve or advance main.

The current defaults allow 10,000 directory names, 8 MiB per file and 64 MiB of captured bytes. A
partial or changing scan, changed exclusion policy, or replaced project/store is reported without
substituting a new project or claiming a saved version. Initial empty projects cannot yet mint a
history point. Existing-project sessions, files, and Git state remain under the user's control.

Run the isolated executable proof after building:

```sh
node apps/desktop/scripts/prove-attached-capture.mjs
```

It uses temporary fixture folders, launches only the checkout's built binaries, checks direct and
periodic saves, catches up after restart, stops through both the command and EOF paths, and verifies
Git and directory identity preservation. It cleans up its fixture and does not replace an installed
application. The default development run explicitly reports `packaged: false`.

To test the executable inside an existing locally sealed bundle, supply both its path and exact
embedded commit:

```sh
node apps/desktop/scripts/prove-attached-capture.mjs \
  --app /absolute/Mesh.app \
  --revision <40-character-lowercase-commit>
```

Packaged mode verifies the bundle seal, embedded revision and current component interface markers
before and after execution. After verifying the seal, it asks the executable for its
`--mesh-build-identity` JSON and compares the reported revision exactly. Incidental revision-like
strings in executable constants are insufficient. Older bundles without the identity-mode marker
are refused before execution, avoiding their ordinary window startup. The identity command accepts no additional
arguments and exits without opening a window, reading a workspace or generating keys. It records the executable SHA-256 and refuses a byte change during the
run. The temporary fixture is registered with this checkout's development `meshctl`; capture,
watch, stop and version listing execute the packaged desktop binary. Output explicitly distinguishes
`packaged: true` from `graphical: false`. This is not a rendered-window, installed-app, Apple trust,
approval or main-integration proof.


Native event batches only request bounded rescans. They do not establish authorship, durability or
a complete snapshot, and do not bypass capture identity checks. Linux currently uses reconciliation
only. Event-triggered scans are coalesced and separated by at least 250 ms after the previous attempt;
this is not yet an incremental content-hashing or large-project performance claim.
