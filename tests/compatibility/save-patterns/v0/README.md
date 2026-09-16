# `mesh-save-patterns/0` — the save-pattern corpus

What real tools do to a folder when a person saves, and what the single meaningful durable change
is in each case. Research item R5, plan §11; task `01KZC2VTSZCB4Q7DC9C9X5NM1V`.

The decision this corpus exists to support is
[ADR-0039](../../../../docs/adr/0039-anchor-a-checkpoint-on-the-last-boundary-a-backend-can-prove-and-close-the-window-on-quiescence.md).
The consumer is `01KZC2YXH6FG9C67JP9DQX7JH6`, whose own `## Automated validation` already names
`cargo nextest run --test save-patterns`.

**Status, stated before anything else.**

| Half | Where | State |
|---|---|---|
| The corpus, the classification and the rule | this directory | **complete — 12 of the 12 required cells.** Sixteen patterns: fourteen captured (eight darwin, six linux), one derived from a shipped binary, one derived from documentation. The capture matrix is six families × two platforms. Real VS Code GUI saves fill both platform cells, a real IntelliJ IDEA GUI save fills JetBrains Linux, and an observed execution of IntelliJ IDEA's own command-line formatter fills JetBrains macOS without claiming GUI input. `contract-check.mjs` fails if a cell claims an observed capture it does not have. |
| The corpus replay and schedule oracle | [`replay.mjs`](replay.mjs) | **executable.** `node tests/compatibility/save-patterns/v0/replay.mjs` checks every stream, every declared trigger at every admissible position, admissible folder omissions, two-process replay and two mutations. |
| The consuming Rust oracle | a test target named `save-patterns`, in a crate `01KZC2YXH6FG9C67JP9DQX7JH6` owns | **does not exist.** `cargo nextest run --test save-patterns` still has no target. |

The second row is the honest form of "replayable" available inside this task's boundary: every
pattern is a total, ordered, clock-free input and the checked reference rule consumes it. The third
row remains unmet because `01KZC2VTSZCB4Q7DC9C9X5NM1V`'s allowed paths are
`tests/compatibility/save-patterns/**` and `docs/adr/**`; registering a Cargo target requires a crate
the consuming task already claims. Section 8 records that boundary rather than smoothing it over.

---

## 1. Why this is a corpus and not a rule of thumb

Three things were true before this corpus and are measurements now.

**A save is not one write.** `sed -i.bak` on a four-byte substitution produced five distinct
filesystem states. `npm install` of one local tarball produced 12,225 changes over 5,662 paths.

**A save passes through states that never existed as a save.** vim 9.0 and rustfmt 1.9.0 both
truncate the document to length zero and then rewrite it, keeping the same object identity. That
zero-length state was captured, at a named inode, on this host. A checkpoint taken there is a
durable record of a person having emptied their own file.

**The dangerous state is not always in the middle of a write.** In `sed -i.bak` and in the
JetBrains safe-write shape, the original is renamed *aside* before the temporary is renamed *into
place*. Between those two renames the document's name is bound to nothing. A checkpoint there
records a deletion nobody performed.

A coalescing rule written from an assumed pattern would have got the first of those and missed the
other two.

---

## 2. What a pattern file is

One JSON object per file under `patterns/`, indexed by [`catalogue.json`](catalogue.json).

```jsonc
{
  "contract": "mesh-save-patterns/0",
  "pattern": "sed-rename-over",
  "tool": { "name": …, "version": …, "platform": … },
  "provenance": { "method": …, "command": [...], "evidence": …, "blind_spots": [...] },
  "objects": [ { "object": 1, "role": "the document" } ],
  "streams": {
    "mount":  { "events": [ { "sequence": 1, "kind": "Opened", "object": 2 }, … ],
                "candidates": [ { "through": 3, "reason": "Closed" }, … ] },
    "folder": { "events": [ … ], "candidates": [], "admissible_omissions": [[1]] }
  },
  "expected": {
    "mount":  { "checkpoints": [ { "from": 1, "through": 4, "reason": … } ],
                 "boundary_evidence": { "through": 4, "confidence": … } },
    "folder": { "checkpoints": [ … ], "boundary_evidence": { … } },
    "meaningful_sequence_point": { "mount": 4, "folder": 3 },
    "meaningful_durable_change": "…",
    "forbidden_sequence_points": { "mount": [1,2,3], "folder": [1,2] },
    "coalescable": "safely" | "only-by-quiescence" | "not-on-a-folder-backend"
  }
}
```

