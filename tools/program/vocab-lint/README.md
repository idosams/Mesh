# vocab-lint

The product vocabulary gate. The internal model is a version history; the user model is six
words. This lint is what keeps the first out of the second.

Plain Node, no dependencies, no network — CI, the desktop build and a clean laptop all run the
identical check.

```bash
node tools/program/vocab-lint/lint.mjs --user-facing   # the gate: surfaces + fixtures
node tools/program/vocab-lint/lint.mjs --self-test     # fixtures only
node tools/program/vocab-lint/lint.mjs --list-words    # the vocabulary, with replacements
node tools/program/vocab-lint/lint.mjs src/strings.ts  # ad-hoc, one file
```

Exit codes: `0` clean · `1` findings · `2` usage or configuration error.

## What it checks

| Rule | Assertion | Source |
|---|---|---|
| `forbidden-words` | None of the nine terms appears in a user-facing string | charter §5, plan §3.4 |
| `six-state` | User-facing status is exactly six words, in one order | charter §5, plan §3.4 |
| `mapping` | The internal↔user-facing mapping is complete in both directions, and its user-facing column stays inside the closed vocabulary | PRD §4.1–4.2 |
| `journey` | No journey step asks the user to author a unit of work | charter P1, PRD §3 |
| `requirements` | Every plan §3.5 requirement is present, unique, at its priority | PRD §5 |
| `allow-region` | Every suppression has a reason and fits its surface's budget | this file |

The nine forbidden terms and their replacements are in `lib/vocabulary.mjs`; run `--list-words`
to see them with the inflections each matcher catches and the near misses it must not.

`mapping` checks more than symmetry, because two tables can be exact inverses of each other and
still both contradict the six status words. Its user-facing column is closed against
`APPROVED_MAPPING_WORDING` — the six statuses plus the four object wordings (*Their work*,
*Shared version*, *Approve to shared version*, *Earlier version*). That is what caught "Needs
review" sitting in §4.1 beside "Needs attention" and "Ready for review": symmetric, self-
consistent, and not a word the product has.

## What counts as a user-facing string

Declared per surface in [`surfaces.json`](surfaces.json), because the answer differs by file type:

| mode | user-facing string | reads |
|---|---|---|
| `prose` | the whole document, minus explicit allow regions | `.md` `.txt` |
| `json-strings` | every JSON string *value* — keys are identifiers, not copy | `.json` |
| `status-catalog` | every JSON string value, plus the *status values* at `statusPath` checked against the six words | `.json` |
| `source-strings` | every string literal; comments are excluded, they ship to nobody | `.ts` `.tsx` `.js` `.jsx` `.mjs` `.cjs` `.rs` `.swift` |

`source-strings` tokenizes rather than stripping comments with a regex, so a literal like
`"https://example.test"` is scanned rather than mistaken for the start of a comment and silently
dropped. A rule that silently scans nothing is the failure mode this tool exists to avoid.

**Mode and extension are cross-checked at manifest load.** A surface whose mode cannot read its
own files scans nothing and reports clean — the worst outcome this tool has, because it looks
exactly like success. `apps/desktop/src/strings/**/*.{json,ts}` in `json-strings` mode shipped
in this manifest and silently scanned no TypeScript at all: the JSON extractor only sees
double-quoted literals, so every forbidden term written in idiomatic single quotes or a template
literal was invisible. That pairing is now a load-time error, pinned by the `manifestCases` in
`fixtures/expectations.json`. One mode per extension; split the surface rather than widening the
glob.

Add a surface the moment it starts shipping strings to a person. Globs are deliberately narrow: a
broad glob turns the gate into background noise that lanes learn to route around, and a glob whose
extension is unbounded (`strings/**`) is rejected outright, since no mode can be shown to read it.
A surface marked `required` that matches no file is an error — the manifest and the tree have
drifted.

### Narrowing a surface

A surface may name files inside its glob that ship to nobody:

```jsonc
{ "glob": "apps/desktop/src/**/*.ts", "exclude": ["apps/desktop/src/**/*.test.ts"] }
```

`exclude` exists because the alternative to it is a surface that stops covering things. A glob can
only be as wide as its narrowest legal form, so covering an application's error messages meant
either listing every directory that holds one — a list that goes stale the day somebody adds a
directory — or widening until the tests came too, where a fixture that deliberately contains a
forbidden term is a finding. Both endings are the same: the surface gets dropped and the strings
stop being scanned.

It is **narrowing, not suppression**, and three rules keep the difference: a pattern that matches
nothing is an error rather than a silent no-op, so it cannot rot into a hole nobody remembers
opening; there is no per-file form of it; and it is illegal beside `path`, since a single named
file is either a surface or it is not. Exempting *shipped* text is `allowRegionBudget` — reviewed,
counted and printed on every run — and this is not a second way to do that.

### Status catalogues

`status-catalog` needs to know which values are statuses, so the surface declares a `statusPath`
— a small selector where `*` visits every entry and a name visits that key:

