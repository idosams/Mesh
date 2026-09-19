import assert from 'node:assert/strict';
import test from 'node:test';
import {
  readRenderedProofImage,
  renderedProofImageFromBuildScript,
} from './rendered-proof-image.mjs';

test('the rendered proof reuses the generated public-source image without an ignored file', async () => {
  const bytes = await readRenderedProofImage();
  assert.equal(bytes.subarray(0, 8).toString('hex'), '89504e470d0a1a0a');
  assert.ok(bytes.length > 100);
});

test('the rendered proof image parser refuses missing, duplicate, or non-PNG definitions', () => {
  const png = Buffer.from('89504e470d0a1a0a00', 'hex').toString('base64');
  const definition = `const DEVELOPMENT_ICON_BASE64: &str = "${png}";`;
  assert.throws(
    () => renderedProofImageFromBuildScript('fn main() {}'),
    /must define one exact proof image/,
  );
  assert.throws(
    () => renderedProofImageFromBuildScript(`${definition}\n${definition}`),
    /must define one exact proof image/,
  );
  assert.throws(
    () => renderedProofImageFromBuildScript(
      `const DEVELOPMENT_ICON_BASE64: &str = "${Buffer.from('not png').toString('base64')}";`,
    ),
    /must be PNG/,
  );
});