**Two streams per pattern, and that is the point.** `mesh-workspace-adapter/0` has a capability a
backend either declares or does not, `ObserveDurableBoundary`, and the two shipped backends land on
opposite sides of it. `mesh-fuse` declares it — a session is handed open, flush, fsync and release
by the kernel — and grades the `BND` family 2 pass / 0 fail / 0 unsupported. The folder-watching
fallback in `mesh-daemon` declares it and refuses nothing: `FALLBACK_CAPABILITIES` is built
`.without(AdapterCapability::ObserveDurableBoundary)`, so `BND` grades 0 / 0 / 2. Its user-facing
sentence says the same thing in words: *"Mesh cannot tell from this folder alone when an
application finished writing a file."*

So every pattern carries the stream each fidelity can actually produce, and an expected result for
each. A corpus with one stream would silently be a corpus about one backend.

**Nothing here carries a clock.** `FsEvent` has no timestamp field and cannot acquire one; order is
`EventSequence`, minted per view. A pattern is replayed by feeding its events in sequence order and
firing the idle trigger once at end of stream. No timer, no wall-clock, no sleep.

---

## 3. The vocabulary this borrows, and does not extend

| Field | Closed list | Where |
|---|---|---|
| `kind` | `Opened` `Written` `Flushed` `Synced` `Closed` `Renamed` `Unlinked` `MetadataChanged` | [`../../adapter/v0/vocabulary.json`](../../adapter/v0/vocabulary.json) `event_kinds` |
| `reason` | `Closed` `Synced` `RenamedIntoPlace` `MetadataSettled` | same file, `boundary_reasons` |
| `boundary_evidence.confidence` | `ExactIntegratedRead` `ExactFilesystemRange` `FilesystemReadAhead` `ProcessInferred` `RecoveryDetected` `Unknown` | `mesh_materializer::AttributionConfidence::ALL` (plan §4.8) |

A pattern naming anything outside those three lists is a defect in this corpus. **No new vocabulary
is introduced by R5.** `boundary_evidence` records where the latest offered candidate came from; it
is not a field of the emitted checkpoint. This separation matters when cleanup follows the last
candidate: `jetbrains-safe-write` emits through 8 while its exact boundary evidence ends at 7.
Whether a published checkpoint eventually carries any confidence field remains undecided (§9).

---

## 4. The rule

Named: **candidate-recorded, settling-closed, view-scoped coalescing.** Decided in ADR-0039;
stated here in the form an implementation is graded against.

### 4.1 The one sentence everything else follows from

**A checkpoint is a state at one sequence point of one view, not a set of edits.**

Coalescing therefore only ever chooses *which sequence points get a checkpoint*. It cannot
synthesise a state that did not exist, it cannot mis-attribute one object's bytes to another
object's name, and merging two unrelated saves into one checkpoint loses nothing. That asymmetry is
the whole argument for merging too much rather than too little: an over-merged checkpoint is a real
state that is coarser than someone might have wanted, and an under-merged one can be a durable
record of a file the person still has.

### 4.2 The state machine

One window is open at a time, per view. `EventSequence` is the only order.

