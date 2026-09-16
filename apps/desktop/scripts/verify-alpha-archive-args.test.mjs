import assert from 'node:assert/strict';
import { mkdtempSync, readdirSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { spawnSync } from 'node:child_process';
import test from 'node:test';

const verifier = new URL('./verify-alpha-archive.mjs', import.meta.url);

test('alpha verifier help exits before requiring or inspecting an artifact', () => {
  for (const argument of ['--help', '-h']) {
    const empty = mkdtempSync(join(tmpdir(), 'mesh-alpha-verifier-help-'));
    try {
      const result = spawnSync(process.execPath, [verifier.pathname, argument], {
        cwd: empty,
        encoding: 'utf8',
      });
      assert.equal(result.status, 0, result.stderr);
      assert.match(result.stdout, /Usage: npm run tauri:verify-alpha/);
      assert.match(result.stdout, /--manifest <path>/);
      assert.deepEqual(readdirSync(empty), []);
    } finally {
      rmSync(empty, { recursive: true, force: true });
    }
  }
});

test('alpha verifier options cannot be swallowed as manifest values', () => {
  const cases = [
    [['--manifest', '--prove-rendered'], '--manifest requires a value'],
    [['--manifest'], '--manifest requires a value'],
    [['--help', '--manifest', 'release.json'], 'help must be used without other arguments'],
    [['--manifest', 'release.json', '--help'], 'help must be used without other arguments'],
  ];
  for (const [arguments_, message] of cases) {
    const result = spawnSync(process.execPath, [verifier.pathname, ...arguments_], {
      encoding: 'utf8',
    });
    assert.notEqual(result.status, 0, `accepted ${arguments_.join(' ')}`);
    assert.match(result.stderr, new RegExp(message));
  }
});
