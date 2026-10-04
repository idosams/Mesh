import { immutableFleetComparison } from './fleet-input-comparison.js';
// Ordinary retained progress has no checkpoint, review bundle, or approval authority.
const check = value => { if (!value) throw new Error('Saved progress could not be verified'); };
const digest = value => typeof value === 'string' && /^[a-f0-9]{64}$/.test(value);
const count = value => Number.isSafeInteger(value) && value >= 0;
function envelope(raw, selection, schema) {
  check(typeof raw !== 'string' || raw.length <= 4 * 1024 * 1024);
  const value = typeof raw === 'string' ? JSON.parse(raw) : raw;
  check(/^fleet-[a-f0-9]{64}$/.test(selection.objective) && /^lane-[a-f0-9]{64}$/.test(selection.lane)
    && value?.schema === schema && value.objective === selection.objective && value.lane === selection.lane
    && count(value.revision) && digest(value.source_version)
    && (value.starting_version === null || digest(value.starting_version))
    && (value.latest_acknowledged_version === null || digest(value.latest_acknowledged_version))
    && value.handoff_authority === false && value.approval_authority === false);
  return value;
}
export function savedProgressPage(raw, selection, after = null) {
  const value = envelope(raw, selection, 'mesh.fleet-saved-progress-page/v1'), page = value.progress;
  check((after === null || digest(after)) && page?.after === after && page.order === 'causal-operation'
    && count(page.total) && Array.isArray(page.versions) && page.versions.length <= 50 && page.total >= page.versions.length);
  let ordinal = 0;
  const seen = new Set(after === null ? [] : [after]);
  const versions = page.versions.map(row => {
    check(digest(row?.version) && !seen.has(row.version) && count(row.ordinal) && row.ordinal > ordinal && row.ordinal <= page.total);
    check(ordinal === 0 || row.ordinal === ordinal + 1);
    if (after === null && ordinal === 0) check(row.ordinal === 1);
    ordinal = row.ordinal; seen.add(row.version);
    return { version: row.version, ordinal };
  });
  check(page.next_after === null || (versions.length === 50 && page.next_after === versions.at(-1).version && ordinal < page.total));
  if (page.next_after === null && versions.length) check(ordinal === page.total);
  if (after === null && page.next_after === null) check(versions.length === page.total);
  return { objective: value.objective, lane: value.lane, source: value.source_version, starting: value.starting_version,
    latest: value.latest_acknowledged_version, revision: value.revision, after, total: page.total, versions, nextAfter: page.next_after };
}
export function savedProgressComparison(raw, selection, after = null, selected = null, prior = null) {
  const value = envelope(raw, selection, 'mesh.fleet-saved-progress-comparison/v1');
  check(digest(selection.version) && digest(selection.source) && digest(selection.starting)
    && value.source_version === selection.source && value.starting_version === selection.starting
    && value.progress?.base === selection.starting);
  return immutableFleetComparison(value.progress, selection.version, after, selected, prior);
}