| In state | On | Do |
|---|---|---|
| no window | any event at `s` | open a window: `from = s`, `last = s`, `candidate = none` |
| window open | any event at `s` | `last = s` |
| window open | a `CheckpointCandidate` offered at `s` | record `candidate = s` and `reason = candidate.reason` as boundary evidence; do not close the window |
| window open | settling established after the final observed event | emit the one meaningful checkpoint `[from, last]`, carrying the latest candidate reason when one exists. No window. |
| window open | any recovery-only trigger at `s` | preserve only the state that exists through `s`; emit no meaningful checkpoint and keep the window open. How preservation advances private state is unresolved (§9). |

The nine trigger conditions of plan §4.5, with the schedule this corpus can actually grade:

| Effect | Trigger conditions | Admissible positions in this corpus |
|---|---|---|
| meaningful checkpoint | actor becomes idle after settling | after the final observed event only |
| recovery preservation only | integrated agent requests a flush · actor process exits · maximum uncheckpointed bytes or time is reached · user opens review · actor disconnects | after every event |
| boundary evidence only | modified file handle closes · `fsync` completes · atomic replacement completes | the matching recorded event |

Every effect is prefix-bounded. A process exit after JetBrains sequence 6 can emit only `[1, 6]`,
not `[1, 8]`; sequence 8 has not happened. Because `[1, 6]` is a forbidden deletion state, process
exit is recovery-only. The oracle's process-exit mutation promotes it back to a meaningful closer
and rejects 73 forbidden emissions, including that exact counterexample. The candidate-anchor
mutation rejects another 24. A candidate proves that one handle crossed a boundary, not that the
whole view settled.

### 4.3 What each clause is there to survive

| Clause | The pattern that forces it |
|---|---|
| a candidate is evidence, not a safe cut | `jetbrains-safe-write`: candidates occur at a temporary sync, temporary close, deletion window and replacement; the first three are forbidden |
| every recovery-only trigger defers the meaningful checkpoint | `git-checkout-switch-branch` and `npm-install-cold`: even an offered candidate can be in the middle of a multi-object rewrite |
| established settling emits at `last`, not at the candidate | `sed-rename-over-with-backup`, `jetbrains-safe-write` and `vim-in-place-truncate`: cleanup follows the last candidate |
| a trigger sees no future event | `jetbrains-safe-write`: process exit at sequence 6 has the real prefix `[1, 6]`, not the final `[1, 8]` |
| the window spans **objects**, not one file | every atomic-replace pattern touches two objects, and `git-checkout-switch-branch` touches three |
| the window never spans **views** | `EventSequence` is per view and is not comparable across views (`mesh-workspace-adapter/0` §6) |
| recovery preservation is not called a meaningful checkpoint | the corpus proves no safe mid-stream point for several patterns; plan §3.5 ACT-004 still requires a separate ratified recovery path (§9) |

### 4.4 The invariants, each with the run that refutes it

| # | Invariant | Enforcer |
|---|---|---|
| I1 | For every pattern and every stream, each declared trigger at each admissible position has its declared prefix-bounded effect; established settling emits exactly `expected.<stream>.checkpoints`. | `node tests/compatibility/save-patterns/v0/replay.mjs`, one case per trigger position per stream |
| I2 | `forbidden_sequence_points` is the complete complement of `meaningful_sequence_point`, and no meaningful schedule emits any member of it. | the same oracle; both the candidate-anchor and process-exit-as-meaningful mutations must be rejected |
| I3 | Replay is identical: the same input yields the same output, in one process twice and in two processes once. | the same oracle; it starts a second Node process, per ADR-0013 |
| I4 | Every checkpoint lies inside exactly one named stream. | the same oracle |
| I5 | Dropping any subset in `streams.folder.admissible_omissions` still yields exactly one settled checkpoint, ending at the last surviving event. | the same oracle |

All five run in the corpus-local oracle. The contract-declared Cargo target remains absent; §8.

---

## 5. How the corpus was captured

