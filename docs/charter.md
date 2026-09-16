# Mesh product charter

> **Document role:** constitutional product authority and target-state constraints. This is not a
> current implementation inventory. Amend it through an explicit lifecycle decision rather than
> quietly rewriting it to match code; use [Project status](project-status.md) for observed
> delivery and limitations.

The charter is what a lane consults when a task's contract is silent. Everything here is a
**rule with an observable consequence** — if a rule and a shipped behaviour disagree, that is a
product bug or a charter amendment, decided in a decision doc, never resolved by quietly
softening the wording.

Normative source: the Mesh execution plan, §1 (strategic thesis), §2 (product principles) and
§3.6 (POC non-goals).

---

## 1. The problem

Existing systems divide work into the wrong primitives for an agent-heavy workload. Git is
optimized around explicit snapshots and repository operations. Git-backed agent tools capture
sessions and checkpoints on top of that model. The workload we are building for looks like this:

```text
One human
Several long-running interactive agents
Many short-lived worker agents
Validators
Remote peers
Frequent intermediate states
Constant file reads and writes
No natural human-created commit boundary
```

The foundational unit should therefore be **an authenticated actor's continuously evolving
private workspace state**. Publication is a separate operation.

## 2. The core strategic claim — stated as a falsifiable hypothesis

> **Hypothesis.** An actor-native workspace protocol has structural advantages that a Git-backed
> system cannot reproduce with a modest feature update — in actor creation cost, lock-free
> concurrent checkpointing, continuous private-state replication, identity stability across
> moves, binary delta size, exact input-version tracking, context invalidation,
> protocol-enforced publication authority, peer shadow views, and a taskless UX.

This is a hypothesis, not an established moat, and it is **not** assumed anywhere in this
repository.

**What falsifies it.** The benchmark programme (plan §12) publishes eight structural-win
criteria measured against real, same-hardware, same-day baselines — native filesystem, Git
worktrees, Jujutsu workspaces, folder replication, and accessible competitors. **Fewer than
three demonstrated wins falsifies the generic-infrastructure thesis**, and decision Gate D
(week 11) narrows the programme to one of four smaller shapes: agent workspace SDK,
context/read dependency system, approval security layer, or Git-compatible actor runtime.

The falsification test is a merged artifact, not a judgement call: see
[`docs/benchmarks.md`](benchmarks.md) for the methodology and the structural-win scorecard.

## 3. Positioning

| Audience | Statement |
|---|---|
| Technical | The real-time, actor-native workspace protocol for humans and agents. |
| Product | Run many agents in one project. See all their work. Nothing reaches the shared version until you approve it. |
| Open source | A safe local workspace for unlimited agents, with automatic versioning and no branches or commits. |
| Paid team | Remote collaboration, private actor-state synchronization, approval, recovery, and governance for human–agent teams. |

The initial audience is behaviourally narrow — **people already running two or more coding
agents against one project** — while the architecture and UX stay generic. First users are
technical because they can install, benchmark, report failures and tolerate an alpha. That is a
statement about who adopts first, not about who the product is for.

---

## 4. The ten constitutional principles

Each is a rule. Each has an observable consequence that makes a violation detectable.

### P1 — Work is automatic; publication is explicit

**Rule.** The user never has to create a task, a branch, a commit, a checkpoint, a proposal or a
merge request. Mesh creates internal sessions, checkpoints and review bundles automatically. A
human must explicitly approve advancement of the protected shared version.

**Observable consequence.** The primary journey contains zero ceremony steps. Any flow that
requires the user to name or create a unit of work violates this principle and is a defect.

### P2 — Every actor works privately

**Rule.** Every actor sees the latest approved shared state plus that actor's own private
changes. No agent writes directly into the canonical shared state.

**Observable consequence.** An actor's write is never visible in another actor's view before
approval. Isolation is asserted by test, not by convention.

### P3 — Private state is durable and remotely visible

**Rule.** An agent's stable work can be synchronized and inspected without being published.
**Replication is not publication.**

**Observable consequence.** A peer can open another actor's complete state read-only, and doing
so alters neither that actor's work nor the canonical head.

### P4 — Local work never waits for the WAN

**Rule.** Ordinary reads and writes complete locally. Network synchronization is asynchronous.

**Observable consequence.** With the network unavailable, local reads and writes complete at
local latency and no operation blocks. Asserted by a test that runs with the relay unreachable.

