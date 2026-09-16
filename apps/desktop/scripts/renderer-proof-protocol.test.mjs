import assert from 'node:assert/strict';
import test from 'node:test';

import {
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
    interaction: 'preview-path',
    outcome: 'verified-preview',
    ...overrides,
  };
}

test('renderer proof accepts one exact nonce-bound surface result', () => {
  assert.deepEqual(parseRendererProofReport(report(), {
    nonce,
    surface: 'onboarding',
  }), report());
  assert.deepEqual(parseRendererProofReport(report({
    surface: 'review',
    interaction: 'content-inline',
    outcome: 'content-inline-selected',
  }), {
    nonce,
    surface: 'review',
  }), report({
    surface: 'review',
    interaction: 'content-inline',
    outcome: 'content-inline-selected',
  }));
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
  }), [report()]);
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
    schema: 'mesh-rendered-app-proof/v4',
    component_interface_mounted: true,
    renderer_controls_driven: true,
    renderer: {
      schema: 'mesh-packaged-renderer-proof/v4',
      nonce_bound: true,
      onboarding: {
        surface: 'onboarding', mounted: true, visible: true,
        interaction: 'preview-path', outcome: 'verified-preview',
      },
      review: {
        surface: 'review', mounted: true, visible: true,
        interaction: 'content-inline', outcome: 'content-inline-selected',
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

test('the final rendered-app claim is derived only from all five exact surface proofs', () => {
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
