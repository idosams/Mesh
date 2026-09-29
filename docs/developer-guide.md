# Developer guide

The canonical development repository is [idosams/Mesh](https://github.com/idosams/Mesh). Verify actual remotes and base ancestry; the local folder name proves nothing. Follow the [agent workflow](../AGENTS.md) and [migration ledger](plan/fleet-migration.md) when transferring older work.

Mesh is a Rust workspace with a Tauri desktop application and a React interface. The daemon owns
durable workspace state; clients use the local IPC protocol rather than opening its database.

## Prerequisites

- Git
- The Rust toolchain pinned in `rust-toolchain.toml`
- Node.js 22.18 or newer (the locked Vite build and direct TypeScript tests require it)
- `cargo-nextest`
- The native Tauri prerequisites for your operating system

On macOS, install Xcode command-line tools. On Linux, install the WebKitGTK and application-indicator
development packages named in `.github/workflows/rust.yml`.

## Verify a checkout

```bash
npm test
```

This runs the documentation audit and its regression tests, license and storage-budget mutation
checks, Rust formatting and linting, all Rust workspace tests, the complete desktop/React suite, and the real daemon restart demo. Individual commands:

```bash
npm run verify:docs
npm run verify:storage
npm run verify:demo
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo nextest run --workspace --no-fail-fast
npm --prefix apps/desktop test
```

## Run the desktop app

```bash
npm --prefix apps/desktop run tauri:dev
```

The desktop client talks to the local daemon through a user-owned Unix socket. Use disposable or
backed-up projects during the alpha.

## Repository map

- `crates/` — protocol, storage, materialization, policy, approval, daemon, SDK, and adapters.
- `apps/desktop/` — Tauri shell, React interface, packaging, and renderer proofs.
- `protocol/` — schemas, wire format, test vectors, and conformance material.
- `tests/` — cross-crate compatibility, authorization, recovery, and fault evidence.
- `benchmarks/` — reproducible workloads and measured budgets.
- `integrations/` — agent and Git integration surfaces.
- `docs/` — user, architecture, protocol, security, and status documentation.

Do not treat an internal Rust module or current JSON shape as a stable public API unless the
protocol documentation explicitly says it is versioned.

## Validation boundaries

Run commands from the repository root unless a guide says otherwise. The documentation audit
checks local Markdown file destinations and literal npm script examples against package manifests.
It checks neither external URLs nor heading fragments and does not execute examples. Rendered HTML
and packaged text guides still need the desktop guide tests and visual verification.

Pull requests and main pushes run the complete `npm test` gate in the macOS test job, followed by
the native renderer checks. The seven existing jobs remain, including independent Linux Rust,
desktop/React, documentation, storage, license and dependency-policy checks. Rust test failures do
not stop the remaining Rust cases; the command still fails, and later gate stages require success.
The nightly job also runs the complete local command plus dependency policy. A configured workflow
is not evidence of a successful hosted run;
inspect the exact revision's results before release.

On macOS, `npm run test:macos-renderers` runs all four PDFKit and Office integration cases using checked-in synthetic fixtures.
It is also a separate macOS CI step. Other measurement and helper cases remain explicitly ignored in the ordinary Rust suite. Run those separately
on the required platform before claiming their behavior. For a local macOS package, use the
[desktop build and rendered-proof instructions](../apps/desktop/README.md). Build from a clean,
committed tree so the embedded revision identifies the tested bytes. Ad-hoc packaging cannot prove
Secure Enclave approval in an eligible signed distribution or installation on a clean Mac.

When a test fails, retain its output and reproduce the named case before changing code. Local
socket restrictions, absent renderer permissions, and missing tools are environment failures;
report them separately from product defects. Never remove a check or relax its assertion merely
to obtain a passing run.

The Node minimum includes default TypeScript stripping, introduced in
[Node 22.18](https://nodejs.org/en/blog/release/v22.18.0), and satisfies the locked Vite engine.

## Checkpoint worker shutdown

Dropping `LiveDaemon` requests idle-checkpoint shutdown, wakes waiting workers and waits until
all worker generations release their strong workspace/checkpoint references. Completion is separate
from timer notifications so draining a retired worker cannot restart another worker's idle interval.
The scheduler's returned join handle remains available to explicit callers. An in-flight native
filesystem or database operation can delay shutdown; requesting stop does not cancel durable work.

Immediate same-path restart must occur after ownership is released. Otherwise a prior connection
can close or replace SQLite sidecars while the new open verifies their identities. Database
single-link, permission and physical identity checks must remain intact. The regression
`daemon_drop_waits_for_checkpoint_database_owners_even_when_worker_unwinds` parks a real pending-save
worker after it owns database references, checks both normal exit and unwind, and immediately
reopens the preserved journal. This does not verify macOS attachment-event registration or packaged
application shutdown; those are separate boundaries.

## Native worker identity provisioning

The native desktop binary has two explicit worker-identity modes. These are source-supported setup
commands; an eligible Apple-signed Mesh application and actual signed-app acceptance are still
required. Unsigned builds refuse before touching worker files. No installed worker listener or
second-machine execution is implied.

Choose an existing, empty metadata directory owned by the current user, with permissions `0700`,
outside existing projects. The native command does not create this directory or select one implicitly.
For an eligible signed application, the invocation is:

```bash
/path/to/signed/Mesh.app/Contents/MacOS/Mesh --worker provision /absolute/private/worker-state
/path/to/signed/Mesh.app/Contents/MacOS/Mesh --worker identity /absolute/private/worker-state
```

Provision writes a durable setup intent, creates an execution-only keychain identity and initializes
its private ledger. Identity reopens existing complete state and checks the expected stored key;
it never creates a replacement. Both print one `mesh.worker-public-identity/v1` JSON object with
public installation and worker identifiers. This output does not establish peer trust or authorize
execution. No project files, selected desktop workspace, human approval key or provider process are
changed by these modes.

Failed or interrupted setup retains its files and any created key for reconciliation. Repeating
provision against a nonempty directory refuses. Keep retained evidence; these commands provide no
repair, cleanup, rotation or deletion operation. The private installation contains only `intent.json`,
`identity.json` and `ledger/`; additional entries make it unavailable rather than being ignored.
Existing standalone worker-ledger directories are not automatically converted into installations.

Current automated evidence covers native directory/receipt/lock behavior with a custody test double,
and command parsing plus refusal by an unentitled executable. It does not prove successful OS
keychain provisioning, an installed resident service or actual remote execution.

### Resident worker configuration (native implementation under validation)

The signed macOS application now has explicit `--worker serve <absolute-config-file>` and
`--worker connect <absolute-endpoint-folder>` modes. These are optional worker setup tools; they
never attach, move or change the user's existing project. Both require the eligible signed Mesh
application preflight. Successful signed-app enrollment and actual second-machine acceptance remain
unverified; an unsigned development build refuses before touching these paths.

Provision the installation with the earlier `--worker provision` command. Supply separate existing
private directories for the endpoint, input store and received allocations. The endpoint directory
must be empty. Place the configuration in an owner-only directory, with an owner-only regular file;
it is read once, bounded to 16 KiB. Example schema (replace every path and the public key):

```json
{
  "schema": "mesh.worker-config/v1",
  "installation": "/absolute/private/worker-installation",
  "endpoint": "/absolute/private/worker-endpoint",
  "store": "/absolute/private/worker-input-store",
  "allocations": "/absolute/private/worker-allocations",
  "provider": "codex",
  "executable": "/absolute/path/to/codex",
  "coordinator": "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
  "capacity": 4,
  "protected": ["/absolute/path/to/existing-project"]
}
```

The coordinator value must be its independently admitted execution public key, not a human approval
key. Unknown fields, duplicate fields, relative paths, unsupported provider names and capacity outside
1–64 refuse. `claude` selects the existing Claude adapter and needs its explicitly configured installed
executable. Native policy caps each objective at 64 lanes, the configured concurrency, depth 8,
8 retries and an initial lease no longer than one hour. These are upper bounds, not permission to
retry an uncertain assignment. The capacity also bounds retained transfer/provider slots; terminal
and uncertain entries are not automatically evicted.

The resident service retains input and provider ownership across bridge disconnects. Each connection
requires fresh signed Mesh dispatch and admission proof, with exact immutable assignment matching.
The local bridge checks the private socket and OS peer user; that alone is not Mesh key authentication.
A connection has a 60-second total budget, including native connection opening, and input can resume
on a fresh authenticated connection. Neither stdin EOF nor a lost final reply means cancellation,
provider completion or permission to launch twice. Provider observation runs independently of transfer
I/O; blocking native provider startup/storage can still delay observation.

The endpoint is create-only. Existing sockets and other entries refuse startup, and shutdown does
not remove them. Restart/reconciliation requires explicit inspection and a new empty endpoint folder;
retained installation/input evidence is preserved. A restarted service cannot reconstruct an old
reservation or adopt a process from a PID. SSH admission/configuration, signed remote result recovery,
renewed leases and actual remote deployment remain unfinished. Do not describe these local native
modes as verified second-machine execution.

### Native SSH connection boundary

The macOS fleet API provides `NativeSshDestination` and `NativeSshConnection` for explicitly
configured worker connections. An operator must separately install an SSH subsystem named
`mesh-worker-v1` that runs the admitted signed Mesh binary's `--worker connect` mode against the
private resident endpoint. Mesh does not edit SSH configuration, accounts, keys or host trust.
No desktop or agent command exposes this transport yet.

Configuration uses a literal DNS/IPv4 host, account, port, and existing absolute private identity
and known-host files. Paths are restricted to ASCII letters, digits, slash, period, underscore and
hyphen; unsupported names/IPv6 literals refuse. File contents must remain operator-controlled.
Ambient SSH configuration, agent authentication, password prompts, forwarding and multiplexed
connections are disabled. Unsupported SSH options fail rather than falling back to weaker settings.
Encrypted identities requiring interaction refuse in this noninteractive route.

The fixed subsystem receives task-bearing Mesh frames through stdin/stdout. Callers must still
perform the native coordinator/worker proofs before transfer. Pipe I/O shares one absolute budget
(up to 300 seconds); spawn and OS reaping are outside that bound. On any protocol/I/O error, drop
the connection and retain assignment uncertainty for explicit reconciliation. Closing its input
does not close output; dropping the connection kills/reaps only the local SSH client, never sends
worker cancellation and never grants a new launch. Diagnostics are drained without retaining raw
text. A local-client exit status is not a remote result receipt. Actual SSH deployment and
second-machine disconnect/result recovery remain unverified.

Native coordinators can call `deliver_remote_input_over_ssh` with an existing saved-history source,
current lane/run, independently configured coordinator/worker execution keys and an explicit
`RemoteInputDeliveryIntent`. `Claim` verifies the peer before recording the selected original
assignment; `Reconnect` is limited to the retained initial-lease input transfer. Neither intent
automatically retries a failed call or adopts a provider. The native signer receives only checked
dispatch and admission payloads. The worker proof must remain fresh throughout SSH setup; the
connection budget never extends proof or lease expiry. A successful return describes input
materialization only. Remote results, lease renewal and real second-machine acceptance remain open.

### Recovering retained worker facts

Native coordinators can call `inspect_remote_worker_over_ssh` with the exact current attempt and
independently configured execution keys. The signed query is fresh for 30 seconds and does not
extend the connection or lease deadline. A signed reply binds the original query and current
coordinator context; older or changed-context replies refuse. These APIs are not exposed to agent
or desktop commands yet. Older workers reject the additive v1 status message rather than guessing.

Interpret the returned facts conservatively: null admission is unknown/unrecorded; admission records
one original allocation reservation; launch intent records the worker's original execution claim
and source-to-worker initial mapping. None proves materialization, current liveness, completion,
termination, a saved output or permission to retry. Initial lease fields are historical, not renewal
acknowledgments. Status can read retained expired/cancelled work under fresh authentication without
changing any ownership or lease. Worker observation timestamps do not make cached facts stay fresh.
No filenames, task text, credentials or provider diagnostics are returned. The protocol carries a
private goal digest for exact correlation and must not be logged. Signed saved results, actual
second-machine recovery and packaged acceptance remain required.


### Remote lease renewal

`RemoteLeaseRenewal::prepare` accepts a native-selected `RemoteLeaseRenewalPlan` and persists an
exact coordinator intent before opening transport. `signed_request` uses a renewal-specific signing
domain; `exchange_over_ssh` sends it on one configured bounded connection. The resident worker
verifies native keys, provider, limits and fresh challenge before updating an existing exact admission.
It commits a separate worker lease record before signing the acknowledgment. `accept` verifies that
acknowledgment and current coordinator context before `AdvanceRemoteLease`.

New renewals require the previous lease to remain valid. After a lost acknowledgment, explicitly
prepare the identical sequence/deadline again; only a previously committed worker renewal can be
recovered after expiry. A changed request at the same sequence refuses. Signer/transport failures
may follow durable work and are not proof of rollback. A retained lease never reconstructs an input
reservation, launch owner or provider process. The original guarded launch reservation and received
session check the effective worker lease while retaining their original custody and attempt.

The additive persisted schemas are `mesh.remote-renewal-intent/v1` and
`mesh.remote-worker-lease/v1`; existing immutable admissions and launch intents are unchanged.
Each assignment permits at most 4,096 renewals, read in bounded store pages; exhaustion refuses
further renewal and never releases ownership. Existing worker-status v1 replies remain unchanged
and report only historical initial leases. A versioned current-lease read is a separate follow-up.
Focused coverage includes dropped resident replies, reopen, forged acknowledgments, failed signing
after commit, concurrent conflicting writers, and corrupt history beyond a page boundary. It does
not establish real second-machine SSH execution or packaged provider/signing acceptance.


### Read-only effective lease inspection

Use `RemoteWorkerStatusChallenge::issue_with_current_lease` or
`inspect_remote_worker_current_lease_over_ssh` for the additive
`mesh.worker-status-query/v2` / `mesh.worker-status-reply/v2` protocol. The v2 signing domains are
separate from both v1 status and renewal mutations. The original query constructor and SSH function
retain their v1 behavior and closed response format.

`RemoteWorkerStatusReceipt::reports_effective_lease` distinguishes v2 coverage from historical-only
v1 facts. `effective_lease` returns the current retained sequence, deadline and native renewal
acceptance time, or no lease for an unrecorded admission. For the initial sequence, acceptance time
is zero because the admission format did not record renewal time. V2 preserves the historical
initial deadline separately. A null admission is unknown, never proof that retry is safe.

The worker reads the same guarded lease history used for launch decisions and rechecks all facts
around signing. The coordinator verifies the exact version/domain, nonce, context, native observation
time and closed facts. Expired work remains readable without changing lease, owner, slot or attempt.
A fresh read is an observation; it grants no execution rights and does not replace signed result
verification or actual remote disconnect/reconnect acceptance.
