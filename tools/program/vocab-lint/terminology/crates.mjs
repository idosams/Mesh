/**
 * The crate scan: what `crates/` publishes, as data the checks can read.
 *
 * Reading files is the only step of this lint that is not pure, so it is behind
 * one narrow seam — a reader with three methods. `repositoryFiles` is the real
 * checkout; `memoryFiles` is a crate written as an object literal, which is what
 * lets the self-test exercise this scan rather than assert around it. The checks
 * themselves never see either: they take the model and nothing else.
 */

import fs from 'node:fs';
import path from 'node:path';

import { declarationLine, declaredItems, reExports } from './rust.mjs';

/** @typedef {{crateNames: () => string[], exists: (rel: string) => boolean,
 *   read: (rel: string) => string}} FileReader */

const CRATE_MANIFEST = /^crates\/([^/]+)\/Cargo\.toml$/;

/** A reader over a real checkout rooted at `root`. @returns {FileReader} */
export function repositoryFiles(root) {
  return {
    crateNames() {
      const cratesRoot = path.join(root, 'crates');
      if (!fs.existsSync(cratesRoot)) return [];
      return fs
        .readdirSync(cratesRoot, { withFileTypes: true })
        .filter((entry) => entry.isDirectory())
        .filter((entry) => fs.existsSync(path.join(cratesRoot, entry.name, 'Cargo.toml')))
        .map((entry) => entry.name)
        .sort();
    },
    exists: (relative) => fs.existsSync(path.join(root, relative)),
    read: (relative) => fs.readFileSync(path.join(root, relative), 'utf8'),
  };
}

/** A reader over an object literal of `relative path → source text`.
 *  @returns {FileReader} */
export function memoryFiles(tree) {
  const files = new Map(Object.entries(tree));
  return {
    crateNames: () =>
      [...files.keys()]
        .map((relative) => CRATE_MANIFEST.exec(relative))
        .filter(Boolean)
        .map((match) => match[1])
        .sort(),
    exists: (relative) => files.has(relative),
    read: (relative) => files.get(relative) ?? '',
  };
}

const libraryRoot = (crate) => `crates/${crate}/src/lib.rs`;

/**
 * The file declaring the module at `segments` inside `crate`, or null when no
 * such file exists — which is what a re-export of a foreign crate's item looks
 * like from here.
 */
function moduleFile(files, crate, segments) {
  if (segments.length === 0) return libraryRoot(crate);
  const base = `crates/${crate}/src/${segments.join('/')}`;
  if (files.exists(`${base}.rs`)) return `${base}.rs`;
  if (files.exists(`${base}/mod.rs`)) return `${base}/mod.rs`;
  return null;
}

/**
 * True when a re-exported leaf names a module rather than an item.
 *
 * `pub use crate::plumbing;` republishes a module, and §6 of `docs/protocol.md`
 * excludes module names from the register — so demanding a row for `plumbing`
 * would put an ordinary English word into a register whose whole purpose is one
 * word, one protocol meaning. The question is decidable rather than ambiguous:
 * the module's file is on disk, so the leaf is resolved against the same module
 * table the declaration site uses.
 *
 * Two deliberate narrowings. A leaf whose module file does not resolve — a
 * foreign crate's module, republished — keeps today's behaviour and is treated
 * as an item, so the exclusion never swallows a name this crate is responsible
 * for. And a name that is *both* a module and an item declared beside it is
 * treated as the item: a missing row is worse than a spurious one.
 */
function isModuleLeaf(files, crate, item) {
  if (typeof item.declaredAs !== 'string') return false;
  if (moduleFile(files, crate, [...item.path, item.declaredAs]) === null) return false;
  const parent = moduleFile(files, crate, item.path);
  if (parent !== null && files.exists(parent)) {
    if (declarationLine(files.read(parent), item.declaredAs) !== 0) return false;
  }
  return true;
}

/**
 * Where a re-exported item is declared: the module file and the line, when the
 * module can be resolved and declares the name; otherwise the crate root and the
 * line the re-export sits on, which always exists and always points somewhere
 * the reader can act on.
 */
