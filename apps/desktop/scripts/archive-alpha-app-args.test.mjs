import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import test from 'node:test';
import {
  ARCHIVE_ALPHA_APP_USAGE,
  parseArchiveArguments,
} from './archive-alpha-app-args.mjs';

const archiver = fileURLToPath(new URL('./archive-alpha-app.mjs', import.meta.url));
const unusablePath = '/mesh-alpha-archive-test/no-tools';

test('archive arguments accept only the documented surface', () => {
  assert.deepEqual(parseArchiveArguments([]), { help: false });
  assert.deepEqual(parseArchiveArguments(['--help']), { help: true });
  assert.deepEqual(parseArchiveArguments(['-h']), { help: true });
});

test('archive arguments fail closed', () => {
  for (const [argv, message] of [
    [['--unknown'], 'unknown argument: --unknown'],
    [['output.zip'], 'unknown argument: output.zip'],
    [['--help', '--unknown'], 'help must be used without other arguments'],
    [['--help', '--help'], 'help must be used without other arguments'],
    [['-h', '--help'], 'help must be used without other arguments'],
  ]) {
    assert.throws(() => parseArchiveArguments(argv), new RegExp(message.replaceAll('-', '\\-')));
  }
});

test('help exits without inspecting Git, building the app, or writing artifacts', () => {
  const result = spawnSync(process.execPath, [archiver, '--help'], {
    encoding: 'utf8',
    env: { ...process.env, PATH: unusablePath },
  });
  assert.equal(result.status, 0, result.stderr);
  assert.equal(result.signal, null);
  assert.equal(result.stderr, '');
  assert.equal(result.stdout, `${ARCHIVE_ALPHA_APP_USAGE}\n`);
});

test('unknown arguments are refused before Git, build, or artifact work', () => {
  const result = spawnSync(process.execPath, [archiver, '--unknown'], {
    encoding: 'utf8',
    env: { ...process.env, PATH: unusablePath },
  });
  assert.equal(result.status, 1);
  assert.match(result.stderr, /unknown argument: --unknown/);
  assert.doesNotMatch(result.stderr, /ENOENT.*git|build-local-app|tauri|Mesh-alpha/);
  assert.equal(result.stdout, '');
});
