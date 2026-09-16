import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import test from 'node:test';
import {
  ARCHIVE_SIGNED_ALPHA_APP_USAGE,
  parseArchiveSignedAlphaArguments,
} from './archive-signed-alpha-app-args.mjs';

const archiver = fileURLToPath(new URL('./archive-signed-alpha-app.mjs', import.meta.url));

test('signed archive arguments require the external identity inputs', () => {
  assert.deepEqual(parseArchiveSignedAlphaArguments([
    '--profile', '/private/Mesh.provisionprofile',
    '--notary-profile', 'Mesh Notary',
    '--identity', 'a'.repeat(40),
  ]), {
    help: false,
    profile: '/private/Mesh.provisionprofile',
    notaryProfile: 'Mesh Notary',
    identity: 'A'.repeat(40),
  });
  assert.deepEqual(parseArchiveSignedAlphaArguments(['--help']), {
    help: true,
    profile: null,
    notaryProfile: null,
    identity: null,
  });
});

test('signed archive arguments fail closed before external work', () => {
  for (const [argv, message] of [
    [[], '--profile is required'],
    [['--profile', '/p'], '--notary-profile is required'],
    [['--notary-profile', 'n'], '--profile is required'],
    [['--profile', '--help'], '--profile requires a value'],
    [['--identity', 'abcd', '--profile', '/p', '--notary-profile', 'n'], 'exact 40-character'],
    [['--help', '--profile', '/p'], 'help must be used without other arguments'],
    [['--unknown'], 'unknown argument: --unknown'],
  ]) assert.throws(() => parseArchiveSignedAlphaArguments(argv), new RegExp(message.replaceAll('-', '\\-')));
});

test('signed archive help is secret-free and performs no tool inspection', () => {
  const result = spawnSync(process.execPath, [archiver, '--help'], {
    encoding: 'utf8',
    env: { ...process.env, PATH: '/mesh-signed-alpha-test/no-tools' },
  });
  assert.equal(result.status, 0, result.stderr);
  assert.equal(result.stderr, '');
  assert.equal(result.stdout, `${ARCHIVE_SIGNED_ALPHA_APP_USAGE}\n`);
});

test('signed archive command refusals are concise and actionable', () => {
  const result = spawnSync(process.execPath, [archiver], {
    encoding: 'utf8',
    env: { ...process.env, PATH: '/mesh-signed-alpha-test/no-tools' },
  });
  assert.equal(result.status, 1);
  assert.equal(result.stdout, '');
  assert.equal(
    result.stderr,
    'Mesh signed alpha archive refused: --profile is required\n',
  );
  assert.doesNotMatch(result.stderr, /\n\s+at /);
});
