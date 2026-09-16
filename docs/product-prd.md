# Mesh product requirements

> **Document role:** normative target-state product requirements. A requirement here does not
> prove that its user journey exists today. Requirement or vocabulary changes require reviewed
> product work and the executable lint below; use [Project status](project-status.md) for current
> maturity.

The internal model is a version history with many concurrent writers. The user model is six
words. This document writes both down in one place, with the mapping between them, because the
mapping is the only thing that stops the first from leaking into the second later — in a label,
an empty state, an error message or an onboarding step.

Normative source: the Mesh execution plan §3 (master PRD) and §3.4 (user-visible state model).
Constitutional rules live in [`docs/charter.md`](charter.md); this document is what those rules
require the product to *do*.

**This document is executable.** `tools/program/vocab-lint/lint.mjs` reads the tables below and
fails the build when the vocabulary, the state mapping, the six status words, the ceremony-free
journey or the requirement inventory drift from what is written here. Editing a table is a
product decision; the lint makes sure it is a *visible* one.

```bash
node tools/program/vocab-lint/lint.mjs --user-facing   # the gate
node tools/program/vocab-lint/lint.mjs --list-words    # the vocabulary, with replacements
```

---

## 1. Target users

### 1.1 Initial user

<!-- vocab-lint:allow reason="names the per-agent isolation tools the initial user is leaving behind; the tools' own vocabulary is the only accurate way to describe them" -->

A developer, founder, researcher or other power user who:

- runs two or more agents over the same project;
- currently uses worktrees, copies, containers or branches to keep them apart;
- wants to inspect agent work before accepting it;
- uses local tools and editors, not a hosted IDE;
- needs to recover intermediate agent work;
- wants agents from different providers to share one governed project.

<!-- vocab-lint:end -->

This is a *behavioural* definition, not a demographic one. First users are technical because
they can install an alpha, benchmark it, and report failures precisely — not because the product
is for engineers.

### 1.2 Expansion user

A non-technical user working with several agents on a folder of documents, spreadsheets,
presentations, research, images, PDFs and generated outputs.

The internal model must support this user from day one — nothing in the state model, the
identity model or the review model may assume source code. The first release does **not** need
production-grade semantic diffing for every business format; it needs to be honest about which
formats it can explain and which it can only show as changed bytes.

### 1.3 What both users have in common

Neither one wants to manage versions. Both want to be certain that nothing an agent did reaches
the shared project without a human looking at it first.

## 2. Jobs to be done

### 2.1 Primary job

> When I run several agents against one project, keep each agent's work safe and visible without
> letting unfinished or unapproved work overwrite the shared project.

### 2.2 Secondary jobs

| Job | What the user is really asking for | Carried by |
|---|---|---|
| Show me exactly what every actor changed | An exact difference, computed for them, not declared by the agent | REV-001, REV-002 |
| Let me open another actor's complete version | A whole working view, read-only, without disturbing that actor | ACT-006, ACT-007 |
| Let me approve all or selected changes | Both shapes of approval, with dependencies revalidated on the selective path | REV-003, REV-004 |
| Restore any previous state | Every durable version reachable until a retention policy says otherwise | OBS-001, WSP-004 |
| Tell me when an agent's context is stale | Exact record of which version an agent read, and a warning when it moves | CTX-001, CTX-004 |
| Let agents reuse work other agents already did | Deterministic derived results, cached and attributed | CTX-006, CTX-007 |
| Keep working when offline | Local reads and writes that never wait for a network round trip | SYN-001, SYN-006 |
| Export approved code changes to Git | An adapter, not an internal dependency | GIT-003, GIT-004 |

## 3. The primary journey

<!-- vocab-lint:journey -->

