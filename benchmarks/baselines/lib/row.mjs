// Assembling a result row, and refusing it at the door.
//
// Rows go out in `mesh-bench/result/v1` — the same schema, the same version
// string, the same required fields as a Mesh row, so `mesh-bench validate`
// accepts a baseline row and `mesh-bench compare` can hold one against the
// other. The two extra sections (`baseline`, `footprint`) are additive: the Rust
// decoder reads the fields it names and ignores the rest, which is what lets a
// baseline carry its version pin without forking the schema.

import { appendFileSync, mkdirSync } from 'node:fs';
import { dirname } from 'node:path';
import { latencySummary } from './stats.mjs';

/** The schema version this runner writes, and the one mesh-bench demands. */
export const SCHEMA_VERSION = 'mesh-bench/result/v1';

/** What a sink refuses beyond the schema — mirrors `crates/mesh-bench/src/sink.rs`. */
export const PUBLISHABLE = {
  name: 'publishable',
  allow_dirty_worktree: false,
  min_sample_count: 20,
  max_failure_permille: 0,
  require_pin_satisfied: true,
};

/** The looser local policy. Exists so "I just want a number" is a different sink. */
export const EXPLORATORY = {
  name: 'exploratory',
  allow_dirty_worktree: true,
  min_sample_count: 1,
  max_failure_permille: 1000,
  require_pin_satisfied: false,
};

/** Builds one row. Throws if the samples cannot support a summary. */
export function buildRow(parts) {
  const latency = latencySummary(parts.samplesNs);
  if (latency === null) {
    throw new Error('row: no samples, so no percentiles — a run with nothing in it is not a result');
  }
  return {
    schema_version: SCHEMA_VERSION,
    benchmark_id: parts.benchmarkId,
    invocation: parts.invocation,
    recorded_at_unix_ms: parts.recordedAtUnixMs,
    repository: parts.repository,
    hardware: parts.hardware,
    platform: parts.platform,
    build: parts.build,
    workload: parts.workload,
    cache_state: parts.cacheState,
    sample_count: parts.samplesNs.length,
    iterations_attempted: parts.samplesNs.length + parts.failureCount,
    failure_count: parts.failureCount,
    samples_ns: parts.samplesNs,
    latency,
    verification: parts.verification,
    baseline: parts.baseline,
    footprint: parts.footprint,
  };
}

/**
 * Checks a row against a policy. Returns an array of refusal reasons, empty when
 * the row may be written.
 */
export function refusals(row, policy) {
  const reasons = [];
  if (!policy.allow_dirty_worktree && row.repository.dirty) {
    reasons.push('measured from a dirty worktree — a stranger cannot reproduce it');
  }
  if (row.sample_count < policy.min_sample_count) {
    reasons.push(
      `${row.sample_count} samples is below the ${policy.min_sample_count} a published row needs`,
    );
  }
  const failurePermille =
    row.iterations_attempted === 0
      ? 0
      : Math.floor((row.failure_count * 1000) / row.iterations_attempted);
  if (failurePermille > policy.max_failure_permille) {
    reasons.push(`${row.failure_count} of ${row.iterations_attempted} iterations failed`);
  }
  if (!row.verification.verified) {
    reasons.push(
      `verification failed (${row.verification.method}): expected ${row.verification.expected_digest}, observed ${row.verification.observed_digest}`,
    );
  }
  if (policy.require_pin_satisfied && !row.baseline.pin_satisfied) {
    reasons.push(
      `${row.baseline.tool} ${row.baseline.version} is not a pinned version (${JSON.stringify(row.baseline.pin_accepted)}) — a drifted tool is a different baseline, not a regression`,
    );
  }
  return reasons;
}

/** Appends an accepted row as one JSON line. */
export function appendRow(path, row) {
  mkdirSync(dirname(path), { recursive: true });
  appendFileSync(path, `${JSON.stringify(row)}\n`, 'utf8');
}

/**
 * The record an unsupported baseline produces *instead of* a timing number.
 *
 * Deliberately a different schema in a different file: "cannot do this" and
 * "did this slowly" must never share a shape, or one will eventually be read as
 * the other and a missing baseline will be quoted as a loss.
 */
export function supportRecord(parts) {
  return {
    schema_version: 'mesh-baseline/support/v1',
    baseline: parts.baselineId,
    workload: parts.workloadId,
    corpus: parts.corpus,
    supported: false,
    reason: parts.reason,
    recorded_at_unix_ms: parts.recordedAtUnixMs,
    repository: parts.repository,
    hardware: parts.hardware,
    platform: parts.platform,
    note: 'no timing number exists for this arm; it is absent, not slow',
  };
}