Six observed patterns were captured on **darwin 14.5, arm64, APFS**, two newer editor patterns
(VS Code and IntelliJ IDEA's command-line formatter) on **darwin 25.6.0, arm64, APFS (macOS
26.6.1)**, and six on **linux
6.10.14, aarch64, Debian 12, overlayfs**, by running the tool in a temporary directory while a loop
re-read the whole tree and recorded, per entry, the inode, the size, the link count and whether it
is a directory. A change is an entry appearing, vanishing, keeping its inode with a different size,
or **keeping its name with a different inode** — which is what makes a rename over a target visible
without a syscall trace.

That loop is now `capture.mjs` in this directory rather than the ad-hoc script it started as. It
was committed because the Linux half could not otherwise be captured the same way as the darwin
half, and a corpus whose two platforms were gathered by two different methods cannot be used to
compare them — which is exactly what §5.1 does. Re-run any cell with:

```console
$ node tests/compatibility/save-patterns/v0/capture.mjs --root <dir> --out <file> -- <tool> [args]
```

The Linux VS Code cell uses the pinned official arm64 package and an Xvfb display. Build
`Dockerfile.vscode-linux`, then run the capture container with `--network none`; the driver uses
Ctrl-S and the same fresh-profile, owned-endpoint and exact-final-byte checks as the macOS driver.
The JetBrains Linux cell is equally pinned by `Dockerfile.jetbrains-linux`: the driver starts an
owned Xvfb and IntelliJ IDEA child, uses a fresh profile, title-validates the agreement and data
sharing dialogs, declines telemetry, drives only the supplied LightEdit document, and terminates
and waits for both children before removing the profile. Its seven real captures also ran with
`--network none`.

```console
$ vim -e -s -c 'set backup backupcopy=no' -c 'normal! ggdGishort' -c wq doc.txt
$ sed -i '' 's/alpha/ALPHA/' doc.txt
$ sed -i.bak 's/alpha/ALPHA/' doc.txt
$ cargo fmt
$ git checkout other
$ npm install --offline --no-audit --no-fund ./dep.tgz
```

**The method has exactly the folder-watching backend's blind spots, deliberately.** It cannot see
open, flush, fsync or close, and anything created and removed between two readings is invisible to
it. That is why every `streams.folder` in this corpus is marked `observed` and every `streams.mount`
is marked `derived`: the mount fidelity is the POSIX call sequence the captured name and inode
transitions imply, not a trace. A run with a syscall tracer, or with a real FUSE mount, can replace
those derivations with observations, and should.

### 5.1 The same tool does not behave the same way on both platforms

Three of the four families captured on both platforms were run with the **same command, the same
flags and — for the formatter — the same build**. Two of them diverged:

| Family | darwin | linux | Same? |
|---|---|---|---|
| a formatter (`cargo fmt`, rustfmt 1.9.0-stable both sides) | one inode, 42 → 0 → 52 | one inode, 37 → 0 → 52 | **yes** |
| vim 9.0, `set backup backupcopy=no` | one inode, 23 → 0 → 6, no backup in the tree | **inode replaced**, original renamed aside to `doc.txt~` | **no** |
| `git checkout <branch>` | `a.txt` vanishes and reappears at a **new** inode | `a.txt` keeps its inode and is rewritten in place | **no** |

The formatter matching exactly is what makes the other two readable. If every Linux capture had
diverged, the honest conclusion would be that the capture method behaves differently per platform.
One family matching byte-for-byte while its neighbours invert locates the difference in the tools.

**This falsifies any coalescing rule inferred from the darwin corpus alone.** `01KZC2YXH6FG9C67JP9DQX7JH6`'s
failure-and-recovery line *never coalesce across a rename that changes object identity* does not
fire once on darwin vim and fires on every Linux vim save — so a rule validated only on darwin
would read as correct while never having been exercised. The two platforms also invert which
hazard they present: darwin risks checkpointing a file that is momentarily **empty**, Linux risks
checkpointing one that is momentarily **absent**.

The editor evidence now separates one observation from two supplements:

- **`vscode-macos-in-place-truncate`** — observed three times through the real VS Code 1.132.1
  document editor, driven through a fresh-profile Chrome DevTools Protocol session. Every run
  preserved one inode and exposed 24 → 0 → 38 bytes. The committed driver rejects the run unless
  the final bytes are exact. It launches the app binary as an owned child, discovers Chromium's
  OS-assigned loopback endpoint through that fresh profile's `DevToolsActivePort`, validates the
  supplied document before input, and waits for the exact child to exit before deleting the profile.
  Its self-test refuses caller-selected and unrelated endpoints, bounds every fetch/command, and
  closes a fake transport with a command in flight to prove child exit precedes directory cleanup.
- **`vscode-linux-in-place-truncate`** — observed four times through the official VS Code 1.133.0
  arm64 Debian package under Xvfb with the capture container network disabled. Every run preserved
  one inode and exact final bytes. One run exposed 24 → 0 → 38; three saw 24 → 38 because truncation
  and rewrite completed between readings. That difference is the folder backend's declared
  admissible omission measured in the same editor rather than assumed.
- **`vscode-atomic-replace`** — the older supplementary derivation from the shipped 1.132.0 bundle,
  which asks for the atomic write by name: `enforceAtomicWriteFile(e){…?{postfix:".vsctmp"}:!1}`.
  The observed document-save path did **not** take this shape on the measured host. The derivation
  remains evidence for an unexercised path or configuration, not for the macOS observed cell.
- **`jetbrains-safe-write`** — derived from documentation. The original GUI probe launched the
  official checksum-verified 2026.2.1 arm64 image, but this automation context was denied both
  Screen Recording and Apple Events to System Events. It could not prove which editor received
  input or inspect the first-run UI and remains uncounted. The separate command-line formatter
  observation below now fills the JetBrains/macOS matrix cell without relabelling that failed GUI
  probe; this documentation derivation still has no local observation of its own and carries its
  own falsifier.
- **`jetbrains-macos-command-line-format-backup-in-place-truncate`** — observed in two repetitions through the
  official IntelliJ IDEA 2026.2.1 arm64 command-line formatter from a SHA-256-verified, read-only
  disk image. The formatter used one isolated IDEA profile and factory defaults. Both polling
  captures kept `Example.java` on the same inode and recorded a 56 → 86-byte size change. The
  retained output after the second repetition was the exact 86-byte formatted source; the captures
  did not hash document content, so cross-repetition byte equality is not claimed. One
  polling run saw only the final resize; the other saw a short-lived `Example.java~` grow 0 → 56,
  the document shrink 56 → 0 and grow 0 → 86, then the backup vanish. The backup's content was not
  read. This is a real IDEA formatter/save execution and fills the product-family/platform cell,
  but it is deliberately not described as a GUI LightEdit save.
- **`jetbrains-linux-backup-in-place-truncate`** — observed seven times through the official
  IntelliJ IDEA 2026.2.1 aarch64 archive in LightEdit mode under Xvfb and network isolation. Every
  run created a 24-byte `doc.txt~` whose content the metadata poller did not read, rewrote
  `doc.txt` from 24 to 40 exact bytes on
  the same inode, and removed the backup. Four polling runs caught the document at 0 bytes; three
  missed that unsafe intermediate state. This directly falsifies the older two-rename derivation
  for the measured path without generalising to other JetBrains products or configurations.

---

## 6. The classification

Twelve classified patterns, four families, and the single meaningful durable change for each.