| Step | Who acts | What happens | What you must create |
|---|---|---|---|
| 1 | You | Install Mesh and open it. | Nothing |
| 2 | You | Choose the folder to work in — one you already have, or a new empty one. | Nothing |
| 3 | You | Connect Codex, Claude Code, another CLI agent, or a local process. | Nothing |
| 4 | Mesh | Gives every actor an isolated private view of that folder. | Nothing |
| 5 | Agents | Read and write ordinary files through ordinary file APIs. | Nothing |
| 6 | Mesh | Saves each actor's stable work privately, without being asked. | Nothing |
| 7 | Mesh | Makes stable private work available to team, so peers can watch progress. | Nothing |
| 8 | Mesh | Raises a review card as soon as an actor's work settles. | Nothing |
| 9 | You | Open the card and read the exact change, or the actor's whole version. | Nothing |
| 10 | You | Approve all of it, or select the part you want. | Nothing |
| 11 | Mesh | Verifies the approval against the shared version you actually reviewed. | Nothing |
| 12 | Mesh | Advances the shared version in one indivisible step. | Nothing |
| 13 | Mesh | Carries the approved change into every other actor's view where it does not clash. | Nothing |
| 14 | Mesh | Keeps every private version, and how it came to be, recoverable. | Nothing |

The last column asks one narrow question: **what unit of work must you author before this step
can happen?** Across the whole journey the answer is *Nothing*: not a task, not a checkpoint, not
a proposal, not a merge request — and none of the version-history primitives §4.4 keeps out of
the product altogether. Six of the fourteen steps are yours, and all six are choices (which
folder, which agent, which change, approve or not), never bookkeeping.

That is charter P1 stated as a test rather than an aspiration, and it is machine-checked: the
`journey` rule fails the build if any row's last cell is not `Nothing`, or if a step you perform
mentions a unit of work at all.

## 4. The user-visible state model

### 4.1 Internal state to user-facing wording

<!-- vocab-lint:mapping-forward -->

| Internal state | User-facing wording | Why the word was chosen |
|---|---|---|
| Actor working head | Their work | Names the actor, not the structure — "Ana's work", "the test agent's work". |
| Canonical head | Shared version | The thing everyone else builds on; the only state approval can advance. |
| Durable actor checkpoint | Saved privately | Two promises in two words: it survived, and nobody else has it yet. |
| Replicated actor checkpoint | Available to team | Replication is not publication — visible to peers, still not shared. |
| Review bundle | Ready for review | Describes what the user does next, not how the bundle was assembled. |
| Publish operation | Approve to shared version | The verb is the user's, and it names its destination. |
| Conflict | Needs attention | A conflict is a request for a decision, never an error or a loss. |
| Historical state | Earlier version | Reachable and restorable; "earlier" implies both. |

### 4.2 User-facing wording back to internal state

The reverse direction is what a developer needs when reading a label and asking what it is
actually asserting. Every row here has a row above; the `mapping` rule fails the build if the
two tables stop being exact inverses.

<!-- vocab-lint:mapping-reverse -->

| User-facing wording | Internal state | What the user is being promised |
|---|---|---|
| Their work | Actor working head | The newest state that actor has produced, including work in flight. |
| Shared version | Canonical head | The state every actor's view is based on, and the only protected one. |
| Saved privately | Durable actor checkpoint | It survives a crash of the daemon, the agent and the machine. |
| Available to team | Replicated actor checkpoint | A peer can open it read-only; it is still not in the shared version. |
| Ready for review | Review bundle | An exact, frozen difference — later work cannot slip into it. |
| Approve to shared version | Publish operation | Exactly these bytes, on exactly this base, signed by a human. |
| Needs attention | Conflict | Two changes disagree; nothing was discarded and nothing was guessed. |
| Earlier version | Historical state | It is still reachable and can be restored. |

### 4.3 The six status words

User-facing status is exactly six words, in this order. Every surface — desktop, notifications,
CLI output — uses these and only these.

<!-- vocab-lint:six-state -->

| Status | What it tells the user | The internal condition that produces it |
|---|---|---|
| Working | An actor is changing files right now. | Actor working head advanced within the activity window. |
| Saved privately | Their work survived; nobody else has it. | Durable actor checkpoint acknowledged locally. |
| Available to team | Peers can open it, read-only. | Durable actor checkpoint replicated to at least one peer. |
| Ready for review | There is an exact change waiting for a person. | Review bundle generated and frozen. |
| Needs attention | A human decision is required before this can proceed. | Conflict, failed validation, or stale approval base. |
| Approved | It is in the shared version. | Publication succeeded and the receipt is signed. |

