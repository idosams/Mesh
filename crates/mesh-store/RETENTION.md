# The retention contract of Mesh's collector

This document is what plan §6.4 calls "the published retention semantics": the precise statement of
what a Mesh workspace keeps, what it is allowed to delete, and what evidence stands behind each
sentence. It follows the discipline of `crates/mesh-cas/DURABILITY.md` — every claim below is
either something a named test demonstrates or something the collector assumes and does not test,
and the two are kept apart on purpose.

Plan §2.10 forbids the words "safe" or "lossless" without reproducible evidence. So the guarantee
is stated first, the evidence second, and the assumptions third.

## The guarantee

> **No content reachable from any retained root is collected.** Where the collector is unsure, it
> keeps.

Reachability is a *set*, so the guarantee is only as good as the set. That set is
`mesh_store::RetainedRoots`, and it is enumerable, printable and closed: adding a way to keep
content means adding a variant, and there is no "weak" root, no priority ordering and no way to
downgrade one.

## The eight retained roots

Plan §6.4 names seven categories of thing a workspace promises to keep. `RetainedRoot` carries
those seven plus a handle for naming a file manifest directly.

| Root | Names | Closes over |
|---|---|---|
| `CanonicalHead` | an operation | that operation and its whole ancestry |
| `ActorHead` | an **actor** | that actor's current head and its ancestry. Named by actor so the root follows the head forward instead of pinning the operation it was written against |
| `ReviewBundle` | a bundle | the bundle's subject operation and its ancestry |
| `UnresolvedConflict` | an operation | that operation and its ancestry. Plan §4.8 preserves both sides of a conflict; this is what keeps both sides' bytes |
| `RestorePoint` | a name and an operation | that operation and its ancestry |
| `RetentionWindow` | an actor and a sequence number | every operation of that actor at or after that sequence, and their ancestries |
| `OfflinePeer` | a peer | everything that peer has not acknowledged — the same difference the `outbox` table renders — unless the policy has expired the peer |
| `Manifest` | a manifest | that manifest's own content digest and every chunk it names |

From the reachable **operations** and **manifests**, the reachable **content** is:

* every reachable operation's `payload_digest`;
* every reachable manifest's `content_digest` and every `chunk_digest` it names;
* plus one derived edge: an operation whose `payload_digest` is itself a manifest identifier keeps
  that manifest. This edge only ever *adds* reachability, so a workspace in which the relation does
  not hold loses nothing by it.

`RetainedRoots::conservative` builds the set from the index itself — every actor head, every review
bundle, every peer, every manifest. **A collection against that set frees exactly the content no
record in the index mentions at all**, which is plan §6.3's crash residue and nothing else. Freeing
more requires naming which root is genuinely gone, and this crate cannot make that judgement.

## What an offline peer's watermark keeps, and when it stops

A peer's watermark says which of each actor's operations it holds. Everything above the watermark
is content the peer is still owed, and the `OfflinePeer` root keeps it. Two things end that claim:

1. **The peer returns.** Its acknowledgement moves the watermark; the operations below the new
   watermark leave the outbox and stop being roots. Nothing is deleted at that moment — they merely
   stop being retained *by this root*, and any other root that reaches them still does.
2. **The policy expires it.** `RetentionPolicy::offline_peer_expiry_epochs` is an allowance in
   **policy epochs**. A peer expires when the workspace's highest policy epoch exceeds the highest
   epoch the peer acknowledged, by more than the allowance. The default is `None`, meaning never.

**The expiry is not a clock.** This repository's ordering is `lamport → event id → content hash`,
and a wall-clock deadline would be both banned and wrong: it would give different answers on two
machines and would not survive an index rebuild. Policy epochs come from the immutable operations,
so the expiry is derived, reproducible and rebuild-stable. A peer that has acknowledged nothing at
all is never expired — an unknown position is not evidence that a peer is gone.

## The two independent checks between a plan and a deleted byte

| # | Check | Where | What it catches |
|---|---|---|---|
| 1 | The reachability closure over the retained roots | `mesh_store::Reachability`, `mesh_store::CollectionPlan` | content any root reaches |
| 2 | A `ReferenceOracle`, consulted at the instant of deletion | `mesh_cas::Cas::collect` | a reference that appeared *after* the plan was computed, and a caller who supplied the wrong plan |

Check 2 is not optional and not a courtesy. `Cas::collect`'s signature requires an oracle, so there
is no path to the deleter that does not carry a veto. A veto is reported as `Collected::refused`,
never as an error — a reference appearing mid-collection is normal. A non-empty `refused` list is
nevertheless worth reading: it is either a workspace that moved, or a plan computed against the
wrong root set, and the second is the shape of the bug that loses data.

Two further refusals, both stated as behaviour rather than as advice:

* **An empty retained-root set is refused.** `CollectionPlan::compute` returns
  `RetentionError::NoRetainedRoots`. "Retain nothing" is never what a caller meant; it is what a
  caller gets when a root set failed to load, and obeying it would delete the workspace.
* **A root the index cannot resolve is an error, not a skipped root.** A root that resolved to
  nothing would silently shrink the retained set, which is exactly the failure mode the guarantee
  is about.

## What the collector deletes, and what it never touches

