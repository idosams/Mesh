import test from 'node:test';
import assert from 'node:assert/strict';
import { fleetArtifactSide, loadFleetArtifact } from './fleet-artifact-preview.js';
const selection = { objective: `fleet-${'a'.repeat(64)}`, lane: 'lane', checkpoint: 'check', version: 'b'.repeat(64), bundle: 'c'.repeat(64) };
const summary = { kind: 'binary', version_id: 'd'.repeat(64), content_digest: 'e'.repeat(64) };
const change = { object_id: 'f'.repeat(32), path_before: null, path_after: 'slide.pdf', before: null, after: summary };
const answer = (page = 1) => ({ schema: 'mesh.fleet-artifact-preview/v1', objective: selection.objective,
  selection: Object.fromEntries(Object.entries(selection).filter(([key]) => key !== 'objective')), object: change.object_id,
  preview: { renderer: 'macos-pdfkit-page-v1', scope: 'exact-page-preview', kind: 'pdf', side: 'after', version_id: summary.version_id, content_digest: summary.content_digest,
    image_data_url: 'data:image/png;base64,AAAA', text_source: null, text_lines: null, text_sections: null, text_truncated: false, page_number: page, page_count: 4, rendering_authorizes_approval: false } });
test('artifact side binds the full selection, object, content identity and requested page', () => {
  assert.equal(fleetArtifactSide(JSON.stringify(answer()), selection, change, 'after', 'pdf', 1).pageCount, 4);
  for (const mutate of [v => v.objective = 'other', v => v.object = 'a'.repeat(32), v => v.selection.lane = 'other', v => v.selection.checkpoint = 'other', v => v.selection.version = 'a'.repeat(64), v => v.selection.bundle = 'b'.repeat(64), v => v.selection.path = '/tmp', v => v.extra = true, v => v.preview.version_id = '0'.repeat(64), v => v.preview.content_digest = '0'.repeat(64), v => v.preview.side = 'before', v => v.preview.page_number = 2, v => v.preview.rendering_authorizes_approval = true, v => v.preview.image_data_url = 'file:///tmp/private', v => v.preview.text_lines = ['unbound']]) {
    const value = answer(); mutate(value); assert.throws(() => fleetArtifactSide(value, selection, change, 'after', 'pdf', 1));
  }
});
test('unequal PDF page counts represent absence only from previously verified exact side evidence', async () => {
  const both = { ...change, path_before: change.path_after, before: summary };
  const known = { side: 'before', versionId: summary.version_id, contentDigest: summary.content_digest, pageCount: 1 };
  const calls = [];
  const envelope = await loadFleetArtifact(async (command, args) => { calls.push({ command, args }); return answer(2); },
    { selection, artifact: { envelope: { changeId: change.object_id, before: known } } }, both, 2, 9);
  assert.deepEqual(envelope.beforeAbsentPage, known); assert.equal(envelope.after.pageNumber, 2);
  assert.equal(calls.length, 1); assert.deepEqual(calls[0].args, { ...selection, objectId: change.object_id, side: 'after', pageNumber: 2 });
  const failed = await loadFleetArtifact(async (_, args) => { if (args.side === 'before') throw new Error('private path'); return answer(2); },
    { selection, artifact: { envelope: { changeId: 'other', before: known } } }, both, 2, 10);
  assert.equal(failed.beforeAbsentPage, null); assert.match(failed.beforeError, /unavailable/); assert.doesNotMatch(failed.beforeError, /private path/);
});
test('invalid pages, unsupported types and total rendering failures refuse without retaining earlier content', async () => {
  let calls = 0; const invoke = async () => { calls++; throw new Error('sensitive'); };
  for (const page of [0, 65, 1.5, '1']) await assert.rejects(loadFleetArtifact(invoke, { selection }, change, page, 1));
  await assert.rejects(loadFleetArtifact(invoke, { selection }, { ...change, path_after: 'unknown.zip' }, 1, 1));
  assert.equal(calls, 0);
  await assert.rejects(loadFleetArtifact(invoke, { selection }, change, 1, 1), /No artifact preview/); assert.equal(calls, 1);
});
