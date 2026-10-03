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
and known-host files. Paths allow ASCII letters, digits, slash, period, underscore, hyphen and
spaces (including ordinary `Application Support` directories). Native code passes the identity as
one argument and quotes the single known-hosts path for OpenSSH. Expansion tokens, quotes, escapes,
control characters and unsupported names/IPv6 literals refuse. File contents must remain operator-controlled.
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


### Exporting a received worker's saved review

`FleetService::prepare_remote_review_input` also handles a native received-worker session.
Select the exact completed checkpoint, recorded bundle and saved version from that session.
The exporter verifies the original assignment attempt, workspace installation and allocation
custody, then pins the immutable saved tree. Later working edits are excluded. The returned
source can outlive the execution owner; each chunk read still checks its retained filesystem
identity and digest. Replacing the workspace path refuses further reads.

This read-only export does not recapture files, revive an execution credential or mutate the
worker ledger. Signed result offers, transfer to the coordinator, restart recovery and candidate
import are subsequent increments tracked in [#163](https://github.com/idosams/Mesh/issues/163).


### Signed saved-result offers

Native worker policy calls `ReceivedWorkerHost::sign_saved_result` with an exact saved selection
and the configured worker-key signer. The received session verifies its original mapping and
saved tree, verifies the returned signature, rechecks custody after signing, and durably records
the offer before returning it alongside the immutable source. The signer callback must not
reenter the same fleet service. Agent IPC does not expose this operation.

`mesh.worker-saved-result/v1` uses the separate `mesh.v1.worker-saved-result` signing domain.
It binds configured coordinator/worker, objective/lane/run/assignment, original input/bundle,
provider/task digest, launch owner, workspace mapping and initial operation, installation,
checkpoint, recorded review, saved result version and exact manifest digest. Messages are closed,
canonical and bounded to 64 KiB. They contain private correlation and must not be logged.

Each launch/checkpoint retains one signed offer in a separate `remote-result-` stream. Exact replay
returns the original bytes without signing again; conflicting content or extra history refuses.
`RemoteAdmissionRegistry::saved_result_offer` reads this evidence after reopening the ledger without
reconstructing execution authority. `RemoteSavedResultOffer::verify` checks the signature against
the coordinator's exact current assignment and supplied manifest without advancing any state.
An immutable offer has no freshness or liveness claim; lease expiry does not erase saved results.

This supplies signed offer construction, persistence and verification. An authenticated result
query/transport route, resumable content transfer and coordinator candidate import are still
required under [#163](https://github.com/idosams/Mesh/issues/163). An offer or manifest alone never
proves complete transfer, releases a slot or approves protected main.


### Recovering a known result offer

`RemoteSavedResultChallenge` issues a fresh authenticated query for one known checkpoint in the
coordinator's current assignment. `inspect_remote_saved_result_over_ssh` sends it over a single
bounded native SSH connection. The resident `NativeWorkerConnections` route reads the original
guarded worker ledger and signs a reply without importing content or reconstructing execution.

`mesh.worker-result-query/v1` and `mesh.worker-result-reply/v1` use separate signing domains.
The query binds a random nonce, 30-second validity, original assignment, objective limits and
checkpoint. The reply binds the complete query digest, observation time and either the original
signed offer or null. The worker rechecks ledger facts around signing, and the coordinator rechecks
its exact context before accepting. Null is an unknown/unrecorded result, never a retry grant.
A lost reply requires an explicit fresh query; it does not trigger another launch or signature
of the persisted offer. No automatic retry extends freshness or the connection budget.

This is known-checkpoint recovery only. Checkpoint discovery, immutable manifest/content transfer,
restart-safe content reopening and exact candidate import remain follow-up work in
[#163](https://github.com/idosams/Mesh/issues/163). A recovered offer must still be verified against
the supplied manifest before content reconstruction; it is not transfer completion or approval.


### Resident saved-result publication

The native worker service now polls its original received owners with the configured worker-key
signer. Each owner attempts publication at most once per second, reads a page of saved review
identities, and durably signs at most one new offer per attempt. The cursor wraps to revisit late
completion and earlier checkpoint IDs; failures advance the cursor without changing execution
authority. Up to 4,096 successful selections are retained in memory to avoid repeated exports.
The underlying saved-review metadata scan is not a constant-time operation.

Publication failures are separate native observations from provider status. The desktop service
currently drains these observations; no live UI delivery is claimed. Existing explicit signing
and resident polling APIs remain available. This adds no persisted or wire format. Durable offers
remain authoritative after cache loss. Signing and storage can block the owner loop, so the poll
interval is not a latency guarantee. Discovery, content reopening and transfer, coordinator import,
and real signed-app/second-machine acceptance remain required.


### Durable native result discovery

`RemoteAdmissionRegistry::saved_result_page` reads at most sixteen offers for one exact original
launch, ordered by durable catalog revision. The page includes its observed revision, continuation
cursor and whether more rows existed at that revision. Reuse the final cursor to observe later
publication. Unknown admission or absent launch returns no page; an empty page is not completion.
Every row must match its checkpoint request and the original guarded signed-offer record. The
reader refuses invalid cursors, gaps, altered records and mismatched launch facts.

Publication now retains the original offer first, then indexes its exact bytes in a separate
`remote-result-catalog-` stream before returning success. Each launch is bounded to 4,096 entries.
The two appends are not one transaction: interruption can leave an original offer without an index
entry, but cannot acknowledge a catalog entry before both records exist. Explicit republication
repairs that gap without another signature. A concurrent conflicting append refuses and does not
automatically retry. Existing per-checkpoint records and known-checkpoint queries are unchanged.

Pre-catalog offers remain readable by checkpoint and enter discovery only after explicit
republication; this page does not claim a complete inventory of legacy offers. Restart enumeration
requires no provider or workspace recreation. This is a native API, not yet authenticated discovery
over SSH. Content transfer/reopening, coordinator import and actual remote acceptance remain open.


### Authenticated result discovery

`RemoteResultDiscoveryChallenge` prepares a fresh read-only query bound to the coordinator's
current assignment and a native catalog revision cursor. The resident connection route and
`discover_remote_saved_results_over_ssh` return a worker-signed page without receiving input,
launching work or transferring result content. Queries and replies have distinct
`mesh.worker-result-discovery-query/v1` and `mesh.worker-result-discovery-reply/v1` schemas and
signing domains. They are closed canonical control messages bounded to 64 KiB.

The coordinator consumes the challenge, checks its nonce/context/deadline, the reply signature and
query digest, and the page's cursor/count relationships. Every offer must have the exact assignment
and a unique checkpoint within the page. The worker rechecks the catalog after signing; concurrent
changes refuse the observation. Native SSH uses one bounded connection and no automatic retries.
A lost reply needs a fresh query at the retained cursor. An empty page or unknown launch is not
completion, permission to retry execution or evidence that all legacy offers have been indexed.

This exposes the native catalog's existing scope and compatibility limits. Exact manifest/content
verification, resumable output transfer, native candidate import and actual second-machine
acceptance remain required. Task correlation and signed envelopes must stay out of general logs.


### Reopening immutable worker results

`RemoteAdmissionRegistry::reopen_saved_result` binds an existing signed offer to an independently
admitted native input destination and its original launch receipt. It reopens the fixed allocation
through retained directory authority, checks the complete workspace-mapping digest, input manifest,
original initialization context and physical installation tokens, then reads saved history using an
ephemeral index. It does not run working-file recovery or create a provider/session/credential.

The recorded review must name the exact saved version, the original initial tree must match the
assigned input, and the reconstructed result manifest must match the offer. The reader rechecks
receipts and guarded launch/offer facts before returning. Reads are bounded and reject links, extra
permissions, replaced roots and changed receipt bytes. Returned chunk reads retain allocation and
workspace pins and rehash declared content; missing content refuses instead of being repaired.

This supplies restart-safe native content access. It adds no persisted or wire format and does not
claim remote delivery, retention against garbage collection, candidate import or real-machine
acceptance. An unknown checkpoint yields no result; unavailable storage never authorizes rerunning
the original work.


### Native resumable result receipt

`NativeRemoteResultReceiver` verifies the signed saved-result offer against the coordinator's
current assignment and complete manifest before initializing its independently admitted private
store. It takes exclusive native receiving ownership and reuses the existing CAS partial-offset,
chunk promotion and whole-file verification rules. Each status/readiness/write operation rechecks
the exact offer/context and native root identity before and after storage access.

Dropping the receiver preserves durable partial offsets. A new receiver revalidates the offer and
manifest, resumes from the retained offset and refuses undeclared chunks, conflicting offsets and
substituted storage. No input admission, synthetic assignment, worker launch or working folder is
created. The receiver has no materialization method. `verify_complete` checks current full content;
it does not create a durable completion/import receipt or protect bytes from later collection.

This is a native receiving component. Authenticated network result serving, transfer orchestration,
durable exact import receipts and real second-machine acceptance remain required. Missing or
corrupt bytes refuse; no receiving failure authorizes rerunning the original worker.


### Authenticated saved-result content transfer

The macOS native resident route accepts a separately signed
`mesh.worker-result-transfer-query/v1`, scoped to a known checkpoint and exact coordinator
assignment. Its reply uses its own signing domain and binds the fresh query to the original
worker-signed offer. The worker reopens the exact retained immutable review before serving its
manifest and declared content. Existing known-offer queries do not authorize this operation.

`receive_remote_saved_result` and `receive_remote_saved_result_over_ssh` verify the signed header
and manifest before opening the independently admitted private result store. They resume retained
CAS offsets, verify frame identity, offsets and final hashes, and check every complete file. A
connection failure preserves partial receipt; a new explicit request has a new challenge. There
is no automatic reconnect, authorization extension, execution retry or working-folder write.

The existing 30-second challenge lifetime also bounds transfer authority; the authenticated stream
owner must provide I/O deadlines. Manifest and part frames remain bounded to 1 MiB and 64 KiB.
Worker requests must name declared chunks in increasing digest order, at most once each, so peer
requests cannot cause unbounded repeated reads. A worker retains at most one verified 4 MiB chunk
while writing parts and checks native root identity and freshness between parts.

A successful return verifies immutable content and retains a native content receipt before writing
the end frame. The end frame itself is not a durable peer acknowledgment, candidate import,
retention pin, proof of provider success, or protected-main approval.
Durable import correlation and real second-machine acceptance remain separate requirements.


### Durable remote result content receipts

`NativeRemoteResultReceiver::record_content_receipt` verifies all files, durably retains the canonical
manifest as a private bounded create-only file, then records an exact receipt in the coordinator
ledger. `mesh.remote-result-content-receipt/v1` binds the original signed offer, manifest identity
and receiving directory's physical identity. Receipt replay returns the same digest without changing
objective revision, run state, capacity or approval. Transfer completion calls this before its end
frame; a lost reply can be reconciled without launching work again.

`reopen_content_receipt` takes a freshly native-admitted destination, the exact original signed offer
and current coordinator assignment. It checks the retained manifest, ledger and every content hash.
Missing, partial, non-private, linked, replaced or conflicting metadata refuses and is preserved.
A missing manifest beside an existing receipt also refuses republication without recreating it.
An interrupted manifest write can leave partial evidence requiring explicit reconciliation; this
never becomes a completion receipt. Existing stores without these files remain valid input/result
stores, but cannot claim a recorded content receipt. This receipt is not retention policy or the
original-project candidate import; those must still establish their own native proofs.


### Native saved-result identity correspondence

The macOS worker registry can reopen a result with `reopen_saved_result_with_correspondence`.
Alongside the original signed offer and read-only content source, it derives bounded
`mesh.remote-result-correspondence/v1` metadata from the exact original worker snapshot and saved
review. Both complete snapshots must match their manifests. Existing project-import correspondence
rules preserve retained object identity across moves; a newly created object at an old path has no
input identity. Duplicate objects, mismatched content/version and retained-object type changes refuse.

The metadata binds assigned input and manifest, worker initial operation, saved result and manifest,
and every result entry's native object identity, kind, path and optional original input path. Its
canonical encoding is limited to 4 MiB and exposes a complete digest for future authenticated
transport. This is private metadata, not a control-frame payload. It has no import mutation
or approval capability. Current transfer does not carry it yet: a coordinator must not trust these
bytes without the separately authenticated binding and native original-project checks.


### Authenticated correspondence retrieval

The native worker route `mesh.worker-result-evidence-query/v1` accepts a fresh coordinator-signed
request for one exact signed-offer digest. Its separately signed reply binds the whole query,
original offer and complete correspondence digest/byte count. The worker derives evidence only by
reopening the exact native retained result and rechecks history, source identity and the 30-second
query lifetime while serving it. Existing offer discovery and content-transfer formats are unchanged.

`receive_remote_result_evidence` and its bounded SSH wrapper require the original offer and complete
input/result manifests. They verify assignment and signatures, receive at most 4 MiB in 64 KiB
parts, enforce digest/offset/final flags, and decode a closed canonical descriptor. Every result path
and kind must match its manifest; objects and retained input paths must be unique, and claimed
input paths must exist with the same kind. Wrong initial operations and input/result identities
refuse. No automatic reconnect extends the fresh authorization.

The returned `AuthenticatedRemoteResultEvidence` is a current authenticated observation, not durable
import authority. It writes no CAS, working files or import state. Durable evidence retention and
coordinator original-project candidate preparation still need to bind this exact offer/evidence
to native source custody before any review import. Real second-machine acceptance remains required.

### Delegated input ancestry

Native integrations preparing a local saved result for remote work can use
`FleetService::prepare_remote_project_input` or the history-only `FleetHistory` equivalent. The
returned borrowed handle retains each ancestor allocation and the original-project input. Use its
verified `manifest`, `original_version`, `original_objects` and `read_chunk` accessors; the delegated
input operation and original-project predecessor are different identities. Revalidate the handle
around operations that yield. The handle proves historical correspondence, not current dependency
eligibility, execution authorization, approval or a retention policy. Remote result import still
requires authenticated result correspondence and the separate native import boundary.

### Native coordinator observations

An eligible signed macOS Mesh application can inspect an already-recorded remote assignment before
opening its graphical window:

```text
Mesh.app/Contents/MacOS/mesh-desktop --coordinator status /absolute/private/coordinator.json
Mesh.app/Contents/MacOS/mesh-desktop --coordinator results /absolute/private/coordinator.json 0
```

The configuration file and its containing directory must belong to the current user and have no
group/other permissions (normally file `0600`, directory `0700`). The file must be a single-link regular file of at most 16 KiB. Use exactly these fields; the example key is a placeholder to
replace with the independently verified worker public key:

```json
{
  "schema": "mesh.coordinator-observation-config/v1",
  "installation": "/absolute/private/coordinator-identity",
  "fleets": "/absolute/private/mesh-fleets",
  "objective": "recorded-objective-id",
  "lane": "recorded-lane-id",
  "run": "recorded-run-id",
  "host": "worker.example.test",
  "account": "mesh",
  "port": 22,
  "identity": "/absolute/private/ssh-key",
  "known_hosts": "/absolute/private/known-hosts",
  "worker": "abababababababababababababababababababababababababababababababab"
}
```

`installation` is an existing dedicated native installation created by the explicit worker identity
provisioning command. Its public key must already be the coordinator identity trusted by the remote
worker; local capture's temporary key cannot replace it. This command only reopens custody and never
provisions or falls back to a temporary key. `fleets` is the existing app-owned fleet catalogue;
the objective, lane, run and remote assignment must already exist. Another process owning the
catalogue causes refusal, without takeover or worker adoption. The command is not a remote dispatch
setup wizard and cannot create a missing assignment.

The host/account and existing private SSH identity/known-hosts files pass the native transport's
checks. The fixed `mesh-worker-v1` SSH subsystem must already be configured on the worker. Mesh does
not enroll host trust, accept arbitrary SSH options, use an agent or proxy, or change SSH settings.
Both commands perform one bounded observation. Disconnect/timeout remains unknown; no reconnect,
lease renewal, automatic retry, provider execution or original-project import follows.

Status outputs `mesh.coordinator-status/v1` with the verified target, observation time and retained
facts. Results outputs `mesh.coordinator-results/v1`; a null `page` means unrecorded/unknown work,
whereas an empty verified page is a distinct result. Pass the returned `after` cursor explicitly for
the next page; accepted input cursors are canonical decimal values from 0 through 4096. Offers and
status correlation can contain private task information, so keep command output local. They confer
no main approval. App controls and composed result ingestion remain separate integration work.

Native parser, private-file and unsigned-refusal tests cover this command. A successful source build
or unsigned refusal does not establish signed-app custody, a real SSH connection, second-host
recovery or packaged end-to-end acceptance.


### Explicit coordinator result receiving (macOS)

An eligible signed Mesh application can receive one selected remote result into native private
storage without launching another worker or blocking the fleet service lock:

```sh
/path/to/Mesh.app/Contents/MacOS/mesh-desktop --coordinator receive /absolute/private/receive.json
```

The owner-private regular configuration uses the same bounded 16 KiB reader as the worker and
observation commands (private parent, no symlink or hard-link file). It has exactly seven fields:

```json
{
  "schema": "mesh.coordinator-receive-config/v1",
  "connection": { "schema": "mesh.coordinator-observation-config/v1", "installation": "/absolute/coordinator-identity", "fleets": "/absolute/fleets", "objective": "fleet-id", "lane": "lane-id", "run": "run-id", "host": "worker.example.test", "account": "mesh", "port": 22, "identity": "/absolute/ssh/key", "known_hosts": "/absolute/ssh/known_hosts", "worker": "64-lowercase-hex-worker-key" },
  "input": { "kind": "project", "storage": "/absolute/attachment-storage", "project": "64-lowercase-hex-project-id", "version": "64-lowercase-hex-saved-version" },
  "store": "/absolute/private-receiving-store",
  "allocations": "/absolute/private-received-results",
  "allocation": "32-lowercase-hex-stable-native-request",
  "offer": "exact signed offer string selected from coordinator results"
}
```

Replace the descriptive identity placeholders with the actual canonical identifiers. `connection`
is the complete existing observation configuration, with its independently trusted worker key,
SSH identity and known-hosts file. Its coordinator identity must already match the retained
assignment; this command does not adopt an ephemeral capture identity or create an assignment.
The exact offer string comes from authenticated `--coordinator results` output. It is bounded to
8 KiB and must pass the native signature and assignment checks before receiving writes. A parsed
configuration alone grants no authority. Keep the complete file private; it contains task correlation.

For a managed saved input, replace `input` with exactly
`{"kind":"review","lane":"parent-lane","checkpoint":"saved-checkpoint","version":"64-lowercase-hex","bundle":"64-lowercase-hex"}`.
Mesh resolves and verifies the saved native history itself. No input manifest, shell command, raw
per-file destination or main-approval choice is accepted. Project input reads a saved version, not
the current original folder. The current original directory must still be identifiable so it can
be protected from destination overlap. Retained review inputs keep the existing history checks.

The receiving store and allocation parent must already be distinct, admissible private directories.
Mesh protects the coordinator installation and fleet catalogue, plus the original project and its
retained storage for project inputs. It does not create missing directories or overwrite existing
allocations. Preserve the same configuration and allocation identity across an uncertain result;
only an explicit rerun may recover an acknowledged local review. Completed ingestion reopens its
retained content/evidence without network or signing, while the command still requires the existing
native context and eligible application. Partial unacknowledged work remains preserved and may
require reconciliation; never rotate the allocation identity to bypass that refusal.

Success prints `mesh.coordinator-received-result/v1` with `correlation`, `version` and `review`
identities. This is a private saved result, not imported or approved project main. A stdout failure
after commit leaves the saved result intact; retain the configuration for explicit recovery. No
lease renewal, automatic retry, provider launch or original-folder write occurs. CLI source and
fixture validation do not establish signed-app, OS-custody, real SSH or second-machine acceptance.


### Native initial remote input delivery

An eligible signed macOS Mesh application accepts `--coordinator start <absolute-private-config>`
before GUI startup. The command creates one native fleet/root from an existing saved project version,
records one attempt and transfers immutable input to the independently configured worker. It uses
the existing Apple-held coordinator identity, SSH admission and signed worker proof. A receiving
acknowledgment means `input-materialized` or `input-retained`; it does not prove provider startup or
completion. The resident worker must already be provisioned and independently configured.

The owner-private JSON file uses the closed `mesh.coordinator-start-config/v1` schema:

```json
{
  "schema": "mesh.coordinator-start-config/v1",
  "connection": {
    "schema": "mesh.coordinator-peer-config/v1",
    "installation": "/absolute/private/coordinator-identity",
    "fleets": "/absolute/private/fleets",
    "host": "worker.example.test",
    "account": "mesh",
    "port": 22,
    "identity": "/absolute/private/ssh-key",
    "known_hosts": "/absolute/private/known-hosts",
    "worker": "<64 lowercase hexadecimal public-key characters>"
  },
  "storage": "/absolute/private/project-attachments",
  "project": "<64 lowercase hexadecimal project characters>",
  "request": "<32 lowercase hexadecimal request characters>",
  "version": "<64 lowercase hexadecimal saved-version characters>",
  "goal": "Perform the authorized work on this saved version",
  "provider": "codex",
  "limits": {"lanes": 2, "concurrency": 1, "depth": 1, "retries": 0},
  "lease_until_ms": 1
}
```

Replace the placeholders with existing native identities. Set `lease_until_ms` to an explicit future
Unix deadline in milliseconds, at most one hour away and within the worker's independent limit; the
example value deliberately refuses. `provider` is `codex` or `claude`; this initial configuration
admits that single provider. The fleet root must already exist, be private, and lie outside the
original project, attachment metadata/storage and coordinator installation. No input manifest,
executable command or lane filesystem destination is accepted from the configuration.

Retain the complete configuration before invoking it. The request deterministically selects its
objective, and native creation derives the lane. The run is `start-<request>` and the assignment is
`assignment-<request>`. The returned `mesh.coordinator-start-result/v1` contains these identities and
transfer correlation for the existing status/results/receive commands. Their v1 formats remain
unchanged. Copy the admitted peer fields and returned objective/lane/run into the observation
configuration; no key or original-project content is emitted.

If output is lost or any stage fails, use `--coordinator created <same-absolute-private-config>`.
This reads retained catalogue facts by request without opening the original project, SSH or signing
custody. It reports an absent entry as `fleet: null`, damaged/incomplete entries as unavailable,
and recovered history as `restored-unattached`. The original app eligibility and private-file rules
still apply. An expired deadline is allowed for this read. Reading facts grants no retry authority.

Repeating `start` after an existing attempt or after reopening a bound lane refuses. Do not change
request IDs, rotate the run, renew the deadline or recreate directories to conceal an uncertain
outcome. Initial-transfer reconciliation after restart remains a separate, unfinished native
capability. Original files and protected main stay under their existing human approval boundary.
GUI start/receiving controls, real OS-key and second-host acceptance remain required.

### Explicit initial-input reconnect

`--coordinator reconnect-input <absolute-private-config>` is an explicit continuation of an
already claimed initial input transfer. It may reopen the coordinator's retained catalogue after
restart; it does not adopt a local lane, dispatch another attempt or renew a lease. The original
worker must still retain its receiving reservation. Completed/running work, cancelled work,
expired or renewed leases, changed input, a different worker and missing claims refuse.

The closed `mesh.coordinator-input-reconnect-config/v1` JSON object has exactly three fields:
`schema`, `connection` and `input`. `connection` is the existing complete
`mesh.coordinator-observation-config/v1` object with the original objective/lane/run and independently
admitted peer. `input` is the same project or saved-review selection used by the receive command.
There is no assignment, replacement lease, allocation or manifest field. Existing receive and
observation configurations remain unchanged. The app eligibility, owner-private file and native
signing rules apply before the operation can contact the worker.

Retain the original configuration and identities. The response schema
`mesh.coordinator-input-reconnect/v1` contains a disposition (`input-materialized` or `input-retained`)
and the existing transfer correlation. Materialization is not provider startup/completion or review
acceptance. A failure may follow a durable worker change; inspect the same assignment rather than
creating another request or treating a timeout as termination. This command cannot reconstruct lost
worker reservations or resolve an assignment that was never durably claimed by the coordinator.

### Packaged existing-project window journey

`node apps/desktop/scripts/prove-attached-project-window.mjs --app /absolute/Mesh.app --revision <40-character-commit>`
verifies the sealed exact-revision app before and after the journey. It creates a separate home and
an isolated dirty Git project, then drives real attachment, history, comparison, pin, independent-line,
resume and detach controls. External edits come from the enclosing verifier, not a simulated native
reply. Saved comparison bytes and two retained selections are checked while newer edits continue;
restart must restore stopped projects before explicit resume. The runner independently compares Git
internals, original folder identity, remaining file content and the persisted pin record. Evidence
and failures remain in its printed private temporary folder. The installed app and user checkpoint
are not replaced. This command does not establish provider execution, protected-main approval,
external-harness attribution or second-host acceptance.

Coordinator execution now returns its verified native result separately from CLI output. Existing
`status`, `results`, `start`, `created`, `receive` and `reconnect-input` arguments and response schemas
are unchanged. A closed output stream may lose a response after durable work; it never retries the
operation. Keep the original configuration and use the action's explicit recovery path. The native
operation still checks signed-app eligibility before opening configuration or custody. This boundary
is groundwork for graphical controls, not a renderer API accepting configuration or credential paths.

### Graphical remote observations

In **Agent fleets → Inspect a remote worker**, choose an existing private
`mesh.coordinator-observation-config/v1` file using the native file chooser. This initial graphical
route requires the configuration's `fleets` path to be this application's fleet catalogue and its
objective/lane/run to identify retained work there. It reuses the catalogue's current native owner;
it does not open a competing owner or import an external fleet directory. The app must be eligible
for Apple-held coordinator custody, and the worker subsystem and private SSH files must already be
configured as described above. Named connection settings can be saved as described below.

The selected configuration is captured natively for this session. The page receives an opaque
selection ID and public target labels, never credential paths or signing data. The selection binds
installation/fleet directory identity, coordinator identity and the originally admitted SSH files.
Replacing those objects requires a new native selection. Cancelling the chooser retains the old
selection; **Forget this selection** drops the session selection without deleting files or history.
Closing Mesh forgets it; reopening does not automatically reconnect or adopt a remote process.

**Read worker status** and **Find saved remote results** each perform one bounded authenticated
read for the exact selection. They do not renew, start, receive, retry or approve work. A recorded
launch is historical evidence, not proof of a live process. Result discovery displays at most sixteen saved-result identities per page, with explicit previous
and next controls. The renderer checks the native returned cursor, bounded row count and monotonic
catalogue revision; failed or regressed reads preserve the last verified page with a warning.
Each row exposes only offer/checkpoint/version/review/manifest identities. Private assignment facts,
installation, launch ownership and signature stay native-only. These identities prove neither local
content receipt nor approval; receipt/import still use the native CLI recovery flows. Failed reads preserve the last observation with a stale warning. There is no
background polling. Rendered tests and unsigned native refusal tests do not establish a real
signed-app/SSH/second-host graphical journey.

### Set up a remote observation without a configuration file

Inside **Inspect a remote worker → Set up a worker connection**, select the existing coordinator
identity folder, private SSH identity and trusted-hosts file with native choosers. Enter the worker's
DNS/IPv4 address, account, port and public identity, then select a fleet and lane with a retained
attempt. Mesh derives the catalogue path and attempt from native records. The renderer cannot
provide file paths or a replacement catalogue. The chosen identity and SSH subsystem must already
exist; setup neither provisions credentials nor enrolls trust.

Each picker reply advances an opaque draft ID. Native code binds the selected path's identity and
metadata and rechecks it before installing the settings. A stale picker reply, changed key/trust
file, replaced identity folder or stale draft refuses. **Clear selected setup files** rotates the
draft and drops those selections; it does not forget the active connection, delete files or change
history. A late reply from an earlier picker cannot restore the cleared draft. After a renderer
reload, clear the draft before selecting files again if the renderer lost its draft ID.

**Use these connection settings** validates the native selection without contacting the worker.
Then explicitly read status or results. Locally recorded attempts need not have a remote assignment;
those reads still require the existing signed remote assignment and peer proof. A refused setup
preserves the active selection. Existing private configuration import remains available.
Active selections and drafts are session-only. To retain settings, use **Load saved connections**,
enter a name, then **Save current connection**. Up to sixteen entries are supported. **Open saved
settings** explicitly rechecks the original directory/coordinator identities and key/trust file
metadata, re-admits native custody/SSH policy, and fills the setup form. It does not contact, start,
renew or adopt a worker. Edit the opened form, use those settings, then save to replace that entry.
To create a separate entry instead, clear the selected setup files and select the files again.
**Remove saved settings** removes only that entry; it retains the active selection, credentials and
work history. Closing Mesh never automatically restores an active connection.

The native-only `mesh.native-remote-connection-settings/v1` catalogue record uses owner-only files,
canonical bounded JSON, catalogue device/inode binding and expected-revision writes. Each entry
contains `mesh.native-saved-connection/v1` configuration and original identity bindings, never key
contents. Paths remain native-only. A pending write prevents further normal reads/writes until
**Recover interrupted settings save** explicitly publishes its exact next revision. Invalid,
foreign, linked or conflicting records are preserved and refused. Recovery never opens the listed
credential paths or contacts a worker. Unknown schemas are refused; existing stores with no record
start at revision zero and require no migration. These settings grant no execution/approval authority.
Remote start/result-receipt controls, lost-worker recovery and signed-app/second-host acceptance remain unfinished.


### Resume an interrupted input transfer in the app

After explicitly selecting or opening the original connection, choose **Resume saved input transfer**.
Native history resolves the original attached project version or an exact completed, reviewed parent
checkpoint. No editable path, manifest, new attempt or lease is accepted from the renderer. Mesh uses
the current catalogue owner and rechecks original file/coordinator identities before transport and
signing. The existing transfer verifies the immutable assignment, input, worker, cancellation and lease.

Only an already claimed, still-launching initial transfer is eligible. The worker must retain its
original receiving reservation. An unavailable source, absent reviewed parent, changed assignment,
newer attempt, cancellation or expired lease refuses; the app does not guess or create a replacement.
An input receipt means accepted input, not a running or completed provider. Errors retain uncertainty;
inspect status and resume only through another explicit action. No automatic retry or worker adoption
occurs. This does not recover a lost worker reservation, expose remote launch/result receipt, or prove
a real signed-app/second-host journey.


The bundled remote observation projection is now `mesh.remote-panel-observation/v2`. It adds bounded
public result rows and validates the returned cursor (the cursor after those rows, not the request
cursor). Old projection shapes are refused by the paired renderer. This is session IPC only; saved
connection and fleet persistence formats are unchanged. Pages replace each other rather than growing
an unbounded list. Discovery never downloads result content or authorizes import.


### Saved input for native result receipt

`FleetHistory::saved_remote_input` resolves an exact recorded remote attempt's original project
version or complete reviewed parent checkpoint independently of whether that attempt is still
running, cancelled, expired or superseded. `prepare_saved_remote_input` exports those immutable
bytes from independently admitted native history, compares both version and manifest against the
original assignment, then rereads the assignment and selector before returning. Missing/substituted
project handles, missing bindings or mismatched manifests refuse without repairing history.

This read/export grants no transport, execution or approval authority. The existing graphical
reconnect uses it only after its stricter latest/claimed/launching/initial-lease checks, and transport
still rechecks cancellation and the original assignment. The separate receipt lookup is groundwork
for graphical result receiving; native receiving destination/intent management and download controls
remain unfinished. Existing explicit CLI receipt and its independent authenticated ingestion checks
are unchanged. No persisted format changes are introduced.


## Native receiving inbox prerequisite

`RemoteResultInbox` creates private `remote-results` storage only beneath an independently admitted
native application directory. An existing or partial inbox is preserved and refused. The native
owner must durably retain the returned installation identity outside the inbox before transport;
reopening requires that identity and validates the exact store/allocation folders against the
owner-only `mesh.remote-result-inbox/v1` record. Missing, replaced, linked, malformed or newly
protected storage refuses without repair. This new record has no migration from unknown folders.
This is receiving storage infrastructure, not an exposed graphical download action: durable
receipt intents, authenticated result selection and graphical receipt/recovery remain required.


## Retained native receipt selections

On macOS, matching the existing signed-result and ingestion API, an admitted receiving inbox can
retain up to 64 immutable `mesh.remote-receipt-intent/v1` records.
Each record binds the exact signed offer, stable allocation, inbox installation and private native
configuration. Identical retention is idempotent; changed context for the same offer refuses.
Listing revalidates signatures, canonical records, physical inbox identity and bounded private
files. Partial, copied or malformed records remain preserved and require reconciliation. No
automatic deletion or migration is provided. A saved intent does not prove any content arrived:
the caller must re-admit peer/configuration/assignment/trust and query the fleet's completed review
ledger. These primitives still require desktop integration and explicit graphical recovery.


## Catalogue-owned receiving storage

`AttachmentStorage::remote_result_inbox` retains the inbox installation outside the receiving
folder in an owner-only `mesh.native-result-inbox-binding/v1` catalogue record. Native callers
supply an independently admitted application parent; saved catalogue and parent identities must
still match on reopen. Provisioning is explicit and lazy. An inbox without its catalogue binding,
a partial binding, or substituted storage refuses without adoption, deletion or repair. All
registered project identities, including detached/offline projects, are automatically protected;
renaming a source does not remove that protection. The format is additive; unknown prior inboxes
require reconciliation. This supplies native lifecycle integration, not graphical receipt or proof
that a remote result has arrived. Desktop download/recovery controls remain unfinished.

Native application-data parents may be readable/searchable by other users, as in the desktop's
ordinary startup layout. Group/other-writable parents refuse. Receiving folders stay owner-only,
and the catalogue binding and receipts remain owner-only files; no permissions are changed on
an existing parent or project.


## Graphical remote result download and saved-attempt recovery

In the macOS remote worker panel, **Find saved remote results**, then **Download for review** on
one exact result. Native code retains at most the last authenticated page's sixteen signed offers;
the renderer supplies only its current selection and an offer digest. The app-owned catalogue
creates/reopens private receiving storage, retains the exact offer/configuration/allocation before
I/O, reconstructs the original immutable input and uses the existing guarded ingestion service.
The app's fleet owner is reused; no second catalogue owner is opened for the command.

**Load saved downloads** lists exact attempts for the current native connection and bindings.
After reopening that connection, **Check or resume saved download** uses the original retained
intent and allocation, without requiring the remote result to be on the current page. Completed
replay revalidates local content/history without another transfer; incomplete stages may resume
only through existing guarded ingestion. Partial/unacknowledged allocations and changed native
bindings still refuse and remain preserved. Listing alone never transfers content. No automatic
retry, provider launch, original-project write or protected-main approval is requested.

Only a verified completed receipt enables **Show downloaded reviews**, which opens the existing
fleet received-result queue for parallel pinned review. A stale/mismatched native reply does not
show completion. Errors retain native attempts for explicit recovery. The UI is localized in
English/Hebrew and disables duplicate in-flight actions. Storage formats remain unchanged; paired
native/UI receipt envelopes are new v1 messages. Eligible native signing remains required. Actual
packaged signed-app/SSH second-host download and recovery acceptance are still unverified; the
fixed user test checkpoint is unchanged.