`Cas::collect` unlinks files under `chunks/<aa>/<bb>/` whose names are 64 hex characters, and
nothing else. `quarantine/` keeps evidence, `scratch/` is `Cas::discard_scratch`'s, and `logs/` is
rewritten by `ArrivalJournal::forget` but never removed.

The order is **delete, then forget**, and the asymmetry is deliberate:

* Forgetting first and crashing before the delete leaves a chunk in the store the cheap candidate
  path can no longer see — recoverable only by the full sweep.
* Deleting first and crashing before the forget leaves a stale `+` record for a chunk that is gone,
  which costs one `stat` on the next run and nothing else.

A collection interrupted halfway is a **smaller collection**, never a broken store: every chunk it
removed was one nothing referenced.

## Collection does not block a local write

`Cas::collect` takes `&self`, acquires no lock, and unlinks one file at a time. The proof is
structural rather than timed: `tests/collection-concurrency.rs` suspends a collection *inside* its
first `remove_file` and completes a whole promotion beside it, then checks that the promotion
returned before the collection had finished its deletions. A collector holding any exclusive hold
on the store would deadlock there.

The one race that remains is a promotion of a digest the collector is deleting at the same instant.
It is benign by construction: a doomed digest is one nothing references, so if the delete wins, the
writer re-promotes and the transaction that was going to reference it has not committed. If it had
committed, the oracle would have said referenced.

## The evidence

| Claim | Test |
|---|---|
| No content reachable from any retained root is collected | `crates/mesh-store/tests/gc.rs` — 200 generated workspaces, the collector's answer compared against a second, independently written fixed-point closure |
| The doomed set is exactly the candidates no root reaches | `gc.rs::the_doomed_set_is_exactly_the_candidates_no_root_reaches` |
| A chunk deduplicated between a dropped root and a retained one survives | `gc.rs::a_chunk_shared_by_a_dropped_root_and_a_retained_one_survives` |
| An offline peer's watermark keeps its content until the peer returns | `gc.rs::an_offline_peers_watermark_keeps_its_content_alive_until_the_peer_returns` |
| …or until the policy expires it, in epochs and not in seconds | `gc.rs::an_offline_peers_claim_expires_by_policy_epoch_and_never_by_a_clock` |
| A dry run reports what would go and why | `gc.rs::a_dry_run_reports_exactly_what_would_be_deleted_and_why`, `crates/mesh-cas/tests/collection.rs` |
| The collector actually frees disk | `crates/mesh-cas/tests/collection-footprint.rs` — a 1 048 576-byte store of 256 chunks, three quarters unreferenced, measured on the filesystem before and after: 786 432 bytes freed, 262 144 left, the disk delta equal to the collector's own count |
| Only content-named files are ever unlinked | `collection.rs::the_collector_only_ever_unlinks_a_content_named_file` |
| The oracle can veto at the instant of deletion | `collection.rs::the_oracle_vetoes_a_deletion_at_the_instant_it_would_happen` |
| The directory entry is synced after the unlink | `collection.rs::a_delete_run_syncs_the_directory_it_removed_from` |
| Collection does not block a write | `crates/mesh-cas/tests/collection-concurrency.rs` |

The safety property test is not written against the collector's own helpers. `gc.rs` carries a
second closure — a naive fixed point over every record, with no work queue and no root attribution
— and asserts the two agree in both directions. Removing the parent walk from
`Reachability::compute` turns three of those tests red, which was checked by making that mutation.

## What is assumed and not tested

1. **A single writer per workspace.** Plan §6.1 specifies one, and `Cas::discard_scratch` already
   depends on it. The collector inherits the assumption: two processes collecting the same store
   concurrently, or one collecting while another discards scratch, is outside what any test here
   covers.
2. **The candidate set is supplied by the caller.** The collector never enumerates the store to
   decide what to consider. `Cas::unreferenced_candidates` (the arrival journal, cheap) and
   `Cas::sweep_all_chunks` (the full sweep, the backstop) are the two sources, and choosing between
   them is the caller's. Feeding a *larger* candidate set can only find more garbage — a retained
   digest is kept whichever list it arrives on.
3. **No crash campaign covers collection.** `crates/mesh-cas/tests/crash-promotion.rs` kills a real
   process at every promotion step; nothing does the equivalent for a collection. The argument that
   an interrupted collection is a smaller collection is written above and is sound, but it is an
   argument, not a `SIGKILL` campaign, and this line is where that is recorded.
4. **Nothing schedules a collection.** There is no daemon loop, no trigger and no policy that
   decides *when* to run. This crate supplies the decision and `mesh-cas` supplies the deletion;
   `mesh-daemon` is where scheduling belongs and it does not call either yet.
5. **The manifest linkage.** This index holds no operation→manifest edge, so a manifest is reachable
   only when a root names it or when the derived payload-digest edge above applies. Until a real
   edge exists, `RetainedRoots::conservative` names every manifest, which is why the default
   collector frees crash residue and not much else.

## If the collector ever removes reachable content

That is a P0, and the task contract states the sequence: disable collection, ship the fix, then
re-enable. Disabling is one line — pass `CollectionMode::DryRun` — and the dry run is the same code
path, so a workspace under investigation still gets a full report of what *would* have gone.
