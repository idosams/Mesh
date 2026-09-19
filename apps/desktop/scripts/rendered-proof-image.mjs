import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';

const DEVELOPMENT_ICON = /const DEVELOPMENT_ICON_BASE64: &str = "([A-Za-z0-9+/]+={0,2})";/gu;
const PNG_SIGNATURE = '89504e470d0a1a0a';

export function renderedProofImageFromBuildScript(buildScript) {
  assert.equal(typeof buildScript, 'string', 'the desktop build script must be readable text');
  const matches = [...buildScript.matchAll(DEVELOPMENT_ICON)];
  assert.equal(matches.length, 1, 'the desktop build script must define one exact proof image');
  const bytes = Buffer.from(matches[0][1], 'base64');
  assert.equal(
    bytes.subarray(0, 8).toString('hex'),
    PNG_SIGNATURE,
    'the desktop build script proof image must be PNG',
  );
  return bytes;
}

export async function readRenderedProofImage() {
  const buildScript = await readFile(new URL('../src-tauri/build.rs', import.meta.url), 'utf8');
  return renderedProofImageFromBuildScript(buildScript);
}