### P5 — No valid work is silently discarded

**Rule.** Concurrent edits may conflict, but every durable version remains reachable until an
explicit retention policy permits deletion.

**Observable consequence.** No conflict resolution deletes a durable version, and the garbage
collector is provably conservative against the retained-root set. A collector that removes
reachable content is a P0.

### P6 — Paths are not file identities

**Rule.** A rename or move does not sever history and is never converted into an inferred
delete-and-create.

**Observable consequence.** A rename concurrent with an edit yields one object carrying both
changes. A rename is presented to a reviewer as a rename. Object IDs survive arbitrary
rename/move chains.

### P7 — Approval refers to exact bytes

**Rule.** An approval is bound to the exact actor state reviewed, the exact canonical base, the
exact selected changes, the exact conflict resolutions, and the validation evidence shown to the
approver.

**Observable consequence.** Modifying any bound field invalidates the signature. A stale base
fails compare-and-swap rather than rebasing bytes the human did not see. Later actor work cannot
enter an existing review bundle.

### P8 — The complete workspace never enters model context automatically

**Rule.** The harness supplies bounded, versioned context selected for the current action.

**Observable consequence.** No unbounded file read can enter a model call through any provided
API. An oversized request is refused or explicitly chunked — **never silently truncated**, which
is the failure mode that produces confidently wrong answers.

### P9 — Git is an adapter

**Rule.** For code projects, Mesh imports from and exports to Git. Git is not the internal state
model.

**Observable consequence.** An exported Git tree hash equals the materialized Mesh tree hash.
Git's own internal `.git` churn never appears as workspace change. The Git bridge is optional
and no core crate depends on it.

### P10 — Reliability claims must be measurable

**Rule.** Do not market "safe", "fast" or "lossless" without reproducible tests and explicit
semantics.

**Observable consequence.** Every reliability adjective in user-facing material, documentation or
marketing links to the test, benchmark or simulator campaign that supports it. A claim without a
citation is cut, not qualified. This charter obeys its own rule: every quantitative statement
below points at the artifact that establishes it.

---

## 5. What the words mean to a user

The internal model is a version graph. The user model is six words. The graph must never leak
into a user-facing surface — not in a label, not in an error message, not in onboarding.

| Internal state | User-facing wording |
|---|---|
| Actor working head | Their work |
| Canonical head | Shared version |
| Durable actor checkpoint | Saved privately |
| Replicated actor checkpoint | Available to team |
| Review bundle | Ready for review |
| Publish operation | Approve to shared version |
| Conflict | Needs review |
| Historical state | Earlier version |

User-facing status is exactly: **Working · Saved privately · Available to team · Ready for
review · Needs attention · Approved.**

Never exposed to a user: `DAG · frontier · vector clock · branch · commit · rebase · staging ·
ref · operation log`. These words are correct and required in the protocol and consistency
documents; they are forbidden in product surfaces, and the prohibition is enforced by a build
lint rather than by review.

A concept that cannot be expressed in these six words is a product design question for the
product owner — never a seventh word added locally.

---

## 6. POC non-goals

The public POC does **not** promise, and no task may assume:

- complete Windows support;
- whole-disk filesystem replacement;
- byte-by-byte collaborative editing;
- automatic semantic merge of arbitrary binary files;
- Byzantine consensus between mutually hostile peers;
- zero-knowledge hosted storage;
- enterprise SSO and SCIM;
- legal hold;
- all Office and Adobe semantics;
- perfect range-level read tracking for unintegrated applications;
- global cross-customer deduplication;
- a production SLA.

Attribution honesty is a non-goal boundary too: where read tracking is inferred rather than
exact, it is recorded at reduced confidence. Overstating what the system knows violates P10.

---

## 7. What this charter constrains

An engineering decision that contradicts a principle here is escalated, not absorbed. The
accountable human owners (plan §14.1) are:

- **Protocol and correctness** — semantics, formal model, operation graph, conflict rules,
  durability, security invariants.
- **Runtime and platform** — content store, local database, synchronization, filesystem
  adapters, performance, packaging.
- **Product and integrations** — desktop UX, agent adapters, Git bridge, telemetry, design
  partners, documentation.

One person may hold two roles, but every decision has exactly one accountable human.

Amendments to this charter require a reviewed architecture decision recorded before the
behaviour changes, never after.
