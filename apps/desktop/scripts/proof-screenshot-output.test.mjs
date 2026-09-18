import assert from 'node:assert/strict';
import { mkdtemp, readFile, rm, stat, symlink, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import test from 'node:test';
import { writeNewPrivateScreenshot } from './proof-screenshot-output.mjs';

test('verified screenshot output is private, exact, and never follows an existing path', async () => {
  const root = await mkdtemp(join(tmpdir(), 'mesh-proof-screenshot-output-'));
  try {
    const screenshot = join(root, 'files.png');
    const bytes = Buffer.from('verified screenshot bytes');
    await writeNewPrivateScreenshot(screenshot, bytes);
    assert.deepEqual(await readFile(screenshot), bytes);
    assert.equal((await stat(screenshot)).mode & 0o777, 0o600);

    await assert.rejects(
      writeNewPrivateScreenshot(screenshot, Buffer.from('replacement')),
      { code: 'EEXIST' },
    );
    assert.deepEqual(await readFile(screenshot), bytes);

    const victim = join(root, 'victim.txt');
    const linked = join(root, 'linked.png');
    await writeFile(victim, 'preserve me', { mode: 0o600 });
    await symlink(victim, linked);
    await assert.rejects(
      writeNewPrivateScreenshot(linked, Buffer.from('redirected')),
      { code: 'EEXIST' },
    );
    assert.equal(await readFile(victim, 'utf8'), 'preserve me');
  } finally {
    await rm(root, { recursive: true, force: true });
  }
});
