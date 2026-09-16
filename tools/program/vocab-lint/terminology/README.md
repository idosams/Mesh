# terminology — the protocol terminology gate

`docs/protocol.md` claims that every protocol term has exactly one definition and that the
register is the only place in the repository where a term is defined. This lint is what makes
that claim mechanical. Without it the claim is review discipline wearing a specification's
clothes.

```console
$ node tools/program/vocab-lint/lint.mjs --terminology
terminology: clean
  384 terms · 12 aliases · 74 member values · 25 crates · 10 checks
```

That command is the declared automated validation of task `01KZC2319JHK2CTB4Y1BSXDFAJ`.
`node tools/program/vocab-lint/terminology.mjs` runs the identical code directly.

**Wired into `npm test`.** `package.json` defines `verify:terminology` as this command, `verify`
invokes it and `test` invokes `verify`, so a deleted register row and a broken check both turn the
merge path red instead of waiting for somebody to remember the command. `tools/program/npm-wiring.mjs`
is what keeps the script from being defined and never invoked. Plain Node, no dependencies, no
network. Exit codes: `0` clean · `1` findings · `2` usage or configuration error.

## The checks

| ID | What it rejects |
|---|---|
| TL-1 | The same term defined in two register rows. |
| TL-2 | A row with no definition, no home, or a graph value outside the vocabulary. |
| TL-3 | An alias pointing at nothing, or an alias that is also a term. |
| TL-4 | A crate directory with no register row. |
| TL-5 | A public type or value a crate's `src/lib.rs` publishes with no register row — declared there behind any `async`/`unsafe`/`const`/`extern` qualifier, or re-exported there by name from a private module — and a glob re-export, which publishes names `src/lib.rs` does not list. A bare re-export of a module is not an item and is skipped. |
| TL-6 | A crate-name term whose home names somebody else's directory. |
| TL-7 | A companion document that defines a concept the register does not own. |
| TL-8 | A quoted word in either document that resolves to nothing. |
| TL-9 | One enumerated member value claimed by two terms. |
| TL-10 | A §3.10 row homed at one crate directory naming an item that crate does not publish — TL-5 run backwards. |

TL-7, TL-8 and TL-9 exist because uniqueness *inside* the register is not the property the
protocol needs. A term can be unique in the table and still have a second definition in a
companion document (TL-7), be used with no definition at all (TL-8), or have its member values
mean two different things in two different rows (TL-9).

## Every check has a violating mutation

```console
$ node tools/program/vocab-lint/lint.mjs --terminology --self-test
  ok   baseline  clean fixture produces no findings
  ok   TL-1      the same term gets a second row
  …
  ok   TL-5      a private module’s item is re-exported with no register row
  …
  ok   TL-10     a register row is homed at a real crate that publishes no such item
self-test: pass (21 mutations over 10 checks)
```

The self-test builds a synthetic register and a synthetic crate — source text in an object
literal, scanned by the same code the repository scan runs — breaks one rule at a time, and asserts
the matching check fires. A case may also state what its finding has to say, because firing on the
wrong item is not the check working. It fails if any registered check has no mutation that breaks
it — a check that cannot fail is not evidence, so it runs by default before the real register is
read.

The clean fixture is where the exclusions are asserted: its crate declares a public module and an
item inside it, and republishes a private module whole with `pub use crate::gubbins;`. None of the
three is a register row, and the baseline is only clean because TL-5 asks none of them to resolve.

## Layout

| File | Responsibility |
|---|---|
| `../lint.mjs` | Dispatches `--terminology` here before parsing its own flags. |
| `../terminology.mjs` | Argument parsing, output, exit code; exports `run` for that dispatch. |
| `parse.mjs` | Markdown parsing: marker blocks, table rows, quoted tokens, headings. Pure over text. |
| `rust.mjs` | Rust parsing: crate-root declarations and re-export trees. Pure over text. |
| `crates.mjs` | The one place that reads files, behind a three-method reader the self-test can replace with an object literal. |
| `model.mjs` | Assembles the model the checks read, from parsed documents and one crate scan. |
| `checks.mjs` | TL-1 … TL-10 as pure functions, plus the registry. |
| `report.mjs` | Human and `--json` rendering. |
| `selftest.mjs` | One violating mutation per check. |

Adding a check means adding a function in `checks.mjs`, one registry entry, one mutation in
`selftest.mjs`, and one row in `docs/protocol.md` §6. Nothing already there needs editing, and the
self-test fails if the mutation is forgotten.

## Relationship to `lint.mjs`

`lint.mjs --user-facing` guards the **user-facing** vocabulary — the six words a product surface may
use. `lint.mjs --terminology` guards the **protocol** vocabulary — the words a product surface may
*not* use. `docs/protocol.md` §1.2 is why the two word lists must never be merged: `DAG` and
`compare-and-swap` are required here and banned there.

One entrypoint, two modes, two word lists that never meet. `lint.mjs` dispatches `--terminology`
into this module *before* parsing its own flags, so the terminology mode owns `--self-test`,
`--json` and `--root` outright and neither mode can quietly reinterpret the other's arguments. The
alternative — folding the checks into the surface engine — is what would eventually produce one
merged list, which §1.2 forbids.