| Pattern | Family | Single meaningful durable change | Coalescable |
|---|---|---|---|
| `vscode-macos-in-place-truncate` | in-place truncate and rewrite | the document holds the edited text in the same object identity | **not on a folder-watching backend** |
| `vscode-linux-in-place-truncate` | in-place truncate and rewrite | the document holds the edited text in the same object identity | **not on a folder-watching backend** |
| `vscode-atomic-replace` | atomic replace, one rename | the document holds the edited text and no `.vsctmp` exists | safely |
| `jetbrains-safe-write` | atomic replace, two renames | the document holds the edited text; neither temporary nor displaced original exists | safely |
| `jetbrains-macos-command-line-format-backup-in-place-truncate` | backup plus in-place truncate and rewrite | the formatted document holds the exact result in the same object identity and `Example.java~` is gone | **not on a folder-watching backend** |
| `jetbrains-linux-backup-in-place-truncate` | backup plus in-place truncate and rewrite | the document holds the edited text in the same object identity and `doc.txt~` is gone | **not on a folder-watching backend** |
| `sed-rename-over` | atomic replace, one rename | the document holds the substituted text and no temporary exists | safely |
| `sed-rename-over-with-backup` | atomic replace, two renames | the document holds the substituted text **and** the backup holds what it replaced | safely |
| `vim-in-place-truncate` | in-place truncate and rewrite | the document holds the edited text and no swap file exists | **not on a folder-watching backend** |
| `rustfmt-in-place-truncate` | in-place truncate and rewrite | the source file holds the formatted text | **not on a folder-watching backend** |
| `git-checkout-switch-branch` | multi-object tree rewrite | the whole working tree matches what was asked for | **only by quiescence** |
| `npm-install-cold` | multi-object tree rewrite, unbounded | the dependency tree **and** the lockfile that describes it | **only by quiescence** |

Two of the four families are the ones an assumed pattern would predict. The other two are the
findings.

**Family 2, in-place truncate and rewrite, is not one tool's quirk.** VS Code, vim and rustfmt do it
from unrelated code on the measured macOS hosts, because it is what truncate followed by write
does. It is very
likely the *most common* save shape in the corpus's real population, and it is the one with no
temporary file, no rename, and therefore no observable landmark of any kind on a backend that
cannot see close.

**Family 4 has no meaningful sub-unit at all.** Half a checkout and half an install are real states
and neither is a save.

---

## 7. Patterns that cannot be coalesced safely

Acceptance criterion 4 of `01KZC2VTSZCB4Q7DC9C9X5NM1V`. Four entries, each recorded as a known
restriction rather than approximated.

**1. In-place truncate and rewrite, on a backend that cannot see close.**
`vscode-macos-in-place-truncate`, `vscode-linux-in-place-truncate`,
`jetbrains-macos-command-line-format-backup-in-place-truncate`,
`jetbrains-linux-backup-in-place-truncate`, `vim-in-place-truncate` and
`rustfmt-in-place-truncate` on `streams.folder` contain **no candidate
at any sequence**. The window can only be closed by quiescence, and the document is observably
empty part-way through. If the process dies there, the last durable state is an empty document and
nothing in the stream says a save was in flight. This is not a defect to be fixed by a better rule:
it is `FALLBACK_NO_SAVE_POINT_FROM_THE_FOLDER`, already declared and already shipped to a person,
met from the other side. The rule's answer is to mark the checkpoint `RecoveryDetected` and never
`ExactFilesystemRange`.

**2. A tree rewrite with a boundary in the middle of it.** `git-checkout-switch-branch` offers a
candidate at mount sequence 4 — one file was genuinely closed — and sequence 4 is a half-switched
tree. A candidate is *not sufficient*; only established settling bounds the operation. A
recovery-only trigger there may preserve recovery data but cannot claim a meaningful save.

**3. An install with no quiescence for the whole of its length.** `npm-install-cold` ran 12,225
changes without a gap. The byte and time triggers exist precisely for recovery durability, and
every meaningful split they might force lands on a forbidden sequence point because every point
except the last one is forbidden. The corpus therefore asserts that every recovery-only trigger
defers the meaningful checkpoint; how recovery data is preserved without claiming completion is a
separate unresolved decision.

**4. Work that is created and removed between two readings.** Not in the corpus, and cannot be: it
is the folder-watching backend's second declared restriction, and there is no event stream to
record. *"Mesh will not claim it was there and will not claim it was not."*