```jsonc
{ "mode": "status-catalog", "statusPath": "$.*.label" }   // { "working": { "label": "Working", "icon": "…" } }
{ "mode": "status-catalog" }                               // default "$.*" — the flat map
```

Everything the path selects must be one of the six words; a path that selects **nothing** is a
finding, not a pass. The rule used to check every string in the file, which meant no realistic
catalogue could survive — an icon name and a help sentence both failed — so the surface's only
legal shape was a flat six-key map. `forbidden-words` still scans every string, icons and help
text included.

The shipped catalogue is `apps/desktop/src/strings/status.json` at `$.states.*.status`. Its other
half, `$.internalStates`, is deliberately **not** selected: it maps every internal state from PRD
§4.1 to its approved wording, and four of those wordings — *Their work*, *Shared version*,
*Approve to shared version*, *Earlier version* — name a thing rather than a state. Checking them
against the six would force a seventh meaning onto one of the six. What holds that half honest is
`apps/desktop/src/app/status.test.ts`, which compares it with PRD §4.1 in both directions, so an
internal state with no wording fails the desktop build instead of reaching a person as a fallback
label.

## Who runs it

`npm --prefix apps/desktop test` runs `--user-facing` first and the desktop suite second, so the
gate is part of that application's build rather than a step somebody remembers. It is **not** on
`npm test` at the repository root: `--terminology` is the other mode of this same entry point and
exits 1 with 70 findings today, which is why `tools/program/invocation-check.mjs` still carries
this file in `NOT_RUN_ENTRIES` with `01KZD3KQRDV8MM2XVG6KTQNYS2` named as the task that owns the
wiring. The register's own words — *"`--user-facing` exits 0 today; it is the other mode of the
same entry point and cannot be wired separately from here"* — are why the desktop build is where
this became required, and root `npm test` is not.

## Suppression

Shipped strings have **no** exemption. In a string catalogue or a source file, a
`vocab-lint:allow` directive is itself a finding.

Prose may carry a suppression, because a document that forbids a word has to be able to write it
down:

```markdown
<!-- vocab-lint:allow reason="names the terms in order to forbid them" -->
…
<!-- vocab-lint:end -->
```

Three properties keep that from eroding into a habit: every region needs a non-empty reason,
every region is printed on every run (clean or not), and every surface declares a hard
`allowRegionBudget`. Raising a budget is a reviewed change to `surfaces.json`, visible in a diff.

A file named on the command line is linted **as its declared surface** — same mode, same rules,
same budget — so `lint.mjs docs/product-prd.md` and `--user-facing` always agree about that file.
They once disagreed: the ad-hoc path forced a budget of 0 and reported the PRD's four reviewed
suppressions as a failure, so the one-file pre-push check went red on a file the gate called
clean. Files with no declared surface fall back to an inferred mode, all rules and a budget of 0.

## The fixtures run every time

`fixtures/expectations.json` declares the fixtures and the findings each must produce, plus
`manifestCases` asserting that the manifest validator still refuses configurations that would
stop a surface being scanned. They run on every invocation unless you pass `--no-self-test`, and
the suite additionally asserts that **all nine forbidden terms are covered** — adding a term to
`lib/vocabulary.mjs` without a fixture fails the run.

**Expectations pin exact messages wherever a message encodes an ordering.** A loose
`messageIncludes` is how `--self-test` once reported `23/23 fixtures pass` and exited 0 while the
six status words were in the wrong order: the two-row `six-state/wrong-order.md` fixture happened
to still match after the first two words were swapped. `six-state/full-order.md` now pins all six
positions exactly, so any permutation of `APPROVED_STATUS` breaks the suite on its own — without
needing the full `--user-facing` scan to notice.

A lint whose matchers have quietly stopped matching reports "clean" forever, which is worse than
no lint at all. The negative fixtures matter as much as the positive ones: `clean/near-misses.md`
holds *reference*, *refactor*, *prefer*, *commitment*, *committee*, *database*, *staged rollout*,
*dagger*, *branchless* and *vector graphics*, and every one of them must survive. A matcher tuned
so tightly that ordinary English trips it is a matcher people switch off.

## Adding a rule

1. Add `lib/rules/<name>.mjs` exporting `id`, `modes` and `run(context)`.
2. Register it in `lib/rules/index.mjs`.
3. Add fixtures — at least one that fires and one that stays clean — to
   `fixtures/expectations.json`.
4. Opt the relevant surfaces into it in `surfaces.json`. Unknown rule names fail manifest
   validation at load, so a typo cannot silently disable a check.

## Where the answers live

Vocabulary, status words and the state mapping → [`docs/product-prd.md`](../../../docs/product-prd.md)
§4 · the constitutional rules behind them → [`docs/charter.md`](../../../docs/charter.md) §5 and
P1 · the requirement inventory → [`requirements.expected.json`](requirements.expected.json),
transcribed from execution plan §3.5.