The third column makes this table bidirectional too: read left to right to know what a user sees,
right to left to know what produces it. A condition that does not appear in the right-hand column
has no user-facing status — it is invisible by design, not by omission.

**Needs attention is deliberately broad, and it is the only word for "a human must decide".** A
conflict, a validator that failed and an approval whose base moved are three different internal
conditions and one user problem: something is waiting for you. Splitting them into three status
words would buy precision the user cannot act on differently.

That is why §4.1 maps Conflict to **Needs attention** and not to a seventh word of its own. A
mapping row whose user-facing wording is not in the closed vocabulary — the six status words of
§4.3 plus the four object wordings *Their work*, *Shared version*, *Approve to shared version*
and *Earlier version* — fails the `mapping` rule. The two tables cannot drift away from the six
status words without failing the build, which is what keeps §4.1 and §4.3 telling one story.

### 4.4 The words that never appear

These nine terms are correct — and required — in the protocol and consistency documents. In a
product surface they are defects, and a seventh status word is a defect too.

<!-- vocab-lint:allow reason="the vocabulary table must name the forbidden terms in order to forbid them; this is the only region in the document that may contain them" -->

| Never exposed to a user | Say instead | Because |
|---|---|---|
| DAG | version history | The shape of the history is not the user's problem. |
| frontier | their work | Names a structure where the user thinks about a person. |
| vector clock | up to date / behind the shared version | The answer, not the mechanism that computes it. |
| branch | their work | Isolation is automatic here; there is nothing to name or switch. |
| commit | saved privately | Saving is automatic; the word implies an act the user never performs. |
| rebase | bring onto the current shared version | Describes what happens to the work, not to the history. |
| staging | ready for review | There is no half-declared state to curate. |
| ref | version | An implementation handle, never a user-facing noun. |
| operation log | activity | The user wants what happened, not how it was recorded. |

<!-- vocab-lint:end -->

A concept that genuinely cannot be expressed in this vocabulary is a **product design question
for the product owner** — escalated, and answered in a decision doc. It is never a tenth word
added locally, and never one of the nine "just this once, in an error message".

### 4.5 How the vocabulary is enforced

By build lint, not by review. `tools/program/vocab-lint/lint.mjs` scans every surface declared in
`tools/program/vocab-lint/surfaces.json` and fails on any of the nine terms in a user-facing
string: a status catalogue, an error-message table, onboarding copy, or this document.

- **Shipped strings have no exemption.** In a string catalogue or a source file, a suppression
  directive is itself a finding.
- **Prose may carry a reasoned, budgeted suppression**, because a document that forbids a word
  has to be able to write it down. Every suppression is printed on every run, each needs a
  reason, and each surface declares a hard budget. This document's budget is four; §1.1 uses one,
  §4.4 uses one, and §5.2 and §5.6 use one each to quote the plan verbatim where it names an
  external system's vocabulary. Those two are the only places where the lint is deliberately
  blind over requirement text, which is why a change inside them needs a human reviewer.
- **The lint proves it still works.** Twenty-three fixtures run on every invocation, one per
  forbidden term plus near misses that must survive; if a matcher stops matching, the run fails
  instead of reporting a comfortable "clean".

## 5. Functional requirements

Every requirement below is transcribed from execution plan §3.5 with its ID, priority and
acceptance condition. **IDs are the citation unit**: a task's acceptance criteria name the
requirement it satisfies (`WSP-004`, `REV-006`), and an epic's exit criteria name the set it
closes. The inventory is asserted against `tools/program/vocab-lint/requirements.expected.json`
— nothing dropped, nothing invented, no P0 quietly demoted to P1.

Priorities: **P0** must ship in the public POC; **P1** is planned and may slip without failing
the POC.

### 5.1 Workspace and filesystem

