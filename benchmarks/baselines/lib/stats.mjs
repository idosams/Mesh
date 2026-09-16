// Percentiles, in the one definition the whole programme publishes.
//
// A deliberate second implementation of `crates/mesh-bench/src/stats.rs`:
// nearest-rank, integer arithmetic, permille ranks. The point of writing it
// twice in two languages is that `verify.mjs` can check the Rust decoder and
// this file land on the same integers for the same `samples_ns` array — a
// published percentile a reader cannot re-derive is not a published percentile.

/** p50, in permille — the unit the rank is computed in, so no float is passed. */
export const P50 = 500;
/** p95, in permille. */
export const P95 = 950;
/** p99, in permille. */
export const P99 = 990;

/** A sorted copy; the input is never mutated. */
export function sortedCopy(samples) {
  return [...samples].sort((left, right) => (left < right ? -1 : left > right ? 1 : 0));
}

/**
 * Nearest-rank percentile over an already-sorted array of BigInt-safe integers.
 *
 * rank = ceil(permille * n / 1000), clamped to 1..=n. `null` for an empty
 * array, because a percentile of nothing is absent rather than zero.
 */
export function percentile(sorted, permille) {
  if (sorted.length === 0) return null;
  const length = sorted.length;
  const rank = Math.min(Math.max(Math.ceil((permille * length) / 1000), 1), length);
  return sorted[rank - 1];
}

/** Arithmetic mean, truncated towards zero, accumulated exactly. */
export function mean(samples) {
  if (samples.length === 0) return null;
  let total = 0n;
  for (const sample of samples) total += BigInt(sample);
  return Number(total / BigInt(samples.length));
}

/** The `latency` block the schema requires, derived from raw samples. */
export function latencySummary(samplesNs) {
  const sorted = sortedCopy(samplesNs);
  if (sorted.length === 0) return null;
  return {
    min_ns: sorted[0],
    p50_ns: percentile(sorted, P50),
    p95_ns: percentile(sorted, P95),
    p99_ns: percentile(sorted, P99),
    max_ns: sorted[sorted.length - 1],
    mean_ns: mean(sorted),
  };
}
