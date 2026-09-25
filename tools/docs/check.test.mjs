import assert from 'node:assert/strict';
import { mkdtempSync, mkdirSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { test } from 'node:test';
import { auditDocument } from './check.mjs';

function fixture(t) {
  const root = mkdtempSync(join(tmpdir(), 'mesh-docs-'));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  mkdirSync(join(root, 'docs'));
  mkdirSync(join(root, 'apps/desktop'), { recursive: true });
  writeFileSync(join(root, 'package.json'), JSON.stringify({ scripts: { test: 'node --test', 'verify:docs': 'node tools/docs/check.mjs' } }));
  writeFileSync(join(root, 'apps/desktop/package.json'), JSON.stringify({ scripts: { 'tauri:dev': 'cargo run' } }));
  writeFileSync(join(root, 'docs/guide (one).md'), '# Guide\n');
  return (source) => auditDocument(root, 'docs/index.md', source);
}

test('accepts local links, encoded spaces, directories, references, and external links', (t) => {
  const audit = fixture(t);
  assert.deepEqual(audit('[guide](<guide (one).md>#top)\n[encoded](guide%20%28one%29.md)\n[root](../)\n[web](https://example.test/missing)\n[anchor](#top)\n[ref][guide]\n[guide]: <guide (one).md> "Title"'), []);
});
test('rejects missing local links with file and line evidence', (t) => {
  const audit = fixture(t);
  assert.match(audit('# Hello\n[bad](missing.md)')[0], /^docs\/index.md:2: missing local link/);
  assert.match(audit('[bad][missing]')[0], /undefined link reference/);
  assert.match(audit('[unused]: missing.md')[0], /missing local link/);
});
test('rejects traversal, absolute paths, and invalid encoding', (t) => {
  const audit = fixture(t);
  for (const path of ['../../outside', '/etc/hosts', '%2e%2e/%2e%2e/outside']) assert.match(audit(`[bad](${path})`)[0], /escapes/);
  assert.match(audit('[bad](%zz)')[0], /invalid URL encoding/);
});
test('does not treat literal examples as links and handles long fences', (t) => {
  const audit = fixture(t);
  assert.deepEqual(audit('`[literal](missing.md)`\n````md\n[example](missing.md)\n```\n[example](missing.md)\n````\n~~~md\n[example](missing.md)\n~~~'), []);
});
test('checks executable shell examples and inline scripts against the selected package', (t) => {
  const audit = fixture(t);
  assert.deepEqual(audit('```bash\nnpm run verify:docs\nnpm --prefix apps/desktop run tauri:dev\n```\nUse `npm run test`.'), []);
  assert.match(audit('```sh\nnpm run preflight\n```')[0], /unknown npm script preflight/);
  assert.match(audit('`npm --prefix apps/desktop run test`')[0], /unknown npm script test/);
  assert.match(audit('`npm --prefix absent run test`')[0], /cannot read npm manifest/);
  assert.match(audit('`npm --prefix ../outside run test`')[0], /prefix escapes/);
});
