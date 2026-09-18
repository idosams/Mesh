import assert from 'node:assert/strict';
import { isAbsolute } from 'node:path';

export const RENDERER_PROOF_PREFIX = 'mesh-renderer-proof:';
const MAX_RENDERER_PROOF_BYTES = 4_096;
const CLAIMS = Object.freeze({
  onboarding: Object.freeze({ interaction: 'preview-path', outcome: 'verified-preview' }),
  files: Object.freeze({
    interaction: 'expand-select-open-reveal-folders',
    outcome: 'native-file-and-folder-actions-completed',
  }),
  review: Object.freeze({
    interaction: 'content-inline-native-open-reveal',
    outcome: 'saved-side-native-launches-completed',
  }),
  versions: Object.freeze({ interaction: 'select-saved-point', outcome: 'verified-preview-ready' }),
  'private-export': Object.freeze({
    interaction: 'refuse-original-then-confirm-private',
    outcome: 'private-export-completed',
  }),
  'agent-handoff': Object.freeze({
    interaction: 'start-finish-rescan',
    outcome: 'agent-handoff-completed',
  }),
});

export function parseRendererProofReport(value, { nonce, surface }) {
  assert.ok(value && typeof value === 'object' && !Array.isArray(value), 'renderer proof must be one object');
  assert.deepEqual(Object.keys(value), [
    'schema',
    'nonce',
    'surface',
    'mounted',
    'visible',
    'interaction',
    'outcome',
  ], 'renderer proof had unrecognized or missing fields');
  assert.match(nonce, /^[0-9a-f]{64}$/u, 'expected renderer proof nonce was invalid');
  assert.ok(CLAIMS[surface], 'expected renderer proof surface was invalid');
  assert.ok(
    Buffer.byteLength(JSON.stringify(value), 'utf8') <= MAX_RENDERER_PROOF_BYTES,
    'renderer proof was not bounded',
  );
  assert.equal(value.schema, 'mesh-renderer-proof/v1');
  assert.equal(value.nonce, nonce, 'renderer proof nonce did not match this process launch');
  assert.equal(value.surface, surface, 'renderer proof described the wrong surface');
  assert.equal(value.mounted, true, 'renderer proof did not establish a mounted React surface');
  assert.equal(value.visible, true, 'renderer proof did not establish a visible React surface');
  assert.equal(value.interaction, CLAIMS[surface].interaction, 'renderer proof used the wrong interaction');
  assert.equal(value.outcome, CLAIMS[surface].outcome, 'renderer proof did not observe the expected outcome');
  return value;
}

export function rendererProofReportsFromText(text, expected) {
  const encoded = String(text)
    .split('\n')
    .filter((line) => line.startsWith(RENDERER_PROOF_PREFIX))
    .map((line) => line.slice(RENDERER_PROOF_PREFIX.length));
  assert.equal(encoded.length, 1, 'renderer process must emit exactly one proof report');
  assert.ok(Buffer.byteLength(encoded[0], 'utf8') <= MAX_RENDERER_PROOF_BYTES, 'renderer proof was not bounded');
  let parsed;
  try {
    parsed = JSON.parse(encoded[0]);
  } catch {
    throw new Error('renderer proof was not valid JSON');
  }
  return [parseRendererProofReport(parsed, expected)];
}

function assertSurfaceClaim(value, expected) {
  assert.ok(value && typeof value === 'object' && !Array.isArray(value), 'rendered surface proof was missing');
  assert.deepEqual(Object.keys(value), ['surface', 'mounted', 'visible', 'interaction', 'outcome']);
  assert.equal(value.surface, expected.surface, 'rendered surface proof named the wrong surface');
  assert.equal(value.mounted, true, 'rendered surface proof did not establish mount');
  assert.equal(value.visible, true, 'rendered surface proof did not establish visibility');
  assert.equal(value.interaction, expected.interaction, 'rendered surface proof used the wrong interaction');
  assert.equal(value.outcome, expected.outcome, 'rendered surface proof did not establish the expected outcome');
}