| ID | Requirement | Priority | Acceptance condition |
|---|---|---:|---|
| WSP-001 | Create a new managed workspace | P0 | Workspace opens as an ordinary directory |
| WSP-002 | Import an existing folder | P0 | Original data is preserved and rollback is possible |
| WSP-003 | Control only the selected workspace | P0 | No interception outside the mounted path |
| WSP-004 | Preserve stable identity across rename and move | P0 | Concurrent edits follow the same object |
| WSP-005 | Support ordinary text and binary files | P0 | Exact bytes round-trip |
| WSP-006 | Portable filename rules | P0 | Case and Unicode collisions are detected |
| WSP-007 | Safe symlink handling | P0 | No workspace escape |
| WSP-008 | Sparse or on-demand materialization | P1 | Unused objects need not be fully hydrated |

### 5.2 Actors and private work

<!-- vocab-lint:allow reason="quotes plan §3.5 verbatim; ACT-002's acceptance condition names the isolation primitive it abolishes, and weakening the wording would weaken the requirement" -->

| ID | Requirement | Priority | Acceptance condition |
|---|---|---:|---|
| ACT-001 | Distinct human, device, agent and agent-run identities | P0 | Every durable change has an actor |
| ACT-002 | Automatic private view per actor | P0 | No manual branch or workspace creation |
| ACT-003 | Automatic activity sessions | P0 | User does not create a task |
| ACT-004 | Automatic durable checkpoints | P0 | Stable work survives daemon failure |
| ACT-005 | Unlimited local agent identities | P0 | No artificial actor limit in Community |
| ACT-006 | Actor work visible to peers | P0 | Stable private state can be inspected remotely |
| ACT-007 | Read-only actor shadow view | P0 | Peer view does not alter local work |
| ACT-008 | Attribution confidence | P1 | Exact integrated attribution is distinguished from inferred attribution |

<!-- vocab-lint:end -->

### 5.3 Review and publication

| ID | Requirement | Priority | Acceptance condition |
|---|---|---:|---|
| REV-001 | Automatic review bundle generation | P0 | Exact difference is computed without actor declaration |
| REV-002 | Immutable review snapshot | P0 | Later actor work cannot enter an existing review |
| REV-003 | Approve all changes | P0 | Canonical state advances atomically |
| REV-004 | Approve selected changes | P0 | Dependencies are revalidated |
| REV-005 | Reject without deleting history | P0 | Rejected work remains reachable |
| REV-006 | Human-only canonical publication | P0 | Agent credentials cannot advance canonical head |
| REV-007 | Stale approval protection | P0 | Base-head mismatch triggers merge or re-review |
| REV-008 | Optional biometric or passkey confirmation | P1 | Approval key cannot be used by an agent process |
| REV-009 | Signed publication receipt | P1 | Exact approval can be verified independently |

### 5.4 Synchronization

| ID | Requirement | Priority | Acceptance condition |
|---|---|---:|---|
| SYN-001 | Local-first writes | P0 | Network loss does not block work |
| SYN-002 | Metadata-before-content replication | P0 | Peer sees change before large content finishes |
| SYN-003 | Resumable chunk transfer | P0 | Interrupted transfers resume |
| SYN-004 | Duplicate-safe operation delivery | P0 | Reapplying an operation has no additional effect |
| SYN-005 | Reorder-safe delivery | P0 | Parent dependencies are resolved safely |
| SYN-006 | Offline reconciliation | P0 | Peers converge after reconnect |
| SYN-007 | Relay support | P0 | Offline peers receive durable state later |
| SYN-008 | Direct LAN or peer transfer | P1 | Relay is not required for local-network content |
| SYN-009 | Content integrity verification | P0 | Corrupt chunks are never materialized |
| SYN-010 | Peer availability states | P0 | Local, metadata-replicated and content-available are distinct |

### 5.5 Context and agent integration

| ID | Requirement | Priority | Acceptance condition |
|---|---|---:|---|
| CTX-001 | Record exact file version exposed to an integrated agent | P0 | Every integrated read includes a version ID |
| CTX-002 | Record byte or semantic ranges when available | P0 | Range-level reads supported through SDK and MCP |
| CTX-003 | Coarse file-level read tracking through the filesystem | P0 | Confidence is marked appropriately |
| CTX-004 | Detect stale reads | P0 | Agent is warned when a relevant input changes |
| CTX-005 | Bounded context retrieval | P0 | No unbounded file read enters a model call |
| CTX-006 | Compact run memory | P0 | A run can compact without losing authoritative history |
| CTX-007 | Derived-result cache | P1 | Deterministic parses and summaries can be reused |
| CTX-008 | Token accounting | P0 | Usage is attributed to workspace, actor and run |
| CTX-009 | MCP integration | P0 | Agents can inspect, search, read, change and save |
| CTX-010 | CLI wrapper integration | P0 | Arbitrary CLI agents can receive a scoped workspace |