---

## 8. What this corpus does not establish

- **The declared Cargo target does not exist.** The corpus-local `replay.mjs` executes I1–I5 and
  rejects the unsafe candidate-anchor and process-exit-as-meaningful mutations. It is not the contract's named command:
  `cargo nextest run --test save-patterns` still has no target. Registering that target and grading
  the Rust implementation remain work for `01KZC2YXH6FG9C67JP9DQX7JH6` inside its crate paths.
- **Six of twelve classified patterns remain single-host, single-run.** The VS Code patterns,
  JetBrains macOS command-line formatter and JetBrains Linux LightEdit were repeated (three macOS
  VS Code, four Linux VS Code, two macOS IDEA formatter and seven Linux IDEA LightEdit), but each
  still covers one kernel and filesystem. A
  tool's behaviour on a case-folding APFS volume is not its behaviour everywhere;
  `01KZG5ASCKFMWQDJY34WCNTCCV` is the standing evidence that this volume is not a neutral observer.
- **Every `streams.mount` is derived.** The mount fidelity is inferred from name and inode
  transitions. No FUSE mount, no syscall trace, no kernel was involved in producing any event in
  this corpus.
- **`npm-install-cold` is an excerpt.** 15 events stand for 12,225. Its totals and its re-capture
  command are in the file; nothing else in the corpus is abridged.
- **No timing, anywhere.** No interval, no latency, no throughput. Plan §2.10 forbids a performance
  claim without a number, and the honest response was to make no such claim: the rule is clock-free
  by construction and the intervals it does need are named in §9 as undecided.
- **It grades a rule, not a materialiser.** What a checkpoint *contains* at a sequence point is
  `mesh-store`'s and `mesh-materializer`'s business. This corpus only says where the sequence points
  should be.

---

## 9. What this run could not decide

Each of these is a decision that is not ratified anywhere in the repository. Named, with the party
who owns it, per plan §14.1.

| # | The decision | Why this run could not make it | Owner |
|---|---|---|---|
| 1 | Whether `FsEvent` carries the name and parent of a `Renamed` event. | It carries `{ view, sequence, kind, object }` and no name, so a coalescer cannot tell an atomic replace from an unrelated move. `01KZC2YXH6FG9C67JP9DQX7JH6`'s failure-and-recovery line — *"never coalesce across a rename that changes object identity"* — is therefore **not implementable at `mesh-workspace-adapter/0` today**. The rule in §4 does not need it; that line does. Changing the published event shape is a contract change, and plan §14.3 rule 3 forbids the same run specifying and implementing one. | protocol and correctness |
| 2 | The idle interval, the maximum uncheckpointed bytes and the maximum uncheckpointed time. | Every one of them is a number, and plan §2.10 forbids a number without a measurement. The measurement is a benchmark against `npm-install-cold` at full scale that nobody has run. | runtime and platform |
| 3 | Whether a `Checkpoint` carries an `AttributionConfidence` at all. | No published checkpoint type has such a field. This corpus keeps confidence only in `boundary_evidence`, separate from the expected checkpoint, so it does not answer that protocol question by accident. | protocol and correctness |
| 4 | How a mid-stream or shutdown recovery-preservation write advances private state, and how its uncertainty reaches a person. | The corpus proves candidates are not safe meaningful-save cuts, while plan §3.5 ACT-004 still requires stable work to survive daemon failure. No ratified type separates preserved recovery data from a meaningful checkpoint. | protocol and correctness, with product and integrations for the person-facing sentence |
| 5 | Whether `BoundaryReason::MetadataSettled` is ever offered. | No backend offers it. `mesh-fuse` explicitly declines to, on the ground that no single event can say metadata stopped changing. It is in the closed list with no producer. | protocol and correctness |

---

## 10. The contract the implementing lane builds against

For `01KZC2YXH6FG9C67JP9DQX7JH6`, whose allowed paths are `crates/mesh-store/**` and
`crates/mesh-daemon/**`. This section is the interface; §4 is the behaviour.