function assertScreenshotProof(value) {
  if (value === null) return;
  assert.ok(
    value && typeof value === 'object' && !Array.isArray(value),
    'rendered screenshot proof was malformed',
  );
  assert.deepEqual(
    Object.keys(value),
    ['schema', 'path', 'nonce', 'sha256', 'bytes', 'width', 'height'],
    'rendered screenshot proof had unrecognized or missing fields',
  );
  assert.equal(value.schema, 'mesh-rendered-screenshot-proof/v1');
  assert.ok(
    typeof value.path === 'string' && value.path.length <= 4_096 && isAbsolute(value.path),
    'rendered screenshot proof path was invalid',
  );
  assert.match(value.nonce, /^[0-9a-f]{64}$/u, 'rendered screenshot proof nonce was invalid');
  assert.match(value.sha256, /^[0-9a-f]{64}$/u, 'rendered screenshot proof digest was invalid');
  assert.ok(
    Number.isSafeInteger(value.bytes) && value.bytes >= 1_024 && value.bytes <= 16 * 1_024 * 1_024,
    'rendered screenshot proof byte count was invalid',
  );
  assert.ok(
    Number.isSafeInteger(value.width) && value.width >= 320 && value.width <= 8_192,
    'rendered screenshot proof width was invalid',
  );
  assert.ok(
    Number.isSafeInteger(value.height) && value.height >= 240 && value.height <= 8_192,
    'rendered screenshot proof height was invalid',
  );
}

export function parseRenderedAppProofOutput(output) {
  const lines = String(output).trim().split('\n').filter(Boolean);
  assert.equal(lines.length, 1, 'rendered app must emit exactly one final proof object');
  assert.ok(Buffer.byteLength(lines[0], 'utf8') <= 16 * 1024 * 1024, 'rendered app proof was not bounded');
  let value;
  try {
    value = JSON.parse(lines[0]);
  } catch {
    throw new Error('rendered app proof was not valid JSON');
  }
  assert.ok(value && typeof value === 'object' && !Array.isArray(value), 'rendered app proof was missing');
  assert.equal(value.schema, 'mesh-rendered-app-proof/v6');
  assert.ok(Object.hasOwn(value, 'screenshot'), 'rendered screenshot proof was missing');
  assertScreenshotProof(value.screenshot);
  assert.equal(value.component_interface_mounted, true, 'rendered app did not prove the component interface mounted');
  assert.equal(value.renderer_controls_driven, true, 'rendered app did not prove renderer controls were driven');
  assert.ok(value.renderer && typeof value.renderer === 'object' && !Array.isArray(value.renderer), 'renderer proof was missing');
  assert.deepEqual(
    Object.keys(value.renderer),
    ['schema', 'nonce_bound', 'onboarding', 'files', 'review', 'versions', 'private_export', 'agent_handoff'],
  );
  assert.equal(value.renderer.schema, 'mesh-packaged-renderer-proof/v5');
  assert.equal(value.renderer.nonce_bound, true, 'renderer proof was not nonce bound');
  assertSurfaceClaim(value.renderer.onboarding, {
    surface: 'onboarding', interaction: 'preview-path', outcome: 'verified-preview',
  });
  assertSurfaceClaim(value.renderer.files, {
    surface: 'files',
    interaction: 'expand-select-open-reveal-folders',
    outcome: 'native-file-and-folder-actions-completed',
  });
  assertSurfaceClaim(value.renderer.review, {
    surface: 'review',
    interaction: 'content-inline-native-open-reveal',
    outcome: 'saved-side-native-launches-completed',
  });
  assertSurfaceClaim(value.renderer.versions, {
    surface: 'versions', interaction: 'select-saved-point', outcome: 'verified-preview-ready',
  });
  assertSurfaceClaim(value.renderer.private_export, {
    surface: 'private-export',
    interaction: 'refuse-original-then-confirm-private',
    outcome: 'private-export-completed',
  });
  assertSurfaceClaim(value.renderer.agent_handoff, {
    surface: 'agent-handoff',
    interaction: 'start-finish-rescan',
    outcome: 'agent-handoff-completed',
  });
  return value;
}
