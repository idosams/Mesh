/**
 * The self-test: one violating mutation per check.
 *
 * `docs/consistency.md` §6 states the rule this file obeys — an invariant no
 * mutation can break is not being checked, and a check that cannot fail is not
 * evidence. Every case here builds a synthetic register, mutates one thing, and
 * asserts the matching check fires. The clean baseline must produce nothing.
 */

import { buildDocument } from './parse.mjs';
import { buildModelFromDocuments } from './model.mjs';
import { memoryFiles, scanCrates } from './crates.mjs';
import { CHECKS, runChecks } from './checks.mjs';

const CLEAN_REGISTER = `# Fixture register

## 2. Concepts

### 2.1 The widget graph — what a widget is

Prose that uses \`widget\` and \`mesh-fixture\`.

<!-- terminology:begin -->

| Term | Definition | Graph | Home |
|---|---|---|---|
| \`widget\` | A thing, exactly one of: \`round\`, \`square\`. | state | \`mesh-fixture\` |
| \`widget graph\` | The graph of widgets. | state | \`mesh-fixture\` |
| \`gadget\` | Another thing, exactly one of: \`tall\`, \`short\`. | trust | \`mesh-fixture\` |
| \`mesh-fixture\` | The fixture crate. | — | \`crates/mesh-fixture\` |
| \`FIXTURE_NAME\` | The constant the fixture crate exports. | — | \`crates/*\` |
| \`Cog\` | A type the fixture crate declares privately and re-exports. | state | \`crates/mesh-fixture\` |
| \`CogError\` | Why a \`Cog\` was refused. | state | \`crates/mesh-fixture\` |

<!-- terminology:end -->

<!-- aliases:begin -->

| Alias | Resolves to | Where the alias comes from |
|---|---|---|
| \`doohickey\` | \`widget\` | Fixture prose. |

<!-- aliases:end -->

<!-- non-terms:begin -->

| Literal | Why it is not a term |
|---|---|
| \`pub\` | Rust keyword quoted as source. |

<!-- non-terms:end -->
`;

const CLEAN_COMPANION = `# Fixture companion

### 1.1 Gadget — how a gadget behaves

A gadget is \`tall\` or \`short\`.
`;

const LIB = 'crates/mesh-fixture/src/lib.rs';
const INNER = 'crates/mesh-fixture/src/inner.rs';
const GUBBINS = 'crates/mesh-fixture/src/gubbins.rs';

/**
 * The fixture crate as source text, in the shape the real crates use: private
 * modules, a flat re-export list at the crate root, and one public module whose
 * name is plumbing rather than protocol vocabulary.
 *
 * The clean baseline is where the three negatives are asserted. `plumbing` is a
 * module name and never becomes a register term; `helper` is public only
 * through that module and is deliberately out of reach; and `gubbins` is a
 * private module republished whole by `pub use`, which resolves to a file on
 * disk and so is a module rather than an item. A clean run over this tree is
 * the standing proof that none of the three is asked to resolve.
 */
const CLEAN_LIB = `mod gubbins;
mod inner;

pub mod plumbing;

pub use crate::gubbins;
pub use crate::inner::{Cog, CogError};

pub const FIXTURE_NAME: &str = "mesh-fixture";
`;

const FIXTURE_TREE = Object.freeze({
  'crates/mesh-fixture/Cargo.toml': '[package]\nname = "mesh-fixture"\n',
  [LIB]: CLEAN_LIB,
  [INNER]: 'pub struct Cog;\n\npub enum CogError {\n    Empty,\n}\n',
  [GUBBINS]: 'pub fn tighten() {}\n',
  'crates/mesh-fixture/src/plumbing.rs': 'pub fn helper() {}\n',
});

/** The fixture crate with its library root replaced — the scan runs for real. */
function crateScan(libSource = CLEAN_LIB) {
  return scanCrates(memoryFiles({ ...FIXTURE_TREE, [LIB]: libSource }));
}

function modelFrom(registerText, companionText, options = {}) {
  const docs = [
    buildDocument(registerText, 'fixture/register.md'),
    buildDocument(companionText, 'fixture/companion.md'),
  ];
  const scan = options.scan ?? crateScan();
  return buildModelFromDocuments(docs, {
    crates: options.crates ?? scan.crates,
    publicItems: scan.publicItems,
    wildcardExports: scan.wildcardExports,
  });
}

/** A model whose only mutation is the fixture crate's library root. */
function modelFromLib(libSource) {
  return modelFrom(CLEAN_REGISTER, CLEAN_COMPANION, { scan: crateScan(libSource) });
}

/** @returns {string|null} an explanation when `predicate` does not hold. */
function expect(findings, predicate, explanation) {
  return predicate(findings) ? null : `${explanation}; got ${JSON.stringify(findings)}`;
}

/** The words that may stand between `pub` and an item's name. No finding may ever
 *  be named after one of them: `pub const fn no_session()` was read as an item
 *  literally called `fn` until the reader learned to step over them. */
const QUALIFIERS = ['fn', 'async', 'unsafe', 'const', 'extern'];

