import assert from 'node:assert/strict';
import test from 'node:test';

import {
  assertCompleteArchiveFixtureProof,
  parseRenderedAppProofOutput,
  parseRendererProofReport,
  rendererProofReportsFromText,
} from './renderer-proof-protocol.mjs';

const nonce = 'ab'.repeat(32);

function report(overrides = {}) {
  return {
    schema: 'mesh-renderer-proof/v1',
    nonce,
    surface: 'onboarding',
    mounted: true,
    visible: true,
    interaction: 'preview-path-confirm-import',
    outcome: 'import-completed-after-busy',
    ...overrides,
  };
}

test('renderer proof accepts one exact nonce-bound surface result', () => {
  assert.deepEqual(parseRendererProofReport(report(), {
    nonce,
    surface: 'onboarding',
  }), report());
  assert.deepEqual(parseRendererProofReport(report({
    surface: 'files',
    interaction: 'expand-select-open-reveal-folders',
    outcome: 'native-file-and-folder-actions-completed',
  }), {
    nonce,
    surface: 'files',
  }), report({
    surface: 'files',
    interaction: 'expand-select-open-reveal-folders',
    outcome: 'native-file-and-folder-actions-completed',
  }));
  assert.deepEqual(parseRendererProofReport(report({
    surface: 'review',
    interaction: 'content-inline-native-open-reveal',
    outcome: 'saved-side-native-launches-completed',
  }), {
    nonce,
    surface: 'review',
  }), report({
    surface: 'review',
    interaction: 'content-inline-native-open-reveal',
    outcome: 'saved-side-native-launches-completed',
  }));
  assert.deepEqual(parseRendererProofReport(report({
    surface: 'review',
    interaction: 'bounded-incomplete-review-inspection',
    outcome: 'incomplete-review-disclosed-without-authority',
  }), {
    nonce,
    surface: 'review',
  }), report({
    surface: 'review',
    interaction: 'bounded-incomplete-review-inspection',
    outcome: 'incomplete-review-disclosed-without-authority',
  }));
  assert.throws(() => parseRendererProofReport(report({
    surface: 'review',
    interaction: 'bounded-incomplete-review-inspection',
    outcome: 'saved-side-native-launches-completed',
  }), {
    nonce,
    surface: 'review',
  }), /interaction and outcome/);
  assert.deepEqual(parseRendererProofReport(report({
    surface: 'versions',
    interaction: 'select-saved-point',
    outcome: 'verified-preview-ready',
  }), {
    nonce,
    surface: 'versions',
  }), report({
    surface: 'versions',
    interaction: 'select-saved-point',
    outcome: 'verified-preview-ready',
  }));
  assert.deepEqual(parseRendererProofReport(report({
    surface: 'private-export',
    interaction: 'refuse-original-then-confirm-private',
    outcome: 'private-export-completed',
  }), {
    nonce,
    surface: 'private-export',
  }), report({
    surface: 'private-export',
    interaction: 'refuse-original-then-confirm-private',
    outcome: 'private-export-completed',
  }));
  assert.deepEqual(parseRendererProofReport(report({
    surface: 'private-export',
    interaction: 'bounded-review-private-export-refusal',
    outcome: 'private-export-blocked-without-complete-review',
  }), {
    nonce,
    surface: 'private-export',
  }), report({
    surface: 'private-export',
    interaction: 'bounded-review-private-export-refusal',
    outcome: 'private-export-blocked-without-complete-review',
  }));
  assert.deepEqual(parseRendererProofReport(report({
    surface: 'agent-handoff',
    interaction: 'start-finish-rescan',
    outcome: 'agent-handoff-completed',
  }), {
    nonce,
    surface: 'agent-handoff',
  }), report({
    surface: 'agent-handoff',
    interaction: 'start-finish-rescan',
    outcome: 'agent-handoff-completed',
  }));
});