MCP is the tool and resource integration layer; A2A is for agent-to-agent interoperability.
Neither defines workspace state or publication semantics, so CWP composes with them rather than
replacing them.

### 5.6 Git interoperability

Git is an adapter, never the internal state model (charter P9). These requirements therefore
name Git's own vocabulary, which is the only accurate way to specify an adapter's contract with
an external system.

<!-- vocab-lint:allow reason="quotes plan §3.5 verbatim; the Git adapter's contract is stated in the external system's own vocabulary, which is what makes it checkable" -->

| ID | Requirement | Priority | Acceptance condition |
|---|---|---:|---|
| GIT-001 | Import Git working tree and current commit | P0 | Canonical Mesh state references the Git base |
| GIT-002 | Ignore internal `.git` mutation as workspace content | P0 | Git metadata is handled by the adapter |
| GIT-003 | Export approved state as a Git commit | P0 | Exact approved bytes become the commit |
| GIT-004 | Export selected actor work as a branch or pull request | P1 | External Git workflow remains possible |
| GIT-005 | Import remote Git changes | P0 | Canonical state can advance from external main |
| GIT-006 | Preserve author and agent provenance | P0 | Git metadata links back to the Mesh receipt |

<!-- vocab-lint:end -->

None of this vocabulary reaches a user-facing surface. In the product these are one control —
*connect this project to Git* — and one status line.

### 5.7 Observability and recovery

| ID | Requirement | Priority | Acceptance condition |
|---|---|---:|---|
| OBS-001 | Structured local event ledger | P0 | Every durable transition is reconstructable |
| OBS-002 | Index reconstruction | P0 | The SQLite index can be rebuilt from durable state |
| OBS-003 | Crash diagnostics | P0 | The last durable boundary is identifiable |
| OBS-004 | Performance metrics | P0 | Required benchmark counters are exposed |
| OBS-005 | OpenTelemetry export | P1 | Agent, model and tool metrics use emerging conventions |
| OBS-006 | Support bundle | P1 | The user can export redacted diagnostic state |

Where OpenTelemetry's GenAI conventions already cover model calls, token counts, tools and agent
activity, use them instead of inventing incompatible telemetry names.

## 6. Non-goals for the public POC

The POC does not promise, and no task may assume: complete Windows support; whole-disk
filesystem replacement; byte-by-byte collaborative editing; automatic semantic merge of arbitrary
binary files; Byzantine consensus between mutually hostile peers; zero-knowledge hosted storage;
enterprise SSO and SCIM; legal hold; all Office and Adobe semantics; perfect range-level read
tracking for unintegrated applications; global cross-customer deduplication; a production SLA.

Attribution honesty is a non-goal boundary too (CTX-003, ACT-008): where read tracking is
inferred rather than exact, it is recorded at reduced confidence. Overstating what the system
knows violates charter P10.

## 7. What makes this document true

| Claim | Enforced by | Fails when |
|---|---|---|
| The nine terms stay out of user-facing strings | `forbidden-words` rule | A term appears outside a reasoned, budgeted region |
| Status is exactly six words, in order | `six-state` rule | A seventh word ships, or the table drifts |
| The mapping is complete in both directions | `mapping` rule | The two tables stop being exact inverses |
| The journey has no ceremony | `journey` rule | A step asks the user to author a unit of work |
| Every plan §3.5 requirement is here, at its priority | `requirements` rule | An ID is dropped, duplicated, invented or demoted |
| The lint itself still works | 23 fixtures, run every invocation | A matcher stops matching |

A disagreement between this document and shipped behaviour is a product bug or a reviewed
architecture decision — never resolved by quietly softening the wording here.
