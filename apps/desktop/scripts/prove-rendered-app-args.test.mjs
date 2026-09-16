import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import test from 'node:test';
import {
  parseProofArguments,
  PROVE_RENDERED_APP_USAGE,
} from './prove-rendered-app-args.mjs';

const proof = fileURLToPath(new URL('./prove-rendered-app.mjs', import.meta.url));

test('proof arguments accept only the documented surface', () => {
  assert.deepEqual(parseProofArguments([]), { help: false, screenshot: null });
  assert.deepEqual(parseProofArguments(['--screenshot', 'proof.png']), {
    help: false,
    screenshot: 'proof.png',
  });
  assert.deepEqual(parseProofArguments(['--help']), { help: true, screenshot: null });
  assert.deepEqual(parseProofArguments(['-h']), { help: true, screenshot: null });
});

test('proof arguments fail closed before execution', () => {
  for (const [argv, message] of [
    [['--unknown'], 'unknown argument: --unknown'],
    [['positional'], 'unknown argument: positional'],
    [['--screenshot'], '--screenshot requires an absolute or relative output path'],
    [['--screenshot', '--help'], '--screenshot requires an absolute or relative output path'],
    [['--screenshot', 'one.png', '--screenshot', 'two.png'], '--screenshot may be provided only once'],
    [['--help', '--unknown'], '--help must be used without other arguments'],
    [['--help', '--help'], '--help must be used without other arguments'],
  ]) {
    assert.throws(() => parseProofArguments(argv), new RegExp(message.replaceAll('-', '\\-')));
  }
});

test('help exits successfully without requiring an app or starting the proof', () => {
  const result = spawnSync(process.execPath, [proof, '--help'], {
    encoding: 'utf8',
    env: { ...process.env, MESH_LOCAL_APP: '/does/not/exist/Mesh.app' },
  });
  assert.equal(result.status, 0, result.stderr);
  assert.equal(result.signal, null);
  assert.equal(result.stderr, '');
  assert.equal(result.stdout, `${PROVE_RENDERED_APP_USAGE}\n`);
});

test('unknown arguments are refused before requiring an app or starting the proof', () => {
  const result = spawnSync(process.execPath, [proof, '--unknown'], {
    encoding: 'utf8',
    env: { ...process.env, MESH_LOCAL_APP: '/does/not/exist/Mesh.app' },
  });
  assert.equal(result.status, 1);
  assert.match(result.stderr, /unknown argument: --unknown/);
  assert.doesNotMatch(result.stderr, /xcrun|ENOENT.*Mesh\.app|mesh-app-/);
  assert.equal(result.stdout, '');
});