**The shape.** Coalescing is one pure function and one accumulator, and neither reads the world:

```rust
pub fn coalesce(
    events: &[FsEvent],                 // one view, ascending EventSequence
    candidates: &[CheckpointCandidate], // possibly empty — a folder-watching backend offers none
    triggers: &[(EventSequence, Trigger)], // where each trigger fired in the stream
) -> Vec<Checkpoint>;
```

`Checkpoint` is new and carries `{ view: ViewId, from: EventSequence, through: EventSequence,
reason: Option<BoundaryReason> }`. `reason` is `None` exactly when no candidate was absorbed.
`AttributionConfidence` is deliberately absent: §9 records that as unratified, and the corpus keeps
candidate provenance in `boundary_evidence` instead of silently deciding the wire shape. `Trigger`
is the nine conditions of plan §4.5 minus the three boundary-evidence conditions in §4.2, each
tagged meaningful-after-settling or recovery-only. The function emits only meaningful checkpoints;
the separate recovery-preservation result required by the other five conditions needs the decision
in §9 before implementation.

**Where it goes.** In `mesh-store`, not `mesh-daemon`: the daemon owns the *sources* of triggers —
timers, byte counters, connection state — and the function must stay testable without any of them.
The daemon's job is to observe and to call.

**Forbidden inside `coalesce` and everything it calls.** No `std::time`, no `SystemTime`, no
`Instant`, no `std::fs`, no `std::net`, no `std::process`, no thread, no allocation of a clock by
any other name. `mesh-materializer` already refuses all four under `tests/no_ambient_io.rs` and the
same refusal has to hold here, because the whole reason the corpus replays deterministically is
that nothing in the rule can read anything the corpus does not contain.

**Allowed dependencies.** `mesh-materializer` for `FsEvent`, `CheckpointCandidate`,
`BoundaryReason`, `ViewId` and `EventSequence`; `mesh-types` for identifiers. Nothing third-party —
`tools/program/arch-check/architecture.json` decides this and a new edge is registered there or it
is not legal.

**Error behaviour.** `coalesce` returns no error, because every input it can be handed has a right
answer: an empty stream is no checkpoints, a stream with no candidate and no trigger is no
checkpoints, and a candidate naming a sequence outside the stream is the *caller's* defect and is
where a `debug_assert` belongs. A function that could refuse would have to be handled at every call
site for a case the type system already prevents.

**What the harness must be.** The corpus-local `replay.mjs` is the executable reference and mutation
oracle. The consuming task still owes a test target named `save-patterns`, reachable as
`cargo nextest run --test save-patterns`, that reads every file under `patterns/`, grades the Rust
implementation against the same five invariants, and rejects both reference mutations.

**Who writes what.** Plan §14.3 rule 4: the run that writes `coalesce` must not be the run that
writes its oracle. This corpus is the oracle and it is already written by a different run. The
harness that reads it is a third thing, and it is the one place the two could quietly become one —
a harness author who finds a pattern inconvenient can weaken it in the same pass. **Changing a file
under `patterns/` and `coalesce` in one pull request is the shape to refuse.** Naming the role that
implements any of this belongs to the caller who spawns it (ADR-0004), not to this page.

---

## 11. Reporting a defect

A pattern you believe is wrong is worth more than a workaround, and there are two distinct ways for
one to be wrong: the **capture** can be wrong, or the **expectation** can be wrong.

- A capture is refuted by a better capture. Re-run the command in §5 on your host, or better, with a
  syscall tracer or a real mount, and file a `kind: bug` task with the new reading. A pattern whose
  provenance is `derived-from-documentation` is refuted the first time anyone observes the tool.
- An expectation is refuted by an argument, and it is settled through protocol review — **never by
  removing the pattern.** A coalescer that fails a pattern here is either a coalescer bug or a
  corpus bug, and deciding which is the whole job.
