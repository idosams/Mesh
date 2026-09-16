// Plan §8.3's last bullet, as a test: the user interface may not write the database directly.
//
// # What this checks and what it cannot
//
// It reads every source under `apps/desktop/src/` and decides, from the import graph and from the
// text, whether this application can reach storage at all. That is a strong statement for a Node
// application, because a Node application reaches storage exactly two ways — a module it imports,
// or a filesystem call — and both are in the text.
//
// The IPC coordinator under `src/` has no npm dependencies. The component-first presentation
// under `ui-next/` has one exact, locked build closure that compiles to an inert browser asset;
// its own boundary test proves that source has no Tauri, network, storage, or socket path. The
// native host has an explicit Cargo graph; the final test holds its direct application edge to
// mesh-daemon and delegates closure capability enforcement to the repository architecture checker.
//
// The Rust half of the same bullet is `tools/program/arch-check/check.mjs`, which reads
// `tools/program/arch-check/architecture.json` and holds `apps/desktop` to the `ui` layer with a
// `direct` restriction on the `database` capability. The two are deliberately different
// instruments on the same rule: that one reads manifests, this one reads source.
//
// This file names every module it searches for, because it searches for them. It exempts itself
// from its own text scan the same way `crates/mesh-types/src/no_ambient_io.rs` does, and the
// exemption is one file wide and written down.

import assert from 'node:assert/strict';
import { readFileSync, readdirSync, statSync } from 'node:fs';
import { dirname, join, relative } from 'node:path';
import { fileURLToPath } from 'node:url';
import { describe, it } from 'node:test';

const HERE = dirname(fileURLToPath(import.meta.url));
const SRC = join(HERE, '..');
const APP_ROOT = join(SRC, '..');
const SELF = fileURLToPath(import.meta.url);

/** Every `.ts` file under `src/`. */
const sources = (): string[] => {
  const found: string[] = [];
  const walk = (directory: string): void => {
    for (const entry of readdirSync(directory).sort()) {
      const path = join(directory, entry);
      if (statSync(path).isDirectory()) walk(path);
      else if (entry.endsWith('.ts')) found.push(path);
    }
  };
  walk(SRC);
  return found;
};

/** Every module specifier imported by `text`: static, dynamic, and the CommonJS form. */
const importsOf = (text: string): string[] => {
  const found: string[] = [];
  for (const match of text.matchAll(/(?:^|\n)\s*import\s[^;]*?from\s+['"]([^'"]+)['"]/g)) found.push(match[1] ?? '');
  for (const match of text.matchAll(/\bimport\s*\(\s*['"]([^'"]+)['"]\s*\)/g)) found.push(match[1] ?? '');
  for (const match of text.matchAll(/\brequire\s*\(\s*['"]([^'"]+)['"]\s*\)/g)) found.push(match[1] ?? '');
  return found;
};

/** Builtins a shipped module may reach. Everything else must be a relative path. */
const RUNTIME_BUILTINS = new Set(['node:net']);

/** Builtins a test module may reach on top of those. Tests read fixtures; the application does not. */
const TEST_BUILTINS = new Set([
  'node:test',
  'node:assert/strict',
  'node:fs',
  'node:fs/promises',
  'node:os',
  'node:path',
  'node:url',
]);

/** Any of these in the text of a source under `src/` is a route to storage or to another process. */
const STORAGE_MARKERS = ['node:sqlite', 'better-sqlite3', 'sqlite3', 'mesh-store', 'mesh_store', 'child_process'];

const files = sources();
const isTest = (path: string): boolean => path.endsWith('.test.ts');
const shipped = files.filter((file) => !isTest(file));
const name = (path: string): string => relative(APP_ROOT, path);

describe('the desktop application’s architecture', () => {
  it('has sources to check at all', () => {
    assert.ok(files.length >= 8, `only ${files.length} sources found — the walk broke`);
    assert.ok(shipped.length >= 5, `only ${shipped.length} shipped sources found — the walk broke`);
  });

  it('imports nothing but relative modules and a tiny set of Node builtins', () => {
    for (const file of files) {
      const allowed = isTest(file) ? new Set([...RUNTIME_BUILTINS, ...TEST_BUILTINS]) : RUNTIME_BUILTINS;
      for (const specifier of importsOf(readFileSync(file, 'utf8'))) {
        assert.ok(
          specifier.startsWith('.') || allowed.has(specifier),
          `${name(file)} imports \`${specifier}\`, which is neither a relative module nor an allowed builtin`,
        );
      }
    }
  });

  it('has no route to a database, a store or another process, in any source', () => {
    for (const file of files) {
      if (file === SELF) continue; // the scanner names what it scans for; see the header.
      const text = readFileSync(file, 'utf8');
      for (const marker of STORAGE_MARKERS) {
        assert.ok(
          !text.includes(marker),
          `${name(file)} names \`${marker}\`: this application reaches state through the IPC surface only`,
        );
      }
    }
  });

  it('opens a socket in exactly one shipped source', () => {
    // Shipped sources only: a `.test.ts` file is not part of the application, and the running
    // process is checked separately and more strongly in `src/ipc/no-network.test.ts`.
    const openers = shipped.filter((file) => file !== SELF && readFileSync(file, 'utf8').includes('node:net'));
    assert.deepEqual(openers.map(name), ['src/ipc/transport.ts'], 'more than one shipped source opens a socket');
  });

  it('keeps the IPC coordinator dependency-free, the React closure exact, and the native host away from database crates', () => {
    const manifest = JSON.parse(readFileSync(join(APP_ROOT, 'package.json'), 'utf8')) as {
      dependencies?: Record<string, string>;
      devDependencies?: Record<string, string>;
    };
    assert.equal(manifest.dependencies, undefined, 'a dependency was added; the closure claim above needs re-arguing');
    assert.equal(manifest.devDependencies, undefined, 'a dev dependency was added; see the note at the top of this file');
    const uiManifest = JSON.parse(readFileSync(join(APP_ROOT, 'ui-next', 'package.json'), 'utf8')) as {
      private?: boolean;
      dependencies?: Record<string, string>;
      devDependencies?: Record<string, string>;
    };
    assert.equal(uiManifest.private, true, 'the embedded React package must never become publishable');
    assert.equal(uiManifest.dependencies, undefined, 'the React island uses one reviewed build closure');
    const expectedUiClosure = [
      '@tailwindcss/vite',
      '@types/react',
      '@types/react-dom',
      'class-variance-authority',
      'clsx',
      'esbuild',
      'jiti',
      'react',
      'react-dom',
      'tailwind-merge',
      'tailwindcss',
      'typescript',
      'vite',
    ];
    assert.deepEqual(
      Object.keys(uiManifest.devDependencies ?? {}).sort(),
      expectedUiClosure,
      'the embedded React dependency closure changed without an architecture review',
    );
    for (const [dependency, source] of Object.entries(uiManifest.devDependencies ?? {})) {
      assert.match(
        source,
        new RegExp(`^https://registry\\.npmjs\\.org/${dependency.replace('/', '\\/')}/-/[^/]+-[0-9][^/]*\\.tgz$`),
        `${dependency} is not pinned to one exact registry tarball`,
      );
    }
    const cargo = readFileSync(join(APP_ROOT, 'src-tauri', 'Cargo.toml'), 'utf8');
    assert.match(cargo, /mesh-daemon\s*=\s*\{/);
    for (const forbidden of ['mesh-store', 'rusqlite', 'sqlite3', 'mesh-cas']) {
      assert.ok(!cargo.includes(forbidden), `the native host directly depends on ${forbidden}`);
    }
  });
});