test('renderer proof rejects stale, incomplete, invented, or unbounded claims', () => {
  for (const [candidate, expected, message] of [
    [report({ nonce: 'cd'.repeat(32) }), { nonce, surface: 'onboarding' }, /nonce/],
    [report({ surface: 'review' }), { nonce, surface: 'onboarding' }, /surface/],
    [report({ mounted: false }), { nonce, surface: 'onboarding' }, /mounted/],
    [report({ visible: false }), { nonce, surface: 'onboarding' }, /visible/],
    [report({ interaction: 'content-inline' }), { nonce, surface: 'onboarding' }, /interaction/],
    [report({ outcome: 'content-inline-selected' }), { nonce, surface: 'onboarding' }, /outcome/],
    [{ ...report(), nativeAuthority: true }, { nonce, surface: 'onboarding' }, /fields/],
    [report({ outcome: 'x'.repeat(5_000) }), { nonce, surface: 'onboarding' }, /bounded/],
  ]) {
    assert.throws(() => parseRendererProofReport(candidate, expected), message);
  }
});

test('stderr extraction is prefix-scoped, bounded, and refuses duplicate reports', () => {
  const line = `mesh-renderer-proof:${JSON.stringify(report())}`;
  assert.deepEqual(rendererProofReportsFromText(`ordinary diagnostic\n${line}\n`, {
    nonce,
    surface: 'onboarding',
    interaction: 'preview-path-confirm-import',
    outcome: 'import-completed-after-busy',
  }), [report()]);
  assert.throws(
    () => rendererProofReportsFromText(
      `mesh-renderer-proof:${JSON.stringify(report({
        surface: 'review',
        interaction: 'bounded-incomplete-review-inspection',
        outcome: 'incomplete-review-disclosed-without-authority',
      }))}\n`,
      {
        nonce,
        surface: 'review',
        interaction: 'content-inline-native-open-reveal',
        outcome: 'saved-side-native-launches-completed',
      },
    ),
    /required claim/,
    'a complete fixture claim must reject the real-workspace bounded alternative',
  );
  assert.throws(
    () => rendererProofReportsFromText(`${line}\n${line}\n`, { nonce, surface: 'onboarding' }),
    /exactly one/,
  );
  assert.throws(
    () => rendererProofReportsFromText('mesh-renderer-proof:{broken}\n', { nonce, surface: 'onboarding' }),
    /valid JSON/,
  );
  assert.throws(
    () => rendererProofReportsFromText(`mesh-renderer-proof:${'x'.repeat(5_000)}\n`, { nonce, surface: 'onboarding' }),
    /bounded/,
  );
});

function renderedProof(overrides = {}) {
  return {
    schema: 'mesh-rendered-app-proof/v6',
    screenshot: null,
    component_interface_mounted: true,
    renderer_controls_driven: true,
    renderer: {
      schema: 'mesh-packaged-renderer-proof/v5',
      nonce_bound: true,
      onboarding: {
        surface: 'onboarding', mounted: true, visible: true,
        interaction: 'preview-path-confirm-import', outcome: 'import-completed-after-busy',
      },
      files: {
        surface: 'files', mounted: true, visible: true,
        interaction: 'expand-select-open-reveal-folders',
        outcome: 'native-file-and-folder-actions-completed',
      },
      review: {
        surface: 'review', mounted: true, visible: true,
        interaction: 'content-inline-native-open-reveal',
        outcome: 'saved-side-native-launches-completed',
      },
      versions: {
        surface: 'versions', mounted: true, visible: true,
        interaction: 'select-saved-point', outcome: 'verified-preview-ready',
      },
      private_export: {
        surface: 'private-export', mounted: true, visible: true,
        interaction: 'refuse-original-then-confirm-private', outcome: 'private-export-completed',
      },
      agent_handoff: {
        surface: 'agent-handoff', mounted: true, visible: true,
        interaction: 'start-finish-rescan', outcome: 'agent-handoff-completed',
      },
    },
    ...overrides,
  };
}

