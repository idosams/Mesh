#!/usr/bin/env node

import { readFileSync } from 'node:fs';

const path = process.argv[2];
if (!path) throw new Error('usage: verify.mjs <results.jsonl> | --self-test');

function verify(rows) {
  if (rows.length !== 5) throw new Error(`expected 5 fresh-process rows, found ${rows.length}`);
  const processes = new Set();
  const novelBytes = new Set();
  for (const [index, row] of rows.entries()) {
    const expectedSample = index + 1;
    if (row.sample !== expectedSample) throw new Error(`row ${expectedSample} has sample ${row.sample}`);
    if (row.workload !== 'W5' || row.scale !== 'reduced') throw new Error(`row ${expectedSample} is not W5 reduced`);
    if (row.generator !== 'mesh-bench/corpus/W5' || row.generator_version !== '1' || row.seed !== 42) {
      throw new Error(`row ${expectedSample} changed the pinned generator`);
    }
    if (row.base_bytes !== 67_108_864 || row.edit !== 'overwrite' || row.edit_bytes !== 1_024) {
      throw new Error(`row ${expectedSample} changed the pinned edit`);
    }
    if (row.physical_paging !== true || row.paging_threshold_references !== 256) {
      throw new Error(`row ${expectedSample} did not exercise candidate-B paging`);
    }
    for (const field of ['baseline_staged_objects', 'edited_staged_objects', 'edited_reused_objects',
      'generation_ms', 'baseline_save_ms', 'edited_save_ms', 'reconstruction_ms', 'cleanup_ms', 'total_ms']) {
      if (!Number.isSafeInteger(row[field]) || row[field] < 0) {
        throw new Error(`row ${expectedSample} has invalid ${field}`);
      }
    }
    if (row.baseline_staged_objects <= 0 || row.edited_reused_objects <= 0 ||
        row.edited_staged_objects >= row.baseline_staged_objects) {
      throw new Error(`row ${expectedSample} did not exercise bounded reuse`);
    }
    if (row.total_ms <= 0 || row.total_ms >= 120_000) {
      throw new Error(`row ${expectedSample} did not complete inside the 120 second collection bound`);
    }
    if (!Number.isSafeInteger(row.process_id) || processes.has(row.process_id)) {
      throw new Error(`row ${expectedSample} did not come from a distinct process`);
    }
    processes.add(row.process_id);
    if (!Number.isSafeInteger(row.novel_cas_bytes) || row.novel_cas_bytes <= 0 || row.novel_cas_bytes >= 4 * 1024 * 1024) {
      throw new Error(`row ${expectedSample} violates the delivery ceiling`);
    }
    novelBytes.add(row.novel_cas_bytes);
    if (row.ceiling_bytes !== 4 * 1024 * 1024 || row.roundtrip_checked !== true || row.roundtrip_failures !== 0) {
      throw new Error(`row ${expectedSample} did not retain a passing reconstruction`);
    }
  }
  if (novelBytes.size !== 1) throw new Error(`byte figures disagreed across processes: ${[...novelBytes].join(', ')}`);
  return [...novelBytes][0];
}

if (path === '--self-test') {
  const base = {
    workload: 'W5', scale: 'reduced', generator: 'mesh-bench/corpus/W5', generator_version: '1', seed: 42,
    base_bytes: 67_108_864, edit: 'overwrite', edit_bytes: 1_024,
    physical_paging: true, paging_threshold_references: 256, novel_cas_bytes: 32_768,
    baseline_staged_objects: 16_000, edited_staged_objects: 8, edited_reused_objects: 15_992,
    generation_ms: 100, baseline_save_ms: 20_000, edited_save_ms: 500,
    reconstruction_ms: 200, cleanup_ms: 100, total_ms: 20_900,
    ceiling_bytes: 4 * 1024 * 1024, roundtrip_checked: true, roundtrip_failures: 0,
  };
  const valid = Array.from({ length: 5 }, (_, index) => ({ ...base, sample: index + 1, process_id: 100 + index }));
  verify(valid);
  for (const mutation of [
    valid.slice(0, 4),
    valid.map((row, index) => ({ ...row, process_id: index === 4 ? 100 : row.process_id })),
    valid.map((row, index) => ({ ...row, physical_paging: index === 4 ? false : row.physical_paging })),
    valid.map((row, index) => ({ ...row, edited_reused_objects: index === 4 ? 0 : row.edited_reused_objects })),
    valid.map((row, index) => ({ ...row, total_ms: index === 4 ? 120_000 : row.total_ms })),
    valid.map((row, index) => ({ ...row, novel_cas_bytes: index === 4 ? 4 * 1024 * 1024 : row.novel_cas_bytes })),
    valid.map((row, index) => ({ ...row, roundtrip_failures: index === 4 ? 1 : 0 })),
  ]) {
    let refused = false;
    try { verify(mutation); } catch { refused = true; }
    if (!refused) throw new Error('a planted invalid capture was accepted');
  }
  console.log('checkpoint delivery verifier self-test passed: valid capture accepted, 7/7 mutations refused');
} else {
  const rows = readFileSync(path, 'utf8')
    .split(/\r?\n/u)
    .filter(Boolean)
    .map((line, index) => {
      try { return JSON.parse(line); }
      catch (error) { throw new Error(`row ${index + 1} is not JSON: ${error.message}`); }
    });
  const novelBytes = verify(rows);
  console.log(`checkpoint delivery verified: 5/5 fresh processes, ${novelBytes} novel CAS bytes, 0 reconstruction failures`);
}