/**
 * One crate-root declaration per qualifier shape the reader has to step over.
 *
 * Each becomes its own violating mutation, because a single fixture carrying all
 * five would pass while four of them were invisible — the failure this whole
 * repair is about. `pub const fn` is the shape that ships today, at
 * `crates/mesh-store/src/index.rs:517`.
 */
const QUALIFIED_DECLARATIONS = Object.freeze([
  { shape: 'a public `async fn`', source: 'pub async fn spin() {}', item: 'spin' },
  { shape: 'a public `unsafe fn`', source: 'pub unsafe fn raw() {}', item: 'raw' },
  { shape: 'a public `extern "C" fn`', source: 'pub extern "C" fn abi() {}', item: 'abi' },
  { shape: 'a public `unsafe trait`', source: 'pub unsafe trait Torque {}', item: 'Torque' },
  { shape: 'a public `const fn`', source: 'pub const fn ratio() -> u8 { 1 }', item: 'ratio' },
  {
    shape: 'a public `unsafe extern "C" fn`',
    source: 'pub unsafe extern "C" fn latch() {}',
    item: 'latch',
  },
]);

/** Each case names the check it must break and the mutation that breaks it. */
const CASES = [
  {
    id: 'TL-1',
    mutation: 'the same term gets a second row',
    model: () =>
      modelFrom(
        CLEAN_REGISTER.replace(
          '| `gadget` |',
          '| `widget` | A second definition of the same word. | state | `mesh-fixture` |\n| `gadget` |',
        ),
        CLEAN_COMPANION,
      ),
  },
  {
    id: 'TL-2',
    mutation: 'a row carries a graph value outside the vocabulary',
    model: () => modelFrom(CLEAN_REGISTER.replace('| The graph of widgets. | state |', '| The graph of widgets. | topology |'), CLEAN_COMPANION),
  },
  {
    id: 'TL-3',
    mutation: 'an alias points at a term that does not exist',
    model: () => modelFrom(CLEAN_REGISTER.replace('| `doohickey` | `widget` |', '| `doohickey` | `sprocket` |'), CLEAN_COMPANION),
  },
  {
    id: 'TL-4',
    mutation: 'a crate directory exists with no register row',
    model: () => modelFrom(CLEAN_REGISTER, CLEAN_COMPANION, { crates: ['mesh-fixture', 'mesh-orphan'] }),
  },
  {
    id: 'TL-5',
    mutation: 'a crate declares a public item with no register row',
    model: () => modelFromLib(`${CLEAN_LIB}\npub struct UndeclaredType;\n`),
    expects: (findings) =>
      expect(
        findings,
        (list) => list.some((item) => item.message.includes('`UndeclaredType`') && item.doc === LIB),
        'the finding names the item and the crate root that declares it',
      ),
  },
  {
    id: 'TL-5',
    mutation: 'a private module’s item is re-exported with no register row',
    model: () =>
      modelFrom(CLEAN_REGISTER, CLEAN_COMPANION, {
        scan: scanCrates(
          memoryFiles({
            ...FIXTURE_TREE,
            [LIB]: CLEAN_LIB.replace('{Cog, CogError}', '{Cog, CogError, Hidden}'),
            [INNER]: `${FIXTURE_TREE[INNER]}\npub struct Hidden;\n`,
          }),
        ),
      }),
    expects: (findings) =>
      expect(
        findings,
        (list) =>
          list.length === 1 && list[0].message.includes('`Hidden`') && list[0].doc === INNER,
        'the finding names the item and resolves to the private module that declares it',
      ),
  },
  {
    id: 'TL-5',
    mutation: 'a re-export renames an item to a word with no register row',
    model: () => modelFromLib(CLEAN_LIB.replace('{Cog, CogError}', '{Cog as Sprocket, CogError}')),
    expects: (findings) =>
      expect(
        findings,
        (list) => list.length === 1 && list[0].message.includes('`Sprocket`'),
        'the finding names the exported name, not the name inside the module',
      ),
  },
  {
    id: 'TL-5',
    mutation: 'a whole module is re-exported with a glob',
    model: () => modelFromLib(`${CLEAN_LIB}\npub use crate::inner::*;\n`),
    expects: (findings) =>
      expect(
        findings,
        (list) => list.length === 1 && list[0].message.includes('`inner::*`'),
        'the finding names the glob rather than guessing what it publishes',
      ),
  },
  ...QUALIFIED_DECLARATIONS.map(({ shape, source, item }) => ({
    id: 'TL-5',
    mutation: `a crate root declares ${shape}`,
    model: () => modelFromLib(`${CLEAN_LIB}\n${source}\n`),
    expects: (findings) =>
      expect(
        findings,
        (list) =>
          list.length === 1 &&
          list[0].message.includes(`public item \`${item}\``) &&
          list[0].doc === LIB &&
          !QUALIFIERS.some((word) => list[0].message.includes(`public item \`${word}\``)),
        `the finding names \`${item}\` and never the qualifier or the item keyword in front of it`,
      ),
  })),
  {
    id: 'TL-5',
    mutation: 'an item is re-exported from the same module a bare re-export republishes whole',
    model: () =>
      modelFrom(CLEAN_REGISTER, CLEAN_COMPANION, {
        scan: scanCrates(
          memoryFiles({
            ...FIXTURE_TREE,
            [LIB]: `${CLEAN_LIB}\npub use crate::gubbins::Sprocket;\n`,
            [GUBBINS]: `${FIXTURE_TREE[GUBBINS]}\npub struct Sprocket;\n`,
          }),
        ),
      }),
    expects: (findings) =>
      expect(
        findings,
        (list) =>
          list.length === 1 && list[0].message.includes('`Sprocket`') && list[0].doc === GUBBINS,
        'the item is a finding while `gubbins`, republished whole beside it, is not',
      ),
  },
  {
    id: 'TL-6',
    mutation: 'a crate term names somebody else’s directory',
    model: () => modelFrom(CLEAN_REGISTER.replace('`crates/mesh-fixture`', '`crates/mesh-other`'), CLEAN_COMPANION),
  },
  {
    id: 'TL-7',
    mutation: 'a companion document defines a concept the register does not own',
    model: () =>
      modelFrom(
        CLEAN_REGISTER,
        CLEAN_COMPANION.replace('### 1.1 Gadget — how a gadget behaves', '### 1.1 Sprocket — a concept defined outside the register'),
      ),
  },
  {
    id: 'TL-8',
    mutation: 'prose uses a backticked word that resolves to nothing',
    model: () => modelFrom(CLEAN_REGISTER.replace('uses `widget` and', 'uses `sprocket` and'), CLEAN_COMPANION),
  },
  {
    id: 'TL-9',
    mutation: 'two enumerated terms share a member value',
    model: () => modelFrom(CLEAN_REGISTER.replace('exactly one of: `tall`, `short`', 'exactly one of: `round`, `short`'), CLEAN_COMPANION),
  },
  {
    id: 'TL-10',
    mutation: 'a register row is homed at a real crate that publishes no such item',
    model: () =>
      modelFrom(
        CLEAN_REGISTER.replace(
          '| `FIXTURE_NAME` |',
          '| `PhantomWidget` | A type that does not exist in any crate. | state | `crates/mesh-fixture` |\n| `FIXTURE_NAME` |',
        ),
        CLEAN_COMPANION,
      ),
    expects: (findings) =>
      expect(
        findings,
        (list) =>
          list.length === 1 &&
          list[0].message.includes('`PhantomWidget`') &&
          list[0].message.includes('`crates/mesh-fixture`'),
        'the finding names the fabricated term and the crate its home cell claims',
      ),
  },
  {
    id: 'TL-10',
    mutation: 'a register row is homed at the wrong crate',
    model: () =>
      modelFrom(
        CLEAN_REGISTER.replace(
          '| `Cog` | A type the fixture crate declares privately and re-exports. | state | `crates/mesh-fixture` |',
          '| `Cog` | A type the fixture crate declares privately and re-exports. | state | `crates/mesh-other` |',
        ),
        CLEAN_COMPANION,
        { crates: ['mesh-fixture', 'mesh-other'] },
      ),
    expects: (findings) =>
      expect(
        findings,
        (list) => list.some((item) => item.message.includes('`crates/mesh-other`')),
        'a row whose home names a crate that does not publish it is a finding',
      ),
  },
];