test('the final rendered-app claim is derived only from all six exact surface proofs', () => {
  const proof = renderedProof();
  assert.deepEqual(parseRenderedAppProofOutput(`${JSON.stringify(proof)}\n`), proof);
  for (const candidate of [
    renderedProof({ component_interface_mounted: false }),
    renderedProof({ renderer_controls_driven: false }),
    renderedProof({ renderer: { ...proof.renderer, nonce_bound: false } }),
    renderedProof({ renderer: { ...proof.renderer, review: { ...proof.renderer.review, visible: false } } }),
  ]) {
    assert.throws(() => parseRenderedAppProofOutput(`${JSON.stringify(candidate)}\n`), /rendered|renderer|surface/);
  }
  const { private_export: _omitted, ...withoutPrivateExport } = proof.renderer;
  assert.throws(
    () => parseRenderedAppProofOutput(`${JSON.stringify(renderedProof({ renderer: withoutPrivateExport }))}\n`),
    /deep-equal/,
    'the archive claim must fail closed when private-export proof is absent',
  );
  const { files: _files, ...withoutFiles } = proof.renderer;
  assert.throws(
    () => parseRenderedAppProofOutput(`${JSON.stringify(renderedProof({ renderer: withoutFiles }))}\n`),
    /deep-equal/,
    'the archive claim must fail closed when Files proof is absent',
  );
  const { versions: _versions, ...withoutVersions } = proof.renderer;
  assert.throws(
    () => parseRenderedAppProofOutput(`${JSON.stringify(renderedProof({ renderer: withoutVersions }))}\n`),
    /deep-equal/,
    'the archive claim must fail closed when workspace-version proof is absent',
  );
  const { agent_handoff: _agentHandoff, ...withoutAgentHandoff } = proof.renderer;
  assert.throws(
    () => parseRenderedAppProofOutput(`${JSON.stringify(renderedProof({ renderer: withoutAgentHandoff }))}\n`),
    /deep-equal/,
    'the archive claim must fail closed when agent-handoff proof is absent',
  );
  assert.throws(
    () => parseRenderedAppProofOutput(`${JSON.stringify(proof)}\n${JSON.stringify(proof)}\n`),
    /exactly one/,
  );
});

test('archive fixtures require complete Review and completed private export', () => {
  const complete = renderedProof();
  assert.equal(assertCompleteArchiveFixtureProof(complete), complete);
  for (const candidate of [
    renderedProof({
      renderer: {
        ...complete.renderer,
        review: {
          surface: 'review', mounted: true, visible: true,
          interaction: 'bounded-incomplete-review-inspection',
          outcome: 'incomplete-review-disclosed-without-authority',
        },
      },
    }),
    renderedProof({
      renderer: {
        ...complete.renderer,
        private_export: {
          surface: 'private-export', mounted: true, visible: true,
          interaction: 'bounded-review-private-export-refusal',
          outcome: 'private-export-blocked-without-complete-review',
        },
      },
    }),
  ]) {
    assert.throws(
      () => assertCompleteArchiveFixtureProof(
        parseRenderedAppProofOutput(`${JSON.stringify(candidate)}\n`),
      ),
      /archive fixture/,
    );
  }
});

test('the final rendered-app claim closes screenshot evidence over its native receipt fields', () => {
  const screenshot = {
    schema: 'mesh-rendered-screenshot-proof/v1',
    path: '/tmp/mesh-files.png',
    nonce: 'ab'.repeat(32),
    sha256: 'cd'.repeat(32),
    bytes: 128_000,
    width: 1_200,
    height: 800,
  };
  const proof = renderedProof({ screenshot });
  assert.deepEqual(parseRenderedAppProofOutput(`${JSON.stringify(proof)}\n`), proof);

  const { screenshot: _missing, ...withoutScreenshot } = proof;
  for (const candidate of [
    withoutScreenshot,
    renderedProof({ screenshot: { ...screenshot, sha256: 'not-a-digest' } }),
    renderedProof({ screenshot: { ...screenshot, nonce: 'ab' } }),
    renderedProof({ screenshot: { ...screenshot, path: 'relative.png' } }),
    renderedProof({ screenshot: { ...screenshot, bytes: 0 } }),
    renderedProof({ screenshot: { ...screenshot, width: 20 } }),
    renderedProof({ screenshot: { ...screenshot, height: 20 } }),
    renderedProof({ screenshot: { ...screenshot, invented: true } }),
  ]) {
    assert.throws(
      () => parseRenderedAppProofOutput(`${JSON.stringify(candidate)}\n`),
      /screenshot|deep-equal/,
    );
  }
});