function declarationSite(files, crate, item) {
  const module = moduleFile(files, crate, item.path);
  if (module === null) return { file: libraryRoot(crate), line: item.line };
  const line = declarationLine(files.read(module), item.declaredAs);
  return { file: module, line };
}

/**
 * Every crate under `crates/`, every public name its root publishes, and every
 * glob re-export that refuses to say which names it publishes.
 *
 * A glob is reported rather than expanded. Expanding it would make the crate's
 * public surface change without `src/lib.rs` changing, so the register could
 * drift silently — the exact failure this gate exists to catch. The full
 * argument, the two rejected alternatives and the measurement behind them are
 * `docs/adr/0019-reject-a-glob-re-export-rather-than-expand-it.md`, which is
 * also where the reason a public module is not descended into is written down.
 * (This comment named `0027` until `01KZD0D413BE4GMX5RA9H3V8Z8`; that file was a
 * second copy of the same decision and is gone. See ADR-0030.)
 *
 * 01KZD21Y7BNY19AQ3VSX9HT63X` finds the doc. It did not when this comment was
 * written. Between `01KZCV213VGTK670DPYC1X2M9Y` merging and the doc landing, the
 * pointer went nowhere and every gate stayed green, which is task
 * `01KZD4NJ4D3901FKAAFKKK3QE5`.
 *
 * NO CHECK COVERS A ULID CITED FROM SOURCE, and that is declared here rather than
 * left to be rediscovered. `gate-link-resolve` reads `links:` envelopes inside
 * neither reads a bare identifier quoted from `tools/**` or `crates/**`. Building
 * one is out of scope for a stated reason rather than by omission: its corpus
 * would be every source file in the repository scanned for 26-character Crockford
 * strings, and a check whose corpus is "all source text" is the kind that gets
 * switched off the first time it fires on a hash. Until somebody decides
 * otherwise, a ULID cited from source is a claim a reader checks with the command
 * above. `01KZCCNCC42ZK9X8X1ENC6A7F5` is the same class of defect one surface out.
 *
 * 01KZD21Y7BNY19AQ3VSX9HT63X` finds the doc. It did not when this comment was
 * written. Between `01KZCV213VGTK670DPYC1X2M9Y` merging and the doc landing, the
 * pointer went nowhere and every gate stayed green, which is task
 * `01KZD4NJ4D3901FKAAFKKK3QE5`.
 *
 * NO CHECK COVERS A ULID CITED FROM SOURCE, and that is declared here rather than
 * left to be rediscovered. `gate-link-resolve` reads `links:` envelopes inside
 * neither reads a bare identifier quoted from `tools/**` or `crates/**`. Building
 * one is out of scope for a stated reason rather than by omission: its corpus
 * would be every source file in the repository scanned for 26-character Crockford
 * strings, and a check whose corpus is "all source text" is the kind that gets
 * switched off the first time it fires on a hash. Until somebody decides
 * otherwise, a ULID cited from source is a claim a reader checks with the command
 * above. `01KZCCNCC42ZK9X8X1ENC6A7F5` is the same class of defect one surface out.
 *
 * @param {FileReader} files
 * @returns {{crates: string[],
 *   publicItems: Array<{crate: string, name: string, file: string, line: number}>,
 *   wildcardExports: Array<{crate: string, path: string, file: string, line: number}>}}
 */
export function scanCrates(files) {
  const crates = files.crateNames();
  const publicItems = [];
  const wildcardExports = [];

  for (const crate of crates) {
    const root = libraryRoot(crate);
    if (!files.exists(root)) continue;
    const source = files.read(root);
    const published = new Set();

    for (const item of declaredItems(source)) {
      if (published.has(item.name)) continue;
      published.add(item.name);
      publicItems.push({ crate, name: item.name, file: root, line: item.line });
    }

    for (const item of reExports(source)) {
      if (item.glob) {
        wildcardExports.push({
          crate,
          path: [...item.path, '*'].join('::'),
          file: root,
          line: item.line,
        });
        continue;
      }
      if (isModuleLeaf(files, crate, item)) continue;
      if (published.has(item.name)) continue;
      published.add(item.name);
      const site = declarationSite(files, crate, item);
      publicItems.push({ crate, name: item.name, file: site.file, line: site.line });
    }
  }

  return { crates, publicItems, wildcardExports };
}