/** @returns {{ok: boolean, lines: string[]}} */
export function runSelfTest() {
  const lines = [];
  let ok = true;

  const baseline = runChecks(modelFrom(CLEAN_REGISTER, CLEAN_COMPANION));
  if (baseline.length === 0) {
    lines.push('  ok   baseline  clean fixture produces no findings');
  } else {
    ok = false;
    lines.push('  FAIL baseline  clean fixture produced findings:');
    for (const item of baseline) lines.push(`         ${item.id} ${item.message}`);
  }

  const covered = new Set();
  for (const testCase of CASES) {
    covered.add(testCase.id);
    let findings;
    try {
      findings = runChecks(testCase.model(), [testCase.id]);
    } catch (error) {
      ok = false;
      lines.push(`  FAIL ${testCase.id}      mutation threw: ${error.message}`);
      continue;
    }
    if (findings.length === 0) {
      ok = false;
      lines.push(`  FAIL ${testCase.id}      ${testCase.mutation} — check did not fire`);
      continue;
    }
    /* A case may also state what the finding has to say. Firing on the wrong
       item, or naming a file the reader cannot act on, is not the check
       working. */
    const complaint = testCase.expects ? testCase.expects(findings) : null;
    if (complaint) {
      ok = false;
      lines.push(`  FAIL ${testCase.id}      ${testCase.mutation} — ${complaint}`);
      continue;
    }
    lines.push(`  ok   ${testCase.id}      ${testCase.mutation}`);
  }

  for (const check of CHECKS) {
    if (!covered.has(check.id)) {
      ok = false;
      lines.push(`  FAIL ${check.id}      no violating mutation — the check is unproven`);
    }
  }

  lines.push(`self-test: ${ok ? 'pass' : 'FAIL'} (${CASES.length} mutations over ${CHECKS.length} checks)`);
  return { ok, lines };
}
